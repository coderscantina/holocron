//! Collection through the real router, identity derivation, SQLite inbox and DuckDB.

mod common;

use std::time::{Duration, Instant};

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode, header};
use holocron_server::clock::Clock;
use holocron_server::config::CollectConfig;
use holocron_storage::{Budgets, Lane, duckdb};
use serde_json::{Value, json};
use sqlx::Connection;

use common::{Answer, Harness, test_budgets};

/// 2026-10-01T10:00:00Z
const MORNING_MS: i64 = 1_790_848_800_000;
const DAY_MS: i64 = 86_400_000;
const PEER: &str = "198.51.100.7:40000";

fn pageview(site: &str, extra: Value) -> Value {
    let mut payload = json!({
        "website": site,
        "hostname": "example.test",
        "url": "/pricing?utm_source=news",
        "screen": "1920x1080",
        "language": "de-AT",
    });
    payload
        .as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    json!({ "type": "event", "payload": payload })
}

async fn query<T: Send + 'static>(
    harness: &Harness,
    f: impl FnOnce(&duckdb::Connection) -> duckdb::Result<T> + Send + 'static,
) -> T {
    let storage = harness.state.collector.storage();
    storage.drain().await;
    storage
        .read(Lane::Live, move |s| f(s.connection()))
        .await
        .unwrap()
        .value
}

async fn count(harness: &Harness, sql: &'static str) -> i64 {
    query(harness, move |c| c.query_row(sql, [], |r| r.get(0))).await
}

fn session(answer: &Answer) -> String {
    assert_eq!(
        answer.status,
        200,
        "{}",
        String::from_utf8_lossy(&answer.bytes)
    );
    answer.json()["sessionId"].as_str().unwrap().to_owned()
}

