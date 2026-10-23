//! The collection burst of the performance contract over real HTTP: an open-loop generator
//! offers events at a fixed rate to an in-process server with real SQLite and DuckDB storage,
//! and a probe looks every 100 ms for each acknowledged event in analytics.
//!
//! `cargo run --release -p holocron-server --example collect_burst -- [rate] [seconds]`
//! (default 500 events/s for 60 s, 30,000 events). Prints one JSON summary.
//!
//! With `--gate GATE`, a data directory `holocron-gate load` and `sessions` filled, the burst
//! lands beside the fixture on a new site, the application clock stays at `--now`, two report
//! users run the declared explore suite (`--contains`, `--compare`) and the summary step runs
//! back to back (see `summary_load`) until every event is visible. The summary then also
//! carries per-case report latency and the step's runs.
//!
//! Acknowledgement latency runs from before connecting to the full answer. Visibility runs
//! from the acknowledgement until a snapshot query finds the event's path, with the
//! watermark of that snapshot covering it. Server and generator share this process and host,
//! on separate Tokio runtimes of two workers each.

mod gate;

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use clap::Parser;
use holocron_protocol::access::WorkspaceKind;
use holocron_server::clock::Clock;
use holocron_server::config::{CollectConfig, Config, parse_net};
use holocron_server::db::Db;
use holocron_server::{AppState, router, sites};
use holocron_storage::{Budgets, Lane, Storage};
use holocron_workload::catalog::Catalog;
use holocron_workload::explore_bench::cases;
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36";

#[derive(Default)]
struct Sample {
    sent: Option<Instant>,
    acked: Option<Instant>,
    status: Option<u16>,
    visible: Option<Instant>,
}

#[derive(Parser)]
struct Args {
    /// Events per second.
    #[arg(default_value_t = 500)]
    rate: u64,
    #[arg(default_value_t = 60)]
    seconds: u64,
    /// A gate data directory to burst into, beside two report users and the summary step.
    #[arg(long)]
    gate: Option<PathBuf>,
    /// Repository `fixtures/` directory.
    #[arg(long, default_value = "fixtures")]
    fixtures: PathBuf,
    /// Only report cases whose ID contains this text.
    #[arg(long, default_value = "-30d-")]
    contains: String,
    /// Replace the comparison of every report state without a report, such as `typical_weekday`.
    #[arg(long)]
    compare: Option<String>,
    /// The application clock with `--gate`, RFC 3339.
    #[arg(long, default_value = "2026-09-30T13:20:00Z")]
    now: String,
}

