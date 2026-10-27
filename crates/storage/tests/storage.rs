//! Admission, transfer and reads through the real SQLite and DuckDB engines.

use std::time::Duration;

use holocron_storage::{
    Budgets, ClientEventId, Dimensions, Error, Event, EventKind, HeatmapInteraction, Identify,
    Interaction, Lane, Outcome, Property, PropertyValue, ReadError, Record, Refusal,
    ReplayFragment, Storage,
};

const SITE: u128 = 0x5a30_3801_78f6_5a96_82f9_d5c4_0398_a7b1;
const NOW_MS: i64 = 1_790_812_800_000;

fn budgets() -> Budgets {
    Budgets {
        transfer_delay: Duration::from_millis(5),
        engine_threads: 1,
        engine_memory_bytes: 256 * 1024 * 1024,
        disk_reserve_bytes: 0,
        ..Budgets::default()
    }
}

fn event(id: u128, client_event_id: Option<(&str, u128)>) -> Record {
    Record::Event(Event {
        id,
        site: SITE,
        kind: EventKind::CustomEvent,
        event_time_ms: NOW_MS,
        receive_time_ms: NOW_MS,
        local_day: (NOW_MS / 86_400_000) as i32,
        daily_id: Some(id + 1000),
        profile_id: None,
        client_event_id: client_event_id.map(|(id, fingerprint)| ClientEventId {
            id: id.into(),
            fingerprint,
        }),
        dimensions: Dimensions {
            path: Some("/pricing".into()),
            event_name: Some("signup".into()),
            ..Dimensions::default()
        },
        performance: None,
        revenue: None,
        properties: vec![
            Property {
                key: "plan".into(),
                value: PropertyValue::String("pro".into()),
            },
            Property {
                key: "price".into(),
                value: PropertyValue::Decimal(129_900),
            },
        ],
    })
}

async fn count(storage: &Storage, sql: &'static str) -> i64 {
    storage
        .read(Lane::Report, move |s| {
            s.connection().query_row(sql, [], |r| r.get(0))
        })
        .await
        .expect("read")
        .value
}

#[tokio::test(flavor = "multi_thread")]
async fn accepted_records_become_visible_with_a_covering_watermark() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::open(dir.path(), budgets()).await.unwrap();
    let records = vec![
        event(1, None),
        event(2, None),
        Record::Identify(Identify {
            id: 3,
            site: SITE,
            event_time_ms: NOW_MS - 1000,
            receive_time_ms: NOW_MS,
            local_day: (NOW_MS / 86_400_000) as i32,
            daily_id: Some(1001),
            profile_id: Some(77),
            properties: vec![Property {
                key: "account_tier".into(),
                value: PropertyValue::String("team".into()),
            }],
        }),
        fragment(4, 65_536),
        Record::HeatmapInteraction(HeatmapInteraction {
            id: 5,
            site: SITE,
            capture_id: 9,
            event_time_ms: NOW_MS,
            receive_time_ms: NOW_MS,
            path: "/pricing".into(),
            viewport_width: 390,
            viewport_height: 844,
            kind: Interaction::Scroll { percent: 40 },
        }),
    ];
    let outcomes = storage.admit(&records).await.unwrap();
    let Outcome::Accepted { sequence: last } = outcomes[4] else {
        panic!("{outcomes:?}")
    };

    let mut watermark = storage.watermark();
    watermark.wait_for(|w| w.sequence >= last).await.unwrap();
    let read = storage
        .read(Lane::Live, |s| {
            s.connection().query_row(
                "SELECT (SELECT count(*) FROM events), (SELECT count(*) FROM event_properties),
                        (SELECT count(*) FROM session_properties), (SELECT sum(octet_length(payload)) FROM replay_fragments),
                        (SELECT count(*) FROM heatmap_interactions)",
                [],
                |r| Ok([r.get::<_, i64>(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?]),
            )
        })
        .await
        .unwrap();
    assert_eq!(read.value, [2, 4, 1, 65_536, 1]);
    assert!(read.watermark.sequence >= last);
    storage.close().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn duplicate_client_event_id_is_suppressed() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::open(dir.path(), budgets()).await.unwrap();
    let outcomes = storage
        .admit(&[event(1, Some(("evt-1", 7))), event(2, Some(("evt-1", 7)))])
        .await
        .unwrap();
    assert!(matches!(outcomes[0], Outcome::Accepted { .. }));
    assert_eq!(outcomes[1], Outcome::Duplicate { conflict: false });
    // A retry whose payload differs is still a duplicate, flagged as a conflict.
    let retry = storage
        .admit(&[event(3, Some(("evt-1", 8)))])
        .await
        .unwrap();
    assert_eq!(retry, vec![Outcome::Duplicate { conflict: true }]);
    storage.drain().await;
    assert_eq!(count(&storage, "SELECT count(*) FROM events").await, 1);
    storage.close().await.unwrap();
}