#[tokio::test(flavor = "multi_thread")]
async fn acknowledged_event_is_stored_with_its_pseudonym_and_typed_properties() {
    let harness = Harness::new().await;
    let site = harness.site("Europe/Vienna").await;
    let body = pageview(
        &site,
        json!({ "name": "signup", "data": { "plan": "pro", "price": 19.995, "revenue": 19.995, "currency": "EUR" } }),
    );
    let answer = harness.collect("POST", "/api/send", PEER, Some(body)).await;
    let session_id = session(&answer);

    let row = query(&harness, |c| {
        c.query_row(
            "SELECT kind, path, utm_source, browser, device, revenue_minor, daily_id::VARCHAR,
                    (SELECT count(*) FROM event_properties) FROM events",
            [],
            |r| {
                Ok((
                    r.get::<_, u8>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, i64>(5)?,
                    r.get::<_, String>(6)?,
                    r.get::<_, i64>(7)?,
                ))
            },
        )
    })
    .await;
    let expected_daily = uuid::Uuid::parse_str(&session_id)
        .unwrap()
        .as_u128()
        .to_string();
    assert_eq!(
        row,
        (
            2,
            "/pricing".into(),
            "news".into(),
            "chrome".into(),
            "laptop".into(),
            2000,
            expected_daily,
            4
        )
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn raw_ip_and_user_agent_never_reach_the_inbox() {
    // Transfer is stalled, so the admitted record stays in the SQLite inbox to be inspected.
    let budgets = Budgets {
        transfer_delay: Duration::from_secs(3600),
        ..test_budgets()
    };
    let harness = Harness::with(CollectConfig::default(), budgets, Clock::system()).await;
    let site = harness.site("UTC").await;
    let answer = harness
        .collect(
            "POST",
            "/api/send",
            "203.0.113.99:1",
            Some(pageview(&site, json!({ "id": "user-4711" }))),
        )
        .await;
    assert_eq!(answer.status, 200);

    let inbox = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new()
            .filename(harness.data_dir().join("inbox.sqlite"))
            .read_only(true),
    )
    .await
    .unwrap();
    let payloads: Vec<Vec<u8>> = sqlx::query_scalar("SELECT payload FROM inbox")
        .fetch_all(&inbox)
        .await
        .unwrap();
    assert_eq!(payloads.len(), 1);
    for secret in ["203.0.113.99", "Macintosh", "user-4711"] {
        assert!(
            !payloads[0]
                .windows(secret.len())
                .any(|w| w == secret.as_bytes()),
            "{secret} was stored"
        );
    }
    inbox.close().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn daily_pseudonyms_rotate_at_midnight_and_follow_identify() {
    let clock = Clock::frozen(MORNING_MS);
    let harness = Harness::with(CollectConfig::default(), test_budgets(), clock.clone()).await;
    let site = harness.site("UTC").await;
    let send = |peer: &'static str, extra: Value| {
        let body = pageview(&site, extra);
        let harness = &harness;
        async move { session(&harness.collect("POST", "/api/send", peer, Some(body)).await) }
    };

    let first = send(PEER, json!({})).await;
    assert_eq!(send(PEER, json!({})).await, first, "same visitor, same day");
    assert_ne!(
        send("198.51.100.8:1", json!({})).await,
        first,
        "another address"
    );
    assert_eq!(
        send(PEER, json!({ "id": "user-1" })).await,
        send("198.51.100.8:1", json!({ "id": "user-1" })).await,
        "identified on two devices"
    );

    clock.set(MORNING_MS + DAY_MS);
    assert_ne!(send(PEER, json!({})).await, first, "next day, new key");
}

#[tokio::test(flavor = "multi_thread")]
async fn events_for_a_destroyed_day_key_are_stored_keyless() {
    let clock = Clock::frozen(MORNING_MS);
    let harness = Harness::with(CollectConfig::default(), test_budgets(), clock.clone()).await;
    let site = harness.site("UTC").await;
    let late = pageview(&site, json!({ "timestamp": MORNING_MS / 1000 }));
    assert_eq!(
        harness
            .collect("POST", "/api/send", PEER, Some(late.clone()))
            .await
            .status,
        200
    );

    // end(2026-10-01) + 25 hours.
    clock.set(MORNING_MS - 10 * 3_600_000 + DAY_MS + 25 * 3_600_000);
    let destroyed = harness
        .state
        .collector
        .pseudonyms()
        .destroy_expired(clock.now_ms())
        .await
        .unwrap();
    assert_eq!(destroyed, 1);
    assert_eq!(
        harness
            .collect("POST", "/api/send", PEER, Some(late))
            .await
            .status,
        200
    );

    let (keyed, keyless) = query(&harness, |c| {
        c.query_row(
            "SELECT count(daily_id), count(*) FILTER (WHERE daily_id IS NULL) FROM events",
            [],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)),
        )
    })
    .await;
    assert_eq!((keyed, keyless), (1, 1));
    let tombstone: Option<Vec<u8>> = sqlx::query_scalar("SELECT key FROM identity_day_keys")
        .fetch_one(&harness.state.db.reader)
        .await
        .unwrap();
    assert_eq!(tombstone, None);
}

