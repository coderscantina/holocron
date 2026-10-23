//! Real ENOSPC on a small disposable volume (milestone 12 open item): fills the volume with a
//! file until writes fail, then drives one engine path at a time and frees the space again.
//! Every scenario checks that each acknowledged event is still there after recovery and after
//! a restart.
//!
//! ```sh
//! hdiutil create -size 512m -fs HFS+ -volname hc-enospc /tmp/hc-enospc.dmg
//! hdiutil attach /tmp/hc-enospc.dmg
//! cargo run --release -p holocron-server --example enospc -- /Volumes/hc-enospc [scenario]
//! hdiutil detach /Volumes/hc-enospc && rm /tmp/hc-enospc.dmg
//! ```
//!
//! Scenarios: `inbox`, `transfer`, `transfer-duckdb`, `spill`, `replay`, `backup`, `retention`;
//! all of them without one. HFS+ fills to the last block; APFS still accepts some small writes
//! after a file write fails, so it injects ENOSPC less reliably. Prints one JSON line each.

use std::io::Write;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, header};
use holocron_server::clock::Clock;
use holocron_server::init::{self, InitArgs};
use holocron_server::maintenance::jobs::{self, Job};
use holocron_server::maintenance::{Runtime, health};
use holocron_server::recordings::replay::Summary;
use holocron_server::recordings::store::{Client, Part};
use holocron_server::router;
use holocron_storage::{
    Budgets, Dimensions, Event, EventKind, Lane, Property, PropertyValue, Record,
};
use serde_json::{Value, json};
use tower::ServiceExt;

const SITE: &str = "0e450000-0000-4000-8000-0000000000c1";
const HOST: &str = "enospc.test";
const DAY_MS: i64 = 86_400_000;

fn main() {
    let volume = PathBuf::from(std::env::args().nth(1).expect("volume path"));
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let only = std::env::args().nth(2);
        let run = |name: &str| only.as_deref().is_none_or(|o| o == name);
        if run("inbox") {
            report("inbox", inbox(&volume).await);
        }
        if run("transfer") {
            report("transfer", transfer(&volume, false).await);
        }
        if run("transfer-duckdb") {
            report("transfer-duckdb", transfer(&volume, true).await);
        }
        if run("spill") {
            report("spill", spill(&volume).await);
        }
        if run("replay") {
            report("replay", replay(&volume).await);
        }
        if run("backup") {
            report("backup", backup(&volume).await);
        }
        if run("retention") {
            report("retention", retention(&volume).await);
        }
    });
}

fn report(name: &str, value: Value) {
    println!("{}", json!({ "scenario": name, "result": value }));
}

fn budgets() -> Budgets {
    Budgets {
        disk_reserve_bytes: 0,
        ..Budgets::default()
    }
}

/// Writes a file until the volume refuses more. Returns the bytes written.
fn fill(volume: &Path) -> u64 {
    let mut file = std::fs::File::create(volume.join("filler")).unwrap();
    let block: Vec<u8> = (0..1 << 20)
        .map(|i: u32| (i.wrapping_mul(2_654_435_761) >> 13) as u8)
        .collect();
    let mut written = 0;
    for size in [1 << 20, 4096, 512] {
        loop {
            match file.write_all(&block[..size]) {
                Ok(()) => written += size as u64,
                Err(e) if e.raw_os_error() == Some(libc::ENOSPC) => break,
                Err(e) => panic!("filling: {e}"),
            }
        }
    }
    let _ = file.sync_all();
    written
}

fn free(volume: &Path) {
    std::fs::remove_file(volume.join("filler")).unwrap();
}

async fn initialized(dir: &Path) {
    let _ = std::fs::remove_dir_all(dir);
    init::init(InitArgs {
        data_dir: dir.to_path_buf(),
        admin_email: "admin@enospc.test".into(),
        admin_password: Some("correct horse battery".into()),
    })
    .await
    .unwrap();
    let db = holocron_server::db::Db::open(dir).await.unwrap();
    sqlx::query(
        "INSERT INTO sites (id, workspace_id, name, domain, created_at)
         SELECT ?, id, 'enospc', 'enospc.test', 0 FROM workspaces;
         INSERT INTO session_policies (site_id, version, epoch, effective_at, timezone, window_minutes, created_at)
         VALUES (?, 1, 1, -9223372036854775808, 'UTC', 30, 0);",
    )
    .bind(SITE)
    .bind(SITE)
    .execute(&db.writer)
    .await
    .unwrap();
    db.close().await;
}

