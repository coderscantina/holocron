//! Milestone 06 over HTTP: tracker payloads through correction into explorations, the
//! acceptance workflow with save, pin and export, refusals that name their field, revoked
//! scope, definitions, realtime and the live subscription.

mod common;

use axum::body::Body;
use axum::http::{Request, header};
use holocron_server::clock::Clock;
use holocron_server::config::{CollectConfig, parse_net};
use holocron_storage::Budgets;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::time::Duration;

use common::{HOST, Harness, test_budgets};

/// 2026-10-01T10:00:00Z, 12:00 in Vienna.
const NOW_MS: i64 = 1_790_848_800_000;
const MINUTE_MS: i64 = 60_000;
const DAY_MS: i64 = 86_400_000;
const IPHONE: &str = "Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Mobile/15E148 Safari/604.1";
const DESKTOP: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36";

async fn harness() -> Harness {
    let collect = CollectConfig {
        trusted_proxies: vec![parse_net("127.0.0.1").unwrap()],
        ..CollectConfig::default()
    };
    Harness::with(collect, test_budgets(), Clock::frozen(NOW_MS)).await
}

/// One tracker event from a client behind the trusted proxy.
struct Hit<'a> {
    ip: &'a str,
    agent: &'a str,
    country: &'a str,
    at_ms: i64,
    name: Option<&'a str>,
    plan: Option<&'a str>,
    referrer: &'a str,
}

async fn send(harness: &Harness, site: &str, hit: Hit<'_>) {
    let mut payload = json!({
        "website": site, "hostname": "example.test", "url": "/signup", "screen": "390x844",
        "language": "de-AT", "referrer": hit.referrer, "timestamp": hit.at_ms / 1000,
    });
    if let Some(name) = hit.name {
        payload["name"] = json!(name);
    }
    if let Some(plan) = hit.plan {
        payload["data"] = json!({ "plan": plan });
    }
    let request = Request::post("/api/send")
        .header(header::HOST, HOST)
        .header(header::USER_AGENT, hit.agent)
        .header(header::CONTENT_TYPE, "application/json")
        .header("x-forwarded-for", hit.ip)
        .header("cf-ipcountry", hit.country)
        .extension(axum::extract::ConnectInfo(
            "127.0.0.1:9000".parse::<std::net::SocketAddr>().unwrap(),
        ))
        .body(Body::from(
            json!({ "type": "event", "payload": payload }).to_string(),
        ))
        .unwrap();
    let answer = harness.send(request).await;
    assert_eq!(
        answer.status,
        200,
        "{}",
        String::from_utf8_lossy(&answer.bytes)
    );
}

/// Mobile signups with plan=pro in Austria and Germany, a desktop one, a free one, and one in
/// the previous period, then correction.
async fn traffic(harness: &Harness, site: &str) {
    let recent = NOW_MS - 30 * MINUTE_MS;
    let mobile = |ip, country, name, plan, at_ms, referrer| Hit {
        ip,
        agent: IPHONE,
        country,
        at_ms,
        name,
        plan,
        referrer,
    };
    for hit in [
        mobile(
            "203.0.113.1",
            "AT",
            None,
            None,
            recent,
            "https://news.example.net/a",
        ),
        mobile(
            "203.0.113.1",
            "AT",
            Some("signup"),
            Some("pro"),
            recent + MINUTE_MS,
            "https://news.example.net/a",
        ),
        mobile(
            "203.0.113.2",
            "AT",
            Some("signup"),
            Some("pro"),
            recent + 2 * MINUTE_MS,
            "https://news.example.net/a",
        ),
        mobile(
            "203.0.113.3",
            "DE",
            Some("signup"),
            Some("pro"),
            recent + 3 * MINUTE_MS,
            "",
        ),
        mobile(
            "203.0.113.4",
            "AT",
            Some("signup"),
            Some("free"),
            recent + 4 * MINUTE_MS,
            "",
        ),
        mobile(
            "203.0.113.5",
            "AT",
            Some("signup"),
            Some("pro"),
            NOW_MS - 40 * DAY_MS,
            "",
        ),
        Hit {
            agent: DESKTOP,
            ..mobile(
                "203.0.113.6",
                "AT",
                Some("signup"),
                Some("pro"),
                recent + 5 * MINUTE_MS,
                "",
            )
        },
    ] {
        send(harness, site, hit).await;
    }
    harness.state.collector.storage().drain().await;
    holocron_analytics::run_until_clean(
        harness.state.collector.storage(),
        holocron_analytics::Limits::default(),
    )
    .await
    .unwrap();
}