fn fragment(id: u128, bytes: usize) -> Record {
    Record::ReplayFragment(ReplayFragment {
        id,
        site: SITE,
        capture_id: 9,
        index: 0,
        total: 1,
        is_final: true,
        event_time_ms: NOW_MS,
        receive_time_ms: NOW_MS,
        payload: vec![b'x'; bytes],
    })
}

#[tokio::test(flavor = "multi_thread")]
async fn full_inbox_refuses_the_whole_request() {
    let dir = tempfile::tempdir().unwrap();
    // Transfer is stalled, so the backlog only grows.
    let budgets = Budgets {
        inbox_bytes: 150_000,
        transfer_delay: Duration::from_secs(3600),
        ..budgets()
    };
    let storage = Storage::open(dir.path(), budgets).await.unwrap();
    storage.admit(&[event(1, None)]).await.unwrap();
    assert!(storage.admit(&[fragment(2, 100_000)]).await.is_ok());
    assert_eq!(
        storage.admit(&[event(3, None), fragment(4, 100_000)]).await,
        Err(Refusal::Busy)
    );
    assert_eq!(storage.status().inbox_records, 2);
    storage.close().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn slow_read_is_interrupted_at_the_timeout() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::open(
        dir.path(),
        Budgets {
            report_workers: 1,
            query_timeout: Duration::from_millis(300),
            ..budgets()
        },
    )
    .await
    .unwrap();
    let slow = storage
        .read(Lane::Report, |s| {
            s.connection().query_row(
                "SELECT count(*) FROM range(1000000000000) a WHERE a.range % 7 = 3",
                [],
                |r| r.get::<_, i64>(0),
            )
        })
        .await;
    assert!(matches!(slow, Err(ReadError::Timeout)), "{slow:?}");
    // The only worker is free again: the interrupted query did not keep running.
    let next = storage
        .read(Lane::Report, |s| {
            s.connection()
                .query_row("SELECT 42", [], |r| r.get::<_, i64>(0))
        })
        .await
        .unwrap();
    assert_eq!(next.value, 42);
    assert!(
        next.queue_wait < Duration::from_millis(200),
        "{:?}",
        next.queue_wait
    );
    storage.close().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn abandoned_read_frees_its_worker() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::open(
        dir.path(),
        Budgets {
            report_workers: 1,
            ..budgets()
        },
    )
    .await
    .unwrap();
    // The caller stops waiting long before the 30 s query timeout, as a replaced exploration does.
    let abandoned = tokio::time::timeout(
        Duration::from_millis(200),
        storage.read(Lane::Report, |s| {
            s.connection().query_row(
                "SELECT count(*) FROM range(1000000000000) a WHERE a.range % 7 = 3",
                [],
                |r| r.get::<_, i64>(0),
            )
        }),
    )
    .await;
    assert!(abandoned.is_err());
    let next = storage
        .read(Lane::Report, |s| {
            s.connection()
                .query_row("SELECT 42", [], |r| r.get::<_, i64>(0))
        })
        .await
        .unwrap();
    assert_eq!(next.value, 42);
    assert!(
        next.queue_wait < Duration::from_millis(500),
        "{:?}",
        next.queue_wait
    );
    storage.close().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn drained_inbox_gives_its_pages_back() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::open(dir.path(), budgets()).await.unwrap();
    let records: Vec<Record> = (0..40).map(|i| fragment(100 + i, 100_000)).collect();
    storage.admit(&records).await.unwrap();
    storage.drain().await;

    let inbox = sqlx::sqlite::SqlitePoolOptions::new()
        .connect_with(
            sqlx::sqlite::SqliteConnectOptions::new()
                .filename(dir.path().join("inbox.sqlite"))
                .read_only(true),
        )
        .await
        .unwrap();
    let (pages, free): (i64, i64) = sqlx::query_as(
        "SELECT (SELECT page_count FROM pragma_page_count), (SELECT freelist_count FROM pragma_freelist_count)",
    )
    .fetch_one(&inbox)
    .await
    .unwrap();
    // 4 MB went through; a drained inbox keeps only its schema pages.
    assert!(pages < 32, "{pages} pages");
    assert_eq!(free, 0);
    inbox.close().await;
    storage.close().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_file_with_outdated_derived_tables_rebuilds_every_day() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::open(dir.path(), budgets()).await.unwrap();
    storage.admit(&[event(1, None)]).await.unwrap();
    storage.drain().await;
    // What correction leaves behind, then the file as it was before the rollup tables, the
    // stored session fields, the member event times and the visitor coordinates.
    storage
        .write(|tx| {
            tx.execute_batch(
                "DELETE FROM dirty_days;
                 INSERT INTO day_aggregates SELECT site, local_day, 1, [], 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1 FROM events;
                 INSERT INTO sessions (site, session_id, population, identity, local_day, start_time, last_time,
                                       ends_at, pageviews, custom_events, policy_version)
                 SELECT site, seq, 1, 1, local_day, event_time, event_time, event_time, 1, 0, 1 FROM events;
                 INSERT INTO session_members SELECT site, seq, seq, 1, event_time FROM events;
                 ALTER TABLE session_members DROP COLUMN event_time;
                 ALTER TABLE sessions DROP COLUMN touch_query;
                 DROP TABLE hour_aggregates; DROP TABLE dimension_aggregates; DROP TABLE visitor_days;
                 ALTER TABLE events DROP COLUMN latitude; ALTER TABLE events DROP COLUMN longitude;
                 DROP TABLE schema_info;",
            )
        })
        .await
        .unwrap();
    storage.close().await.unwrap();

    let refused = Storage::open(dir.path(), budgets()).await.err().unwrap();
    assert!(matches!(refused, Error::UpgradeRequired(_)), "{refused}");
    holocron_storage::upgrade(dir.path(), &budgets())
        .await
        .unwrap();
    let storage = Storage::open(dir.path(), budgets()).await.unwrap();
    assert_eq!(
        count(
            &storage,
            "SELECT count(*) FROM duckdb_columns()
             WHERE table_name = 'events' AND column_name IN ('latitude', 'longitude')"
        )
        .await,
        2
    );
    for sql in [
        "SELECT count(*) FROM day_aggregates",
        "SELECT count(*) FROM sessions",
        "SELECT count(*) FROM session_members",
        "SELECT count(*) FROM visitor_days",
    ] {
        assert_eq!(count(&storage, sql).await, 0, "{sql}");
    }
    assert_eq!(count(&storage, "SELECT count(*) FROM dirty_days").await, 1);
    storage.close().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_file_with_one_property_table_moves_values_to_key_partitions() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::open(dir.path(), budgets()).await.unwrap();
    storage
        .admit(&[event(1, None), event(2, None)])
        .await
        .unwrap();
    storage.drain().await;
    // The file as it was before the key partitions.
    let mut sql = "CREATE TABLE unpartitioned AS SELECT * FROM event_properties;
                   DROP VIEW event_properties; DROP TABLE property_keys;"
        .to_owned();
    for part in 0..holocron_storage::PROPERTY_PARTS {
        sql += &format!("DROP TABLE event_properties_{part};");
    }
    sql += "ALTER TABLE unpartitioned RENAME TO event_properties; DROP TABLE schema_info;";
    storage
        .write(move |tx| tx.execute_batch(&sql))
        .await
        .unwrap();
    storage.close().await.unwrap();

    holocron_storage::upgrade(dir.path(), &budgets())
        .await
        .unwrap();
    let storage = Storage::open(dir.path(), budgets()).await.unwrap();
    for (sql, expected) in [
        (
            "SELECT count(*) FROM property_keys WHERE key = 'plan' AND part = 0",
            1,
        ),
        (
            "SELECT count(*) FROM property_keys WHERE key = 'price' AND part = 1",
            1,
        ),
        (
            "SELECT count(*) FROM event_properties_0 WHERE key = 'plan'",
            2,
        ),
        (
            "SELECT count(*) FROM event_properties_1 WHERE key = 'price'",
            2,
        ),
        ("SELECT count(*) FROM event_properties", 4),
    ] {
        assert_eq!(count(&storage, sql).await, expected, "{sql}");
    }
    storage.close().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_file_without_entry_paths_fills_them_from_the_stored_members() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::open(dir.path(), budgets()).await.unwrap();
    storage
        .admit(&[event(1, None), event(2, None), event(3, None)])
        .await
        .unwrap();
    storage.drain().await;
    // Session 1 holds a custom event and a later pageview, session 3 only a custom event; the
    // file is as it was at schema 5, without the column.
    storage
        .write(|tx| {
            tx.execute_batch(
                "DELETE FROM dirty_days;
                 UPDATE events SET kind = 1, path = '/landing' WHERE id = 2;
                 INSERT INTO sessions (site, session_id, population, identity, local_day, start_time, last_time,
                                       ends_at, pageviews, custom_events, policy_version)
                 SELECT site, seq, 1, 1, local_day, event_time, event_time, event_time, 1, 0, 1
                 FROM events WHERE id IN (1, 3);
                 INSERT INTO session_members
                 SELECT site, seq, CASE WHEN id = 3 THEN seq ELSE (SELECT seq FROM events WHERE id = 1) END,
                        1, event_time + to_milliseconds(seq) FROM events;
                 ALTER TABLE sessions DROP COLUMN entry_path;
                 UPDATE schema_info SET analytics_schema = 5;",
            )
        })
        .await
        .unwrap();
    storage.close().await.unwrap();

    holocron_storage::upgrade(dir.path(), &budgets())
        .await
        .unwrap();
    let storage = Storage::open(dir.path(), budgets()).await.unwrap();
    for (sql, expected) in [
        (
            "SELECT count(*) FROM sessions s JOIN events e ON e.seq = s.session_id
             WHERE e.id = 1 AND s.entry_path = '/landing'",
            1,
        ),
        (
            "SELECT count(*) FROM sessions s JOIN events e ON e.seq = s.session_id
             WHERE e.id = 3 AND s.entry_path = '/pricing'",
            1,
        ),
    ] {
        assert_eq!(count(&storage, sql).await, expected, "{sql}");
    }
    storage.close().await.unwrap();
}
