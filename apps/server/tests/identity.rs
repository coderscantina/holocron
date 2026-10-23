//! Milestone 05 over HTTP: tracker payloads through correction to sessions and freshness,
//! consent grant and revocation, session and identity policy, and recalculation.

mod common;

use std::time::{Duration, Instant};

use holocron_server::background::Background;
use holocron_server::clock::Clock;
use holocron_server::config::CollectConfig;
use serde_json::{Value, json};

use common::{Harness, test_budgets};

/// 2026-10-01T10:00:00Z, 12:00 in Vienna.
const MORNING_MS: i64 = 1_790_848_800_000;
const MINUTE_MS: i64 = 60_000;
const DAY_MS: i64 = 86_400_000;
const PEER: &str = "198.51.100.7:40000";

async fn harness() -> (Harness, Clock) {
    let clock = Clock::frozen(MORNING_MS);
    let harness = Harness::with(CollectConfig::default(), test_budgets(), clock.clone()).await;
    (harness, clock)
}

fn payload(kind: &str, site: &str, extra: Value) -> Value {
    let mut payload = json!({
        "website": site,
        "hostname": "example.test",
        "url": "/pricing",
        "screen": "1920x1080",
        "language": "de-AT",
    });
    payload
        .as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    json!({ "type": kind, "payload": payload })
}

async fn send(harness: &Harness, body: Value) {
    let answer = harness.collect("POST", "/api/send", PEER, Some(body)).await;
    assert_eq!(
        answer.status,
        200,
        "{}",
        String::from_utf8_lossy(&answer.bytes)
    );
}

async fn command(harness: &Harness, cookie: &str, name: &str, mut body: Value) -> Value {
    body["idempotency_key"] = json!(uuid::Uuid::new_v4().to_string());
    let answer = harness.command(Some(cookie), name, body).await;
    assert_eq!(
        answer.status,
        200,
        "{name}: {}",
        String::from_utf8_lossy(&answer.bytes)
    );
    answer.json()
}

async fn freshness(harness: &Harness, cookie: &str, site: &str) -> Value {
    let range = format!(
        "site={site}&start={}&end={}",
        MORNING_MS - DAY_MS,
        MORNING_MS + DAY_MS
    );
    let answer = harness
        .query(Some(cookie), "analytics.freshness", &range)
        .await;
    assert_eq!(
        answer.status,
        200,
        "{}",
        String::from_utf8_lossy(&answer.bytes)
    );
    answer.json()
}

