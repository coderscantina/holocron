//! Configurable event retention over HTTP (decision 0016): a site's own retention or the
//! installation default decides what collection stores, how far queries reach and what the
//! retention job removes. Shortening previews what goes and needs a fresh password check.

mod common;

use holocron_server::clock::Clock;
use holocron_server::config::CollectConfig;
use holocron_server::maintenance::jobs::{self, Job};
use serde_json::{Value, json};

use common::{Answer, Harness, test_budgets};

/// 2026-10-01T10:00:00Z.
const NOW_MS: i64 = 1_790_848_800_000;
const DAY_MS: i64 = 86_400_000;
const PEER: &str = "198.51.100.7:40000";

async fn harness() -> Harness {
    Harness::with(
        CollectConfig::default(),
        test_budgets(),
        Clock::frozen(NOW_MS),
    )
    .await
}

async fn pageview(h: &Harness, site: &str, at_ms: i64) {
    let body = json!({ "type": "event", "payload": {
        "website": site, "hostname": "example.test", "url": "/", "timestamp": at_ms / 1000,
    }});
    assert_eq!(
        h.collect("POST", "/api/send", PEER, Some(body))
            .await
            .status,
        200
    );
    let storage = h.state.collector.storage();
    storage.drain().await;
    holocron_analytics::run_until_clean(storage, holocron_analytics::Limits::default())
        .await
        .unwrap();
}

async fn command(h: &Harness, cookie: &str, name: &str, mut body: Value) -> Answer {
    body["idempotency_key"] = json!(uuid::Uuid::new_v4().to_string());
    h.command(Some(cookie), name, body).await
}

fn ok(answer: Answer) -> Value {
    assert_eq!(
        answer.status,
        200,
        "{}",
        String::from_utf8_lossy(&answer.bytes)
    );
    answer.json()
}

async fn pageviews(h: &Harness, cookie: &str, site: &str, range: Value) -> Answer {
    let state = json!({
        "v": 1, "site": site, "measures": ["pageviews"], "dimensions": [], "filter": null,
        "range": range, "timezone": "Europe/Vienna", "granularity": "month",
        "compare": "none", "metric_version": 1, "sort": [], "page": { "size": 50, "index": 0 },
        "visualization": "table"
    });
    let state: String =
        url::form_urlencoded::byte_serialize(state.to_string().as_bytes()).collect();
    h.query(Some(cookie), "analytics.explore", &format!("state={state}"))
        .await
}

async fn retention_job(h: &Harness) -> Value {
    let id = jobs::enqueue(&h.state.db.writer, &Job::Retention, NOW_MS)
        .await
        .unwrap();
    jobs::run_until(&h.state, &id).await.unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_site_keeps_old_events_for_its_own_retention_until_it_is_shortened() {
    let h = harness().await;
    let fresh = h.session("p_owner").await;
    let stale = h.session_reauthenticated("p_owner", None).await;
    let site = h.site("Europe/Vienna").await;

    // Lengthening needs no password check.
    let changed = ok(command(
        &h,
        &stale,
        "sites.updateRetention",
        json!({ "site": site, "retention": "5y" }),
    )
    .await);
    assert_eq!(changed["site"]["retention"], "5y");
    assert_eq!(changed["site"]["retention_override"], "5y");

    pageview(&h, &site, NOW_MS - 2 * 365 * DAY_MS).await;
    pageview(&h, &site, NOW_MS - DAY_MS).await;
    let three_years = json!({ "kind": "relative", "unit": "month", "count": 36 });
    let result = ok(pageviews(&h, &fresh, &site, three_years).await);
    assert_eq!(result["main"]["totals"][0]["v"], 2);
    let ten_years = json!({ "kind": "relative", "unit": "month", "count": 120 });
    assert_eq!(
        pageviews(&h, &fresh, &site, ten_years).await.code(),
        "invalid_arguments"
    );
    assert_eq!(retention_job(&h).await["events_removed"], 0);

    // Back to the installation default of 1y: the preview names what goes.
    let preview = ok(h
        .query(
            Some(&fresh),
            "sites.retentionPreview",
            &format!("site={site}"),
        )
        .await);
    assert_eq!(preview["retention"], "1y");
    assert_eq!(preview["events_removed"], 1);
    let refused = command(&h, &stale, "sites.updateRetention", json!({ "site": site })).await;
    assert_eq!(refused.code(), "reauthentication_required");
    let changed = ok(command(&h, &fresh, "sites.updateRetention", json!({ "site": site })).await);
    assert_eq!(changed["site"]["retention"], "1y");
    assert!(changed["site"].get("retention_override").is_none());

    // Queries hide the old event at once; the job removes it.
    let result = ok(pageviews(&h, &fresh, &site, json!({ "kind": "all" })).await);
    assert_eq!(result["main"]["totals"][0]["v"], 1);
    assert_eq!(retention_job(&h).await["events_removed"], 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn sites_without_their_own_retention_follow_the_installation_default() {
    let h = harness().await;
    let admin = h.session_reauthenticated("p_install_admin", None).await;
    let owner = h.session("p_owner").await;
    let site = h.site("Europe/Vienna").await;

    // Under the 1y default an event from four years ago is not stored.
    let old = NOW_MS - 4 * 365 * DAY_MS;
    pageview(&h, &site, old).await;
    ok(command(
        &h,
        &admin,
        "retention.updateDefault",
        json!({ "retention": "forever" }),
    )
    .await);
    pageview(&h, &site, old).await;

    let detail = ok(h
        .query(Some(&owner), "sites.get", &format!("site={site}"))
        .await);
    assert_eq!(detail["site"]["retention"], "forever");
    // `all` starts at the first stored event, with no cutoff to report.
    let result = ok(pageviews(&h, &owner, &site, json!({ "kind": "all" })).await);
    assert_eq!(result["main"]["totals"][0]["v"], 1);
    assert!(result["resolved"].get("retention_start").is_none());
    assert!(result["resolved"]["start"].as_i64().unwrap() <= old);

    let preview = ok(h
        .query(Some(&admin), "retention.previewDefault", "retention=3y")
        .await);
    assert_eq!(preview["events_removed"], 1);
    let refused = command(
        &h,
        &admin,
        "retention.updateDefault",
        json!({ "retention": "3y" }),
    )
    .await;
    assert_eq!(refused.code(), "reauthentication_required");
}
