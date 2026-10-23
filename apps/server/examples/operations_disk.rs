//! Disk growth of operations (performance validation, "Sustained load, backpressure,
//! maintenance, and restore"; milestone 12 work item 6): WAL growth during a load, checkpoint
//! and reclamation after retention, compaction of closed months, query spill, the inbox
//! reserve, backup temporary space, restore time, and cleanup of an interrupted backup. One
//! data directory through the real engines with the reference budgets.
//!
//! `cargo run --release -p holocron-server --example operations_disk -- [events] [dir]`
//! (default 1,000,000 events over 13 months, in a temporary directory). Prints one JSON
//! summary. Backups go to `<dir>/backups`, which stands in for the separate backup volume.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use holocron_server::clock::Clock;
use holocron_server::init::{self, InitArgs};
use holocron_server::maintenance::jobs::{self, Job};
use holocron_server::maintenance::{Runtime, compaction, restore};
use holocron_storage::{
    Budgets, Dimensions, Event, EventKind, Lane, Property, PropertyValue, Record,
};
use serde_json::{Value, json};

const SITE: &str = "0d150000-0000-4000-8000-000000000001";
const DAY_MS: i64 = 86_400_000;
/// 2026-10-01T00:00:00Z, the end of the loaded range.
const END_MS: i64 = 1_790_812_800_000;
const RANGE_DAYS: i64 = 396;
const PER_ADMIT: usize = 1_000;
/// Engine memory for the spill phase, small enough that a sort of every event spills.
const SPILL_MEMORY_MIB: u64 = 16;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let events: usize = args.get(1).map_or(1_000_000, |v| v.parse().unwrap());
    let temp = tempfile::tempdir().unwrap();
    let root = args
        .get(2)
        .map_or_else(|| temp.path().to_path_buf(), PathBuf::from);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    let summary = runtime.block_on(run(&root, events));
    println!("{}", serde_json::to_string_pretty(&summary).unwrap());
}

fn budgets() -> Budgets {
    Budgets {
        disk_reserve_bytes: 0,
        ..Budgets::default()
    }
}

fn size(path: &Path) -> u64 {
    match std::fs::metadata(path) {
        Ok(meta) if meta.is_dir() => std::fs::read_dir(path)
            .map(|entries| {
                entries
                    .filter_map(|e| e.ok())
                    .map(|e| size(&e.path()))
                    .sum()
            })
            .unwrap_or(0),
        Ok(meta) => meta.len(),
        Err(_) => 0,
    }
}

/// Samples the largest size `path` reaches until stopped.
fn peak(path: PathBuf) -> (Arc<AtomicBool>, Arc<AtomicU64>, std::thread::JoinHandle<()>) {
    let (stop, max) = (
        Arc::new(AtomicBool::new(false)),
        Arc::new(AtomicU64::new(0)),
    );
    let (s, m) = (stop.clone(), max.clone());
    let handle = std::thread::spawn(move || {
        while !s.load(Ordering::Relaxed) {
            m.fetch_max(size(&path), Ordering::Relaxed);
            std::thread::sleep(Duration::from_millis(50));
        }
    });
    (stop, max, handle)
}

fn event(n: usize, total: usize) -> Record {
    let time = END_MS - RANGE_DAYS * DAY_MS + (n as i64 * RANGE_DAYS * DAY_MS / total as i64);
    let id = n as u128 + 1;
    Record::Event(Event {
        id,
        site: uuid::Uuid::parse_str(SITE).unwrap().as_u128(),
        kind: if n.is_multiple_of(5) {
            EventKind::CustomEvent
        } else {
            EventKind::Pageview
        },
        event_time_ms: time,
        receive_time_ms: time,
        local_day: time.div_euclid(DAY_MS) as i32,
        daily_id: Some(id % 50_000 + (time.div_euclid(DAY_MS) as u128) * 100_000),
        profile_id: None,
        client_event_id: None,
        dimensions: Dimensions {
            hostname: Some("disk.test".into()),
            path: Some(format!("/docs/{}", n % 400)),
            referrer_domain: Some(format!("ref{}.example", n % 30)),
            browser: Some(["chrome", "firefox", "safari"][n % 3].into()),
            os: Some(["mac", "windows", "linux", "ios"][n % 4].into()),
            device: Some(["desktop", "mobile"][n % 2].into()),
            country: Some(["AT", "DE", "US", "FR", "IT"][n % 5].into()),
            event_name: (n.is_multiple_of(5)).then(|| "signup".into()),
            ..Dimensions::default()
        },
        performance: None,
        revenue: None,
        properties: if n.is_multiple_of(5) {
            vec![Property {
                key: "plan".into(),
                value: PropertyValue::String(["free", "pro", "team"][n % 3].into()),
            }]
        } else {
            Vec::new()
        },
    })
}