async fn open(dir: &Path, budgets: Budgets, clock: Clock) -> Runtime {
    Runtime::open(Runtime::config(dir), budgets, clock)
        .await
        .unwrap()
}

/// Sends a pageview through the tracker endpoint. The HTTP status.
async fn send(runtime: &Runtime, path: &str) -> u16 {
    let body =
        json!({ "type": "event", "payload": { "website": SITE, "hostname": HOST, "url": path } });
    let request = Request::post("/api/send")
        .header(header::HOST, HOST)
        .header(
            header::USER_AGENT,
            "Mozilla/5.0 (Macintosh) Chrome/140.0.0.0 Safari/537.36",
        )
        .header(header::CONTENT_TYPE, "application/json")
        .extension(axum::extract::ConnectInfo(
            "198.51.100.9:40000".parse::<SocketAddr>().unwrap(),
        ))
        .body(Body::from(body.to_string()))
        .unwrap();
    router(runtime.state.clone())
        .oneshot(request)
        .await
        .unwrap()
        .status()
        .as_u16()
}

/// Sends `n` pageviews and returns the paths acknowledged with 200 and the other statuses.
async fn send_many(runtime: &Runtime, prefix: &str, n: usize) -> (Vec<String>, Vec<u16>) {
    let (mut acked, mut other) = (Vec::new(), Vec::new());
    for i in 0..n {
        let path = format!("/{prefix}/{i}");
        match send(runtime, &path).await {
            200 => acked.push(path),
            status => other.push(status),
        }
    }
    (acked, other)
}

async fn health(runtime: &Runtime) -> Value {
    let health = health::check(&runtime.state).await;
    json!({ "ready": health.ready, "problems": health.problems })
}

/// Paths of every event in analytics.
async fn stored_paths(runtime: &Runtime) -> Vec<String> {
    runtime
        .state
        .collector
        .storage()
        .read(Lane::Report, |s| {
            s.connection()
                .prepare("SELECT path FROM events WHERE path IS NOT NULL ORDER BY path")?
                .query_map([], |r| r.get(0))?
                .collect()
        })
        .await
        .unwrap()
        .value
}

/// Waits up to `limit` for the inbox to drain.
async fn drained(runtime: &Runtime, limit: Duration) -> bool {
    tokio::time::timeout(limit, runtime.state.collector.storage().drain())
        .await
        .is_ok()
}

/// Every acknowledged path is in analytics, now and after a restart.
async fn none_lost(dir: &Path, runtime: Runtime, acked: &[String]) -> Value {
    let present = |paths: Vec<String>| acked.iter().filter(|p| !paths.contains(p)).count();
    let drained_before = drained(&runtime, Duration::from_secs(30)).await;
    let missing_before = present(stored_paths(&runtime).await);
    let closed = runtime.close().await.map_err(|f| f.message.to_string());
    let runtime = open(dir, budgets(), Clock::system()).await;
    let drained_after_restart = drained(&runtime, Duration::from_secs(30)).await;
    let missing_after = present(stored_paths(&runtime).await);
    runtime.close().await.unwrap();
    json!({ "acked": acked.len(), "drained": drained_before, "missing": missing_before, "close": closed,
            "drained_after_restart": drained_after_restart, "missing_after_restart": missing_after })
}

