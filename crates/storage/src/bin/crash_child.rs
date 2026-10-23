//! Test support for `tests/failpoints.rs`, built only with the `failpoints` feature.
//!
//! `run <dir> <seed>` admits seeded requests one at a time and logs each
//! request before admission (`begun`) and after its acknowledgement (`acked`),
//! fsyncing the log, then waits for transfer to drain. The parent arms one
//! failpoint, so the process is usually killed partway.
//!
//! `verify <dir> <seed> <boundary>` reopens the store without failpoints,
//! drains, and checks every request against the logs and the analytical
//! tables. It prints `ok <summary>` or exits non-zero with the violation.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::time::Duration;

use holocron_storage::{
    Budgets, Dimensions, Event, EventKind, HeatmapInteraction, Identify, Interaction, Lane,
    Outcome, Property, PropertyValue, Record, ReplayFragment, Revenue, Storage,
};

const DAY_MS: i64 = 86_400_000;
const NOW_MS: i64 = 1_790_812_800_000; // 2026-10-01T00:00:00Z

fn mix64(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
    z ^ (z >> 31)
}

struct Rng(u64);
impl Rng {
    fn below(&mut self, n: u64) -> u64 {
        self.0 = self.0.wrapping_add(0x9E3779B97F4A7C15);
        ((mix64(self.0) as u128 * n as u128) >> 64) as u64
    }
}

/// One seeded request, admitted as one call.
struct Request {
    records: Vec<Record>,
}

fn budgets(seed: u64) -> Budgets {
    Budgets {
        batch_records: 3 + (seed % 9) as usize,
        batch_bytes: 256 * 1024,
        transfer_delay: Duration::from_millis(2),
        engine_threads: 1,
        engine_memory_bytes: 256 * 1024 * 1024,
        disk_reserve_bytes: 0,
        ..Budgets::default()
    }
}

fn requests(seed: u64) -> Vec<Request> {
    let mut rng = Rng(seed.wrapping_mul(0x2545F4914F6CDD1D) ^ 0x484F4C4F43524F4E);
    let sites = [1u128 << 100, 2u128 << 100];
    let count = 12 + rng.below(8);
    let mut next_id = (seed as u128) << 64;
    (0..count)
        .map(|_| {
            let size = 1 + rng.below(25);
            let records = (0..size)
                .map(|_| {
                    next_id += 1;
                    let id = next_id;
                    let site = sites[rng.below(2) as usize];
                    // One in eight events is late: its day is up to 300 days back.
                    let late = rng.below(8) == 0;
                    let event_time_ms = NOW_MS
                        - if late {
                            rng.below(300) as i64 * DAY_MS
                        } else {
                            rng.below(3_600_000) as i64
                        };
                    let local_day = (event_time_ms / DAY_MS) as i32;
                    match rng.below(10) {
                        0 => Record::ReplayFragment(ReplayFragment {
                            id,
                            site,
                            capture_id: id >> 3,
                            index: (id % 5) as u16,
                            total: 5,
                            is_final: id % 5 == 4,
                            event_time_ms,
                            receive_time_ms: NOW_MS,
                            payload: vec![b'{'; 1024 + rng.below(65_536) as usize],
                        }),
                        1 => Record::HeatmapInteraction(HeatmapInteraction {
                            id,
                            site,
                            capture_id: id >> 3,
                            event_time_ms,
                            receive_time_ms: NOW_MS,
                            path: "/pricing".into(),
                            viewport_width: 390,
                            viewport_height: 844,
                            kind: Interaction::Click {
                                x: 10,
                                page_y: 2000,
                            },
                        }),
                        2 => {
                            let count = 1 + rng.below(2) as usize;
                            Record::Identify(Identify {
                                id,
                                site,
                                event_time_ms,
                                receive_time_ms: NOW_MS,
                                local_day,
                                daily_id: Some(id ^ 7),
                                profile_id: Some(id ^ 9),
                                properties: properties(&mut rng, count),
                            })
                        }
                        _ => {
                            let kind = [
                                EventKind::Pageview,
                                EventKind::CustomEvent,
                                EventKind::Performance,
                            ][rng.below(3) as usize];
                            let property_count = if kind == EventKind::Performance {
                                0
                            } else {
                                rng.below(4) as usize
                            };
                            Record::Event(Event {
                                id,
                                site,
                                kind,
                                event_time_ms,
                                receive_time_ms: NOW_MS,
                                local_day,
                                daily_id: (!late).then_some(id ^ 7),
                                profile_id: None,
                                client_event_id: None,
                                dimensions: Dimensions {
                                    path: Some(format!("/p/{}", rng.below(50))),
                                    device: Some("mobile".into()),
                                    event_name: (kind == EventKind::CustomEvent)
                                        .then(|| "signup".into()),
                                    ..Dimensions::default()
                                },
                                performance: None,
                                revenue: (kind == EventKind::CustomEvent && rng.below(4) == 0)
                                    .then(|| Revenue {
                                        currency: "EUR".into(),
                                        minor_units: 1999,
                                    }),
                                properties: properties(&mut rng, property_count),
                            })
                        }
                    }
                })
                .collect();
            Request { records }
        })
        .collect()
}

