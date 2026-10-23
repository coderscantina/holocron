//! Erasure fences, pair checks, snapshots, shutdown and upgrade through the real engines,
//! across closes and reopens of the same files (decision 0014).

use std::path::Path;
use std::time::Duration;

use holocron_storage::{
    Budgets, Dimensions, Erasure, Error, Event, EventKind, Identify, Lane, Property, PropertyValue,
    Record, Refusal, Storage,
};

const SITE: u128 = 0xa1;
const OTHER: u128 = 0xb2;
const PROFILE: u128 = 0x9e;
const NOW_MS: i64 = 1_790_812_800_000;
const DAY: i32 = (NOW_MS / 86_400_000) as i32;

fn budgets() -> Budgets {
    Budgets {
        transfer_delay: Duration::from_millis(5),
        engine_threads: 1,
        engine_memory_bytes: 256 * 1024 * 1024,
        disk_reserve_bytes: 0,
        ..Budgets::default()
    }
}

/// Admitted records wait in the inbox for a minute before transfer picks them up.
fn holding() -> Budgets {
    Budgets {
        transfer_delay: Duration::from_secs(60),
        ..budgets()
    }
}

fn event(id: u128, site: u128, profile: Option<u128>) -> Record {
    Record::Event(Event {
        id,
        site,
        kind: EventKind::Pageview,
        event_time_ms: NOW_MS,
        receive_time_ms: NOW_MS,
        local_day: DAY,
        daily_id: Some(id + 1000),
        profile_id: profile,
        client_event_id: None,
        dimensions: Dimensions {
            path: Some("/".into()),
            ..Dimensions::default()
        },
        performance: None,
        revenue: None,
        properties: vec![Property {
            key: "plan".into(),
            value: PropertyValue::String("pro".into()),
        }],
    })
}

fn identify(id: u128, site: u128, profile: u128) -> Record {
    Record::Identify(Identify {
        id,
        site,
        event_time_ms: NOW_MS,
        receive_time_ms: NOW_MS,
        local_day: DAY,
        daily_id: Some(id + 1000),
        profile_id: Some(profile),
        properties: vec![Property {
            key: "tier".into(),
            value: PropertyValue::String("team".into()),
        }],
    })
}

async fn count(storage: &Storage, sql: String) -> i64 {
    storage
        .read(Lane::Report, move |s| {
            s.connection().query_row(&sql, [], |r| r.get(0))
        })
        .await
        .unwrap()
        .value
}

/// Events, event properties and identifies stored for `site`.
async fn rows(storage: &Storage, site: u128) -> [i64; 3] {
    let mut out = [0; 3];
    for (i, table) in ["events", "event_properties", "session_properties"]
        .iter()
        .enumerate()
    {
        out[i] = count(
            storage,
            format!("SELECT count(*) FROM {table} WHERE site = {site}"),
        )
        .await;
    }
    out
}

/// Opens with [`holding`] once transfer found the inbox empty, so the next admitted records
/// stay pending.
async fn open_holding(dir: &Path) -> Storage {
    let storage = Storage::open(dir, holding()).await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    storage
}

