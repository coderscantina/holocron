//! Milestone 12 through the real engines: backup and clean-directory restore, mismatched
//! snapshots, erasure across restarts, retention, compaction, upgrade interruption, health and
//! diagnostics (with the problems `operator.status` names), and the `holocron` binary's
//! commands and graceful shutdown.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use holocron_server::clock::Clock;
use holocron_server::init::{self, Exit, InitArgs};
use holocron_server::maintenance::backup::{self, Source};
use holocron_server::maintenance::commands::{self, DataDir, EraseProfileArgs};
use holocron_server::maintenance::jobs::{self, Job};
use holocron_server::maintenance::{Runtime, compaction, restore, upgrade, versions};
use holocron_server::router;
use holocron_storage::{
    Budgets, Dimensions, Event, EventKind, Lane, Property, PropertyValue, Record,
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

const BIN: &str = env!("CARGO_BIN_EXE_holocron");
const SITE: &str = "0b5e0000-0000-4000-8000-0000000000a1";
const ADMIN: &str = "admin@ops.test";
const PASSWORD: &str = "correct horse battery";
const HOST: &str = "ops.test";
const PEER: &str = "198.51.100.7:40000";
/// 2026-10-01T10:00:00Z.
const MORNING_MS: i64 = 1_790_848_800_000;
const DAY_MS: i64 = 86_400_000;

fn budgets() -> Budgets {
    Budgets {
        engine_threads: 1,
        engine_memory_bytes: 256 * 1024 * 1024,
        disk_reserve_bytes: 0,
        transfer_delay: Duration::from_millis(5),
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

/// `holocron init` plus one site in Vienna.
async fn initialized(dir: &Path) {
    init::init(InitArgs {
        data_dir: dir.to_path_buf(),
        admin_email: ADMIN.into(),
        admin_password: Some(PASSWORD.into()),
    })
    .await
    .unwrap();
    let db = holocron_server::db::Db::open(dir).await.unwrap();
    let workspace: String = sqlx::query_scalar("SELECT id FROM workspaces")
        .fetch_one(&db.reader)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO sites (id, workspace_id, name, domain, created_at) VALUES (?, ?, 'ops', 'ops.test', 0);
         INSERT INTO session_policies (site_id, version, epoch, effective_at, timezone, window_minutes, created_at)
         VALUES (?, 1, 1, -9223372036854775808, 'Europe/Vienna', 30, 0);",
    )
    .bind(SITE)
    .bind(&workspace)
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

/// Opens with [`holding`] once transfer found the inbox empty.
async fn open_holding(dir: &Path) -> Runtime {
    let runtime = open(dir, holding(), Clock::system()).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    runtime
}

struct Answer {
    status: StatusCode,
    cookie: Option<String>,
    json: Value,
}

async fn request(runtime: &Runtime, request: Request<Body>) -> Answer {
    let response = router(runtime.state.clone())
        .oneshot(request)
        .await
        .unwrap();
    let status = response.status();
    let cookie = response
        .headers()
        .get(header::SET_COOKIE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(';').next())
        .map(str::to_owned);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    Answer {
        status,
        cookie,
        json: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    }
}

/// A tracker payload from a desktop browser.
async fn collect(runtime: &Runtime, payload: Value) {
    collect_as(
        runtime,
        payload,
        "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) Chrome/140.0.0.0 Safari/537.36",
        PEER,
    )
    .await;
}

async fn collect_as(runtime: &Runtime, payload: Value, user_agent: &str, peer: &str) {
    let answer = request(
        runtime,
        Request::post("/api/send")
            .header(header::HOST, HOST)
            .header(header::USER_AGENT, user_agent)
            .header(header::CONTENT_TYPE, "application/json")
            .extension(axum::extract::ConnectInfo(
                peer.parse::<SocketAddr>().unwrap(),
            ))
            .body(Body::from(payload.to_string()))
            .unwrap(),
    )
    .await;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.json);
}

fn pageview(path: &str) -> Value {
    json!({ "type": "event", "payload": { "website": SITE, "hostname": HOST, "url": path } })
}

async fn command(runtime: &Runtime, cookie: Option<&str>, name: &str, mut body: Value) -> Answer {
    body["idempotency_key"] = json!(uuid::Uuid::new_v4().to_string());
    let mut builder = Request::post(format!("/api/commands/{name}"))
        .header(header::HOST, HOST)
        .header(header::ORIGIN, format!("http://{HOST}"))
        .header("x-holocron-request", "1")
        .header(header::CONTENT_TYPE, "application/json")
        .extension(axum::extract::ConnectInfo(
            PEER.parse::<SocketAddr>().unwrap(),
        ));
    if let Some(cookie) = cookie {
        builder = builder.header(header::COOKIE, cookie);
    }
    request(runtime, builder.body(Body::from(body.to_string())).unwrap()).await
}

/// Logs the admin in with the password given at init.
async fn login(runtime: &Runtime) -> String {
    let answer = command(
        runtime,
        None,
        "auth.login",
        json!({ "email": ADMIN, "password": PASSWORD }),
    )
    .await;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.json);
    let cookie = answer.cookie.expect("a session cookie");
    // Deleting and resetting a site need a fresh password check.
    let confirmed = command(
        runtime,
        Some(&cookie),
        "auth.reauthenticate",
        json!({ "password": PASSWORD }),
    )
    .await;
    assert_eq!(confirmed.status, StatusCode::OK, "{}", confirmed.json);
    cookie
}

async fn count(runtime: &Runtime, sql: &str) -> i64 {
    let sql = sql.to_owned();
    runtime
        .state
        .collector
        .storage()
        .read(Lane::Report, move |s| {
            s.connection().query_row(&sql, [], |r| r.get(0))
        })
        .await
        .unwrap()
        .value
}

async fn control_count(runtime: &Runtime, sql: &'static str) -> i64 {
    sqlx::query_scalar(sql)
        .fetch_one(&runtime.state.db.reader)
        .await
        .unwrap()
}

async fn run_job(runtime: &Runtime, job: Job) -> Value {
    let state = &runtime.state;
    let id = jobs::enqueue(&state.db.writer, &job, state.collector.clock().now_ms())
        .await
        .unwrap();
    jobs::run_until(state, &id).await.unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_backup_with_waiting_records_restores_into_a_new_directory() {
    let root = tempfile::tempdir().unwrap();
    let (data, backups, restored) = (
        root.path().join("data"),
        root.path().join("backups"),
        root.path().join("restored"),
    );
    initialized(&data).await;
    let runtime = open(&data, budgets(), Clock::system()).await;
    for n in 0..3 {
        collect(&runtime, pageview(&format!("/applied/{n}"))).await;
    }
    runtime.state.collector.storage().drain().await;
    runtime.close().await.unwrap();

    // Two events still wait for transfer when the serving installation is backed up.
    let runtime = open_holding(&data).await;
    for n in 0..2 {
        collect(&runtime, pageview(&format!("/waiting/{n}"))).await;
    }
    let summary = run_job(&runtime, Job::Backup { to: backups }).await;
    assert_eq!(summary["positions"]["inbox_records"], 2);
    assert_eq!(summary["positions"]["ledger_records"], 3);
    runtime.close().await.unwrap();

    let from = PathBuf::from(summary["path"].as_str().unwrap());
    let result = restore::restore(&from, &restored).await.unwrap();
    assert_eq!(result["state"], "ready");
    assert_eq!(result["records_applied"], 2);

    let runtime = open(&restored, budgets(), Clock::system()).await;
    assert_eq!(count(&runtime, "SELECT count(*) FROM events").await, 5);
    login(&runtime).await;
    runtime.close().await.unwrap();
    versions::check_dir(&restored).await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn mixed_or_damaged_snapshots_are_refused() {
    let root = tempfile::tempdir().unwrap();
    let (data, backups) = (root.path().join("data"), root.path().join("backups"));
    initialized(&data).await;
    let (first, _) = backup::take(Source::Stopped(&data), &backups)
        .await
        .unwrap();
    let runtime = open(&data, budgets(), Clock::system()).await;
    collect(&runtime, pageview("/later")).await;
    runtime.state.collector.storage().drain().await;
    runtime.close().await.unwrap();
    let (second, _) = backup::take(Source::Stopped(&data), &backups)
        .await
        .unwrap();

    // A damaged file fails its checksum.
    let damaged = root.path().join("damaged");
    copy_dir(&second, &damaged);
    let mut bytes = std::fs::read(damaged.join("inbox.sqlite")).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 0xff;
    std::fs::write(damaged.join("inbox.sqlite"), bytes).unwrap();
    let refused = restore::restore(&damaged, &root.path().join("r1"))
        .await
        .unwrap_err();
    assert_eq!(
        (refused.exit, refused.code.as_ref()),
        (Exit::Refused, "backup_corrupt")
    );

    // An analytics file from the earlier backup, with a manifest rewritten to match it.
    let mixed = root.path().join("mixed");
    copy_dir(&second, &mixed);
    std::fs::copy(
        first.join("analytics.duckdb"),
        mixed.join("analytics.duckdb"),
    )
    .unwrap();
    let mut manifest: Value =
        serde_json::from_str(&std::fs::read_to_string(mixed.join("manifest.json")).unwrap())
            .unwrap();
    let analytics = std::fs::read(mixed.join("analytics.duckdb")).unwrap();
    for file in manifest["files"].as_array_mut().unwrap() {
        if file["name"] == "analytics.duckdb" {
            file["bytes"] = json!(analytics.len());
            file["blake3"] = json!(blake3::hash(&analytics).to_hex().to_string());
        }
    }
    std::fs::write(mixed.join("manifest.json"), manifest.to_string()).unwrap();
    let refused = restore::restore(&mixed, &root.path().join("r2"))
        .await
        .unwrap_err();
    assert_eq!(refused.code, "engines_mismatched");

    // The same file copied over a live data directory.
    std::fs::copy(
        first.join("analytics.duckdb"),
        data.join("analytics.duckdb"),
    )
    .unwrap();
    let refused = Runtime::open(Runtime::config(&data), budgets(), Clock::system())
        .await
        .err()
        .unwrap();
    assert_eq!(
        (refused.exit, refused.code.as_ref()),
        (Exit::Refused, "engines_mismatched")
    );
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        std::fs::copy(entry.path(), to.join(entry.file_name())).unwrap();
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn site_deletion_erases_applied_waiting_and_replay_data_after_a_restart() {
    let root = tempfile::tempdir().unwrap();
    let data = root.path().join("data");
    initialized(&data).await;
    let runtime = open(&data, budgets(), Clock::system()).await;
    collect(&runtime, pageview("/one")).await;
    collect(&runtime, pageview("/two")).await;
    runtime.state.collector.storage().drain().await;
    runtime.close().await.unwrap();
    store_capture(&data).await;

    // One event waits in the inbox when the site is deleted, and the server stops before the
    // erasure job runs.
    let runtime = open_holding(&data).await;
    collect(&runtime, pageview("/three")).await;
    let cookie = login(&runtime).await;
    let deleted = command(
        &runtime,
        Some(&cookie),
        "sites.delete",
        json!({ "site": SITE }),
    )
    .await;
    assert_eq!(deleted.status, StatusCode::OK, "{}", deleted.json);
    runtime.close().await.unwrap();

    let runtime = open(&data, budgets(), Clock::system()).await;
    let id: String =
        sqlx::query_scalar("SELECT id FROM maintenance_jobs WHERE kind = 'erase_site'")
            .fetch_one(&runtime.state.db.reader)
            .await
            .unwrap();
    let result = jobs::run_until(&runtime.state, &id).await.unwrap();
    assert_eq!(result["captures"], 1);
    runtime.state.collector.storage().drain().await;
    assert_eq!(count(&runtime, "SELECT count(*) FROM events").await, 0);
    assert_eq!(
        count(&runtime, "SELECT count(*) FROM event_properties").await,
        0
    );
    assert_eq!(
        count(&runtime, "SELECT count(*) FROM day_aggregates").await,
        0
    );
    assert_eq!(runtime.state.collector.storage().status().inbox_records, 0);
    assert_eq!(
        runtime
            .state
            .recordings
            .store
            .usage()
            .await
            .unwrap()
            .captures,
        0
    );
    assert_eq!(
        control_count(&runtime, "SELECT count(*) FROM identity_day_keys").await,
        0
    );
    runtime.close().await.unwrap();
}

/// One stored replay capture of [`SITE`], written as the replay store writes it.
async fn store_capture(dir: &Path) {
    let store = holocron_server::recordings::store::Store::open(dir)
        .await
        .unwrap();
    store.close().await;
    let pool = sqlx::SqlitePool::connect(&format!(
        "sqlite://{}",
        dir.join("recordings.sqlite").display()
    ))
    .await
    .unwrap();
    let site = uuid::Uuid::parse_str(SITE).unwrap().as_bytes().to_vec();
    sqlx::query(
        "INSERT INTO captures (site, capture, received_at, last_received_at, started_at, ended_at,
                               first_chunk, chunks, events, bytes, pages)
         VALUES (?1, x'0102', ?2, ?2, ?2, ?2, 1, 1, 1, 100, 1);
         INSERT INTO chunks (site, capture, chunk, received_at, events, raw_bytes, fingerprint, payload)
         VALUES (?1, x'0102', 1, ?2, 1, 100, x'00', x'00');
         UPDATE usage SET bytes = bytes + 100;",
    )
    .bind(site)
    .bind(holocron_server::db::now_ms())
    .execute(&pool)
    .await
    .unwrap();
    pool.close().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_reset_keeps_the_site_and_events_sent_after_it() {
    let root = tempfile::tempdir().unwrap();
    let data = root.path().join("data");
    initialized(&data).await;
    let runtime = open(&data, budgets(), Clock::system()).await;
    collect(&runtime, pageview("/before")).await;
    runtime.state.collector.storage().drain().await;
    let cookie = login(&runtime).await;
    let reset = command(
        &runtime,
        Some(&cookie),
        "sites.reset",
        json!({ "site": SITE }),
    )
    .await;
    assert_eq!(reset.status, StatusCode::OK, "{}", reset.json);
    assert!(reset.json["reset_at"].is_i64());
    collect(&runtime, pageview("/after")).await;

    let id: String =
        sqlx::query_scalar("SELECT id FROM maintenance_jobs WHERE kind = 'reset_site'")
            .fetch_one(&runtime.state.db.reader)
            .await
            .unwrap();
    jobs::run_until(&runtime.state, &id).await.unwrap();
    runtime.state.collector.storage().drain().await;
    assert_eq!(
        count(
            &runtime,
            "SELECT count(*) FROM events WHERE path = '/before'"
        )
        .await,
        0
    );
    assert_eq!(
        count(
            &runtime,
            "SELECT count(*) FROM events WHERE path = '/after'"
        )
        .await,
        1
    );
    assert_eq!(
        control_count(&runtime, "SELECT count(*) FROM sites").await,
        1
    );
    runtime.close().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn erasing_a_profile_removes_its_history_and_sessions_and_keeps_other_visitors() {
    let root = tempfile::tempdir().unwrap();
    let data = root.path().join("data");
    initialized(&data).await;
    let runtime = open(&data, budgets(), Clock::system()).await;
    sqlx::query("UPDATE sites SET persistent_identity = 1")
        .execute(&runtime.state.db.writer)
        .await
        .unwrap();
    let with = |id: &str, extra: Value| {
        let mut body = pageview("/profile");
        body["payload"]["id"] = json!(id);
        if let Value::Object(extra) = extra {
            body["payload"].as_object_mut().unwrap().extend(extra);
        }
        body
    };
    for person in ["anna", "bert"] {
        let mut grant = with(person, json!({ "consent": "persistent" }));
        grant["type"] = json!("identify");
        collect(&runtime, grant).await;
        collect(&runtime, with(person, json!({}))).await;
    }
    collect(&runtime, pageview("/anonymous")).await;
    let storage = runtime.state.collector.storage();
    storage.drain().await;
    holocron_analytics::run_until_clean(storage, holocron_analytics::Limits::default())
        .await
        .unwrap();
    assert_eq!(
        count(
            &runtime,
            "SELECT count(*) FROM sessions WHERE population = 2"
        )
        .await,
        2
    );
    runtime.close().await.unwrap();

    // Without a server the command takes the lock and runs the job itself.
    let site = uuid::Uuid::parse_str(SITE).unwrap();
    commands::erase_profile(EraseProfileArgs {
        dir: DataDir {
            data_dir: data.clone(),
        },
        site,
        distinct_id: "anna".into(),
    })
    .await
    .unwrap();

    let runtime = open(&data, budgets(), Clock::system()).await;
    let storage = runtime.state.collector.storage();
    holocron_analytics::run_until_clean(storage, holocron_analytics::Limits::default())
        .await
        .unwrap();
    assert_eq!(
        count(
            &runtime,
            "SELECT count(*) FROM events WHERE profile_id IS NOT NULL"
        )
        .await,
        1
    );
    assert_eq!(
        count(
            &runtime,
            "SELECT count(*) FROM sessions WHERE population = 2"
        )
        .await,
        1
    );
    assert_eq!(
        count(
            &runtime,
            "SELECT count(*) FROM events WHERE profile_id IS NULL"
        )
        .await,
        1
    );
    assert_eq!(
        control_count(&runtime, "SELECT count(*) FROM consents").await,
        1
    );
    runtime.close().await.unwrap();
}

/// `source_table:source_key` of every import inventory row.
async fn inventory(runtime: &Runtime) -> Vec<String> {
    sqlx::query_scalar("SELECT source_table || ':' || source_key FROM import_inventory ORDER BY 1")
        .fetch_all(&runtime.state.db.reader)
        .await
        .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn erasure_reaches_the_import_inventory_and_provenance_of_its_site_or_visitor() {
    const OTHER: &str = "0b5e0000-0000-4000-8000-0000000000b2";
    let root = tempfile::tempdir().unwrap();
    let data = root.path().join("data");
    initialized(&data).await;
    let runtime = open(&data, budgets(), Clock::system()).await;
    let rows = [
        ("website", SITE, json!({ "website_id": SITE })),
        ("segment", "segment-1", json!({ "website_id": SITE })),
        (
            "share",
            "share-1",
            json!({ "entity_id": SITE, "share_type": 1 }),
        ),
        (
            "session_link",
            "link-anna",
            json!({ "website_id": SITE, "distinct_id": "anna" }),
        ),
        (
            "session_link",
            "link-bert",
            json!({ "website_id": SITE, "distinct_id": "bert" }),
        ),
        ("session_replay", "replay-1", json!({ "website_id": SITE })),
        ("website", OTHER, json!({ "website_id": OTHER })),
        (
            "session_link",
            "link-anna-other",
            json!({ "website_id": OTHER, "distinct_id": "anna" }),
        ),
        ("api_key", "key-1", json!({ "user_id": "user-1" })),
    ];
    for (table, key, row) in rows {
        sqlx::query(
            "INSERT INTO import_inventory (source_table, source_key, reason, representation)
             VALUES (?, ?, 'test', ?)",
        )
        .bind(table)
        .bind(key)
        .bind(json!({ "row": row }).to_string())
        .execute(&runtime.state.db.writer)
        .await
        .unwrap();
    }
    let site = uuid::Uuid::parse_str(SITE).unwrap();
    let storage = runtime.state.collector.storage();
    let id = site.as_u128();
    storage
        .write(move |tx| {
            tx.execute(
                "INSERT INTO import_provenance VALUES (1, 1, ?1, 1, NULL, 'anna'), (1, 2, ?1, 2, NULL, 'bert')",
                [id],
            )
        })
        .await
        .unwrap();

    // A visitor's erasure finds the identity links and provenance of their distinct ID.
    let profile = runtime.state.collector.pseudonyms().profile_id(id, "anna");
    run_job(
        &runtime,
        Job::EraseProfile {
            site,
            profile: uuid::Uuid::from_u128(profile),
        },
    )
    .await;
    assert!(
        !inventory(&runtime)
            .await
            .contains(&"session_link:link-anna".into())
    );
    assert!(
        inventory(&runtime)
            .await
            .contains(&"session_link:link-anna-other".into())
    );
    assert_eq!(
        count(
            &runtime,
            "SELECT count(*) FROM import_provenance WHERE distinct_id = 'bert'"
        )
        .await,
        1
    );
    assert_eq!(
        count(&runtime, "SELECT count(*) FROM import_provenance").await,
        1
    );

    // A reset removes the site's imported history and keeps its imported configuration.
    let through_seq = storage.last_sequence().await.unwrap();
    run_job(&runtime, Job::ResetSite { site, through_seq }).await;
    assert_eq!(
        inventory(&runtime).await,
        [
            "api_key:key-1".to_owned(),
            "segment:segment-1".into(),
            "session_link:link-anna-other".into(),
            "share:share-1".into(),
            format!("website:{SITE}"),
            format!("website:{OTHER}"),
        ]
    );

    // A deletion removes every row of the site.
    run_job(&runtime, Job::EraseSite { site }).await;
    assert_eq!(
        inventory(&runtime).await,
        [
            "api_key:key-1".to_owned(),
            "session_link:link-anna-other".into(),
            format!("website:{OTHER}"),
        ]
    );
    runtime.close().await.unwrap();
}

/// An event of `site` at `at_ms` with one property.
fn stored_event(id: u128, site: u128, at_ms: i64) -> Record {
    Record::Event(Event {
        id,
        site,
        kind: EventKind::Pageview,
        event_time_ms: at_ms,
        receive_time_ms: at_ms,
        local_day: (at_ms / DAY_MS) as i32,
        daily_id: Some(id),
        profile_id: None,
        client_event_id: None,
        dimensions: Dimensions::default(),
        performance: None,
        revenue: None,
        properties: vec![Property {
            key: "plan".into(),
            value: PropertyValue::String(format!("plan-{id}")),
        }],
    })
}

/// Runs of one (site, month) in storage order, the (site, month) groups, and rows earlier than
/// the row before them within a run, for `table`.
async fn layout(runtime: &Runtime, table: &str) -> [i64; 3] {
    let sql = format!(
        "SELECT count(*) FILTER (WHERE key IS DISTINCT FROM previous),
                count(DISTINCT key),
                count(*) FILTER (WHERE key = previous AND event_time < previous_time)
         FROM (SELECT key, event_time, lag(key) OVER w AS previous, lag(event_time) OVER w AS previous_time
               FROM (SELECT rowid, event_time, site::VARCHAR || date_trunc('month', event_time)::VARCHAR AS key
                     FROM {table})
               WINDOW w AS (ORDER BY rowid))"
    );
    runtime
        .state
        .collector
        .storage()
        .read(Lane::Report, move |s| {
            s.connection()
                .query_row(&sql, [], |r| Ok([r.get(0)?, r.get(1)?, r.get(2)?]))
        })
        .await
        .unwrap()
        .value
}

/// Every event and property value, in sequence order.
async fn contents(runtime: &Runtime) -> (String, String) {
    let events = "SELECT string_agg(concat_ws(':', seq, id, site, event_time), ',' ORDER BY seq) FROM events";
    let properties =
        "SELECT string_agg(concat_ws(':', seq, site, event_time, key, v_string), ',' ORDER BY seq)
                      FROM event_properties";
    let read = |sql: &'static str| async move {
        runtime
            .state
            .collector
            .storage()
            .read(Lane::Report, move |s| {
                s.connection().query_row(sql, [], |r| r.get(0))
            })
            .await
            .unwrap()
            .value
    };
    (read(events).await, read(properties).await)
}

#[tokio::test(flavor = "multi_thread")]
async fn compaction_resumes_after_a_crash_and_clusters_closed_months_by_site() {
    let root = tempfile::tempdir().unwrap();
    let (data, crashed) = (root.path().join("data"), root.path().join("crashed"));
    initialized(&data).await;
    let runtime = open(&data, budgets(), Clock::system()).await;
    let storage = runtime.state.collector.storage();
    // July and August 2026 are closed on the morning of October 1; three sites, one of them
    // above `i128::MAX`, arrive interleaved.
    let august = MORNING_MS - 10 * 3_600_000 - 61 * DAY_MS;
    let july = august - 31 * DAY_MS;
    let sites = [7, u128::MAX - 7, 1 << 100];
    let records: Vec<Record> = (0..60)
        .map(|n| {
            let month = if n % 2 == 0 { july } else { august };
            stored_event(n as u128 + 1, sites[n % 3], month + n as i64 * 3_600_000)
        })
        .collect();
    storage.admit(&records).await.unwrap();
    storage.drain().await;
    let before = contents(&runtime).await;
    assert!(layout(&runtime, "events").await[0] > 6);

    // Three steps of seven events, then the process dies: the copy holds what was committed.
    let mut moved = 0;
    for _ in 0..3 {
        let step = storage.compact(MORNING_MS, 7).await.unwrap().unwrap();
        assert!(!step.month_done);
        moved += step.events;
    }
    for entry in std::fs::read_dir(&data).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name();
        if entry.file_type().unwrap().is_file() && !name.to_string_lossy().ends_with("-shm") {
            std::fs::create_dir_all(&crashed).unwrap();
            std::fs::copy(entry.path(), crashed.join(name)).unwrap();
        }
    }
    runtime.close().await.unwrap();

    let runtime = open(&crashed, budgets(), Clock::system()).await;
    let storage = runtime.state.collector.storage();
    assert_eq!(
        count(
            &runtime,
            "SELECT count(*) FROM compaction_months WHERE NOT done"
        )
        .await,
        1
    );
    let (_stop, stopped) = tokio::sync::watch::channel(false);
    moved += compaction::run(storage, MORNING_MS, &stopped)
        .await
        .unwrap();
    assert_eq!(moved, 60, "every event moved once");
    assert_eq!(contents(&runtime).await, before);
    // Two months of three sites, each one run in time order.
    assert_eq!(layout(&runtime, "events").await, [6, 6, 0]);
    let part: i64 = count(
        &runtime,
        "SELECT part FROM property_keys WHERE key = 'plan'",
    )
    .await;
    assert_eq!(
        layout(&runtime, &format!("event_properties_{part}")).await,
        [6, 6, 0]
    );

    // One late event is not worth rewriting its month.
    storage
        .admit(&[stored_event(61, sites[0], july + 5 * DAY_MS)])
        .await
        .unwrap();
    storage.drain().await;
    assert_eq!(storage.compact(MORNING_MS, 7).await.unwrap(), None);
    runtime.close().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn retention_removes_events_past_twelve_months_and_keeps_the_rest() {
    let root = tempfile::tempdir().unwrap();
    let data = root.path().join("data");
    initialized(&data).await;
    let clock = Clock::frozen(MORNING_MS);
    let runtime = open(&data, budgets(), clock.clone()).await;
    collect(&runtime, pageview("/last-year")).await;
    runtime.state.collector.storage().drain().await;
    clock.set(MORNING_MS + 400 * DAY_MS);
    collect(&runtime, pageview("/today")).await;
    runtime.state.collector.storage().drain().await;

    let result = run_job(&runtime, Job::Retention).await;
    assert_eq!(result["events_removed"], 1);
    assert_eq!(
        count(
            &runtime,
            "SELECT count(*) FROM events WHERE path = '/today'"
        )
        .await,
        1
    );
    assert_eq!(count(&runtime, "SELECT count(*) FROM events").await, 1);
    assert_eq!(
        count(&runtime, "SELECT count(*) FROM event_properties").await,
        0
    );
    runtime.close().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unfinished_import_refuses_operator_jobs_until_it_resumes() {
    let root = tempfile::tempdir().unwrap();
    let data = root.path().join("data");
    initialized(&data).await;
    let db = holocron_server::db::Db::open(&data).await.unwrap();
    sqlx::query(
        "INSERT INTO import_runs (id, source, fingerprint, options, clock_ms, state, started_at, updated_at)
         VALUES (1, 'umami-3.4.0-postgresql', '{}', '{}', 0, 'running', 0, 0)",
    )
    .execute(&db.writer)
    .await
    .unwrap();
    db.close().await;

    let refused = commands::retention(DataDir {
        data_dir: data.clone(),
    })
    .await
    .unwrap_err();
    assert_eq!(refused.code, "import_in_progress");
    assert_eq!(refused.exit, Exit::Refused);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failed_upgrade_is_refused_by_serve_and_resumes() {
    use std::os::unix::fs::PermissionsExt;

    let root = tempfile::tempdir().unwrap();
    let (data, backups) = (root.path().join("data"), root.path().join("backups"));
    initialized(&data).await;
    // The data as the previous release left it: control without the last migration, and
    // analytics at schema 3.
    let db = holocron_server::db::Db::open(&data).await.unwrap();
    let latest = versions::release().control;
    sqlx::query(
        "DELETE FROM _sqlx_migrations WHERE version = ?;
         ALTER TABLE user_preferences DROP COLUMN accent;",
    )
    .bind(latest)
    .execute(&db.writer)
    .await
    .unwrap();
    db.close().await;
    let analytics = data.join("analytics.duckdb");
    let conn = holocron_storage::duckdb::Connection::open(&analytics).unwrap();
    conn.execute_batch("UPDATE schema_info SET analytics_schema = 3")
        .unwrap();
    drop(conn);
    let refused = versions::check_dir(&data).await.unwrap_err();
    assert_eq!(refused.code, "upgrade_required");

    // DuckDB cannot be written: the upgrade stops after SQLite.
    std::fs::set_permissions(&analytics, std::fs::Permissions::from_mode(0o444)).unwrap();
    let failed = upgrade::upgrade(&data, Some(&backups)).await.unwrap_err();
    assert_eq!(failed.code, "upgrade_failed");
    assert!(
        failed.message.contains("holocron restore --from"),
        "{}",
        failed.message
    );
    let refused = versions::check_dir(&data).await.unwrap_err();
    assert_eq!(
        (refused.exit, refused.code.as_ref()),
        (Exit::Refused, "upgrade_interrupted")
    );
    assert_eq!(versions::stored(&data).await.unwrap().control, latest);

    std::fs::set_permissions(&analytics, std::fs::Permissions::from_mode(0o644)).unwrap();
    let done = upgrade::upgrade(&data, None).await.unwrap();
    assert_eq!(done["state"], "upgraded");
    assert_eq!(done["from"]["analytics"], 3);
    versions::check_dir(&data).await.unwrap();
    backup::verify(Path::new(done["backup"].as_str().unwrap()))
        .await
        .unwrap();

    // Data from a newer release is refused, never served.
    let conn = holocron_storage::duckdb::Connection::open(&analytics).unwrap();
    conn.execute_batch("UPDATE schema_info SET analytics_schema = 99")
        .unwrap();
    drop(conn);
    let refused = versions::check_dir(&data).await.unwrap_err();
    assert_eq!(refused.code, "data_newer_than_binary");
}

async fn problems(runtime: &Runtime) -> (StatusCode, Value) {
    let answer = request(
        runtime,
        Request::get("/health/ready")
            .header(header::HOST, HOST)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    (answer.status, answer.json["problems"].clone())
}

/// The readiness problems `operator.status` names for an install admin.
async fn operator_problems(runtime: &Runtime, cookie: &str) -> Value {
    let answer = request(
        runtime,
        Request::get("/api/queries/operator.status")
            .header(header::HOST, HOST)
            .header(header::COOKIE, cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.json);
    answer.json["problems"].clone()
}

/// `/health/ready` and `operator.status` name the same problems.
async fn both_problems(runtime: &Runtime, cookie: &str) -> (StatusCode, Value) {
    let health = problems(runtime).await;
    assert_eq!(operator_problems(runtime, cookie).await, health.1);
    health
}

#[tokio::test(flavor = "multi_thread")]
async fn health_tells_disk_full_failing_transfer_stale_analytics_and_recorder_budget_apart() {
    let root = tempfile::tempdir().unwrap();
    let data = root.path().join("data");
    initialized(&data).await;

    let runtime = open(&data, budgets(), Clock::system()).await;
    let cookie = login(&runtime).await;
    assert_eq!(
        both_problems(&runtime, &cookie).await,
        (StatusCode::OK, json!([]))
    );
    runtime
        .state
        .recordings
        .refused_at(runtime.state.collector.clock().now_ms());
    assert_eq!(
        both_problems(&runtime, &cookie).await,
        (StatusCode::OK, json!(["recorder_budget"]))
    );
    runtime.close().await.unwrap();

    let full = Budgets {
        disk_reserve_bytes: u64::MAX,
        ..budgets()
    };
    let runtime = open(&data, full, Clock::system()).await;
    assert_eq!(
        both_problems(&runtime, &cookie).await,
        (StatusCode::SERVICE_UNAVAILABLE, json!(["disk_full"]))
    );
    runtime.close().await.unwrap();

    // Accepted an hour ago and still waiting: transfer works but is behind.
    let runtime = open_holding(&data).await;
    collect(&runtime, pageview("/late")).await;
    let inbox = inbox_pool(&data).await;
    sqlx::query("UPDATE inbox SET received_at = received_at - 3600000")
        .execute(&inbox)
        .await
        .unwrap();
    assert_eq!(
        both_problems(&runtime, &cookie).await,
        (StatusCode::OK, json!(["analytics_stale"]))
    );
    runtime.close().await.unwrap();

    // A record transfer cannot read: every attempt fails.
    sqlx::query("UPDATE inbox SET format = 1")
        .execute(&inbox)
        .await
        .unwrap();
    inbox.close().await;
    let runtime = open(&data, budgets(), Clock::system()).await;
    let deadline = Instant::now() + Duration::from_secs(10);
    while runtime.state.collector.storage().status().transfer_failures == 0 {
        assert!(Instant::now() < deadline, "transfer never failed");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(
        both_problems(&runtime, &cookie).await,
        (StatusCode::OK, json!(["transfer_failing"]))
    );
    runtime.close().await.unwrap();
}

async fn inbox_pool(dir: &Path) -> sqlx::SqlitePool {
    sqlx::SqlitePool::connect(&format!("sqlite://{}", dir.join("inbox.sqlite").display()))
        .await
        .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn diagnostics_carry_state_but_no_identity_inputs_credentials_or_bodies() {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../fixtures/gaps/diagnostics-canaries.json"
    ))
    .unwrap();
    let canary = |id: &str| -> String {
        fixture["canaries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["id"] == id)
            .unwrap()["value"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    let root = tempfile::tempdir().unwrap();
    let data = root.path().join("data");
    init::init(InitArgs {
        data_dir: data.clone(),
        admin_email: ADMIN.into(),
        admin_password: Some(canary("bootstrap_password")),
    })
    .await
    .unwrap();
    let runtime = open(&data, budgets(), Clock::system()).await;
    let db = &runtime.state.db;
    let workspace: String = sqlx::query_scalar("SELECT id FROM workspaces")
        .fetch_one(&db.reader)
        .await
        .unwrap();
    let mut conn = db.writer.acquire().await.unwrap();
    let site = holocron_server::sites::create_site(
        &mut conn,
        &holocron_protocol::ids::WorkspaceId::new(workspace),
        "c",
        HOST,
        "UTC",
    )
    .await
    .unwrap();
    holocron_server::sites::policy::mirror(
        &mut conn,
        runtime.state.collector.storage(),
        site.as_str(),
    )
    .await
    .unwrap();
    drop(conn);
    let body = canary("event_body");
    collect_as(
        &runtime,
        json!({ "type": "event", "payload": {
            "website": site.as_str(), "hostname": HOST, "url": "/c", "name": body,
            "data": { "note": body }, "id": canary("identify_id"),
        }}),
        &canary("ua"),
        &format!("{}:40000", canary("ip")),
    )
    .await;
    runtime.state.collector.storage().drain().await;

    let bundle = run_job(&runtime, Job::Diagnostics).await;
    runtime.close().await.unwrap();
    let text = bundle.to_string();
    for c in fixture["canaries"].as_array().unwrap() {
        let value = c["value"].as_str().unwrap();
        assert!(!text.contains(value), "{} leaked", c["id"]);
    }
    assert_eq!(bundle["holocron"], env!("CARGO_PKG_VERSION"));
    assert_eq!(bundle["schema"]["versions"], json!(versions::release()));
    assert_eq!(bundle["inbox"]["records"], 0);
    assert!(bundle["analytics"]["watermark_batch"].as_i64().unwrap() > 0);
    assert!(bundle["disk"]["analytics_bytes"].as_u64().unwrap() > 0);
    assert!(bundle["recording"]["cap_bytes"].as_u64().unwrap() > 0);
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn holocron(args: &[&str], data: &Path) -> std::process::Output {
    Command::new(BIN)
        .args(args)
        .arg("--data-dir")
        .arg(data)
        .env("HOLOCRON_LOG", "error")
        .output()
        .unwrap()
}

fn serve(data: &Path, port: u16) -> Child {
    let child = Command::new(BIN)
        .args(["serve", "--data-dir"])
        .arg(data)
        .args([
            "--bind",
            &format!("127.0.0.1:{port}"),
            "--disk-reserve-mib",
            "0",
        ])
        .env("HOLOCRON_LOG", "error")
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    while http(port, "GET", "/health", "").is_none() {
        assert!(Instant::now() < deadline, "server did not start");
        std::thread::sleep(Duration::from_millis(20));
    }
    child
}

fn http(port: u16, method: &str, path: &str, body: &str) -> Option<u16> {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).ok()?;
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .ok()?;
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {HOST}\r\nUser-Agent: Mozilla/5.0 Chrome/140.0.0.0\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(request.as_bytes()).ok()?;
    let mut answer = String::new();
    stream.read_to_string(&mut answer).ok()?;
    answer.split(' ').nth(1)?.parse().ok()
}

#[test]
fn the_binary_backs_up_while_serving_and_stops_in_order_on_sigterm() {
    let root = tempfile::tempdir().unwrap();
    let (data, backups) = (root.path().join("data"), root.path().join("backups"));
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(initialized(&data));
    let port = free_port();
    let mut server = serve(&data, port);
    for n in 0..20 {
        let body = pageview(&format!("/served/{n}")).to_string();
        assert_eq!(http(port, "POST", "/api/send", &body), Some(200));
    }
    assert_eq!(http(port, "GET", "/health/ready", ""), Some(200));

    // A second server on the same directory is refused; operator commands go to the first.
    let second = holocron(
        &["serve", "--bind", &format!("127.0.0.1:{}", free_port())],
        &data,
    );
    assert_eq!(second.status.code(), Some(4));
    let backup = holocron(&["backup", "--to", backups.to_str().unwrap()], &data);
    assert!(
        backup.status.success(),
        "{}",
        String::from_utf8_lossy(&backup.stderr)
    );
    let summary: Value = serde_json::from_slice(&backup.stdout).unwrap();
    assert!(summary["pause_ms"].is_u64());
    let diagnostics = holocron(&["diagnostics"], &data);
    assert!(diagnostics.status.success());

    let started = Instant::now();
    // SAFETY: signals a child process this test started.
    unsafe { libc::kill(server.id() as i32, libc::SIGTERM) };
    let status = server.wait().unwrap();
    assert!(status.success(), "{status:?}");
    assert!(started.elapsed() < Duration::from_secs(10));
    let wal = std::fs::metadata(data.join("analytics.duckdb.wal")).map_or(0, |m| m.len());
    assert_eq!(wal, 0, "the analytics WAL was checkpointed");

    // Restart: everything accepted before the stop is there.
    let events = runtime.block_on(async {
        let runtime = open(&data, budgets(), Clock::system()).await;
        runtime.state.collector.storage().drain().await;
        let events = count(&runtime, "SELECT count(*) FROM events").await;
        runtime.close().await.unwrap();
        events
    });
    assert_eq!(events, 20);
}

/// Data as the previous release left it: control without its last migration, analytics at
/// schema 3.
async fn older_data(data: &Path) {
    initialized(data).await;
    let db = holocron_server::db::Db::open(data).await.unwrap();
    sqlx::query(
        "DELETE FROM _sqlx_migrations WHERE version = ?;
         ALTER TABLE user_preferences DROP COLUMN accent;",
    )
    .bind(versions::release().control)
    .execute(&db.writer)
    .await
    .unwrap();
    db.close().await;
    set_analytics_schema(data, 3);
}

fn set_analytics_schema(data: &Path, schema: i64) {
    let conn = holocron_storage::duckdb::Connection::open(data.join("analytics.duckdb")).unwrap();
    conn.execute_batch(&format!(
        "UPDATE schema_info SET analytics_schema = {schema}"
    ))
    .unwrap();
}

/// `holocron serve` with `args` that is expected to exit; returns its status and stderr.
fn serve_refused(data: &Path, args: &[&str]) -> (Option<i32>, String) {
    let output = Command::new(BIN)
        .args(["serve", "--data-dir"])
        .arg(data)
        .args(["--bind", &format!("127.0.0.1:{}", free_port())])
        .args(args)
        .env("HOLOCRON_LOG", "info")
        .output()
        .unwrap();
    (
        output.status.code(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

fn stop(mut server: Child) {
    // SAFETY: signals a child process this test started.
    unsafe { libc::kill(server.id() as i32, libc::SIGTERM) };
    assert!(server.wait().unwrap().success());
}

fn kept_backups(backups: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(backups).map_or_else(
        |_| Vec::new(),
        |dir| dir.map(|entry| entry.unwrap().path()).collect(),
    )
}

#[test]
fn upgrade_on_start_upgrades_older_data_after_a_backup_and_serves() {
    let root = tempfile::tempdir().unwrap();
    let (data, backups) = (root.path().join("data"), root.path().join("backups"));
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(older_data(&data));

    // Without the flag nothing changes: older data is refused.
    let (code, stderr) = serve_refused(&data, &["--backup-dir", backups.to_str().unwrap()]);
    assert_eq!(code, Some(3), "{stderr}");
    assert!(stderr.contains("upgrade_required"), "{stderr}");
    assert!(kept_backups(&backups).is_empty());

    let port = free_port();
    let server = Command::new(BIN)
        .args(["serve", "--data-dir"])
        .arg(&data)
        .args([
            "--bind",
            &format!("127.0.0.1:{port}"),
            "--disk-reserve-mib",
            "0",
        ])
        .env("HOLOCRON_UPGRADE_ON_START", "1")
        .env("HOLOCRON_BACKUP_DIR", &backups)
        .env("HOLOCRON_LOG", "error")
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    while http(port, "GET", "/health", "").is_none() {
        assert!(Instant::now() < deadline, "server did not start");
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(http(port, "GET", "/health/ready", ""), Some(200));
    stop(server);

    let kept = kept_backups(&backups);
    assert_eq!(kept.len(), 1);
    runtime.block_on(async {
        let manifest = backup::verify(&kept[0]).await.unwrap();
        assert_eq!(manifest.versions.analytics, 3);
        versions::check_dir(&data).await.unwrap();
    });
}

#[test]
fn upgrade_on_start_refuses_without_a_usable_backup_and_leaves_the_data() {
    let root = tempfile::tempdir().unwrap();
    let data = root.path().join("data");
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(older_data(&data));
    let before = runtime.block_on(versions::stored(&data)).unwrap();

    // A file where the backup directory should be.
    let blocked = root.path().join("blocked");
    std::fs::write(&blocked, b"").unwrap();
    let (code, stderr) = serve_refused(
        &data,
        &[
            "--upgrade-on-start",
            "--backup-dir",
            blocked.to_str().unwrap(),
        ],
    );
    assert_eq!(code, Some(3), "{stderr}");
    assert!(stderr.contains("upgrade_backup_unavailable"), "{stderr}");

    // A volume that cannot hold the backup and the reserve.
    let backups = root.path().join("backups");
    let (code, stderr) = serve_refused(
        &data,
        &[
            "--upgrade-on-start",
            "--backup-dir",
            backups.to_str().unwrap(),
            "--disk-reserve-mib",
            "1073741824",
        ],
    );
    assert_eq!(code, Some(3), "{stderr}");
    assert!(stderr.contains("upgrade_backup_unavailable"), "{stderr}");
    assert!(kept_backups(&backups).is_empty());

    assert_eq!(runtime.block_on(versions::stored(&data)).unwrap(), before);
    assert!(!data.join(versions::UPGRADE_MARKER).exists());
}

#[test]
fn upgrade_on_start_resumes_an_interrupted_upgrade_without_a_second_backup() {
    use std::os::unix::fs::PermissionsExt;

    let root = tempfile::tempdir().unwrap();
    let (data, backups) = (root.path().join("data"), root.path().join("backups"));
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(older_data(&data));
    let analytics = data.join("analytics.duckdb");
    std::fs::set_permissions(&analytics, std::fs::Permissions::from_mode(0o444)).unwrap();
    let failed = runtime
        .block_on(upgrade::upgrade(&data, Some(&backups)))
        .unwrap_err();
    assert_eq!(failed.code, "upgrade_failed");
    std::fs::set_permissions(&analytics, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(kept_backups(&backups).len(), 1);

    let (code, stderr) = serve_refused(&data, &[]);
    assert_eq!(code, Some(3), "{stderr}");
    assert!(stderr.contains("upgrade_interrupted"), "{stderr}");

    let mut server = Command::new(BIN)
        .args(["serve", "--upgrade-on-start", "--data-dir"])
        .arg(&data)
        .args(["--bind", &format!("127.0.0.1:{}", free_port())])
        .args([
            "--backup-dir",
            backups.to_str().unwrap(),
            "--disk-reserve-mib",
            "0",
        ])
        .env("HOLOCRON_LOG", "error")
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    while data.join(versions::UPGRADE_MARKER).exists() {
        assert!(Instant::now() < deadline, "upgrade did not finish");
        std::thread::sleep(Duration::from_millis(50));
    }
    // SAFETY: signals a child process this test started.
    unsafe { libc::kill(server.id() as i32, libc::SIGTERM) };
    server.wait().unwrap();
    assert_eq!(kept_backups(&backups).len(), 1);
    runtime.block_on(versions::check_dir(&data)).unwrap();
}

#[test]
fn upgrade_on_start_never_downgrades() {
    let root = tempfile::tempdir().unwrap();
    let (data, backups) = (root.path().join("data"), root.path().join("backups"));
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(initialized(&data));
    set_analytics_schema(&data, 99);
    let (code, stderr) = serve_refused(
        &data,
        &[
            "--upgrade-on-start",
            "--backup-dir",
            backups.to_str().unwrap(),
        ],
    );
    assert_eq!(code, Some(3), "{stderr}");
    assert!(stderr.contains("data_newer_than_binary"), "{stderr}");
    assert!(kept_backups(&backups).is_empty());
}