fn properties(rng: &mut Rng, n: usize) -> Vec<Property> {
    (0..n)
        .map(|i| Property {
            key: format!("k{i}"),
            value: match rng.below(5) {
                0 => PropertyValue::String("pro".into()),
                1 => PropertyValue::Integer(rng.below(100) as i64),
                2 => PropertyValue::Decimal(-12_345),
                3 => PropertyValue::Null,
                _ => PropertyValue::Array(vec![
                    PropertyValue::Bool(true),
                    PropertyValue::String("t-01".into()),
                ]),
            },
        })
        .collect()
}

fn id_of(record: &Record) -> u128 {
    match record {
        Record::Event(e) => e.id,
        Record::Identify(i) => i.id,
        Record::ReplayFragment(f) => f.id,
        Record::HeatmapInteraction(h) => h.id,
        Record::Revocation(r) => r.id,
    }
}

fn property_rows(record: &Record) -> usize {
    match record {
        Record::Event(e) => e.properties.len(),
        Record::Identify(i) => i.properties.len(),
        _ => 0,
    }
}

fn log(path: &Path, line: &str) {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .expect("open log");
    writeln!(file, "{line}").expect("write log");
    file.sync_data().expect("sync log");
}

fn read_log(path: &Path) -> HashSet<usize> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(|line| line.parse().expect("log line"))
        .collect()
}

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let dir = Path::new(&args[2]);
    let seed: u64 = args[3].parse().expect("seed");
    match args[1].as_str() {
        "run" => run(dir, seed).await,
        "verify" => verify(dir, seed, &args[4]).await,
        other => panic!("unknown mode {other}"),
    }
}

async fn run(dir: &Path, seed: u64) {
    let storage = Storage::open(dir, budgets(seed)).await.expect("open");
    for (index, request) in requests(seed).into_iter().enumerate() {
        log(&dir.join("begun.log"), &index.to_string());
        let outcomes = storage.admit(&request.records).await.expect("admit");
        assert!(
            outcomes
                .iter()
                .all(|o| matches!(o, Outcome::Accepted { .. }))
        );
        log(&dir.join("acked.log"), &index.to_string());
    }
    storage.drain().await;
    storage.close().await.expect("close");
}