fn main() {
    let args = Args::parse();
    let (rate, seconds) = (args.rate, args.seconds);
    let total = (rate * seconds) as usize;

    let server_rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    let client_rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();

    let dir = tempfile::tempdir().unwrap();
    let clock = match &args.gate {
        Some(_) => Clock::frozen(
            chrono::DateTime::parse_from_rfc3339(&args.now)
                .unwrap()
                .timestamp_millis(),
        ),
        None => Clock::system(),
    };
    let data = args.gate.as_deref().unwrap_or(dir.path());
    let (addr, state, storage, site) = server_rt.block_on(start(
        data,
        clock,
        args.gate.is_some().then_some(&args.fixtures),
    ));
    let samples: Arc<Vec<Mutex<Sample>>> = Arc::new((0..total).map(|_| Mutex::default()).collect());

    // Report users and the summary step, on the server runtime.
    let (stop, stopped) = tokio::sync::watch::channel(false);
    let reports = args.gate.as_ref().map(|_| {
        let cases = Arc::new(cases(
            &args.fixtures,
            "",
            &args.contains,
            args.compare.as_deref(),
        ));
        let users = server_rt.spawn(gate::users(
            storage.clone(),
            cases.clone(),
            usize::MAX,
            stopped.clone(),
        ));
        let step = server_rt.spawn(gate::summary_step(
            state.clone(),
            stopped.clone(),
            Duration::ZERO,
        ));
        (cases, users, step)
    });

    // Probe on the server runtime, outside the generator's timing.
    let probe = {
        let (state, samples) = (state.clone(), samples.clone());
        server_rt.spawn(async move { probe(state, samples).await })
    };

    let started = Instant::now();
    client_rt.block_on(async {
        let interval = Duration::from_nanos(1_000_000_000 / rate);
        let mut tasks = tokio::task::JoinSet::new();
        for i in 0..total {
            let due = started + interval * i as u32;
            tokio::time::sleep_until(due.into()).await;
            let samples = samples.clone();
            let site = site.clone();
            tasks.spawn(async move {
                let sent = Instant::now();
                let status = send(addr, &body(&site, i), i).await;
                let mut sample = samples[i].lock().unwrap();
                sample.sent = Some(sent);
                sample.acked = Some(Instant::now());
                sample.status = status;
            });
        }
        while tasks.join_next().await.is_some() {}
    });
    let offered_for = started.elapsed();
    let probe = server_rt.block_on(probe).unwrap();
    stop.send(true).unwrap();
    let reports = reports.map(|(cases, users, step)| {
        let samples = server_rt.block_on(users).unwrap();
        let step = server_rt.block_on(step).unwrap();
        let (report, users_ms) = gate::report(&cases, &samples);
        let step_ms = step["engine_ms"].as_f64().unwrap_or(0.0);
        json!({
            "compare": args.compare,
            "cases": report,
            "users_engine_ms": users_ms,
            "summaries": step,
            "summary_engine_share": step_ms / (step_ms + users_ms),
        })
    });

    let samples: Vec<Sample> = Arc::try_unwrap(samples)
        .ok()
        .unwrap()
        .into_iter()
        .map(|m| m.into_inner().unwrap())
        .collect();
    let mut statuses: BTreeMap<String, usize> = BTreeMap::new();
    for s in &samples {
        *statuses
            .entry(s.status.map_or("no_answer".into(), |c| c.to_string()))
            .or_default() += 1;
    }
    let ms = |d: Duration| d.as_secs_f64() * 1000.0;
    let ack: Vec<f64> = samples
        .iter()
        .filter_map(|s| Some(ms(s.acked? - s.sent?)))
        .collect();
    let visible: Vec<f64> = samples
        .iter()
        .filter(|s| s.status == Some(200))
        .filter_map(|s| Some(ms(s.visible? - s.acked?)))
        .collect();
    let send_to_visible: Vec<f64> = samples
        .iter()
        .filter(|s| s.status == Some(200))
        .filter_map(|s| Some(ms(s.visible? - s.sent?)))
        .collect();
    let accepted = samples.iter().filter(|s| s.status == Some(200)).count();

    let mut rusage: libc::rusage = unsafe { std::mem::zeroed() };
    // SAFETY: getrusage fills the struct for the calling process.
    unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut rusage) };
    let summary = json!({
        "rate_per_s": rate,
        "seconds": seconds,
        "offered": total,
        "offered_for_s": offered_for.as_secs_f64(),
        "statuses": statuses,
        "accepted": accepted,
        "visible": visible.len(),
        "ack_ms": percentiles(ack),
        "ack_to_visible_ms": percentiles(visible),
        "send_to_visible_ms": percentiles(send_to_visible),
        "probes": probe.probes,
        "probe_mean_ms": probe.total.as_secs_f64() * 1000.0 / probe.probes.max(1) as f64,
        "inbox_peak_records": probe.inbox_peak_records,
        "inbox_peak_bytes": probe.inbox_peak_bytes,
        "max_rss_bytes": if cfg!(target_os = "macos") { rusage.ru_maxrss } else { rusage.ru_maxrss * 1024 },
        "reports": reports,
    });
    println!("{}", serde_json::to_string_pretty(&summary).unwrap());
    drop(state);
    drop(dir);
}

/// Starts the server on `data`. With `fixtures`, `data` is a gate directory whose sites are
/// registered first.
async fn start(
    data: &Path,
    clock: Clock,
    fixtures: Option<&PathBuf>,
) -> (SocketAddr, AppState, Arc<Storage>, String) {
    let db = Db::open(data).await.unwrap();
    if let Some(fixtures) = fixtures {
        let catalog = Catalog::load(&fixtures.join("workload/manifest.json"));
        gate::register(&db, catalog, fixtures).await.unwrap();
    }
    let site = {
        let mut conn = db.writer.acquire().await.unwrap();
        let workspace = sites::create_workspace(&mut conn, "Burst", WorkspaceKind::Team)
            .await
            .unwrap();
        sites::create_site(
            &mut conn,
            &workspace,
            "Burst",
            "burst.test",
            "Europe/Vienna",
        )
        .await
        .unwrap()
        .as_str()
        .to_owned()
    };
    // The development disk may hold less than the 4 GiB production reserve.
    let budgets = Budgets {
        disk_reserve_bytes: 64 * 1024 * 1024,
        ..holocron_workload::gate::budgets()
    };
    let storage = Arc::new(Storage::open(data, budgets).await.unwrap());
    let config = Config {
        data_dir: data.to_path_buf(),
        bind: "127.0.0.1:0".parse().unwrap(),
        web_dir: None,
        public_origin: None,
        instance_name: "burst".into(),
        setup_token: None,
        collect: CollectConfig {
            trusted_proxies: vec![parse_net("127.0.0.1").unwrap()],
            ..CollectConfig::default()
        },
        disk_reserve_bytes: 64 * 1024 * 1024,
    };
    let state = AppState::new(db, storage.clone(), config, clock)
        .await
        .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = router(state.clone()).into_make_service_with_connect_info::<SocketAddr>();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (addr, state, storage, site)
}