fn cond(id: &str, field: Value, cmp: &str, value: &str) -> Value {
    json!({ "kind": "cond", "id": id, "field": field, "cmp": cmp, "value": { "t": "string", "v": value } })
}

fn acceptance_state(site: &str, extra: Option<Value>) -> Value {
    let mut items = vec![
        cond("f1", json!("device"), "eq", "mobile"),
        cond("f2", json!("event_name"), "eq", "signup"),
        cond(
            "f3",
            json!({ "scope": "event", "key": "plan" }),
            "eq",
            "pro",
        ),
    ];
    items.extend(extra);
    json!({
        "v": 1, "site": site, "measures": ["events", "visitor_days"], "dimensions": ["country", "referrer_domain"],
        "filter": { "kind": "group", "op": "and", "items": items },
        "range": { "kind": "relative", "unit": "day", "count": 30 }, "timezone": "Europe/Vienna",
        "granularity": "day", "compare": "previous_period", "metric_version": 1,
        "sort": [{ "by": "events", "dir": "desc" }], "page": { "size": 50, "index": 0 }, "visualization": "table"
    })
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

async fn command(harness: &Harness, cookie: &str, name: &str, mut body: Value) -> common::Answer {
    body["idempotency_key"] = json!(uuid::Uuid::new_v4().to_string());
    harness.command(Some(cookie), name, body).await
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

#[tokio::test(flavor = "multi_thread")]
async fn acceptance_workflow_saves_pins_exports_and_reopens_the_same_state() {
    let harness = harness().await;
    let site = harness.site("Europe/Vienna").await;
    traffic(&harness, &site).await;
    let owner = harness.session("p_owner").await;

    let state = acceptance_state(&site, None);
    let result = ok(&explore(&harness, &owner, &state).await);
    let main = &result["main"];
    assert_eq!(
        main["totals"],
        json!([{ "t": "count", "v": 3 }, { "t": "count", "v": 3 }])
    );
    assert_eq!(
        main["compare"][0]["previous"],
        json!({ "t": "count", "v": 1 })
    );
    assert_eq!(main["compare"][0]["change"], json!(2.0));
    assert_eq!(main["compare"][0]["relative_change"], json!(2.0));
    let keys: Vec<Value> = main["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["key"].clone())
        .collect();
    assert_eq!(
        keys,
        vec![
            json!([{ "t": "string", "v": "AT" }, { "t": "string", "v": "news.example.net" }]),
            json!([{ "t": "string", "v": "DE" }, { "t": "not_set" }]),
        ]
    );
    assert_eq!(result["resolved"]["timezone"], "Europe/Vienna");
    assert_eq!(result["freshness"]["correction"], "corrected");

    // Drilling into Austria appends f4 and keeps every other part.
    let drilled = acceptance_state(&site, Some(cond("f4", json!("country"), "eq", "AT")));
    let result = ok(&explore(&harness, &owner, &drilled).await);
    assert_eq!(result["main"]["totals"][0], json!({ "t": "count", "v": 2 }));
    assert_eq!(result["main"]["row_count"], 1);

    // Save, pin and export carry the identical state.
    let saved = ok(&command(
        &harness,
        &owner,
        "explorations.save",
        json!({ "site": site, "name": "Mobile pro signups", "state": drilled }),
    )
    .await);
    let id = saved["exploration"]["id"].as_str().unwrap();
    let reopened = ok(&harness
        .query(
            Some(&owner),
            "explorations.get",
            &format!("site={site}&exploration={id}"),
        )
        .await);
    assert_eq!(reopened["exploration"]["state"], drilled);

    let workspace = harness.workspaces["ws_a"].as_str().to_owned();
    let board = ok(&command(
        &harness,
        &owner,
        "boards.create",
        json!({ "workspace": workspace, "name": "Growth" }),
    )
    .await);
    let board_id = board["board"]["id"].as_str().unwrap();
    ok(&command(
        &harness,
        &owner,
        "boards.pin",
        json!({ "board": board_id, "title": "Pro signups", "state": drilled, "exploration": id }),
    )
    .await);
    let board = ok(&harness
        .query(Some(&owner), "boards.get", &format!("board={board_id}"))
        .await);
    assert_eq!(board["board"]["cards"][0]["state"], drilled);
    assert_eq!(board["board"]["cards"][0]["exploration"], id);

    let export = ok(&harness
        .query(
            Some(&owner),
            "analytics.export",
            &format!("format=json&state={}", encode(&drilled.to_string())),
        )
        .await);
    let file: Value = serde_json::from_str(export["content"].as_str().unwrap()).unwrap();
    assert_eq!(file["state"], drilled);
    assert_eq!(file["result"]["main"]["rows"], result["main"]["rows"]);
    assert_eq!(file["result"]["resolved"], result["resolved"]);
    let csv = ok(&harness
        .query(
            Some(&owner),
            "analytics.export",
            &format!("format=csv&state={}", encode(&drilled.to_string())),
        )
        .await);
    assert_eq!(
        csv["content"].as_str().unwrap().lines().next().unwrap(),
        "country,referrer_domain,events,events_previous,events_change,events_relative_change,visitor_days,visitor_days_previous,visitor_days_change,visitor_days_relative_change"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn freshness_says_until_when_data_counts_once_analytics_is_stale() {
    const HOUR_MS: i64 = 60 * MINUTE_MS;
    let collect = CollectConfig {
        trusted_proxies: vec![parse_net("127.0.0.1").unwrap()],
        ..CollectConfig::default()
    };
    // Transfer waits a minute, so an accepted record stays in the inbox.
    let budgets = Budgets {
        transfer_delay: Duration::from_secs(60),
        ..test_budgets()
    };
    let harness = Harness::with(collect, budgets, Clock::frozen(NOW_MS)).await;
    let site = harness.site("Europe/Vienna").await;
    let owner = harness.session("p_owner").await;
    let mut state = acceptance_state(&site, None);
    state["filter"] = Value::Null;

    let current = ok(&explore(&harness, &owner, &state).await)["freshness"].clone();
    assert_eq!(current["stale"], false);
    assert_eq!(current["watermark_at"], NOW_MS);

    // Accepted an hour ago and still waiting.
    send(
        &harness,
        &site,
        Hit {
            ip: "203.0.113.1",
            agent: DESKTOP,
            country: "AT",
            at_ms: NOW_MS,
            name: None,
            plan: None,
            referrer: "",
        },
    )
    .await;
    let inbox = sqlx::SqlitePool::connect(&format!(
        "sqlite://{}",
        harness.data_dir().join("inbox.sqlite").display()
    ))
    .await
    .unwrap();
    sqlx::query("UPDATE inbox SET received_at = received_at - ?")
        .bind(HOUR_MS)
        .execute(&inbox)
        .await
        .unwrap();
    let stale = ok(&explore(&harness, &owner, &state).await)["freshness"].clone();
    assert_eq!(stale["stale"], true);
    // An hour behind the frozen clock, less the wall time this test took.
    let at = stale["watermark_at"].as_i64().unwrap();
    assert!(
        (NOW_MS - HOUR_MS - 10_000..=NOW_MS - HOUR_MS).contains(&at),
        "{at}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn today_compares_with_yesterday_until_the_same_time() {
    // Tuesday 2026-10-13 15:20 in Vienna.
    const TUESDAY_1520: i64 = 1_791_897_600_000;
    const HOUR_MS: i64 = 60 * MINUTE_MS;
    let collect = CollectConfig {
        trusted_proxies: vec![parse_net("127.0.0.1").unwrap()],
        ..CollectConfig::default()
    };
    let harness = Harness::with(collect, test_budgets(), Clock::frozen(TUESDAY_1520)).await;
    let site = harness.site("Europe/Vienna").await;
    let pageview = |ip, country, at_ms| Hit {
        ip,
        agent: DESKTOP,
        country,
        at_ms,
        name: None,
        plan: None,
        referrer: "",
    };
    let monday_1520 = TUESDAY_1520 - DAY_MS;
    for hit in [
        pageview(
            "203.0.113.1",
            "AT",
            monday_1520 - 7 * HOUR_MS - 20 * MINUTE_MS,
        ),
        pageview("203.0.113.2", "DE", monday_1520 - 20 * MINUTE_MS),
        // After 15:20 on Monday: only in the whole previous day.
        pageview("203.0.113.3", "AT", monday_1520 + 10 * MINUTE_MS),
        pageview("203.0.113.4", "AT", monday_1520 + 5 * HOUR_MS),
        pageview("203.0.113.5", "AT", TUESDAY_1520 - 6 * HOUR_MS),
        pageview("203.0.113.6", "AT", TUESDAY_1520 - 10 * MINUTE_MS),
        pageview("203.0.113.7", "DE", TUESDAY_1520 - HOUR_MS),
    ] {
        send(&harness, &site, hit).await;
    }
    harness.state.collector.storage().drain().await;
    holocron_analytics::run_until_clean(
        harness.state.collector.storage(),
        holocron_analytics::Limits::default(),
    )
    .await
    .unwrap();
    let owner = harness.session("p_owner").await;

    let state = json!({
        "v": 1, "site": site, "measures": ["pageviews"], "dimensions": ["country"], "filter": null,
        "range": { "kind": "relative", "unit": "day", "count": 1 }, "timezone": "Europe/Vienna",
        "granularity": "hour", "compare": "previous_period", "metric_version": 1,
        "sort": [{ "by": "pageviews", "dir": "desc" }], "page": { "size": 50, "index": 0 }, "visualization": "table"
    });
    let result = ok(&explore(&harness, &owner, &state).await);
    let main = &result["main"];
    assert_eq!(main["totals"], json!([{ "t": "count", "v": 3 }]));
    assert_eq!(
        main["compare"][0],
        json!({ "previous": { "t": "count", "v": 2 }, "change": 1.0, "relative_change": 0.5 })
    );
    let previous: Vec<_> = main["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            (
                row["key"][0]["v"].clone(),
                row["compare"][0]["previous"]["v"].clone(),
            )
        })
        .collect();
    assert_eq!(
        previous,
        vec![(json!("AT"), json!(1)), (json!("DE"), json!(1))]
    );
    // The bounds still name the whole previous day.
    let monday = TUESDAY_1520 - DAY_MS - 15 * HOUR_MS - 20 * MINUTE_MS;
    assert_eq!(result["resolved"]["compare_start"], json!(monday));
    assert_eq!(result["resolved"]["compare_end"], json!(monday + DAY_MS));

    // The CSV names the projection in its own columns. One day old, the site has no baseline
    // Tuesday, so both the baseline and the projection are empty.
    let mut typical = state;
    typical["compare"] = json!("typical_weekday");
    typical["dimensions"] = json!([]);
    typical["sort"] = json!([]);
    let csv = ok(&harness
        .query(
            Some(&owner),
            "analytics.export",
            &format!("format=csv&state={}", encode(&typical.to_string())),
        )
        .await);
    let lines: Vec<&str> = csv["content"].as_str().unwrap().lines().collect();
    assert_eq!(
        lines,
        [
            "pageviews,pageviews_previous,pageviews_change,pageviews_relative_change,pageviews_projection,pageviews_projection_low,pageviews_projection_high",
            "3,,,,,,"
        ]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn unsupported_states_are_refused_naming_the_field() {
    let harness = harness().await;
    let site = harness.site("Europe/Vienna").await;
    traffic(&harness, &site).await;
    let owner = harness.session("p_owner").await;
    let base = acceptance_state(&site, None);
    let with_all = |changes: &[(&str, Value)]| {
        let mut state = base.clone();
        for (path, value) in changes {
            state[*path] = value.clone();
        }
        state
    };
    let with = |path: &str, value: Value| with_all(&[(path, value)]);
    let nested = |depth: usize| {
        let mut group =
            json!({ "kind": "group", "op": "and", "items": [cond("x", json!("path"), "eq", "/")] });
        for _ in 1..depth {
            group = json!({ "kind": "group", "op": "or", "items": [group] });
        }
        group
    };
    let cases = [
        (
            with(
                "filter",
                json!({ "kind": "group", "op": "and", "items": [cond("a", json!("path"), "eq", "/"), cond("a", json!("path"), "eq", "/x")] }),
            ),
            "duplicate_predicate_id",
            "filter.a",
        ),
        (with("filter", nested(4)), "filter_too_deep", "filter"),
        (
            with("page", json!({ "size": 100, "index": 100 })),
            "page_out_of_range",
            "page",
        ),
        (
            with("metric_version", json!(2)),
            "invalid_arguments",
            "metric_version",
        ),
        (
            with("visualization", json!("funnel")),
            "report_missing",
            "visualization",
        ),
        (
            with(
                "filter",
                json!({ "kind": "group", "op": "and", "items": [cond("p", json!({ "scope": "event", "key": "nope" }), "eq", "x")] }),
            ),
            "invalid_arguments",
            "filter.p",
        ),
        (
            with(
                "filter",
                json!({ "kind": "group", "op": "and", "items": [{ "kind": "cond", "id": "w", "field": "weekday", "cmp": "contains", "value": { "t": "integer", "v": 1 } }] }),
            ),
            "invalid_arguments",
            "filter.w",
        ),
        (
            with_all(&[
                ("granularity", json!("hour")),
                ("visualization", json!("line")),
                (
                    "range",
                    json!({ "kind": "relative", "unit": "month", "count": 2 }),
                ),
            ]),
            "invalid_arguments",
            "granularity",
        ),
    ];
    for (state, code, field) in cases {
        let answer = explore(&harness, &owner, &state).await;
        assert_eq!(answer.code(), code, "{state}");
        assert!(
            answer.json()["errors"][field].is_array(),
            "{field}: {}",
            answer.json()
        );
    }
    // A state from a later version does not parse rather than computing something else.
    let answer = explore(
        &harness,
        &owner,
        &with("report", json!({ "kind": "funnel" })),
    )
    .await;
    assert_eq!(answer.code(), "malformed_request");
}

#[tokio::test(flavor = "multi_thread")]
async fn revoked_scope_is_refused_by_reads_exports_and_definitions() {
    let harness = harness().await;
    let site = harness.site("Europe/Vienna").await;
    traffic(&harness, &site).await;
    let member = harness.session("p_member").await;
    let viewer = harness.session("p_viewer").await;
    let state = acceptance_state(&site, None);
    let export = format!("format=csv&state={}", encode(&state.to_string()));

    ok(&explore(&harness, &viewer, &state).await);
    ok(&harness
        .query(Some(&viewer), "analytics.export", &export)
        .await);
    let segment = json!({ "site": site, "name": "Mobile", "filter": state["filter"] });
    assert_eq!(
        command(&harness, &viewer, "segments.save", segment.clone())
            .await
            .code(),
        "forbidden"
    );
    let workspace = harness.workspaces["ws_a"].as_str().to_owned();
    assert_eq!(
        command(
            &harness,
            &viewer,
            "boards.create",
            json!({ "workspace": workspace, "name": "Mine" })
        )
        .await
        .code(),
        "forbidden"
    );

    ok(&explore(&harness, &member, &state).await);
    harness.remove_member("p_member", "ws_a").await;
    assert_eq!(explore(&harness, &member, &state).await.code(), "not_found");
    assert_eq!(
        harness
            .query(Some(&member), "analytics.export", &export)
            .await
            .code(),
        "not_found"
    );
    assert_eq!(
        harness
            .query(Some(&member), "realtime.snapshot", &format!("site={site}"))
            .await
            .code(),
        "not_found"
    );
    assert_eq!(
        command(
            &harness,
            &member,
            "explorations.save",
            json!({ "site": site, "name": "x", "state": state })
        )
        .await
        .code(),
        "not_found"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn segments_and_annotations_are_site_definitions() {
    let harness = harness().await;
    let site = harness.site("Europe/Vienna").await;
    let owner = harness.session("p_owner").await;
    let filter = json!({ "kind": "group", "op": "or", "items": [cond("d1", json!("device"), "eq", "mobile"), cond("d2", json!("device"), "eq", "tablet")] });

    ok(&command(
        &harness,
        &owner,
        "segments.save",
        json!({ "site": site, "name": " Mobile ", "filter": filter }),
    )
    .await);
    let duplicate = command(
        &harness,
        &owner,
        "segments.save",
        json!({ "site": site, "name": "MOBILE", "filter": filter }),
    )
    .await;
    assert_eq!(
        (
            duplicate.code().as_str(),
            duplicate.json()["errors"]["name"][0]["code"].clone()
        ),
        ("invalid_arguments", json!("invalid_value"))
    );
    let deep = json!({ "kind": "group", "op": "and", "items": [{ "kind": "group", "op": "and", "items": [{ "kind": "group", "op": "and", "items": [{ "kind": "group", "op": "and", "items": [cond("z", json!("path"), "eq", "/")] }] }] }] });
    assert_eq!(
        command(
            &harness,
            &owner,
            "segments.save",
            json!({ "site": site, "name": "Deep", "filter": deep })
        )
        .await
        .code(),
        "filter_too_deep"
    );
    let list = ok(&harness
        .query(Some(&owner), "segments.list", &format!("site={site}"))
        .await);
    assert_eq!(list["segments"][0]["name"], "Mobile");
    assert_eq!(list["segments"][0]["filter"], filter);

    let created = ok(&command(
        &harness,
        &owner,
        "annotations.create",
        json!({ "site": site, "at": NOW_MS - DAY_MS, "text": "Launch" }),
    )
    .await);
    let id = created["annotation"]["id"].as_str().unwrap();
    ok(&command(
        &harness,
        &owner,
        "annotations.update",
        json!({ "site": site, "annotation": id, "at": NOW_MS - DAY_MS, "text": "Relaunch" }),
    )
    .await);
    let range = format!("site={site}&start={}&end={NOW_MS}", NOW_MS - 7 * DAY_MS);
    let list = ok(&harness
        .query(Some(&owner), "annotations.list", &range)
        .await);
    assert_eq!(list["annotations"][0]["text"], "Relaunch");
    ok(&command(
        &harness,
        &owner,
        "annotations.delete",
        json!({ "site": site, "annotation": id }),
    )
    .await);
    let list = ok(&harness
        .query(Some(&owner), "annotations.list", &range)
        .await);
    assert_eq!(list["annotations"], json!([]));
}

/// Reads server-sent frames until `count` have arrived.
async fn frames(body: Body, count: usize) -> Vec<Value> {
    let mut body = body;
    let mut text = String::new();
    let mut out = Vec::new();
    while out.len() < count {
        let frame = tokio::time::timeout(std::time::Duration::from_secs(10), body.frame())
            .await
            .expect("a frame in time")
            .expect("the stream stays open")
            .unwrap();
        if let Ok(data) = frame.into_data() {
            text.push_str(&String::from_utf8_lossy(&data));
        }
        while let Some(end) = text.find("\n\n") {
            let event: String = text.drain(..end + 2).collect();
            if let Some(data) = event.lines().find_map(|line| line.strip_prefix("data: ")) {
                out.push(serde_json::from_str(data).unwrap());
            }
        }
    }
    out
}

#[tokio::test(flavor = "multi_thread")]
async fn realtime_snapshot_and_live_subscription_resume_from_the_cursor() {
    let harness = harness().await;
    let site = harness.site("Europe/Vienna").await;
    let owner = harness.session("p_owner").await;
    send(
        &harness,
        &site,
        Hit {
            ip: "203.0.113.9",
            agent: IPHONE,
            country: "AT",
            at_ms: NOW_MS - MINUTE_MS,
            name: None,
            plan: None,
            referrer: "",
        },
    )
    .await;
    harness.state.collector.storage().drain().await;

    let snapshot = ok(&harness
        .query(Some(&owner), "realtime.snapshot", &format!("site={site}"))
        .await);
    assert_eq!(snapshot["active"], 1);
    assert_eq!(snapshot["series"].as_array().unwrap().len(), 30);
    assert_eq!(snapshot["recent"][0]["path"], "/signup");
    let topic = snapshot["live_topic"].as_str().unwrap().to_owned();
    let cursor = snapshot["live_cursor"].as_str().unwrap().to_owned();
    let batch: i64 = cursor.rsplit_once('@').unwrap().1.parse().unwrap();

    let subscribe = |cookie: &str, cursor: String| {
        Request::get(format!(
            "/api/subscribe?topic={}&cursor={}",
            encode(&topic),
            encode(&cursor)
        ))
        .header(header::HOST, HOST)
        .header(header::COOKIE, cookie)
        .body(Body::empty())
        .unwrap()
    };
    let router = holocron_server::router(harness.state.clone());
    let open = |request| {
        let router = router.clone();
        async move { tower::ServiceExt::oneshot(router, request).await.unwrap() }
    };

    // Current cursor: subscribed, then an events frame once newer data is applied.
    let response = open(subscribe(&owner, cursor.clone())).await;
    assert_eq!(response.status(), 200);
    let reader = tokio::spawn(frames(response.into_body(), 2));
    send(
        &harness,
        &site,
        Hit {
            ip: "203.0.113.10",
            agent: IPHONE,
            country: "AT",
            at_ms: NOW_MS,
            name: None,
            plan: None,
            referrer: "",
        },
    )
    .await;
    let got = reader.await.unwrap();
    assert_eq!(got[0]["kind"], "subscribed");
    assert_eq!(got[1]["kind"], "events");
    assert!(got[1]["events"][0]["seq"].as_i64().unwrap() > batch);

    // A cursor behind the head resumes with one events frame at once; one ahead is expired.
    let got = frames(open(subscribe(&owner, cursor.clone())).await.into_body(), 2).await;
    assert_eq!(got[1]["kind"], "events");
    let got = frames(
        open(subscribe(&owner, format!("{topic}@{}", batch + 1000)))
            .await
            .into_body(),
        2,
    )
    .await;
    assert_eq!(got[1]["kind"], "expired");

    // Another topic's cursor and a caller without the site are refused before streaming.
    let foreign = open(subscribe(&owner, format!("analytics:other@{batch}"))).await;
    assert_eq!(foreign.status(), 400);
    let outsider = harness.session("p_other_owner").await;
    assert_eq!(open(subscribe(&outsider, cursor)).await.status(), 404);
}

/// `serve --frozen-clock` in the v09 acceptance fixture's notation.
const PINNED: &str = "2026-10-13T15:20:00+02:00";

#[cfg(not(feature = "dev-clock"))]
#[test]
fn a_build_without_dev_clock_refuses_a_frozen_clock() {
    let refused = holocron_server::config::clock(Some(PINNED)).err().unwrap();
    assert!(
        refused
            .to_string()
            .contains("this build does not support a frozen clock"),
        "{refused}"
    );
    assert!(!holocron_server::config::clock(None).unwrap().is_test());
}

#[cfg(feature = "dev-clock")]
#[tokio::test(flavor = "multi_thread")]
async fn a_frozen_clock_resolves_today_from_its_instant() {
    const TUESDAY_1520: i64 = 1_791_897_600_000;
    const TUESDAY_MIDNIGHT: i64 = TUESDAY_1520 - (15 * 60 + 20) * MINUTE_MS;
    let clock = holocron_server::config::clock(Some(PINNED)).unwrap();
    assert_eq!(clock.now_ms(), TUESDAY_1520);
    assert!(holocron_server::config::clock(Some("Tuesday")).is_err());

    let harness = Harness::with(CollectConfig::default(), test_budgets(), clock).await;
    let site = harness.site("Europe/Vienna").await;
    let owner = harness.session("p_owner").await;
    let state = json!({
        "v": 1, "site": site, "measures": ["pageviews"], "dimensions": [], "filter": null,
        "range": { "kind": "relative", "unit": "day", "count": 1 }, "timezone": "Europe/Vienna",
        "granularity": "hour", "compare": "none", "metric_version": 1,
        "sort": [], "page": { "size": 50, "index": 0 }, "visualization": "line"
    });
    let result = ok(&explore(&harness, &owner, &state).await);
    assert_eq!(result["resolved"]["start"], json!(TUESDAY_MIDNIGHT));
    assert_eq!(result["resolved"]["end"], json!(TUESDAY_MIDNIGHT + DAY_MS));
}
