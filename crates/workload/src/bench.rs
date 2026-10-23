//! Load and latency harness. Runs in the same process as storage (there is no
//! HTTP server before milestone 04), so collection latency is admission
//! latency without network, and process memory includes the harness.
//!
//! Modes:
//! - `suite`: both report users loop over their early-gate cases until every
//!   case has `--observations` samples, with baseline ingestion.
//! - `streams`: both users follow the frozen `streams.json` early-gate order
//!   for `--seconds`, with baseline ingestion and an optional 60 s burst.
//! - `ingest`: open-loop collection at 500 events/s for `--groups` groups.
//! - `restart`: process-restart starts per cold case (OS page cache stays warm).

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use holocron_storage::{Lane, Outcome, Record, Refusal, Storage};
use serde_json::{Value as Json, json};

use crate::associated::{self, FRAGMENT_BYTES, Offer};
use crate::cases::Case;
use crate::catalog::Catalog;
use crate::fixture::Fixture;
use crate::gate;
use crate::load::Identities;
use crate::prng::{Stream, uniform};

const MEASUREMENT_CLOCK_MS: i64 = 1_790_812_800_000;

#[derive(Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Mode {
    Suite,
    Streams,
    Ingest,
    Restart,
}

#[derive(clap::Args)]
pub struct Args {
    #[arg(long)]
    pub data: PathBuf,
    #[arg(long)]
    pub out: PathBuf,
    #[arg(long, value_enum)]
    pub mode: Mode,
    #[arg(long, default_value_t = 200)]
    pub observations: usize,
    #[arg(long, default_value_t = 300)]
    pub seconds: u64,
    /// Seconds after start when the 60 s, 30,000-event burst begins (streams mode).
    #[arg(long)]
    pub burst_at: Option<u64>,
    #[arg(long, default_value_t = 4)]
    pub baseline_rate: u64,
    #[arg(long, default_value_t = 10_000)]
    pub groups: u64,
    #[arg(long, default_value_t = 40)]
    pub starts: usize,
    /// Run number, part of burst event IDs so repeated runs on one store stay distinct.
    #[arg(long, default_value_t = 1)]
    pub run: u32,
}

#[derive(Default)]
struct Samples {
    queries: Vec<Json>,
    offers: Vec<Json>,
    visible: Vec<Json>,
    probes: Vec<Json>,
    resources: Vec<Json>,
}