async fn run(root: &Path, total: usize) -> Value {
    let data = root.join("data");
    let backups = root.join("backups");
    init::init(InitArgs {
        data_dir: data.clone(),
        admin_email: "admin@disk.test".into(),
        admin_password: Some("correct horse battery".into()),
    })
    .await
    .unwrap();
    {
        let db = holocron_server::db::Db::open(&data).await.unwrap();
        sqlx::query(
            "INSERT INTO sites (id, workspace_id, name, domain, created_at)
             SELECT ?, id, 'disk', 'disk.test', 0 FROM workspaces;
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

    // Load: the WAL files and the inbox at their largest.
    let clock = Clock::frozen(END_MS);
    let rt = Runtime::open(Runtime::config(&data), budgets(), clock.clone())
        .await
        .unwrap();
    let storage = rt.state.collector.storage();
    let samplers = ["analytics.duckdb.wal", "inbox.sqlite", "inbox.sqlite-wal"]
        .map(|file| peak(data.join(file)));
    let started = Instant::now();
    let (mut backlog_peak, mut encoded_per_event) = (0u64, 0.0);
    for first in (0..total).step_by(PER_ADMIT) {
        let records: Vec<Record> = (first..total.min(first + PER_ADMIT))
            .map(|n| event(n, total))
            .collect();
        while storage.admit(&records).await.is_err() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let status = storage.status();
        backlog_peak = backlog_peak.max(status.inbox_bytes);
        if status.inbox_records > 0 {
            encoded_per_event = status.inbox_bytes as f64 / status.inbox_records as f64;
        }
    }
    storage.drain().await;
    let load_s = started.elapsed().as_secs_f64();
    let peaks: Vec<u64> = samplers
        .into_iter()
        .map(|(stop, max, handle)| {
            stop.store(true, Ordering::Relaxed);
            handle.join().unwrap();
            max.load(Ordering::Relaxed)
        })
        .collect();
    let after_load = storage.disk().unwrap();

    // Retention: one month later, the oldest month goes; DuckDB keeps the file size and
    // reuses the freed blocks.
    clock.set(END_MS + 31 * DAY_MS);
    let database_size = || async {
        storage
            .read(Lane::Live, |s| {
                s.connection().query_row(
                    "SELECT used_blocks, free_blocks, block_size FROM pragma_database_size()",
                    [],
                    |r| {
                        Ok((
                            r.get::<_, i64>(0)?,
                            r.get::<_, i64>(1)?,
                            r.get::<_, i64>(2)?,
                        ))
                    },
                )
            })
            .await
            .unwrap()
            .value
    };
    let before_retention = database_size().await;
    let id = jobs::enqueue(&rt.state.db.writer, &Job::Retention, clock.now_ms())
        .await
        .unwrap();
    let started = Instant::now();
    let retention = jobs::run_until(&rt.state, &id).await.unwrap();
    let retention_s = started.elapsed().as_secs_f64();
    let wal_after_retention = storage.disk().unwrap().analytics_wal_bytes;
    let after_retention = database_size().await;

    // Compaction: every closed month clustered by site, one slice per step.
    let (stop, max, handle) = peak(data.join("analytics.duckdb.wal"));
    let started = Instant::now();
    let (mut steps, mut slowest_step_ms, mut compacted) = (0, 0u128, 0);
    loop {
        let step_started = Instant::now();
        let Some(step) = storage
            .compact(clock.now_ms(), compaction::STEP_EVENTS)
            .await
            .unwrap()
        else {
            break;
        };
        slowest_step_ms = slowest_step_ms.max(step_started.elapsed().as_millis());
        steps += 1;
        compacted += step.events;
    }
    let compaction_s = started.elapsed().as_secs_f64();
    stop.store(true, Ordering::Relaxed);
    handle.join().unwrap();
    let compaction_wal_peak = max.load(Ordering::Relaxed);
    let after_compaction = database_size().await;

    // Spill: a sort of every event under a 16 MiB engine limit.
    rt.close().await.unwrap();
    let after_checkpoint = (
        size(&data.join("analytics.duckdb")),
        size(&data.join("analytics.duckdb.wal")),
    );
    let small = Budgets {
        engine_memory_bytes: SPILL_MEMORY_MIB * 1024 * 1024,
        ..budgets()
    };
    let rt = Runtime::open(Runtime::config(&data), small, clock.clone())
        .await
        .unwrap();
    let (stop, max, handle) = peak(data.join("spill"));
    let started = Instant::now();
    let sorted = rt
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
        .await;
    let spill_s = started.elapsed().as_secs_f64();
    stop.store(true, Ordering::Relaxed);
    handle.join().unwrap();
    let spill_peak = max.load(Ordering::Relaxed);
    let spill_after = size(&data.join("spill"));

    // Backup on the serving installation, after an interrupted one left its directory.
    std::fs::create_dir_all(backups.join(".holocron-backup-interrupted.partial")).unwrap();
    std::fs::write(
        backups.join(".holocron-backup-interrupted.partial/analytics.duckdb"),
        vec![0u8; 8 * 1024 * 1024],
    )
    .unwrap();
    let interrupted_bytes = size(&backups);
    let id = jobs::enqueue(
        &rt.state.db.writer,
        &Job::Backup {
            to: backups.clone(),
        },
        clock.now_ms(),
    )
    .await
    .unwrap();
    let started = Instant::now();
    let backup = jobs::run_until(&rt.state, &id).await.unwrap();
    let backup_s = started.elapsed().as_secs_f64();
    let leftover = std::fs::read_dir(&backups)
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".partial")
        })
        .count();
    rt.close().await.unwrap();

    let started = Instant::now();
    let restored = restore::restore(
        Path::new(backup["path"].as_str().unwrap()),
        &root.join("restored"),
    )
    .await
    .unwrap();
    let restore_s = started.elapsed().as_secs_f64();

    json!({
        "events": total,
        "load_s": load_s,
        "encoded_bytes_per_event": encoded_per_event,
        "peak_inbox_backlog_bytes": backlog_peak,
        "peak_bytes": {
            "analytics_wal": peaks[0],
            "inbox": peaks[1],
            "inbox_wal": peaks[2],
        },
        "after_load": after_load,
        "retention": {
            "result": retention,
            "seconds": retention_s,
            "database_blocks_before": before_retention,
            "database_blocks_after": after_retention,
            "analytics_wal_after": wal_after_retention,
            "after_checkpoint_bytes": { "analytics": after_checkpoint.0, "analytics_wal": after_checkpoint.1 },
        },
        "compaction": {
            "step_events": compaction::STEP_EVENTS,
            "steps": steps,
            "events_moved": compacted,
            "seconds": compaction_s,
            "slowest_step_ms": slowest_step_ms,
            "analytics_wal_peak": compaction_wal_peak,
            "database_blocks_after": after_compaction,
        },
        "spill": {
            "engine_memory_mib": SPILL_MEMORY_MIB,
            "query_ok": sorted.is_ok(),
            "seconds": spill_s,
            "peak_bytes": spill_peak,
            "after_bytes": spill_after,
        },
        "backup": {
            "summary": backup,
            "seconds": backup_s,
            "interrupted_partial_bytes_before": interrupted_bytes,
            "partial_dirs_left": leftover,
            "backup_dir_bytes": size(&backups),
        },
        "restore": { "seconds": restore_s, "result": restored },
        "data_dir_bytes": size(&data),
    })
}