/// Polls freshness until the range is corrected for everything admitted so far.
async fn corrected(harness: &Harness, cookie: &str, site: &str) -> Value {
    harness.state.collector.storage().drain().await;
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let fresh = freshness(harness, cookie, site).await;
        if fresh["correction"] == "corrected" {
            return fresh;
        }
        assert!(Instant::now() < deadline, "still pending: {fresh}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// Daily sessions: count, pageviews, first start.
async fn sessions(harness: &Harness) -> (i64, i64, i64) {
    harness
        .analytics(|c| {
            c.query_row(
                "SELECT count(*), coalesce(sum(pageviews), 0)::BIGINT, coalesce(min(epoch_ms(start_time)), 0)
                 FROM sessions WHERE population = 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
        })
        .await
}

#[tokio::test(flavor = "multi_thread")]
async fn tracker_payloads_become_corrected_sessions_and_freshness_says_when() {
    let (harness, clock) = harness().await;
    let site = harness.site("Europe/Vienna").await;
    let viewer = harness.session("p_viewer").await;

    send(&harness, payload("event", &site, json!({}))).await;
    clock.set(MORNING_MS + 10 * MINUTE_MS);
    send(&harness, payload("event", &site, json!({}))).await;
    harness.state.collector.storage().drain().await;
    let fresh = freshness(&harness, &viewer, &site).await;
    assert_eq!(
        (fresh["correction"].as_str(), fresh["dirty_days"].as_i64()),
        (Some("pending"), Some(1))
    );

    let background = Background::start(&harness.state);
    let fresh = corrected(&harness, &viewer, &site).await;
    assert_eq!(fresh["corrected_through"], fresh["watermark_batch"]);
    assert_eq!(fresh["session_policy_versions"], json!([1]));
    assert_eq!(fresh["metric_version"], 1);
    assert_eq!(sessions(&harness).await, (1, 2, MORNING_MS));

    // A late event 15 minutes before the first one extends the session backwards.
    send(
        &harness,
        payload(
            "event",
            &site,
            json!({ "timestamp": (MORNING_MS - 15 * MINUTE_MS) / 1000 }),
        ),
    )
    .await;
    corrected(&harness, &viewer, &site).await;
    assert_eq!(
        sessions(&harness).await,
        (1, 3, MORNING_MS - 15 * MINUTE_MS)
    );
    background.stop().await;
}

/// Events, linked events and revocations as analytics stored them.
async fn linked(harness: &Harness) -> (i64, i64, Vec<Option<String>>) {
    harness
        .analytics(|c| {
            let (events, linked) = c.query_row(
                "SELECT count(*), count(profile_id) FROM events WHERE kind = 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            let mut statement = c.prepare(
                "SELECT profile_id::VARCHAR FROM consent_revocations ORDER BY revoked_at_ms",
            )?;
            let revocations = statement
                .query_map([], |r| r.get(0))?
                .collect::<Result<Vec<_>, _>>()?;
            Ok((events, linked, revocations))
        })
        .await
}

#[tokio::test(flavor = "multi_thread")]
async fn consent_links_events_until_it_is_revoked() {
    let (harness, clock) = harness().await;
    let site = harness.site("Europe/Vienna").await;
    let owner = harness.session("p_owner").await;
    let anna = json!({ "id": "anna" });
    let grant = json!({ "id": "anna", "consent": "persistent" });

    // Without persistent identity the signal is ignored.
    send(&harness, payload("identify", &site, grant.clone())).await;
    send(&harness, payload("event", &site, anna.clone())).await;
    assert_eq!(linked(&harness).await, (1, 0, vec![]));

    command(
        &harness,
        &owner,
        "sites.updateIdentityPolicy",
        json!({ "site": site, "persistent_identity": true }),
    )
    .await;
    clock.set(MORNING_MS + MINUTE_MS);
    send(&harness, payload("identify", &site, grant)).await;
    send(&harness, payload("event", &site, anna.clone())).await;
    clock.set(MORNING_MS + 2 * MINUTE_MS);
    send(
        &harness,
        payload(
            "identify",
            &site,
            json!({ "id": "anna", "consent": "none" }),
        ),
    )
    .await;
    send(&harness, payload("event", &site, anna)).await;

    let (events, linked_events, revocations) = linked(&harness).await;
    assert_eq!(
        (events, linked_events),
        (3, 1),
        "only the event inside the consent interval links"
    );
    let profile: String = harness
        .analytics(|c| {
            c.query_row(
                "SELECT profile_id::VARCHAR FROM events WHERE profile_id IS NOT NULL",
                [],
                |r| r.get(0),
            )
        })
        .await;
    assert_eq!(revocations, vec![Some(profile)]);
    let interval: (i64, Option<i64>) =
        sqlx::query_as("SELECT granted_at, revoked_at FROM consents")
            .fetch_one(&harness.state.db.reader)
            .await
            .unwrap();
    assert_eq!(
        interval,
        (MORNING_MS + MINUTE_MS, Some(MORNING_MS + 2 * MINUTE_MS))
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn turning_persistent_identity_off_revokes_every_profile() {
    let (harness, _clock) = harness().await;
    let site = harness.site("Europe/Vienna").await;
    let owner = harness.session("p_owner").await;
    let viewer = harness.session("p_viewer").await;

    let denied = harness
        .query(
            Some(&viewer),
            "sites.identityPolicy",
            &format!("site={site}"),
        )
        .await;
    assert_eq!(denied.code(), "forbidden");
    let on = command(
        &harness,
        &owner,
        "sites.updateIdentityPolicy",
        json!({ "site": site, "persistent_identity": true }),
    )
    .await;
    assert_eq!(
        (
            on["persistent_identity"].as_bool(),
            on["identity_policy_version"].as_i64()
        ),
        (Some(true), Some(2))
    );
    for id in ["anna", "ben"] {
        send(
            &harness,
            payload(
                "identify",
                &site,
                json!({ "id": id, "consent": "persistent" }),
            ),
        )
        .await;
    }

    let off = command(
        &harness,
        &owner,
        "sites.updateIdentityPolicy",
        json!({ "site": site, "persistent_identity": false }),
    )
    .await;
    assert_eq!(off["identity_policy_version"], 3);
    let read = harness
        .query(
            Some(&owner),
            "sites.identityPolicy",
            &format!("site={site}"),
        )
        .await
        .json();
    assert_eq!(
        (
            read["persistent_identity"].as_bool(),
            read["identity_policy_version"].as_i64()
        ),
        (Some(false), Some(3))
    );
    let open: i64 = sqlx::query_scalar("SELECT count(*) FROM consents WHERE revoked_at IS NULL")
        .fetch_one(&harness.state.db.reader)
        .await
        .unwrap();
    assert_eq!(open, 0);
    send(&harness, payload("event", &site, json!({ "id": "anna" }))).await;
    assert_eq!(
        linked(&harness).await,
        (1, 0, vec![None]),
        "one site-wide revocation, nothing links"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn session_policy_versions_reach_collection_and_analytics() {
    let (harness, clock) = harness().await;
    let site = harness.site("Europe/Vienna").await;
    let owner = harness.session("p_owner").await;
    let viewer = harness.session("p_viewer").await;
    let change = |body: Value| {
        let mut body = body;
        body["site"] = json!(site);
        body["idempotency_key"] = json!(uuid::Uuid::new_v4().to_string());
        body
    };

    let denied = harness
        .command(
            Some(&viewer),
            "sites.updateSessionPolicy",
            change(json!({ "timezone": "UTC", "window_minutes": 30 })),
        )
        .await;
    assert_eq!(denied.code(), "forbidden");
    let retroactive = harness
        .command(
            Some(&owner),
            "sites.updateSessionPolicy",
            change(json!({ "timezone": "UTC", "window_minutes": 30, "effective_at": MORNING_MS - MINUTE_MS })),
        )
        .await;
    assert_eq!(retroactive.code(), "invalid_arguments");
    assert!(retroactive.json()["errors"]["effective_at"].is_array());

    let window = command(
        &harness,
        &owner,
        "sites.updateSessionPolicy",
        change(json!({ "timezone": "Europe/Vienna", "window_minutes": 15 })),
    )
    .await;
    assert_eq!(
        (
            window["site"]["session_window_minutes"].as_i64(),
            window["site"]["session_policy_version"].as_i64()
        ),
        (Some(15), Some(2))
    );
    let moved = command(
        &harness,
        &owner,
        "sites.updateSessionPolicy",
        change(json!({ "timezone": "UTC", "window_minutes": 15, "effective_at": MORNING_MS + 60 * MINUTE_MS })),
    )
    .await;
    assert_eq!(
        (
            moved["site"]["timezone"].as_str(),
            moved["site"]["session_policy_version"].as_i64()
        ),
        (Some("UTC"), Some(3))
    );

    let copy: Vec<(i32, i32, String, i64)> = harness
        .analytics(|c| {
            let mut statement = c.prepare(
                "SELECT version, epoch, timezone, window_ms FROM session_policies ORDER BY version",
            )?;
            statement
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
                .collect()
        })
        .await;
    assert_eq!(
        copy,
        [
            (1, 1, "Europe/Vienna".into(), 30 * MINUTE_MS),
            (2, 1, "Europe/Vienna".into(), 15 * MINUTE_MS),
            (3, 2, "UTC".into(), 15 * MINUTE_MS),
        ]
    );

    // Events either side of the change take day keys of their own epoch.
    send(&harness, payload("event", &site, json!({}))).await;
    clock.set(MORNING_MS + 61 * MINUTE_MS);
    send(&harness, payload("event", &site, json!({}))).await;
    let epochs: Vec<i64> =
        sqlx::query_scalar("SELECT timezone_epoch FROM identity_day_keys ORDER BY timezone_epoch")
            .fetch_all(&harness.state.db.reader)
            .await
            .unwrap();
    assert_eq!(epochs, [1, 2]);
}

#[tokio::test(flavor = "multi_thread")]
async fn recalculation_reports_its_status() {
    let (harness, _clock) = harness().await;
    let site = harness.site("Europe/Vienna").await;
    let owner = harness.session("p_owner").await;
    // Running and idle before the recalculation: nothing but the command can wake it.
    let background = Background::start(&harness.state);
    tokio::time::sleep(Duration::from_millis(100)).await;

    let backwards = harness
        .command(
            Some(&owner),
            "recalculations.start",
            json!({ "idempotency_key": "r0", "site": site, "first_day": "2026-10-02", "last_day": "2026-10-01" }),
        )
        .await;
    assert_eq!(backwards.code(), "invalid_arguments");

    let started = command(
        &harness,
        &owner,
        "recalculations.start",
        json!({ "site": site, "first_day": "2026-09-30", "last_day": "2026-10-01" }),
    )
    .await;
    let job = &started["recalculation"];
    assert_eq!(job["pending_days"].as_i64(), Some(2));

    let status = format!("site={site}&recalculation={}", job["id"].as_str().unwrap());
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let job = harness
            .query(Some(&owner), "recalculations.status", &status)
            .await
            .json();
        if job["recalculation"]["state"] == "corrected" {
            assert_eq!(job["recalculation"]["pending_days"], 0);
            break;
        }
        assert!(Instant::now() < deadline, "still pending: {job}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    background.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn startup_repairs_the_analytical_policy_copy() {
    let (harness, _clock) = harness().await;
    let site = harness.site("Europe/Vienna").await;
    // A version the owner committed while analytics missed it, as after a crash between the two.
    sqlx::query(
        "INSERT INTO session_policies (site_id, version, epoch, effective_at, timezone, window_minutes, created_at)
         VALUES (?, 2, 1, ?, 'Europe/Vienna', 10, 0)",
    )
    .bind(&site)
    .bind(MORNING_MS)
    .execute(&harness.state.db.writer)
    .await
    .unwrap();

    holocron_server::sites::policy::mirror_all(
        &harness.state.db,
        harness.state.collector.storage(),
    )
    .await
    .unwrap();
    let id = uuid::Uuid::parse_str(&site).unwrap().as_u128();
    let windows: Vec<i64> = harness
        .analytics(move |c| {
            let mut statement = c.prepare(
                "SELECT window_ms FROM session_policies WHERE site = ? ORDER BY version",
            )?;
            statement.query_map([id], |r| r.get(0))?.collect()
        })
        .await;
    assert_eq!(windows, [30 * MINUTE_MS, 10 * MINUTE_MS]);
}
