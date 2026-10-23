//! Milestone 08 over HTTP: consented retention, revenue and performance from tracker payloads,
//! their labels through save, pin, export and reopen, refusals, and a late event that changes
//! a result while its freshness says correction is pending.

mod common;

use holocron_server::clock::Clock;
use holocron_server::config::CollectConfig;
use serde_json::{Value, json};

use common::{Harness, test_budgets};

/// 2026-10-01T10:00:00Z, a Thursday, 12:00 in Vienna.
const NOW_MS: i64 = 1_790_848_800_000;
const HOUR_MS: i64 = 3_600_000;
const DAY_MS: i64 = 86_400_000;
const PEER: &str = "198.51.100.7:40000";

async fn harness() -> (Harness, Clock) {
    let clock = Clock::frozen(NOW_MS);
    let harness = Harness::with(CollectConfig::default(), test_budgets(), clock.clone()).await;
    (harness, clock)
}

async fn send(harness: &Harness, site: &str, kind: &str, at_ms: i64, extra: Value) {
    let mut payload = json!({
        "website": site, "hostname": "example.test", "url": "/checkout", "screen": "1920x1080",
        "language": "de-AT", "timestamp": at_ms / 1000,
    });
    payload
        .as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    let body = json!({ "type": kind, "payload": payload });
    let answer = harness.collect("POST", "/api/send", PEER, Some(body)).await;
    assert_eq!(
        answer.status,
        200,
        "{}",
        String::from_utf8_lossy(&answer.bytes)
    );
}

async fn settle(harness: &Harness) {
    harness.state.collector.storage().drain().await;
    holocron_analytics::run_until_clean(
        harness.state.collector.storage(),
        holocron_analytics::Limits::default(),
    )
    .await
    .unwrap();
}

async fn command(harness: &Harness, cookie: &str, name: &str, mut body: Value) -> Value {
    body["idempotency_key"] = json!(uuid::Uuid::new_v4().to_string());
    ok(&harness.command(Some(cookie), name, body).await)
}

fn ok(answer: &common::Answer) -> Value {
    assert_eq!(
        answer.status,
        200,
        "{}",
        String::from_utf8_lossy(&answer.bytes)
    );
    answer.json()
}