/// Admits `records` and lets transfer apply them.
async fn applied(dir: &Path, records: &[Record]) {
    let storage = Storage::open(dir, budgets()).await.unwrap();
    storage.admit(records).await.unwrap();
    storage.drain().await;
    storage.close().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn site_deletion_removes_applied_and_pending_records_and_survives_restart() {
    let dir = tempfile::tempdir().unwrap();
    applied(dir.path(), &[event(1, SITE, None), event(2, OTHER, None)]).await;

    // A record still waiting in the inbox when the site is deleted, and one that a request
    // validated before the deletion admits after it.
    let storage = open_holding(dir.path()).await;
    storage.admit(&[event(3, SITE, None)]).await.unwrap();
    let fence = storage
        .fence("job-1", Erasure::Site(SITE), None)
        .await
        .unwrap();
    storage.admit(&[event(4, SITE, None)]).await.unwrap();
    let erased = storage.erase(fence).await.unwrap();
    assert_eq!(erased.events, 1);
    assert_eq!(storage.status().inbox_records, 2);
    storage.close().await.unwrap();

    let storage = Storage::open(dir.path(), budgets()).await.unwrap();
    assert_eq!(
        storage
            .fence("job-1", Erasure::Site(SITE), None)
            .await
            .unwrap(),
        fence
    );
    storage.drain().await;
    assert_eq!(rows(&storage, SITE).await, [0, 0, 0]);
    assert_eq!(rows(&storage, OTHER).await, [1, 1, 0]);
    assert_eq!(
        count(
            &storage,
            format!("SELECT count(*) FROM dirty_days WHERE site = {SITE}")
        )
        .await,
        0
    );
    storage.close().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn reset_keeps_records_admitted_after_the_fence() {
    let dir = tempfile::tempdir().unwrap();
    applied(dir.path(), &[event(1, SITE, None)]).await;

    let storage = Storage::open(dir.path(), budgets()).await.unwrap();
    // The reset was asked for before the second event; its job fences later.
    let asked = storage.last_sequence().await.unwrap();
    storage.admit(&[event(2, SITE, None)]).await.unwrap();
    let fence = storage
        .fence("job-2", Erasure::SiteHistory(SITE), Some(asked))
        .await
        .unwrap();
    storage.drain().await;
    storage.erase(fence).await.unwrap();
    assert_eq!(rows(&storage, SITE).await, [1, 1, 0]);
    // The day that keeps an event is rebuilt by correction.
    assert_eq!(
        count(
            &storage,
            format!("SELECT count(*) FROM dirty_days WHERE site = {SITE}")
        )
        .await,
        1
    );
    storage.close().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn profile_erasure_removes_only_that_profile() {
    let dir = tempfile::tempdir().unwrap();
    applied(
        dir.path(),
        &[
            event(1, SITE, Some(PROFILE)),
            identify(2, SITE, PROFILE),
            event(3, SITE, Some(PROFILE + 1)),
            event(4, SITE, None),
        ],
    )
    .await;

    let storage = open_holding(dir.path()).await;
    storage
        .admit(&[event(5, SITE, Some(PROFILE))])
        .await
        .unwrap();
    let fence = storage
        .fence(
            "job-3",
            Erasure::Profile {
                site: SITE,
                profile: PROFILE,
            },
            None,
        )
        .await
        .unwrap();
    let erased = storage.erase(fence).await.unwrap();
    assert_eq!(erased.events, 1);
    assert_eq!(erased.daily_ids, vec![1001]);
    storage.close().await.unwrap();

    let storage = Storage::open(dir.path(), budgets()).await.unwrap();
    storage.drain().await;
    assert_eq!(rows(&storage, SITE).await, [2, 2, 0]);
    assert_eq!(
        count(
            &storage,
            format!("SELECT count(*) FROM events WHERE profile_id = {PROFILE}")
        )
        .await,
        0
    );
    storage.close().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn files_of_another_pair_or_moment_are_refused() {
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    let older = tempfile::tempdir().unwrap();
    applied(first.path(), &[event(1, SITE, None)]).await;
    std::fs::copy(
        first.path().join("analytics.duckdb"),
        older.path().join("analytics.duckdb"),
    )
    .unwrap();
    applied(first.path(), &[event(2, SITE, None)]).await;
    applied(second.path(), &[event(3, SITE, None)]).await;

    // Another installation's analytics file.
    std::fs::copy(
        first.path().join("analytics.duckdb"),
        second.path().join("analytics.duckdb"),
    )
    .unwrap();
    let error = Storage::open(second.path(), budgets()).await.err().unwrap();
    assert!(matches!(error, Error::Mismatch(_)), "{error}");

    // The same installation's analytics file from before the last batch.
    std::fs::copy(
        older.path().join("analytics.duckdb"),
        first.path().join("analytics.duckdb"),
    )
    .unwrap();
    let error = Storage::open(first.path(), budgets()).await.err().unwrap();
    assert!(matches!(error, Error::Mismatch(_)), "{error}");

    // An analytics file deleted by hand.
    std::fs::remove_file(first.path().join("analytics.duckdb")).unwrap();
    let error = Storage::open(first.path(), budgets()).await.err().unwrap();
    assert!(matches!(error, Error::Mismatch(_)), "{error}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_snapshot_with_pending_records_reopens_with_every_record() {
    let dir = tempfile::tempdir().unwrap();
    let copy = tempfile::tempdir().unwrap();
    applied(dir.path(), &[event(1, SITE, None), event(2, SITE, None)]).await;

    let storage = open_holding(dir.path()).await;
    storage.admit(&[event(3, SITE, None)]).await.unwrap();
    storage.snapshot(copy.path()).await.unwrap();
    storage.close().await.unwrap();

    let positions = holocron_storage::inspect(copy.path()).await.unwrap();
    assert_eq!(positions.ledger_records, 2);
    assert_eq!(positions.inbox_records, 1);
    let restored = Storage::open(copy.path(), budgets()).await.unwrap();
    restored.drain().await;
    assert_eq!(rows(&restored, SITE).await, [3, 3, 0]);
    restored.close().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn shutdown_refuses_admission_and_leaves_no_analytics_wal() {
    let dir = tempfile::tempdir().unwrap();
    let storage = open_holding(dir.path()).await;
    storage.admit(&[event(1, SITE, None)]).await.unwrap();
    storage.shutdown().await.unwrap();
    assert_eq!(
        storage.admit(&[event(2, SITE, None)]).await,
        Err(Refusal::Unavailable)
    );
    let wal = std::fs::metadata(dir.path().join("analytics.duckdb.wal")).map_or(0, |m| m.len());
    assert_eq!(wal, 0);
    drop(storage);

    // The accepted record waited in the inbox and transfers after the restart.
    let storage = Storage::open(dir.path(), budgets()).await.unwrap();
    storage.drain().await;
    assert_eq!(rows(&storage, SITE).await, [1, 1, 0]);
    storage.close().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn upgrade_retires_batches_the_old_schema_applied() {
    let dir = tempfile::tempdir().unwrap();
    applied(dir.path(), &[event(1, SITE, None)]).await;

    // The files as an older release leaves them: schema 3, and the last batch applied but
    // not yet retired from the inbox.
    let storage = Storage::open(dir.path(), budgets()).await.unwrap();
    storage
        .write(|tx| {
            tx.execute_batch(
                "UPDATE schema_info SET analytics_schema = 3;
                 UPDATE applied_batches SET analytics_schema = 3;",
            )
        })
        .await
        .unwrap();
    storage.close().await.unwrap();
    let inbox = sqlx::SqlitePool::connect(&format!(
        "sqlite://{}",
        dir.path().join("inbox.sqlite").display()
    ))
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO batches (id, first_seq, last_seq, records, bytes, assigned_at) VALUES (1, 1, 1, 1, 1, 0);
         INSERT INTO inbox (seq, received_at, format, bytes, payload, batch_id) VALUES (1, 0, 2, 1, x'00', 1);",
    )
    .execute(&inbox)
    .await
    .unwrap();
    inbox.close().await;

    let error = Storage::open(dir.path(), budgets()).await.err().unwrap();
    assert!(matches!(error, Error::UpgradeRequired(_)), "{error}");
    holocron_storage::upgrade(dir.path(), &budgets())
        .await
        .unwrap();
    let storage = Storage::open(dir.path(), budgets()).await.unwrap();
    assert_eq!(storage.status().inbox_records, 0);
    storage.admit(&[event(2, SITE, None)]).await.unwrap();
    storage.drain().await;
    assert_eq!(rows(&storage, SITE).await, [2, 2, 0]);
    assert_eq!(storage.status().transfer_failures, 0);
    storage.close().await.unwrap();
}