/// Inbox commit on a full volume: collection refuses, then accepts again once space is free.
async fn inbox(volume: &Path) -> Value {
    let dir = volume.join("inbox");
    initialized(&dir).await;
    let runtime = open(&dir, budgets(), Clock::system()).await;
    let (mut acked, _) = send_many(&runtime, "before", 20).await;
    drained(&runtime, Duration::from_secs(10)).await;
    let filled = fill(volume);
    let (during, refused) = send_many(&runtime, "full", 50).await;
    acked.extend(during.clone());
    let health_full = health(&runtime).await;
    free(volume);
    tokio::time::sleep(Duration::from_secs(2)).await;
    let (after, refused_after) = send_many(&runtime, "after", 20).await;
    acked.extend(after.clone());
    let drained_ok = drained(&runtime, Duration::from_secs(30)).await;
    let health_after = health(&runtime).await;
    json!({
        "filled_bytes": filled,
        "while_full": { "acked": during.len(), "refused_statuses": refused, "health": health_full },
        "after_free": { "acked": after.len(), "refused": refused_after.len(), "drained": drained_ok, "health": health_after },
        "loss": none_lost(&dir, runtime, &acked).await,
    })
}

/// Transfer on a full volume: 20,000 admitted events wait in the inbox, then reach analytics.
/// With `inbox_elsewhere` the inbox lives on another volume through a symlink, so only DuckDB
/// runs out of space.
async fn transfer(volume: &Path, inbox_elsewhere: bool) -> Value {
    let dir = volume.join(if inbox_elsewhere {
        "transfer-duckdb"
    } else {
        "transfer"
    });
    initialized(&dir).await;
    let elsewhere = tempfile::tempdir().unwrap();
    if inbox_elsewhere {
        let inbox = elsewhere.path().join("inbox.sqlite");
        std::fs::copy(dir.join("inbox.sqlite"), &inbox).unwrap();
        std::fs::remove_file(dir.join("inbox.sqlite")).unwrap();
        std::os::unix::fs::symlink(&inbox, dir.join("inbox.sqlite")).unwrap();
    }
    let held = Budgets {
        transfer_delay: Duration::from_secs(20),
        ..budgets()
    };
    let runtime = open(&dir, held, Clock::system()).await;
    let storage = runtime.state.collector.storage();
    let now = holocron_server::db::now_ms();
    let records: Vec<Record> = (0..20_000).map(|n| event(n, now + n as i64)).collect();
    let mut acked = Vec::new();
    for chunk in records.chunks(1_000) {
        storage.admit(chunk).await.unwrap();
        acked.extend(chunk.iter().filter_map(|r| match r {
            Record::Event(e) => e.dimensions.path.clone(),
            _ => None,
        }));
    }
    let waiting = storage.status().inbox_records;
    let filled = fill(volume);
    tokio::time::sleep(Duration::from_secs(25)).await;
    let status = storage.status();
    let health_full = health(&runtime).await;
    free(volume);
    let drained_ok = drained(&runtime, Duration::from_secs(60)).await;
    let status_after = storage.status();
    let (later, _) = send_many(&runtime, "later", 5).await;
    acked.extend(later);
    json!({
        "filled_bytes": filled,
        "waiting_before_fill": waiting,
        "while_full": { "inbox_records": status.inbox_records, "transfer_failures": status.transfer_failures, "health": health_full },
        "after_free": { "drained": drained_ok, "failing": status_after.transfer_failing_since_ms.is_some(),
                        "health": health(&runtime).await },
        "loss": none_lost(&dir, runtime, &acked).await,
    })
}

fn event(n: usize, at_ms: i64) -> Record {
    let id = n as u128 + 1;
    Record::Event(Event {
        id,
        site: uuid::Uuid::parse_str(SITE).unwrap().as_u128(),
        kind: EventKind::Pageview,
        event_time_ms: at_ms,
        receive_time_ms: at_ms,
        local_day: at_ms.div_euclid(DAY_MS) as i32,
        daily_id: Some(id % 5_000),
        profile_id: None,
        client_event_id: None,
        dimensions: Dimensions {
            hostname: Some(HOST.into()),
            path: Some(format!("/load/{n:07}")),
            referrer_domain: Some(format!("ref{}.example", n % 30)),
            ..Dimensions::default()
        },
        performance: None,
        revenue: None,
        properties: vec![Property {
            key: "plan".into(),
            value: PropertyValue::String(format!("plan-{}", n % 7)),
        }],
    })
}