#[tokio::test(flavor = "multi_thread")]
async fn batch_items_are_reported_in_order_and_committed_together() {
    let harness = Harness::new().await;
    let site = harness.site("UTC").await;
    let batch = json!([
        pageview(&site, json!({})),
        { "type": "event", "payload": { "website": "not-a-uuid" } },
        pageview(&site, json!({ "name": "signup" })),
    ]);
    let answer = harness
        .collect("POST", "/api/batch", PEER, Some(batch))
        .await;
    assert_eq!(answer.status, 200);
    let body = answer.json();
    assert_eq!(
        (
            body["size"].clone(),
            body["processed"].clone(),
            body["errors"].clone()
        ),
        (json!(3), json!(2), json!(1))
    );
    assert_eq!(body["details"][0]["index"], 1);
    assert_eq!(count(&harness, "SELECT count(*) FROM events").await, 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn storage_outage_refuses_collection_but_links_still_redirect() {
    let budgets = Budgets {
        disk_reserve_bytes: u64::MAX,
        ..test_budgets()
    };
    let harness = Harness::with(CollectConfig::default(), budgets, Clock::system()).await;
    let site = harness.site("UTC").await;
    let workspace = harness
        .workspaces
        .values()
        .next()
        .unwrap()
        .as_str()
        .to_owned();
    let link = uuid::Uuid::new_v4().to_string();
    sqlx::query("INSERT INTO links (id, workspace_id, name, slug, url, created_at) VALUES (?, ?, 'l', 'go', 'https://example.com/', 0)")
        .bind(&link)
        .bind(&workspace)
        .execute(&harness.state.db.writer)
        .await
        .unwrap();

    let send = harness
        .collect("POST", "/api/send", PEER, Some(pageview(&site, json!({}))))
        .await;
    assert_eq!(send.status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(send.header("retry-after"), "5");
    let batch = harness
        .collect(
            "POST",
            "/api/batch",
            PEER,
            Some(json!([pageview(&site, json!({}))])),
        )
        .await;
    assert_eq!(batch.status, StatusCode::SERVICE_UNAVAILABLE);

    let hit = harness.collect("GET", "/q/go", PEER, None).await;
    assert_eq!(hit.status, StatusCode::TEMPORARY_REDIRECT);
    let lost: i64 = sqlx::query_scalar("SELECT lost_hits FROM links WHERE id = ?")
        .bind(&link)
        .fetch_one(&harness.state.db.reader)
        .await
        .unwrap();
    assert_eq!(lost, 1);
    assert_eq!(harness.state.collector.storage().status().inbox_records, 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn locked_inbox_is_refused_within_the_busy_timeout() {
    let harness = Harness::new().await;
    let site = harness.site("UTC").await;
    let mut lock = sqlx::SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new()
            .filename(harness.data_dir().join("inbox.sqlite")),
    )
    .await
    .unwrap();
    sqlx::query("BEGIN EXCLUSIVE")
        .execute(&mut lock)
        .await
        .unwrap();

    let started = Instant::now();
    let refused = harness
        .collect("POST", "/api/send", PEER, Some(pageview(&site, json!({}))))
        .await;
    assert_eq!(refused.status, StatusCode::SERVICE_UNAVAILABLE);
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "{:?}",
        started.elapsed()
    );

    sqlx::query("ROLLBACK").execute(&mut lock).await.unwrap();
    lock.close().await.unwrap();
    let accepted = harness
        .collect("POST", "/api/send", PEER, Some(pageview(&site, json!({}))))
        .await;
    assert_eq!(accepted.status, 200);
}

#[tokio::test(flavor = "multi_thread")]
async fn full_inbox_backlog_answers_429_and_stops_growing() {
    // Transfer is stalled; the inbox budget holds a handful of records.
    let budgets = Budgets {
        inbox_bytes: 2_000,
        transfer_delay: Duration::from_secs(3600),
        ..test_budgets()
    };
    let harness = Harness::with(CollectConfig::default(), budgets, Clock::system()).await;
    let site = harness.site("UTC").await;
    let mut statuses = Vec::new();
    for _ in 0..20 {
        statuses.push(
            harness
                .collect("POST", "/api/send", PEER, Some(pageview(&site, json!({}))))
                .await
                .status,
        );
    }
    let accepted = statuses.iter().filter(|s| **s == StatusCode::OK).count();
    assert!(
        accepted > 0 && statuses.ends_with(&[StatusCode::TOO_MANY_REQUESTS]),
        "{statuses:?}"
    );
    let status = harness.state.collector.storage().status();
    assert_eq!(status.inbox_records as usize, accepted);
    assert!(status.inbox_bytes <= 2_000);
}

#[tokio::test(flavor = "multi_thread")]
async fn in_flight_body_budget_refuses_instead_of_buffering() {
    let collect = CollectConfig {
        intake_bytes: 100 * 1024,
        ..CollectConfig::default()
    };
    let harness = Harness::with(collect, test_budgets(), Clock::system()).await;
    let site = harness.site("UTC").await;

    // A slow client declares 60 KiB and stops sending: its share stays reserved.
    let (sender, channel) = http_body_util::channel::Channel::<axum::body::Bytes>::new(1);
    let slow = Request::post("/api/send")
        .header(header::CONTENT_LENGTH, 60 * 1024)
        .extension(ConnectInfo(PEER.parse::<std::net::SocketAddr>().unwrap()))
        .body(Body::new(channel))
        .unwrap();
    let pending = {
        let router = holocron_server::router(harness.state.clone());
        tokio::spawn(async move { tower::ServiceExt::oneshot(router, slow).await })
    };
    tokio::time::sleep(Duration::from_millis(100)).await;

    let second = Request::post("/api/send")
        .header(header::CONTENT_LENGTH, 60 * 1024)
        .extension(ConnectInfo(PEER.parse::<std::net::SocketAddr>().unwrap()))
        .body(Body::from(vec![b' '; 60 * 1024]))
        .unwrap();
    assert_eq!(
        harness.send(second).await.status,
        StatusCode::TOO_MANY_REQUESTS
    );

    drop(sender);
    pending.await.unwrap().unwrap();
    let small = harness
        .collect("POST", "/api/send", PEER, Some(pageview(&site, json!({}))))
        .await;
    assert_eq!(small.status, 200, "the budget is released after an answer");
}

#[tokio::test(flavor = "multi_thread")]
async fn stored_counts_reconcile_with_counted_outcomes() {
    let harness = Harness::new().await;
    let site = harness.site("Europe/Vienna").await;
    let admin = harness.session("p_install_admin").await;
    let bot = "Mozilla/5.0 (compatible; Googlebot/2.1; +http://www.google.com/bot.html)";
    let mut expected_stored = 0;

    for i in 0..20 {
        let answer = harness
            .collect(
                "POST",
                "/api/send",
                PEER,
                Some(pageview(&site, json!({ "url": format!("/p/{i}") }))),
            )
            .await;
        assert_eq!(answer.status, 200);
        expected_stored += 1;
    }
    for _ in 0..3 {
        let duplicate = pageview(&site, json!({ "eventId": "order-17" }));
        assert_eq!(
            harness
                .collect("POST", "/api/send", PEER, Some(duplicate))
                .await
                .status,
            200
        );
    }
    expected_stored += 1;
    let old = pageview(&site, json!({ "timestamp": 1_000_000_000 }));
    assert_eq!(
        harness
            .collect("POST", "/api/send", PEER, Some(old))
            .await
            .status,
        200
    );
    let invalid = json!({ "type": "event", "payload": { "website": site, "lcp": -1 } });
    assert_eq!(
        harness
            .collect("POST", "/api/send", PEER, Some(invalid))
            .await
            .status,
        400
    );
    let request = Request::post("/api/send")
        .header(header::USER_AGENT, bot)
        .extension(ConnectInfo(PEER.parse::<std::net::SocketAddr>().unwrap()))
        .body(Body::from(pageview(&site, json!({})).to_string()))
        .unwrap();
    assert_eq!(
        harness.send(request).await.json(),
        json!({ "beep": "boop" })
    );
    let batch: Vec<Value> = (0..10)
        .map(|_| pageview(&site, json!({ "name": "batched" })))
        .collect();
    assert_eq!(
        harness
            .collect("POST", "/api/batch", PEER, Some(json!(batch)))
            .await
            .status,
        200
    );
    expected_stored += 10;

    let stored = count(&harness, "SELECT count(*) FROM events").await;
    assert_eq!(stored, expected_stored);
    let status = harness
        .query(Some(&admin), "collection.status", "")
        .await
        .json();
    let counted = |outcome: &str| -> i64 {
        status["outcomes"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| row["outcome"] == outcome)
            .map(|row| row["count"].as_i64().unwrap())
            .sum()
    };
    assert_eq!(counted("accepted"), stored);
    assert_eq!(
        (
            counted("duplicate"),
            counted("suppressed"),
            counted("refused")
        ),
        (2, 2, 1)
    );
    assert_eq!(status["inbox_records"], 0);
    assert_eq!(status["availability"], "available");
}

#[tokio::test(flavor = "multi_thread")]
async fn collection_status_is_for_install_admins_only() {
    let harness = Harness::new().await;
    let owner = harness.session("p_owner").await;
    let answer = harness.query(Some(&owner), "collection.status", "").await;
    assert_eq!(
        (answer.status.as_u16(), answer.code()),
        (403, "forbidden".to_owned())
    );
    assert_eq!(
        harness.query(None, "collection.status", "").await.status,
        401
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn tracking_code_names_the_tracker_aliases_and_the_recorder_when_recording() {
    let collect = CollectConfig {
        tracker_script_names: vec!["stats.js".into()],
        ..CollectConfig::default()
    };
    let harness = Harness::with(collect, test_budgets(), Clock::system()).await;
    let site = harness.site("Europe/Vienna").await;
    let member = harness.session("p_member").await;
    let read = || async {
        let answer = harness
            .query(
                Some(&member),
                "sites.trackingCode",
                &format!("site={site}&origin=https%3A%2F%2Fstats.example.com%2Fsettings"),
            )
            .await;
        assert_eq!(
            answer.status,
            200,
            "{}",
            String::from_utf8_lossy(&answer.bytes)
        );
        answer.json()
    };
    let tag = |path: &str| {
        format!(
            r#"<script defer src="https://stats.example.com{path}" data-website-id="{site}"></script>"#
        )
    };

    let code = read().await;
    assert_eq!(
        code["tracker"],
        json!([
            { "path": "/stats.js", "code": tag("/stats.js") },
            { "path": "/script.js", "code": tag("/script.js") },
        ])
    );
    assert_eq!(code.get("recorder"), None, "recording is off");

    let policy = json!({ "idempotency_key": "record", "site": site, "replay_enabled": true,
        "heatmap_enabled": false, "replay_sample_percent": 10, "heatmap_sample_percent": 10,
        "max_duration_minutes": 5, "excluded_elements": [], "excluded_paths": [] });
    let updated = harness
        .command(Some(&member), "recordings.updatePolicy", policy)
        .await;
    assert_eq!(
        updated.status,
        200,
        "{}",
        String::from_utf8_lossy(&updated.bytes)
    );
    assert_eq!(
        read().await["recorder"],
        json!({ "path": "/recorder.js", "code": tag("/recorder.js") })
    );
}

/// Creates a collection key for `site` as `p_member` and returns its secret and ID.
async fn collection_key(harness: &Harness, site: &str) -> (String, String) {
    let member = harness.session("p_member").await;
    let body = json!({ "idempotency_key": uuid::Uuid::new_v4().to_string(), "site": site, "name": "Backend" });
    let answer = harness
        .command(Some(&member), "collectionKeys.create", body)
        .await;
    assert_eq!(
        answer.status,
        200,
        "{}",
        String::from_utf8_lossy(&answer.bytes)
    );
    let created = answer.json();
    (
        created["secret"].as_str().unwrap().to_owned(),
        created["key"]["id"].as_str().unwrap().to_owned(),
    )
}

/// A server integration's request: its own address and user agent, with `authorization`.
async fn from_server(harness: &Harness, path: &str, authorization: &str, body: Value) -> Answer {
    let request = Request::post(path)
        .header(header::USER_AGENT, "python-requests/2.32")
        .header(header::AUTHORIZATION, authorization)
        .header(header::CONTENT_TYPE, "application/json")
        .extension(ConnectInfo(
            "192.0.2.50:443".parse::<std::net::SocketAddr>().unwrap(),
        ))
        .body(Body::from(body.to_string()))
        .unwrap();
    harness.send(request).await
}

#[tokio::test(flavor = "multi_thread")]
async fn collection_keys_make_payload_ip_and_user_agent_the_visitor() {
    let harness = Harness::new().await;
    let site = harness.site("Europe/Vienna").await;
    let (secret, _) = collection_key(&harness, &site).await;
    let browser = session(
        &harness
            .collect("POST", "/api/send", PEER, Some(pageview(&site, json!({}))))
            .await,
    );

    // The server's own address and its bot-like user agent are not the visitor.
    let visitor = json!({
        "ip": "198.51.100.7",
        "userAgent": "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36",
    });
    let keyed = from_server(
        &harness,
        "/api/send",
        &format!("Bearer {secret}"),
        pageview(&site, visitor.clone()),
    )
    .await;
    assert_eq!(session(&keyed), browser, "same visitor, same pseudonym");

    // Without the key the payload fields are ignored, as before.
    let unkeyed = harness
        .collect(
            "POST",
            "/api/send",
            "192.0.2.50:443",
            Some(pageview(&site, visitor)),
        )
        .await;
    assert_ne!(session(&unkeyed), browser);
    let browsers = count(
        &harness,
        "SELECT count(*) FROM events WHERE browser = 'chrome'",
    )
    .await;
    assert_eq!(browsers, 3);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_keyed_batch_carries_consent_from_a_trusted_server() {
    let harness = Harness::new().await;
    let site = harness.site("Europe/Vienna").await;
    let owner = harness.session("p_owner").await;
    let policy =
        json!({ "idempotency_key": "persistent", "site": site, "persistent_identity": true });
    assert_eq!(
        harness
            .command(Some(&owner), "sites.updateIdentityPolicy", policy)
            .await
            .status,
        200
    );
    let (secret, _) = collection_key(&harness, &site).await;
    let identify = json!({ "type": "identify", "payload": {
        "website": site, "id": "anna", "consent": "persistent", "ip": "203.0.113.9" } });
    let event = pageview(&site, json!({ "id": "anna", "ip": "203.0.113.9" }));

    let answer = from_server(
        &harness,
        "/api/batch",
        &format!("Bearer {secret}"),
        json!([identify, event]),
    )
    .await;
    assert_eq!(answer.json()["errors"], 0, "{}", answer.json());
    let consents: i64 =
        sqlx::query_scalar("SELECT count(*) FROM consents WHERE revoked_at IS NULL")
            .fetch_one(&harness.state.db.reader)
            .await
            .unwrap();
    assert_eq!(consents, 1);
    let linked = count(
        &harness,
        "SELECT count(*) FROM events WHERE profile_id IS NOT NULL",
    )
    .await;
    assert_eq!(linked, 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn unknown_revoked_and_foreign_collection_keys_are_refused() {
    let harness = Harness::new().await;
    let site = harness.site("Europe/Vienna").await;
    let other = harness.site("Europe/Vienna").await;
    let (secret, id) = collection_key(&harness, &site).await;
    let unauthorized =
        json!({ "error": { "message": "Unauthorized", "code": "unauthorized", "status": 401 } });

    let unknown = from_server(
        &harness,
        "/api/send",
        "Bearer not-a-key",
        pageview(&site, json!({})),
    )
    .await;
    assert_eq!(
        (unknown.status, unknown.json()),
        (StatusCode::UNAUTHORIZED, unauthorized.clone())
    );

    // A key vouches for its own site only; the other item of the batch is stored.
    let batch = json!([pageview(&other, json!({})), pageview(&site, json!({}))]);
    let foreign = from_server(&harness, "/api/batch", &format!("Bearer {secret}"), batch).await;
    assert_eq!(
        foreign.json()["details"],
        json!([{ "index": 0, "response": unauthorized }])
    );

    let member = harness.session("p_member").await;
    let revoke = json!({ "idempotency_key": "revoke", "site": site, "key": id });
    assert_eq!(
        harness
            .command(Some(&member), "collectionKeys.revoke", revoke)
            .await
            .status,
        200
    );
    let revoked = from_server(
        &harness,
        "/api/send",
        &format!("Bearer {secret}"),
        pageview(&site, json!({})),
    )
    .await;
    assert_eq!(revoked.status, StatusCode::UNAUTHORIZED);

    // Another scheme is no collection key: the request is collected as without one, so the
    // connection's bot user agent counts and the payload's is ignored.
    let basic = from_server(
        &harness,
        "/api/send",
        "Basic dXNlcjpwYXNz",
        pageview(
            &site,
            json!({ "userAgent": "Mozilla/5.0 (Windows NT 10.0)" }),
        ),
    )
    .await;
    assert_eq!(basic.json(), json!({ "beep": "boop" }));
    assert_eq!(count(&harness, "SELECT count(*) FROM events").await, 1);
}