async fn verify(dir: &Path, seed: u64, boundary: &str) {
    let begun = read_log(&dir.join("begun.log"));
    let acked = read_log(&dir.join("acked.log"));
    let storage = Storage::open(dir, budgets(seed)).await.expect("reopen");
    storage.drain().await;
    let status = storage.status();
    let stored = storage
        .read(Lane::Report, |snapshot| {
            let conn = snapshot.connection();
            let mut ids: HashMap<u128, (i64, usize)> = HashMap::new();
            for query in [
                "SELECT id, min(seq), count(*) FROM events GROUP BY id",
                "SELECT id, min(seq), count(*) FROM replay_fragments GROUP BY id",
                "SELECT id, min(seq), count(*) FROM heatmap_interactions GROUP BY id",
                "SELECT id, min(seq), count(DISTINCT seq) FROM session_properties GROUP BY id",
            ] {
                let mut statement = conn.prepare(query)?;
                for row in statement.query_map([], |r| Ok((r.get::<_, u128>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?)))? {
                    let (id, seq, copies) = row?;
                    ids.insert(id, (seq, copies as usize));
                }
            }
            let mut props: HashMap<i64, usize> = HashMap::new();
            for table in ["event_properties", "session_properties"] {
                let mut statement = conn.prepare(&format!("SELECT seq, count(*) FROM {table} GROUP BY seq"))?;
                for row in statement.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))? {
                    let (seq, n) = row?;
                    *props.entry(seq).or_default() += n as usize;
                }
            }
            let mut ledger = Vec::new();
            let mut statement = conn.prepare("SELECT batch_id, first_seq, last_seq, records FROM applied_batches ORDER BY batch_id")?;
            for row in statement.query_map([], |r| Ok([r.get::<_, i64>(0)?, r.get(1)?, r.get(2)?, r.get(3)?]))? {
                ledger.push(row?);
            }
            let undirty: i64 = conn.query_row(
                "SELECT count(*) FROM (SELECT DISTINCT site, local_day FROM events EXCEPT SELECT site, local_day FROM dirty_days)",
                [],
                |r| r.get(0),
            )?;
            Ok((ids, props, ledger, undirty))
        })
        .await
        .expect("read");
    let (ids, props, ledger, undirty) = stored.value;

    let mut failures = Vec::new();
    for (index, request) in requests(seed).iter().enumerate() {
        let present: Vec<bool> = request
            .records
            .iter()
            .map(|r| ids.contains_key(&id_of(r)))
            .collect();
        let all = present.iter().all(|p| *p);
        let none = present.iter().all(|p| !*p);
        if !(all || none) {
            failures.push(format!("request {index} is partly stored"));
        }
        if acked.contains(&index) && !all {
            failures.push(format!("acknowledged request {index} lost records"));
        }
        if !begun.contains(&index) && !none {
            failures.push(format!("request {index} was never admitted but is stored"));
        }
        let in_flight = begun.contains(&index) && !acked.contains(&index);
        if in_flight && boundary == "admission.before_commit" && !none {
            failures.push(format!("request {index} killed before commit is stored"));
        }
        if in_flight && boundary == "admission.after_commit" && !all {
            failures.push(format!(
                "request {index} committed before the kill is missing"
            ));
        }
        for record in &request.records {
            if let Some((seq, copies)) = ids.get(&id_of(record)) {
                if *copies != 1 {
                    failures.push(format!("record {} stored {copies} times", id_of(record)));
                }
                if props.get(seq).copied().unwrap_or(0) != property_rows(record) {
                    failures.push(format!("record {} has wrong property rows", id_of(record)));
                }
            }
        }
    }
    let ledger_records: i64 = ledger.iter().map(|b| b[3]).sum();
    let seqs: BTreeSet<i64> = ids.values().map(|(seq, _)| *seq).collect();
    if ledger_records as usize != ids.len() || seqs.len() != ids.len() {
        failures.push(format!(
            "ledger counts {ledger_records} records, tables hold {} ids and {} seqs",
            ids.len(),
            seqs.len()
        ));
    }
    for pair in ledger.windows(2) {
        if pair[1][0] <= pair[0][0] || pair[1][1] <= pair[0][2] {
            failures.push(format!(
                "ledger entries overlap or go backwards: {:?} {:?}",
                pair[0], pair[1]
            ));
        }
    }
    if let Some(last) = ledger.last()
        && (status.watermark.batch, status.watermark.sequence) != (last[0], last[2])
    {
        failures.push(format!(
            "watermark {:?} differs from ledger {last:?}",
            status.watermark
        ));
    }
    if status.inbox_records != 0 {
        failures.push(format!(
            "{} records left in the inbox",
            status.inbox_records
        ));
    }
    if undirty != 0 {
        failures.push(format!(
            "{undirty} stored identity days missing from dirty_days"
        ));
    }
    storage.close().await.expect("close");
    if failures.is_empty() {
        println!(
            "ok begun={} acked={} records={} batches={}",
            begun.len(),
            acked.len(),
            ids.len(),
            ledger.len()
        );
    } else {
        eprintln!("{}", failures.join("\n"));
        std::process::exit(1);
    }
}