async fn load(runtime: &Runtime, total: usize, at_ms: impl Fn(usize) -> i64) {
    let storage = runtime.state.collector.storage();
    for first in (0..total).step_by(1_000) {
        let records: Vec<Record> = (first..total.min(first + 1_000))
            .map(|n| event(n, at_ms(n)))
            .collect();
        while storage.admit(&records).await.is_err() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
    storage.drain().await;
}

/// A query that spills on a full volume: it fails, the spill directory is cleaned, and the
/// same query runs once space is free.
async fn spill(volume: &Path) -> Value {
    let dir = volume.join("spill");
    initialized(&dir).await;
    let runtime = open(&dir, budgets(), Clock::system()).await;
    let now = holocron_server::db::now_ms();
    load(&runtime, 300_000, |n| now - DAY_MS + n as i64).await;
    runtime.close().await.unwrap();
    let small = Budgets {
        engine_memory_bytes: 24 * 1024 * 1024,
        ..budgets()
    };
    let runtime = open(&dir, small, Clock::system()).await;
    let sort = || async {
        runtime
            .state
            .collector
            .storage()
            .read(Lane::Report, |s| {
                s.connection().query_row(
                    "SELECT max(r) FROM (SELECT row_number() OVER (ORDER BY hash(id)) AS r, * FROM events)",
                    [],
                    |r| r.get::<_, i64>(0),
                )
            })
            .await
            .map(|read| read.value)
            .map_err(|e| e.to_string())
    };
    let spill_dir = dir.join("spill");
    let baseline = sort().await;
    let filled = fill(volume);
    let full = sort().await;
    let spill_left = std::fs::read_dir(&spill_dir).unwrap().count();
    let (acked, refused) = send_many(&runtime, "during-spill", 5).await;
    free(volume);
    let after = sort().await;
    json!({
        "filled_bytes": filled,
        "baseline": format!("{baseline:?}"),
        "while_full": { "query": format!("{full:?}"), "spill_files_left": spill_left,
                        "send_acked": acked.len(), "send_refused": refused },
        "after_free": format!("{after:?}"),
        "loss": none_lost(&dir, runtime, &acked).await,
    })
}

/// A replay part on a full volume: refused with an error, stored once space is free.
async fn replay(volume: &Path) -> Value {
    let dir = volume.join("replay");
    initialized(&dir).await;
    let runtime = open(&dir, budgets(), Clock::system()).await;
    let store = &runtime.state.recordings.store;
    let summary = Summary {
        events: 1,
        first_ms: Some(1),
        last_ms: Some(2),
        pages: vec!["/".into()],
    };
    let client = Client {
        browser: None,
        os: None,
        device: None,
        country: None,
    };
    let part = |chunk: i64| Part {
        site: uuid::Uuid::parse_str(SITE).unwrap().as_u128(),
        capture: 42,
        visitor: None,
        chunk,
        received_at: holocron_server::db::now_ms(),
        summary: &summary,
        raw_bytes: 256 * 1024,
        payload: (0..256 * 1024)
            .map(|i: u32| (i.wrapping_mul(2_654_435_761) >> 11) as u8)
            .collect(),
        fingerprint: [chunk as u8; 16],
        client: &client,
    };
    let first = store
        .put(part(1), &[], u64::MAX)
        .await
        .map_err(|e| e.to_string());
    let filled = fill(volume);
    let mut full = Vec::new();
    for chunk in 2..22 {
        full.push(format!(
            "{:?}",
            store
                .put(part(chunk), &[], u64::MAX)
                .await
                .map_err(|e| e.to_string())
        ));
    }
    free(volume);
    let after = store
        .put(part(6), &[], u64::MAX)
        .await
        .map_err(|e| e.to_string());
    let usage = store.usage().await.unwrap();
    runtime.close().await.unwrap();
    let pool = sqlx::SqlitePool::connect(&format!(
        "sqlite://{}",
        dir.join("recordings.sqlite").display()
    ))
    .await
    .unwrap();
    let integrity: String = sqlx::query_scalar("PRAGMA integrity_check")
        .fetch_one(&pool)
        .await
        .unwrap();
    let chunks: i64 = sqlx::query_scalar("SELECT count(*) FROM chunks")
        .fetch_one(&pool)
        .await
        .unwrap();
    pool.close().await;
    json!({
        "filled_bytes": filled,
        "before": format!("{first:?}"),
        "while_full": full,
        "after_free": format!("{after:?}"),
        "captures": usage.captures,
        "chunks": chunks,
        "integrity": integrity,
    })
}

/// A backup whose destination volume is full, from a serving installation elsewhere.
async fn backup(volume: &Path) -> Value {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("data");
    initialized(&dir).await;
    let runtime = open(&dir, budgets(), Clock::system()).await;
    load(&runtime, 100_000, |n| {
        holocron_server::db::now_ms() - DAY_MS + n as i64
    })
    .await;
    let (mut acked, _) = send_many(&runtime, "before-backup", 5).await;
    let to = volume.join("backups");
    let filled = fill(volume);
    let run = |to: PathBuf| async {
        let state = &runtime.state;
        let id = jobs::enqueue(
            &state.db.writer,
            &Job::Backup { to },
            state.collector.clock().now_ms(),
        )
        .await
        .unwrap();
        jobs::run_until(state, &id)
            .await
            .map(|v| v["path"].clone())
            .map_err(|f| format!("{}: {}", f.code, f.message))
    };
    let full = run(to.clone()).await;
    let partials_left = list(&to);
    let (during, _) = send_many(&runtime, "after-failed-backup", 5).await;
    acked.extend(during.clone());
    free(volume);
    let after = run(to.clone()).await;
    let left_after = list(&to);
    json!({
        "filled_bytes": filled,
        "while_full": { "backup": full, "destination_entries": partials_left, "collection_acked": during.len() },
        "after_free": { "backup": after, "destination_entries": left_after },
        "loss": none_lost(&dir, runtime, &acked).await,
    })
}

fn list(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(|e| Some(e.ok()?.file_name().to_string_lossy().into_owned()))
                .collect()
        })
        .unwrap_or_default()
}