/// Event ID, scheduled send, acknowledgement, phase.
type Pending = (u128, Instant, Instant, &'static str);

struct Harness {
    storage: Arc<Storage>,
    fx: Fixture,
    ids: Identities,
    started: Instant,
    samples: Mutex<Samples>,
    /// Accepted primary events not yet seen by a probe: seq -> (id, scheduled, acked).
    pending: Mutex<HashMap<i64, Pending>>,
    /// Stops the report users; samplers keep running until `stop`.
    users_stop: AtomicBool,
    stop: AtomicBool,
}

impl Harness {
    fn at(&self) -> f64 {
        self.started.elapsed().as_secs_f64()
    }

    fn clock_ms(&self) -> i64 {
        MEASUREMENT_CLOCK_MS + self.started.elapsed().as_millis() as i64
    }
}

pub async fn run(fixtures: &Path, args: Args) {
    std::fs::create_dir_all(&args.out).unwrap();
    let cat: &'static Catalog = Catalog::load(&fixtures.join("workload/manifest.json"));
    let cases = Case::early_gate(fixtures);
    if args.mode == Mode::Restart {
        restart(&args, &cases).await;
        return;
    }
    let opened = Instant::now();
    let storage = Arc::new(
        Storage::open(&args.data, gate::budgets())
            .await
            .expect("open storage"),
    );
    eprintln!("bench: storage ready in {:.0?}", opened.elapsed());
    let h = Arc::new(Harness {
        storage,
        fx: Fixture::new(cat),
        ids: Identities::new(cat),
        started: Instant::now(),
        samples: Mutex::default(),
        pending: Mutex::default(),
        users_stop: AtomicBool::new(false),
        stop: AtomicBool::new(false),
    });

    let mut tasks = tokio::task::JoinSet::new();
    tasks.spawn(resources(h.clone(), args.data.clone(), args.out.clone()));
    tasks.spawn(visibility(h.clone()));
    let streams: Json = serde_json::from_str(
        &std::fs::read_to_string(fixtures.join("workload/streams.json")).unwrap(),
    )
    .unwrap();
    let by_id: HashMap<String, Case> = cases.iter().map(|c| (c.id.clone(), c.clone())).collect();

    match args.mode {
        Mode::Suite | Mode::Streams => {
            let mut users = tokio::task::JoinSet::new();
            for (index, user) in streams["early_gate"]["users"]
                .as_array()
                .unwrap()
                .iter()
                .enumerate()
            {
                let sequence = if args.mode == Mode::Streams {
                    stream_order(user, &by_id)
                } else {
                    user["families"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .flat_map(|f| {
                            cases
                                .iter()
                                .filter(move |c| c.family == f.as_str().unwrap())
                        })
                        .cloned()
                        .collect()
                };
                users.spawn(report_user(
                    h.clone(),
                    index as u64,
                    user["user"].as_str().unwrap().to_owned(),
                    Duration::from_millis(user["start_offset_ms"].as_u64().unwrap()),
                    sequence,
                    args.mode == Mode::Streams,
                ));
            }
            let baseline = tokio::spawn(baseline(h.clone(), args.baseline_rate, args.run));
            if let Some(at) = args.burst_at {
                let h = h.clone();
                let run = args.run;
                tasks.spawn(async move {
                    tokio::time::sleep(Duration::from_secs(at)).await;
                    eprintln!("bench: burst starts at {:.1}s", h.at());
                    let offers = associated::burst(&h.fx, &h.ids, run, 1, 3_000, h.clock_ms(), 2);
                    offer_open_loop(h.clone(), offers, "burst").await;
                    eprintln!("bench: burst offered by {:.1}s", h.at());
                });
            }
            match args.mode {
                Mode::Streams => tokio::time::sleep(Duration::from_secs(args.seconds)).await,
                _ => {
                    let target = args.observations;
                    loop {
                        tokio::time::sleep(Duration::from_secs(5)).await;
                        let counts = per_case_counts(&h);
                        let done = cases
                            .iter()
                            .all(|c| counts.get(&c.id).copied().unwrap_or(0) >= target);
                        if done {
                            break;
                        }
                    }
                }
            }
            h.users_stop.store(true, Ordering::SeqCst);
            while users.join_next().await.is_some() {}
            baseline.abort();
        }
        Mode::Ingest => {
            let offers =
                associated::burst(&h.fx, &h.ids, args.run, 2, args.groups, h.clock_ms(), 2);
            offer_open_loop(h.clone(), offers, "ingest").await;
        }
        Mode::Restart => unreachable!(),
    }
    // Let the probe catch every accepted event, then stop the samplers.
    let deadline = Instant::now() + Duration::from_secs(60);
    while !h.pending.lock().unwrap().is_empty() && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    h.stop.store(true, Ordering::SeqCst);
    while tasks.join_next().await.is_some() {}
    write_results(&h, &args, &cases);
    let storage = Arc::into_inner(
        Arc::into_inner(h)
            .unwrap_or_else(|| panic!("harness still shared"))
            .storage,
    )
    .unwrap();
    storage.close().await.expect("close");
}

fn per_case_counts(h: &Harness) -> HashMap<String, usize> {
    let mut counts = HashMap::new();
    for q in &h.samples.lock().unwrap().queries {
        *counts
            .entry(q["case"].as_str().unwrap().to_owned())
            .or_default() += 1;
    }
    counts
}

/// The frozen order rule of streams.json, expanded for 1,000 cycles.
fn stream_order(user: &Json, cases: &HashMap<String, Case>) -> Vec<Case> {
    let families: Vec<&str> = user["families"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f.as_str().unwrap())
        .collect();
    let list = |window: &str, family: &str| -> Vec<String> {
        user[window][family].as_array().map_or(Vec::new(), |a| {
            a.iter().map(|c| c.as_str().unwrap().to_owned()).collect()
        })
    };
    let mut out = Vec::new();
    for c in 0..1000usize {
        for r in 0..4usize {
            for family in &families {
                let (window, index) = if r < 3 {
                    ("thirty_day_cases", 3 * c + r)
                } else {
                    ("annual_cases", c)
                };
                let ids = list(window, family);
                if !ids.is_empty() {
                    out.push(cases[&ids[index % ids.len()]].clone());
                }
            }
        }
    }
    out
}

async fn report_user(
    h: Arc<Harness>,
    index: u64,
    name: String,
    offset: Duration,
    sequence: Vec<Case>,
    stream: bool,
) {
    let jitter = Stream::named("stream.jitter");
    tokio::time::sleep(offset).await;
    for (request, case) in sequence.iter().cycle().enumerate() {
        if h.users_stop.load(Ordering::SeqCst) || (stream && request >= sequence.len()) {
            return;
        }
        let started = Instant::now();
        let at = h.at();
        let result = crate::probes::run(&h.storage, case.clone(), Lane::Report).await;
        let latency = started.elapsed();
        let expected = case.file["expected"]["golden_result_hash"].as_str();
        let sample = match &result {
            Ok(run) => json!({
                "user": name, "case": case.id, "window": case.file["window"], "at_s": at,
                "latency_ms": latency.as_secs_f64() * 1e3,
                "queue_ms": run.queue_wait.as_secs_f64() * 1e3,
                "execution_ms": run.execution.as_secs_f64() * 1e3,
                "hash_match": Some(run.hash.as_str()) == expected,
            }),
            Err(error) => json!({
                "user": name, "case": case.id, "window": case.file["window"], "at_s": at,
                "latency_ms": latency.as_secs_f64() * 1e3, "error": error.to_string(),
            }),
        };
        h.samples.lock().unwrap().queries.push(sample);
        // Think time: 500 ms + uniform(draw(stream.jitter, user * 2^32 + request), 101) - 50.
        let think = 500
            + uniform(
                crate::prng::draw(jitter.0, (index << 32) + request as u64),
                101,
            ) as i64
            - 50;
        tokio::time::sleep(Duration::from_millis(think as u64)).await;
    }
}

/// Open-loop baseline collection: one event every `1000 / rate` ms until stopped.
async fn baseline(h: Arc<Harness>, rate: u64, run: u32) {
    if rate == 0 {
        return;
    }
    let spacing = 1000 / rate;
    let mut chunk = 0u32;
    loop {
        chunk += 1;
        let start = h.clock_ms();
        let offers = associated::burst(&h.fx, &h.ids, run, 1000 + chunk, 360, start, spacing);
        offer_open_loop(h.clone(), offers, "baseline").await;
    }
}

/// Sends each offer at its scheduled time regardless of earlier responses.
/// Latency counts from the scheduled time, so a late send is not hidden.
async fn offer_open_loop(h: Arc<Harness>, offers: Vec<Offer>, phase: &'static str) {
    let origin = tokio::time::Instant::now();
    let mut sends = tokio::task::JoinSet::new();
    for offer in offers {
        let due = origin + Duration::from_millis(offer.offset_ms);
        tokio::time::sleep_until(due).await;
        let h = h.clone();
        sends.spawn(async move {
            let scheduled = due.into_std();
            let mut record = offer.record;
            if let (Some((seed, index)), Record::ReplayFragment(f)) = (offer.payload, &mut record) {
                let stream = associated::capture_stream(seed, (index + 1) * FRAGMENT_BYTES);
                f.payload = stream[index * FRAGMENT_BYTES..].to_vec();
            }
            let sent = Instant::now();
            let result = h.storage.admit(std::slice::from_ref(&record)).await;
            let acked = Instant::now();
            let kind = match record {
                Record::Event(_) => "event",
                Record::ReplayFragment(_) => "fragment",
                Record::HeatmapInteraction(_) => "interaction",
                Record::Identify(_) => "identify",
                Record::Revocation(_) => "revocation",
            };
            let outcome = match result {
                Ok(outcomes) => match outcomes[0] {
                    Outcome::Accepted { sequence } => {
                        if let (true, Record::Event(e)) = (offer.primary, &record) {
                            h.pending
                                .lock()
                                .unwrap()
                                .insert(sequence, (e.id, scheduled, acked, phase));
                        }
                        "accepted"
                    }
                    Outcome::Duplicate { .. } => "duplicate",
                },
                Err(Refusal::Busy) => "refused_busy",
                Err(Refusal::Unavailable) => "refused_unavailable",
            };
            h.samples.lock().unwrap().offers.push(json!({
                "phase": phase,
                "kind": kind,
                "at_s": scheduled.duration_since(h.started).as_secs_f64(),
                "send_lag_ms": sent.duration_since(scheduled).as_secs_f64() * 1e3,
                "latency_ms": acked.duration_since(scheduled).as_secs_f64() * 1e3,
                "outcome": outcome,
            }));
        });
    }
    while sends.join_next().await.is_some() {}
}

/// Batched ID lookups every 100 ms on the live lane, outside the report pool.
/// An event counts as visible once a snapshot both returns its row and has a
/// watermark covering its sequence.
async fn visibility(h: Arc<Harness>) {
    let mut tick = tokio::time::interval(Duration::from_millis(100));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    while !h.stop.load(Ordering::SeqCst) {
        tick.tick().await;
        let pending: Vec<(i64, u128)> = h
            .pending
            .lock()
            .unwrap()
            .iter()
            .map(|(s, (id, ..))| (*s, *id))
            .collect();
        if pending.is_empty() {
            continue;
        }
        let min_seq = pending.iter().map(|(s, _)| *s).min().unwrap();
        let list = pending
            .iter()
            .map(|(_, id)| id.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!("SELECT seq FROM events WHERE seq >= {min_seq} AND id IN ({list})");
        let started = Instant::now();
        let read = h
            .storage
            .read(Lane::Live, move |snap| {
                let mut statement = snap.connection().prepare(&sql)?;
                let rows = statement.query_map([], |r| r.get::<_, i64>(0))?;
                rows.collect::<Result<Vec<_>, _>>()
            })
            .await;
        let seen = Instant::now();
        let Ok(read) = read else { continue };
        let mut visible = Vec::new();
        {
            let mut pending = h.pending.lock().unwrap();
            for seq in read.value.iter().filter(|s| **s <= read.watermark.sequence) {
                if let Some((_, scheduled, acked, phase)) = pending.remove(seq) {
                    visible.push(json!({
                        "phase": phase,
                        "seq": seq,
                        "ack_to_visible_ms": seen.duration_since(acked).as_secs_f64() * 1e3,
                        "send_to_visible_ms": seen.duration_since(scheduled).as_secs_f64() * 1e3,
                    }));
                }
            }
        }
        let mut samples = h.samples.lock().unwrap();
        samples.probes.push(json!({
            "at_s": h.at(), "ids": pending.len(), "visible": visible.len(),
            "duration_ms": started.elapsed().as_secs_f64() * 1e3, "watermark": read.watermark.sequence,
        }));
        samples.visible.extend(visible);
    }
}

fn file_size(path: &Path) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

pub(crate) fn dir_size(path: &Path) -> u64 {
    std::fs::read_dir(path).map_or(0, |entries| {
        entries
            .flatten()
            .map(|e| e.metadata().map(|m| m.len()).unwrap_or(0))
            .sum()
    })
}

/// Linux cgroup v2 and /proc readings; empty elsewhere.
pub(crate) fn memory() -> Json {
    let read = |p: &str| std::fs::read_to_string(p).ok();
    let field = |text: &Option<String>, key: &str| {
        text.as_ref().and_then(|t| {
            t.lines()
                .find(|l| l.starts_with(key))
                .and_then(|l| l.split_whitespace().nth(1))
                .and_then(|v| v.parse::<u64>().ok())
        })
    };
    let status = read("/proc/self/status");
    let stat = read("/sys/fs/cgroup/memory.stat");
    let events = read("/sys/fs/cgroup/memory.events");
    json!({
        "rss_kib": field(&status, "VmRSS:"),
        "hwm_kib": field(&status, "VmHWM:"),
        "cgroup_current": read("/sys/fs/cgroup/memory.current").and_then(|v| v.trim().parse::<u64>().ok()),
        "cgroup_peak": read("/sys/fs/cgroup/memory.peak").and_then(|v| v.trim().parse::<u64>().ok()),
        "cgroup_anon": field(&stat, "anon "),
        "cgroup_file": field(&stat, "file "),
        "cgroup_kernel": field(&stat, "kernel "),
        "cgroup_shmem": field(&stat, "shmem "),
        "swap_current": read("/sys/fs/cgroup/memory.swap.current").and_then(|v| v.trim().parse::<u64>().ok()),
        "oom_kill": field(&events, "oom_kill "),
    })
}

/// Samples every second and appends each sample to `live-resources.jsonl` at
/// once, so a run killed by the OOM killer still leaves its memory trace.
async fn resources(h: Arc<Harness>, data: PathBuf, out: PathBuf) {
    use std::io::Write;
    let mut live = std::fs::File::create(out.join("live-resources.jsonl")).unwrap();
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    while !h.stop.load(Ordering::SeqCst) {
        tick.tick().await;
        let status = h.storage.status();
        let sample = json!({
            "at_s": h.at(),
            "memory": memory(),
            "inbox_records": status.inbox_records,
            "inbox_bytes": status.inbox_bytes,
            "watermark": status.watermark.sequence,
            "disk": {
                "inbox_sqlite": file_size(&data.join("inbox.sqlite")),
                "inbox_wal": file_size(&data.join("inbox.sqlite-wal")),
                "duckdb": file_size(&data.join("analytics.duckdb")),
                "duckdb_wal": file_size(&data.join("analytics.duckdb.wal")),
                "spill": dir_size(&data.join("spill")),
            },
        });
        writeln!(live, "{sample}").unwrap();
        let _ = live.sync_data();
        h.samples.lock().unwrap().resources.push(sample);
    }
}

async fn restart(args: &Args, cases: &[Case]) {
    let cold: Vec<&Case> = cases.iter().filter(|c| c.file["cold"] == true).collect();
    let mut samples = Vec::new();
    for case in cold {
        for start in 0..args.starts {
            let opened = Instant::now();
            let storage = Storage::open(&args.data, gate::budgets())
                .await
                .expect("open storage");
            let ready = opened.elapsed();
            let started = Instant::now();
            let result = crate::probes::run(&storage, case.clone(), Lane::Report).await;
            let latency = started.elapsed();
            storage.close().await.expect("close");
            let expected = case.file["expected"]["golden_result_hash"].as_str();
            samples.push(json!({
                "case": case.id, "start": start,
                "startup_to_ready_ms": ready.as_secs_f64() * 1e3,
                "latency_ms": latency.as_secs_f64() * 1e3,
                "hash_match": result.as_ref().is_ok_and(|r| Some(r.hash.as_str()) == expected),
                "error": result.err().map(|e| e.to_string()),
            }));
        }
        eprintln!("bench: restart {} done", case.id);
    }
    write_lines(&args.out.join("restarts.jsonl"), &samples);
    let mut summary = BTreeMap::new();
    for case in cases.iter().filter(|c| c.file["cold"] == true) {
        let rows: Vec<&Json> = samples
            .iter()
            .filter(|s| s["case"] == case.id.as_str())
            .collect();
        summary.insert(case.id.clone(), json!({
            "latency_ms": stats(rows.iter().map(|s| s["latency_ms"].as_f64().unwrap()).collect()),
            "startup_to_ready_ms": stats(rows.iter().map(|s| s["startup_to_ready_ms"].as_f64().unwrap()).collect()),
            "hash_mismatches": rows.iter().filter(|s| s["hash_match"] != true).count(),
            "target_p95_ms": case.file["latency"]["target_p95_ms"],
        }));
    }
    std::fs::write(
        args.out.join("summary.json"),
        serde_json::to_string_pretty(&json!({ "mode": "restart", "cases": summary })).unwrap(),
    )
    .unwrap();
}

/// Nearest-rank statistics (0003 Percentiles).
/// Nearest-rank percentiles of `values`.
pub fn stats(mut values: Vec<f64>) -> Json {
    if values.is_empty() {
        return json!({ "n": 0 });
    }
    values.sort_by(f64::total_cmp);
    let rank = |p: f64| values[((p * values.len() as f64).ceil() as usize).max(1) - 1];
    json!({
        "n": values.len(), "min": values[0], "p50": rank(0.5), "p95": rank(0.95), "p99": rank(0.99),
        "max": values[values.len() - 1],
    })
}

fn histogram(values: &[f64]) -> BTreeMap<String, usize> {
    const EDGES: [f64; 14] = [
        5.0, 10.0, 25.0, 50.0, 100.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 10000.0,
        20000.0, 30000.0,
    ];
    let mut out = BTreeMap::new();
    for v in values {
        let edge = EDGES
            .iter()
            .find(|e| v <= *e)
            .map_or("inf".to_owned(), |e| format!("{e:>7}"));
        *out.entry(edge).or_default() += 1;
    }
    out
}

pub fn write_lines(path: &Path, rows: &[Json]) {
    let text: String = rows.iter().map(|r| r.to_string() + "\n").collect();
    std::fs::write(path, text).unwrap();
}

fn write_results(h: &Harness, args: &Args, cases: &[Case]) {
    let samples = h.samples.lock().unwrap();
    write_lines(&args.out.join("queries.jsonl"), &samples.queries);
    write_lines(&args.out.join("offers.jsonl"), &samples.offers);
    write_lines(&args.out.join("visible.jsonl"), &samples.visible);
    write_lines(&args.out.join("probes.jsonl"), &samples.probes);
    write_lines(&args.out.join("resources.jsonl"), &samples.resources);

    let mut per_case = BTreeMap::new();
    for case in cases {
        let rows: Vec<&Json> = samples
            .queries
            .iter()
            .filter(|q| q["case"] == case.id.as_str())
            .collect();
        let latency: Vec<f64> = rows
            .iter()
            .map(|q| q["latency_ms"].as_f64().unwrap())
            .collect();
        per_case.insert(case.id.clone(), json!({
            "latency_ms": stats(latency.clone()),
            "queue_ms": stats(rows.iter().filter_map(|q| q["queue_ms"].as_f64()).collect()),
            "execution_ms": stats(rows.iter().filter_map(|q| q["execution_ms"].as_f64()).collect()),
            "histogram_ms": histogram(&latency),
            "errors": rows.iter().filter(|q| q.get("error").is_some()).count(),
            "hash_mismatches": rows.iter().filter(|q| q["hash_match"] == false).count(),
            "target_p95_ms": case.file["latency"]["target_p95_ms"],
        }));
    }
    let mut offers = BTreeMap::new();
    for (phase, kind) in ["burst", "baseline", "ingest"]
        .iter()
        .flat_map(|p| ["event", "fragment", "interaction"].map(move |k| (*p, k)))
    {
        let rows: Vec<&Json> = samples
            .offers
            .iter()
            .filter(|o| o["kind"] == kind && o["phase"] == phase)
            .collect();
        if rows.is_empty() {
            continue;
        }
        let mut outcomes: BTreeMap<String, usize> = BTreeMap::new();
        for o in &rows {
            *outcomes
                .entry(o["outcome"].as_str().unwrap().to_owned())
                .or_default() += 1;
        }
        offers.insert(format!("{phase} {kind}"), json!({
            "offered": rows.len(),
            "outcomes": outcomes,
            "ack_latency_ms": stats(rows.iter().filter(|o| o["outcome"] == "accepted").map(|o| o["latency_ms"].as_f64().unwrap()).collect()),
            "send_lag_ms": stats(rows.iter().map(|o| o["send_lag_ms"].as_f64().unwrap()).collect()),
        }));
    }
    let peak = |path: &[&str]| {
        samples
            .resources
            .iter()
            .filter_map(|r| {
                path.iter()
                    .try_fold(r, |v, k| v.get(*k))
                    .and_then(Json::as_u64)
            })
            .max()
    };
    let by_phase = |field: &str| -> BTreeMap<&str, Json> {
        ["burst", "baseline", "ingest"]
            .into_iter()
            .map(|p| {
                (
                    p,
                    stats(
                        samples
                            .visible
                            .iter()
                            .filter(|v| v["phase"] == p)
                            .map(|v| v[field].as_f64().unwrap())
                            .collect(),
                    ),
                )
            })
            .collect()
    };
    let summary = json!({
        "mode": match args.mode { Mode::Suite => "suite", Mode::Streams => "streams", Mode::Ingest => "ingest", Mode::Restart => "restart" },
        "seconds": h.at(),
        "cases": per_case,
        "collection": offers,
        "visibility": {
            "ack_to_visible_ms": by_phase("ack_to_visible_ms"),
            "send_to_visible_ms": by_phase("send_to_visible_ms"),
            "never_seen": h.pending.lock().unwrap().len(),
            "probe_duration_ms": stats(samples.probes.iter().map(|p| p["duration_ms"].as_f64().unwrap()).collect()),
            "probes": samples.probes.len(),
        },
        "peaks": {
            "rss_kib": peak(&["memory", "rss_kib"]),
            "hwm_kib": peak(&["memory", "hwm_kib"]),
            "cgroup_peak": peak(&["memory", "cgroup_peak"]),
            "cgroup_current": peak(&["memory", "cgroup_current"]),
            "cgroup_anon": peak(&["memory", "cgroup_anon"]),
            "cgroup_file": peak(&["memory", "cgroup_file"]),
            "cgroup_kernel": peak(&["memory", "cgroup_kernel"]),
            "oom_kill": peak(&["memory", "oom_kill"]),
            "inbox_records": peak(&["inbox_records"]),
            "inbox_bytes": peak(&["inbox_bytes"]),
            "inbox_wal": peak(&["disk", "inbox_wal"]),
            "duckdb_wal": peak(&["disk", "duckdb_wal"]),
            "spill": peak(&["disk", "spill"]),
            "duckdb": peak(&["disk", "duckdb"]),
        },
    });
    std::fs::write(
        args.out.join("summary.json"),
        serde_json::to_string_pretty(&summary).unwrap(),
    )
    .unwrap();
    eprintln!("bench: results in {}", args.out.display());
}