fn encode(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

async fn explore(harness: &Harness, cookie: &str, state: &Value) -> common::Answer {
    harness
        .query(
            Some(cookie),
            "analytics.explore",
            &format!("state={}", encode(&state.to_string())),
        )
        .await
}

async fn export(harness: &Harness, cookie: &str, format: &str, state: &Value) -> String {
    let file = ok(&harness
        .query(
            Some(cookie),
            "analytics.export",
            &format!("format={format}&state={}", encode(&state.to_string())),
        )
        .await);
    file["content"].as_str().unwrap().to_owned()
}

fn base(site: &str) -> Value {
    json!({
        "v": 1, "site": site, "measures": [], "dimensions": [], "filter": null,
        "range": { "kind": "relative", "unit": "week", "count": 2 }, "timezone": "Europe/Vienna",
        "granularity": "week", "compare": "none", "metric_version": 1, "sort": [],
        "page": { "size": 50, "index": 0 }, "visualization": "cohort",
        "report": { "kind": "retention", "v": 1, "intervals": 2 }
    })
}

fn revenue_state(site: &str) -> Value {
    json!({
        "v": 1, "site": site, "measures": ["revenue_net", "revenue_refunds", "revenue_count"],
        "dimensions": ["currency"], "filter": null,
        "range": { "kind": "relative", "unit": "day", "count": 14 }, "timezone": "Europe/Vienna",
        "granularity": "day", "compare": "none", "metric_version": 1,
        "sort": [{ "by": "revenue_net", "dir": "desc" }], "page": { "size": 50, "index": 0 },
        "visualization": "table"
    })
}

fn performance_state(site: &str) -> Value {
    json!({
        "v": 1, "site": site, "measures": ["p50", "p75", "p95"], "dimensions": ["perf_metric"],
        "filter": { "kind": "group", "op": "and", "items": [{
            "kind": "cond", "id": "m", "field": "perf_metric", "cmp": "in",
            "value": { "t": "list", "v": [{ "t": "string", "v": "lcp" }, { "t": "string", "v": "cls" }] }
        }]},
        "range": { "kind": "relative", "unit": "day", "count": 14 }, "timezone": "Europe/Vienna",
        "granularity": "day", "compare": "none", "metric_version": 1,
        "sort": [{ "by": "perf_metric", "dir": "asc" }], "page": { "size": 50, "index": 0 },
        "visualization": "table"
    })
}

/// Anna consents in last week's cohort and returns this week; Ben consents this week. An
/// anonymous visitor, three purchases in two currencies with a refund, and two performance
/// reports, one without `lcp`.
async fn traffic(harness: &Harness, owner: &str, site: &str) {
    command(
        harness,
        owner,
        "sites.updateIdentityPolicy",
        json!({ "site": site, "persistent_identity": true }),
    )
    .await;
    let last_week = NOW_MS - 7 * DAY_MS;
    let anna = json!({ "id": "anna" });
    send(
        harness,
        site,
        "identify",
        last_week,
        json!({ "id": "anna", "consent": "persistent" }),
    )
    .await;
    send(harness, site, "event", last_week + 1000, anna.clone()).await;
    send(harness, site, "event", NOW_MS - HOUR_MS, anna).await;
    send(
        harness,
        site,
        "identify",
        NOW_MS - 2 * HOUR_MS,
        json!({ "id": "ben", "consent": "persistent" }),
    )
    .await;
    send(
        harness,
        site,
        "event",
        NOW_MS - 2 * HOUR_MS + 1000,
        json!({ "id": "ben" }),
    )
    .await;
    send(harness, site, "event", last_week, json!({})).await;

    let purchase = |revenue: Value, currency: &str| json!({ "name": "purchase", "data": { "revenue": revenue, "currency": currency } });
    send(
        harness,
        site,
        "event",
        NOW_MS - 3 * HOUR_MS,
        purchase(json!(19.995), "EUR"),
    )
    .await;
    send(
        harness,
        site,
        "event",
        NOW_MS - 3 * HOUR_MS,
        purchase(json!(-5), "EUR"),
    )
    .await;
    send(
        harness,
        site,
        "event",
        NOW_MS - 3 * HOUR_MS,
        purchase(json!(1500.5), "JPY"),
    )
    .await;

    send(
        harness,
        site,
        "performance",
        NOW_MS - 3 * HOUR_MS,
        json!({ "lcp": 1800, "cls": 0.05 }),
    )
    .await;
    send(
        harness,
        site,
        "performance",
        NOW_MS - 3 * HOUR_MS,
        json!({ "cls": 0.25 }),
    )
    .await;
    settle(harness).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn reports_keep_their_labels_through_save_pin_export_and_reopen() {
    let (harness, _clock) = harness().await;
    let site = harness.site("Europe/Vienna").await;
    let owner = harness.session("p_owner").await;
    traffic(&harness, &owner, &site).await;

    let retention = base(&site);
    let result = ok(&explore(&harness, &owner, &retention).await);
    let report = &result["report"];
    assert_eq!(report["kind"], "retention");
    let sizes: Vec<i64> = report["cohorts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["size"].as_i64().unwrap())
        .collect();
    assert_eq!(sizes, vec![1, 1], "anna last week, ben this week");
    let anna = &report["cohorts"][0]["cells"];
    assert_eq!(
        (
            &anna[0]["rate"]["v"],
            &anna[1]["active"],
            &anna[1]["complete"]
        ),
        (&json!(1.0), &json!(1), &json!(false)),
        "anna returned this week, which is not over"
    );
    assert_eq!(report["averages"][1]["reason"], "no_denominator");
    // The anonymous pageview is eligible but not consented.
    assert_eq!(
        report["cohorts"][0]["coverage"],
        json!({ "with_identity": 1, "eligible_events": 2 })
    );
    assert_eq!(report["watermark"], NOW_MS);
    assert!(report["history_start"].as_i64().unwrap() < NOW_MS - 360 * DAY_MS);

    // The state carries the report and its version; the result its labels, everywhere.
    let saved = command(
        &harness,
        &owner,
        "explorations.save",
        json!({ "site": site, "name": "Weekly retention", "state": retention }),
    )
    .await;
    let id = saved["exploration"]["id"].as_str().unwrap();
    let reopened = ok(&harness
        .query(
            Some(&owner),
            "explorations.get",
            &format!("site={site}&exploration={id}"),
        )
        .await);
    assert_eq!(reopened["exploration"]["state"], retention);
    let workspace = harness.workspaces["ws_a"].as_str().to_owned();
    let board = command(
        &harness,
        &owner,
        "boards.create",
        json!({ "workspace": workspace, "name": "Retention" }),
    )
    .await;
    let board_id = board["board"]["id"].as_str().unwrap();
    command(
        &harness,
        &owner,
        "boards.pin",
        json!({ "board": board_id, "title": "Weekly", "state": retention, "exploration": id }),
    )
    .await;
    let board = ok(&harness
        .query(Some(&owner), "boards.get", &format!("board={board_id}"))
        .await);
    let pinned = &board["board"]["cards"][0]["state"];
    assert_eq!(pinned, &retention);
    assert_eq!(
        ok(&explore(&harness, &owner, pinned).await)["report"],
        *report
    );

    let file: Value =
        serde_json::from_str(&export(&harness, &owner, "json", &retention).await).unwrap();
    assert_eq!(file["state"], retention);
    assert_eq!(file["result"]["report"], *report);
    assert_eq!(file["result"]["freshness"], result["freshness"]);
    let csv = export(&harness, &owner, "csv", &retention).await;
    let lines: Vec<&str> = csv.lines().collect();
    assert_eq!(
        lines[0],
        "cohort,size,consented_events,eligible_events,week_0_active,week_0_rate,week_0_complete,week_1_active,week_1_rate,week_1_complete"
    );
    assert_eq!(lines.last().unwrap(), &"average,,3,7,,1,,,,");

    // Revenue: exact minor units per currency, amounts as decimal strings, no conversion.
    let revenue = revenue_state(&site);
    let result = ok(&explore(&harness, &owner, &revenue).await);
    let rows = &result["main"]["rows"];
    assert_eq!(
        rows[0]["values"][0],
        json!({ "t": "money", "v": [{ "currency": "EUR", "exponent": 2, "minor_units": 1500, "amount": "15.00" }] })
    );
    assert_eq!(rows[0]["values"][1]["v"][0]["amount"], "-5.00");
    assert_eq!(rows[0]["values"][2], json!({ "t": "count", "v": 2 }));
    assert_eq!(
        rows[1]["values"][0]["v"][0]["amount"], "1500",
        "JPY rounds half to even"
    );
    assert_eq!(
        result["main"]["totals"][0]["v"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["currency"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["EUR", "JPY"]
    );
    let csv = export(&harness, &owner, "csv", &revenue).await;
    assert_eq!(
        csv.lines().nth(1).unwrap(),
        "EUR,15.00 EUR,-5.00 EUR,2",
        "{csv}"
    );

    // Performance: nearest rank per metric, the missing lcp excluded, cls with 4 places.
    let performance = performance_state(&site);
    let result = ok(&explore(&harness, &owner, &performance).await);
    let rows = &result["main"]["rows"];
    assert_eq!(rows[0]["key"][0]["v"], "cls");
    assert_eq!(
        rows[0]["values"][0],
        json!({ "t": "decimal", "v": "0.0500" })
    );
    assert_eq!(
        rows[0]["values"][2],
        json!({ "t": "decimal", "v": "0.2500" })
    );
    assert_eq!(
        rows[1]["values"],
        json!([{ "t": "millis", "v": 1800 }, { "t": "millis", "v": 1800 }, { "t": "millis", "v": 1800 }])
    );
    assert_eq!(
        result["main"]["totals"][0],
        json!({ "t": "unavailable", "reason": "mixed_metrics" })
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn unsupported_report_states_are_refused_naming_the_field() {
    let (harness, _clock) = harness().await;
    let site = harness.site("Europe/Vienna").await;
    let owner = harness.session("p_owner").await;
    let refused = |answer: common::Answer| {
        let body = answer.json();
        let field = body["errors"]
            .as_object()
            .and_then(|e| e.keys().next().cloned())
            .unwrap_or_default();
        (answer.code(), field)
    };

    let mut mixed = performance_state(&site);
    mixed["measures"] = json!(["p75", "pageviews"]);
    let mut unsorted = revenue_state(&site);
    unsorted["dimensions"] = json!(["country"]);
    let mut no_metric = performance_state(&site);
    no_metric["filter"] = Value::Null;
    no_metric["dimensions"] = json!([]);
    no_metric["sort"] = json!([]);
    let mut cohort_metric = base(&site);
    cohort_metric["filter"] = performance_state(&site)["filter"].clone();
    let mut too_long = base(&site);
    too_long["report"]["intervals"] = json!(54);
    for (state, code, field) in [
        (mixed, "invalid_arguments", "measures"),
        (unsorted, "invalid_arguments", "sort"),
        (no_metric, "invalid_arguments", "measures"),
        (cohort_metric, "invalid_arguments", "filter.m"),
        (too_long, "report_invalid", "report.intervals"),
    ] {
        assert_eq!(
            refused(explore(&harness, &owner, &state).await),
            (code.to_owned(), field.to_owned()),
            "{state}"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_late_event_changes_reports_while_freshness_says_correction_is_pending() {
    let (harness, _clock) = harness().await;
    let site = harness.site("Europe/Vienna").await;
    let owner = harness.session("p_owner").await;
    traffic(&harness, &owner, &site).await;
    let retention = base(&site);
    let revenue = revenue_state(&site);
    let before = ok(&explore(&harness, &owner, &retention).await);
    let revenue_before = ok(&explore(&harness, &owner, &revenue).await);
    assert_eq!(before["freshness"]["correction"], "corrected");

    // Anna's event from yesterday and a purchase from two days ago arrive now.
    send(
        &harness,
        &site,
        "event",
        NOW_MS - DAY_MS,
        json!({ "id": "anna" }),
    )
    .await;
    send(
        &harness,
        &site,
        "event",
        NOW_MS - 2 * DAY_MS,
        json!({ "name": "purchase", "data": { "revenue": 10, "currency": "EUR" } }),
    )
    .await;
    harness.state.collector.storage().drain().await;

    let pending = ok(&explore(&harness, &owner, &retention).await);
    assert_eq!(pending["freshness"]["correction"], "pending");
    assert!(pending["freshness"]["dirty_days"].as_i64().unwrap() > 0);
    assert!(
        pending["freshness"]["watermark_batch"].as_i64()
            > before["freshness"]["watermark_batch"].as_i64()
    );
    // Anna was consented yesterday, so this week's periods gain a consented event.
    assert_eq!(
        pending["report"]["cohorts"][1]["coverage"]["with_identity"],
        before["report"]["cohorts"][1]["coverage"]["with_identity"]
            .as_i64()
            .unwrap()
            + 1
    );
    let revenue_pending = ok(&explore(&harness, &owner, &revenue).await);
    assert_eq!(
        revenue_pending["main"]["rows"][0]["values"][0]["v"][0]["amount"],
        "25.00"
    );
    assert_ne!(revenue_pending["main"], revenue_before["main"]);

    settle(&harness).await;
    let corrected = ok(&explore(&harness, &owner, &retention).await);
    assert_eq!(corrected["freshness"]["correction"], "corrected");
    assert_eq!(
        corrected["report"], pending["report"],
        "correction is deterministic"
    );
}