/// Retention on a full volume: the job fails and leaves the data as it was; the next run
/// removes what is past the cutoff.
async fn retention(volume: &Path) -> Value {
    let dir = volume.join("retention");
    initialized(&dir).await;
    let now = holocron_server::db::now_ms();
    let runtime = open(&dir, budgets(), Clock::system()).await;
    // Half the events are 14 months old.
    load(&runtime, 100_000, |n| {
        (if n % 2 == 0 {
            now - 425 * DAY_MS
        } else {
            now - DAY_MS
        }) + n as i64
    })
    .await;
    let (acked, _) = send_many(&runtime, "recent", 5).await;
    drained(&runtime, Duration::from_secs(10)).await;
    let count = || async {
        runtime
            .state
            .collector
            .storage()
            .read(Lane::Report, |s| {
                s.connection()
                    .query_row("SELECT count(*) FROM events", [], |r| r.get::<_, i64>(0))
            })
            .await
            .map(|r| r.value)
            .map_err(|e| e.to_string())
    };
    let run = || async {
        let state = &runtime.state;
        let id = jobs::enqueue(
            &state.db.writer,
            &Job::Retention,
            state.collector.clock().now_ms(),
        )
        .await
        .map_err(|e| e.to_string())?;
        jobs::run_until(state, &id)
            .await
            .map(|v| v["events_removed"].clone())
            .map_err(|f| format!("{}: {}", f.code, f.message))
    };
    let before = count().await;
    let filled = fill(volume);
    let full = tokio::time::timeout(Duration::from_secs(60), run()).await;
    let during = count().await;
    let (acked_during, refused) = send_many(&runtime, "during-retention", 5).await;
    free(volume);
    let after = run().await;
    let remaining = count().await;
    let mut all = acked.clone();
    all.extend(acked_during.clone());
    json!({
        "filled_bytes": filled,
        "events_before": before,
        "while_full": { "retention": format!("{full:?}"), "events": during, "send_acked": acked_during.len(), "send_refused": refused },
        "after_free": { "retention": after, "events": remaining },
        "loss": none_lost(&dir, runtime, &all).await,
    })
}