/// The primary fixture's mix: of every 20 events, 15 pageviews, 4 custom events with three
/// properties, 1 performance report. Ten consecutive events share one visitor.
fn body(site: &str, i: usize) -> String {
    let url = format!("/burst/{i}");
    let payload = match i % 20 {
        0..15 => {
            json!({ "website": site, "hostname": "burst.test", "url": url, "screen": "1920x1080", "language": "de-AT", "referrer": "https://news.example/a" })
        }
        15..19 => {
            json!({ "website": site, "hostname": "burst.test", "url": url, "name": "signup", "data": { "plan": "pro", "price": 129.9, "seats": i % 7 } })
        }
        _ => {
            json!({ "website": site, "hostname": "burst.test", "url": url, "lcp": 1830, "fcp": 640, "ttfb": 120, "inp": 88, "cls": 0.04 })
        }
    };
    let kind = if i % 20 == 19 { "performance" } else { "event" };
    json!({ "type": kind, "payload": payload }).to_string()
}

async fn send(addr: SocketAddr, body: &str, i: usize) -> Option<u16> {
    let mut stream = tokio::net::TcpStream::connect(addr).await.ok()?;
    let visitor = i / 10;
    let request = format!(
        "POST /api/send HTTP/1.1\r\nHost: burst.test\r\nUser-Agent: {UA}\r\nX-Forwarded-For: 10.{}.{}.{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        (visitor >> 16) & 255,
        (visitor >> 8) & 255,
        visitor & 255,
        body.len()
    );
    stream.write_all(request.as_bytes()).await.ok()?;
    let mut answer = Vec::new();
    stream.read_to_end(&mut answer).await.ok()?;
    std::str::from_utf8(&answer)
        .ok()?
        .split(' ')
        .nth(1)?
        .parse()
        .ok()
}

struct Probe {
    probes: u64,
    total: Duration,
    inbox_peak_records: u64,
    inbox_peak_bytes: u64,
}

/// Every 100 ms, looks up every acknowledged event not yet seen, in one snapshot query.
async fn probe(state: AppState, samples: Arc<Vec<Mutex<Sample>>>) -> Probe {
    let storage = state.collector.storage();
    let mut result = Probe {
        probes: 0,
        total: Duration::ZERO,
        inbox_peak_records: 0,
        inbox_peak_bytes: 0,
    };
    let mut quiet_since: Option<Instant> = None;
    // Only rows after the ones present at start: a gate directory holds the fixture and
    // earlier bursts with the same paths.
    let after: i64 = storage
        .read(Lane::Live, |s| {
            s.connection()
                .query_row("SELECT coalesce(max(seq), 0) FROM events", [], |r| r.get(0))
        })
        .await
        .unwrap()
        .value;
    loop {
        tokio::time::sleep(Duration::from_millis(100)).await;
        let status = storage.status();
        result.inbox_peak_records = result.inbox_peak_records.max(status.inbox_records);
        result.inbox_peak_bytes = result.inbox_peak_bytes.max(status.inbox_bytes);
        let waiting: Vec<usize> = samples
            .iter()
            .enumerate()
            .filter(|(_, s)| {
                let s = s.lock().unwrap();
                s.status == Some(200) && s.visible.is_none()
            })
            .map(|(i, _)| i)
            .collect();
        let unanswered = samples.iter().any(|s| s.lock().unwrap().acked.is_none());
        if waiting.is_empty() {
            if !unanswered {
                return result;
            }
            continue;
        }
        let sql = format!(
            "SELECT path FROM events WHERE seq > {after} AND path IN ({})",
            waiting
                .iter()
                .map(|i| format!("'/burst/{i}'"))
                .collect::<Vec<_>>()
                .join(",")
        );
        let began = Instant::now();
        let found = storage
            .read(Lane::Live, move |s| {
                let mut statement = s.connection().prepare(&sql)?;
                let paths = statement
                    .query_map([], |r| r.get::<_, String>(0))?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(paths)
            })
            .await
            .unwrap();
        let seen = Instant::now();
        result.probes += 1;
        result.total += seen - began;
        for path in &found.value {
            let i: usize = path.trim_start_matches("/burst/").parse().unwrap();
            samples[i].lock().unwrap().visible = Some(seen);
        }
        // Give up 30 s after the last newly visible event.
        if found.value.is_empty() {
            let since = *quiet_since.get_or_insert(seen);
            if !unanswered && since.elapsed() > Duration::from_secs(30) {
                return result;
            }
        } else {
            quiet_since = None;
        }
    }
}

/// Nearest-rank percentiles.
fn percentiles(mut values: Vec<f64>) -> serde_json::Value {
    if values.is_empty() {
        return json!(null);
    }
    values.sort_by(f64::total_cmp);
    let rank = |p: f64| {
        values[((p / 100.0 * values.len() as f64).ceil() as usize).clamp(1, values.len()) - 1]
    };
    json!({
        "count": values.len(),
        "min": values[0],
        "p50": rank(50.0),
        "p95": rank(95.0),
        "p99": rank(99.0),
        "max": values[values.len() - 1],
    })
}
