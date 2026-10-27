//! Milestone 15 over HTTP: `analytics.report` answers named blocks with the numbers of the
//! equivalent explorations, fans out and refers between blocks, derives columns, and refuses
//! what it cannot answer by path before anything runs.

mod common;

use axum::body::Body;
use axum::http::{Request, header};
use holocron_protocol::operations::api_keys::READ_ANALYTICS_PRESET;
use holocron_server::clock::Clock;
use holocron_server::config::{CollectConfig, parse_net};
use serde_json::{Value, json};

use common::{Answer, HOST, Harness, test_budgets};

/// 2026-10-01T10:00:00Z, 12:00 in Vienna.
const NOW_MS: i64 = 1_790_848_800_000;
const MINUTE_MS: i64 = 60_000;
const DESKTOP: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36";

async fn harness() -> Harness {
    let collect = CollectConfig {
        trusted_proxies: vec![parse_net("127.0.0.1").unwrap()],
        ..CollectConfig::default()
    };
    Harness::with(collect, test_budgets(), Clock::frozen(NOW_MS)).await
}

/// One tracker event: a pageview of `path`, or a custom event with `name`.
async fn send(
    harness: &Harness,
    site: &str,
    ip: &str,
    minute: i64,
    path: &str,
    name: Option<&str>,
) {
    let mut payload = json!({
        "website": site, "hostname": "example.test", "url": path, "language": "de-AT",
        "referrer": "", "timestamp": (NOW_MS - 60 * MINUTE_MS + minute * MINUTE_MS) / 1000,
    });
    if let Some(name) = name {
        payload["name"] = json!(name);
    }
    let request = Request::post("/api/send")
        .header(header::HOST, HOST)
        .header(header::USER_AGENT, DESKTOP)
        .header(header::CONTENT_TYPE, "application/json")
        .header("x-forwarded-for", ip)
        .extension(axum::extract::ConnectInfo(
            "127.0.0.1:9000".parse::<std::net::SocketAddr>().unwrap(),
        ))
        .body(Body::from(
            json!({ "type": "event", "payload": payload }).to_string(),
        ))
        .unwrap();
    assert_eq!(harness.send(request).await.status, 200);
}

/// Three visitors: the first views `/pricing`, signs up there and reads `/docs`; the second
/// views `/pricing` and `/blog`; the third views `/docs`. Pageviews: 2 `/docs`, 2 `/pricing`,
/// 1 `/blog`.
async fn traffic(harness: &Harness, site: &str) {
    send(harness, site, "203.0.113.1", 0, "/pricing", None).await;
    send(harness, site, "203.0.113.1", 2, "/pricing", Some("signup")).await;
    send(harness, site, "203.0.113.1", 3, "/docs", None).await;
    send(harness, site, "203.0.113.2", 5, "/pricing", None).await;
    send(harness, site, "203.0.113.2", 6, "/blog", None).await;
    send(harness, site, "203.0.113.3", 7, "/docs", None).await;
    harness.state.collector.storage().drain().await;
    holocron_analytics::run_until_clean(
        harness.state.collector.storage(),
        holocron_analytics::Limits::default(),
    )
    .await
    .unwrap();
}

/// A key with the "Read analytics" preset, created as the dialog does.
async fn read_key(harness: &Harness) -> String {
    let session = harness.session("p_owner").await;
    let created = harness
        .command(
            Some(&session),
            "apiKeys.create",
            json!({
                "idempotency_key": uuid::Uuid::new_v4().to_string(),
                "name": "agent",
                "operations": READ_ANALYTICS_PRESET,
                "sites": { "kind": "all" },
            }),
        )
        .await
        .json();
    format!("Bearer {}", created["secret"].as_str().unwrap())
}

async fn report(harness: &Harness, key: &str, query: &Value) -> Answer {
    harness
        .query_post(
            Some(key),
            "analytics.report",
            query.to_string().into_bytes(),
        )
        .await
}

async fn explore(harness: &Harness, cookie: &str, state: &Value) -> Value {
    let encoded: String =
        url::form_urlencoded::byte_serialize(state.to_string().as_bytes()).collect();
    ok(&harness
        .query(
            Some(cookie),
            "analytics.explore",
            &format!("state={encoded}"),
        )
        .await)
}

fn ok(answer: &Answer) -> Value {
    assert_eq!(
        answer.status,
        200,
        "{}",
        String::from_utf8_lossy(&answer.bytes)
    );
    answer.json()
}

/// The code and the field of a refused query over `blocks`.
async fn refuse(harness: &Harness, key: &str, blocks: Value) -> (String, String) {
    refused_at(
        &report(
            harness,
            key,
            &json!({ "v": 1, "range": range(), "blocks": blocks }),
        )
        .await,
    )
}

/// The field a refusal names.
fn refused_at(answer: &Answer) -> (String, String) {
    let body = answer.json();
    let field = body["errors"]
        .as_object()
        .and_then(|errors| errors.keys().next().cloned())
        .unwrap_or_default();
    (answer.code(), field)
}

fn cond(id: &str, field: &str, cmp: &str, value: Value) -> Value {
    json!({ "kind": "cond", "id": id, "field": field, "cmp": cmp, "value": value })
}

fn text(v: &str) -> Value {
    json!({ "t": "string", "v": v })
}

fn group(items: Vec<Value>) -> Value {
    json!({ "kind": "group", "op": "and", "items": items })
}

const RANGE: &str = r#"{ "kind": "relative", "unit": "day", "count": 7 }"#;

fn range() -> Value {
    serde_json::from_str(RANGE).unwrap()
}

fn state(site: &str, visualization: &str, report: Option<Value>) -> Value {
    let mut state = json!({
        "v": 1, "site": site, "measures": [], "dimensions": [], "filter": null,
        "range": range(), "timezone": "Europe/Vienna", "granularity": "day", "compare": "none",
        "metric_version": 1, "sort": [], "page": { "size": 50, "index": 0 },
        "visualization": visualization,
    });
    if let Some(report) = report {
        state["report"] = report;
    }
    state
}

fn funnel() -> Value {
    json!({ "kind": "funnel", "v": 2, "window_seconds": 3600, "scope": "daily", "group_by": "path",
        "steps": [
            group(vec![cond("s1", "path", "in", json!({ "t": "list", "v": [text("/pricing"), text("/docs")] }))]),
            group(vec![cond("s2", "event_name", "eq", text("signup"))]),
        ] })
}

fn goals() -> Value {
    json!({ "kind": "goals", "v": 1, "goals": [{
        "predicate": group(vec![cond("g1", "event_name", "eq", text("signup"))]),
        "scope": "session", "counting": "once",
    }] })
}

#[tokio::test(flavor = "multi_thread")]
async fn blocks_answer_the_numbers_of_their_explorations_from_one_snapshot() {
    let harness = harness().await;
    let site = harness.site("Europe/Vienna").await;
    traffic(&harness, &site).await;
    let key = read_key(&harness).await;
    let cookie = harness.session("p_owner").await;

    let measures = json!(["pageviews", "visitors"]);
    let sort = json!([{ "by": "pageviews", "dir": "desc" }]);
    let answer = ok(&report(
        &harness,
        &key,
        &json!({ "v": 1, "range": range(), "timezone": "Europe/Vienna", "blocks": [
            { "name": "pages", "site": site, "body": {
                "kind": "cut", "measures": measures, "dimensions": ["path"], "sort": sort } },
            { "name": "flow", "site": site, "body": { "kind": "report", "report": funnel() } },
            { "name": "signups", "site": site, "body": { "kind": "report", "report": goals() } },
        ] }),
    )
    .await);
    assert!(answer["cursor"].is_string());
    let blocks = answer["blocks"].as_array().unwrap();

    // The cut: rows are the exploration's keys then values, totals lead with a null per key.
    let mut cut = state(&site, "table", None);
    cut["measures"] = measures.clone();
    cut["dimensions"] = json!(["path"]);
    cut["sort"] = sort;
    let explored = explore(&harness, &cookie, &cut).await;
    let expected: Vec<Value> = explored["main"]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            let mut cells = row["key"].as_array().unwrap().clone();
            cells.extend(row["values"].as_array().unwrap().iter().cloned());
            Value::Array(cells)
        })
        .collect();
    assert_eq!(blocks[0]["rows"], Value::Array(expected));
    let mut totals = vec![json!({ "t": "null" })];
    totals.extend(
        explored["main"]["totals"]
            .as_array()
            .unwrap()
            .iter()
            .cloned(),
    );
    assert_eq!(blocks[0]["totals"], Value::Array(totals));
    let names: Vec<_> = blocks[0]["columns"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| (c["name"].clone(), c["type"].clone(), c["role"].clone()))
        .collect();
    assert_eq!(
        names,
        [
            (json!("path"), json!("string"), json!("dimension")),
            (json!("pageviews"), json!("integer"), json!("measure")),
            (json!("visitors"), json!("integer"), json!("measure")),
        ]
    );

    // The funnel: per group a row per step, then one from step 1 to the last.
    let explored = explore(&harness, &cookie, &state(&site, "funnel", Some(funnel()))).await;
    let result = &explored["report"];
    let groups = result["groups"].as_array().unwrap();
    assert_eq!(groups.len(), 2);
    let rows = blocks[1]["rows"].as_array().unwrap();
    assert_eq!(rows.len(), groups.len() * 3);
    for (g, expected) in groups.iter().enumerate() {
        let rows = &rows[g * 3..g * 3 + 3];
        for (i, step) in expected["steps"].as_array().unwrap().iter().enumerate() {
            assert_eq!(rows[i][0], expected["value"]);
            assert_eq!(rows[i][1], json!({ "t": "integer", "v": i + 1 }));
            assert_eq!(rows[i][3], json!({ "t": "count", "v": step["reached"] }));
        }
        let durations = &expected["durations"];
        assert_eq!(rows[1][6], durations[0]["p25"]);
        assert_eq!(rows[1][7], durations[0]["median"]);
        assert_eq!(rows[2][2], json!({ "t": "integer", "v": 1 }));
        assert_eq!(rows[2][5], expected["conversion"]);
        assert_eq!(rows[2][8], durations[1]["p75"]);
    }
    let codes: Vec<_> = blocks[1]["notes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["code"].clone())
        .collect();
    assert!(codes.contains(&json!("daily_limit")), "{codes:?}");
    // The exploration's leaks carry the session IDs that dropped; the report query has no
    // leaks and no session lists anywhere.
    assert!(
        result["leaks"][0]["sessions"]
            .as_array()
            .is_some_and(|s| !s.is_empty())
    );
    let text = answer.to_string();
    assert!(!text.contains("\"leaks\"") && !text.contains("\"sessions\""));

    // Goals: one row per goal.
    let explored = explore(&harness, &cookie, &state(&site, "table", Some(goals()))).await;
    let goal = &explored["report"]["goals"][0];
    assert_eq!(
        blocks[2]["rows"],
        json!([[
            { "t": "integer", "v": 1 },
            { "t": "count", "v": goal["completions"] },
            { "t": "count", "v": goal["denominator"] },
            goal["rate"],
        ]])
    );

    // Every block read the same snapshot.
    for block in blocks {
        assert_eq!(
            block["freshness"]["watermark_batch"],
            answer["watermark"]["batch"]
        );
        assert_eq!(
            block["freshness"]["watermark_sequence"],
            answer["watermark"]["sequence"]
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn for_each_and_refs_take_values_from_earlier_blocks_and_derive_shares() {
    let harness = harness().await;
    let site = harness.site("Europe/Vienna").await;
    traffic(&harness, &site).await;
    let key = harness
        .api_key("p_member", &["analytics.report"], None)
        .await;
    let top_paths = json!({ "ref": { "block": "top", "column": "path" } });

    let answer = ok(&report(
        &harness,
        &key,
        &json!({ "v": 1, "range": range(), "blocks": [
            { "name": "all", "site": site, "body": { "kind": "cut", "measures": ["pageviews"] } },
            { "name": "top", "site": site, "body": { "kind": "cut", "measures": ["pageviews"],
                "dimensions": ["path"], "sort": [{ "by": "pageviews", "dir": "desc" }], "limit": 2 } },
            { "name": "per_page", "site": site,
              "for_each": { "field": "path", "values": top_paths },
              "body": { "kind": "cut", "measures": ["visitors", "pageviews"] },
              "derive": [{ "op": "ratio", "name": "of_site", "a": { "column": "pageviews" },
                           "b": { "block": "all", "column": "pageviews", "total": true } }] },
            { "name": "split", "site": site,
              "filter": group(vec![cond("f1", "path", "in", top_paths.clone())]),
              "body": { "kind": "cut", "measures": ["pageviews"], "dimensions": ["path"] },
              "derive": [{ "op": "share", "name": "share", "a": { "column": "pageviews" } }] },
        ] }),
    )
    .await);
    let blocks = answer["blocks"].as_array().unwrap();
    let count = |v: i64| json!({ "t": "count", "v": v });
    let path = |v: &str| json!({ "t": "string", "v": v });
    let ratio =
        |v: f64, n: i64, d: i64| json!({ "t": "ratio", "v": v, "numerator": n, "denominator": d });

    assert_eq!(blocks[0]["rows"], json!([[count(5)]]));
    assert_eq!(blocks[0]["totals"], json!([count(5)]));
    assert_eq!(
        blocks[1]["notes"][0]["code"],
        json!("rows_truncated"),
        "three paths, two rows"
    );
    assert_eq!(
        blocks[2]["rows"],
        json!([
            [path("/docs"), count(2), count(2), ratio(0.4, 2, 5)],
            [path("/pricing"), count(2), count(2), ratio(0.4, 2, 5)],
        ])
    );
    assert_eq!(blocks[2]["columns"][0]["role"], json!("for_each"));
    assert!(blocks[2].get("totals").is_none());
    assert_eq!(
        blocks[3]["rows"],
        json!([
            [path("/docs"), count(2), ratio(0.5, 2, 4)],
            [path("/pricing"), count(2), ratio(0.5, 2, 4)],
        ])
    );
    assert_eq!(
        blocks[3]["totals"],
        json!([{ "t": "null" }, count(4), ratio(1.0, 4, 4)])
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_cut_over_a_hundred_rows_returns_them_in_one_response() {
    let harness = harness().await;
    let site = harness.site("Europe/Vienna").await;
    for i in 0..150 {
        send(&harness, &site, "203.0.113.1", 0, &format!("/p{i}"), None).await;
    }
    harness.state.collector.storage().drain().await;
    holocron_analytics::run_until_clean(
        harness.state.collector.storage(),
        holocron_analytics::Limits::default(),
    )
    .await
    .unwrap();
    let key = harness
        .api_key("p_member", &["analytics.report"], None)
        .await;
    let cut = |name: &str, limit: u32| {
        json!({ "name": name, "site": site, "body": { "kind": "cut", "measures": ["pageviews"],
            "dimensions": ["path"], "limit": limit } })
    };

    let answer = ok(&report(
        &harness,
        &key,
        &json!({ "v": 1, "range": range(), "blocks": [cut("wide", 500), cut("default", 100)] }),
    )
    .await);
    let blocks = answer["blocks"].as_array().unwrap();
    assert_eq!(blocks[0]["rows"].as_array().unwrap().len(), 150);
    assert_eq!(blocks[1]["rows"].as_array().unwrap().len(), 100);
    for block in blocks {
        assert_eq!(
            block["freshness"]["watermark_sequence"],
            answer["watermark"]["sequence"]
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn refusals_name_their_path_before_anything_runs() {
    let harness = harness().await;
    let site = harness.site("Europe/Vienna").await;
    let key = harness
        .api_key("p_member", &["analytics.report"], None)
        .await;
    let cut = json!({ "kind": "cut", "measures": ["pageviews"] });
    let block = |name: &str| json!({ "name": name, "site": site, "body": cut });

    let mut bad_filter = block("b");
    bad_filter["filter"] = group(vec![cond("f3", "weekday", "contains", text("x"))]);
    assert_eq!(
        refuse(&harness, &key, json!([block("a"), bad_filter])).await,
        ("invalid_arguments".into(), "blocks.1.filter.f3".into())
    );

    let mut bad_operand = block("a");
    bad_operand["derive"] = json!([{ "op": "ratio", "name": "r",
        "a": { "column": "pageviews" }, "b": { "column": "visitors" } }]);
    assert_eq!(
        refuse(&harness, &key, json!([bad_operand])).await,
        ("invalid_arguments".into(), "blocks.0.derive.0.b".into())
    );

    let mut later = block("a");
    later["filter"] = group(vec![cond(
        "f1",
        "path",
        "in",
        json!({ "ref": { "block": "b", "column": "path" } }),
    )]);
    assert_eq!(
        refuse(&harness, &key, json!([later, block("b")])).await,
        ("invalid_arguments".into(), "blocks.0.filter.f1".into())
    );

    let sessions = json!({ "name": "s", "site": site, "body": { "kind": "report", "report": {
        "kind": "session_list", "v": 1, "columns": ["session_id"], "order": "start_desc" } } });
    assert_eq!(
        refuse(&harness, &key, json!([sessions])).await,
        (
            "report_field_unsupported".into(),
            "blocks.0.body.report.kind".into()
        )
    );

    // Over budget is refused before authorization, so even an unknown site answers the budget.
    let fan_out = |name: String| {
        let values: Vec<Value> = (0..50).map(|i| text(&format!("/p{i}"))).collect();
        json!({ "name": name, "site": uuid::Uuid::new_v4().to_string(), "body": cut,
            "for_each": { "field": "path", "values": values } })
    };
    let blocks: Vec<Value> = (0..5).map(|i| fan_out(format!("b{i}"))).collect();
    assert_eq!(
        refuse(&harness, &key, Value::Array(blocks)).await,
        ("report_too_large".into(), "blocks".into())
    );
    let blocks: Vec<Value> = (0..17).map(|i| block(&format!("b{i}"))).collect();
    assert_eq!(
        refuse(&harness, &key, Value::Array(blocks)).await.0,
        "report_too_large"
    );

    let mut unknown = block("a");
    unknown["body"]["colour"] = json!("red");
    let answer = report(
        &harness,
        &key,
        &json!({ "v": 1, "range": range(), "blocks": [unknown] }),
    )
    .await;
    assert_eq!(answer.code(), "malformed_request");
    let message = answer.json()["message"].as_str().unwrap().to_owned();
    assert!(message.starts_with("blocks.0.body"), "{message}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_key_reads_blocks_on_its_own_sites_only() {
    let harness = harness().await;
    let site = harness.site("Europe/Vienna").await;
    let key = harness
        .api_key("p_member", &["analytics.report"], Some(&["site_a1"]))
        .await;
    let cut = json!({ "kind": "cut", "measures": ["pageviews"] });
    let answer = report(
        &harness,
        &key,
        &json!({ "v": 1, "range": range(), "blocks": [
            { "name": "mine", "site": harness.sites["site_a1"].to_string(), "body": cut },
            { "name": "other", "site": site, "body": cut },
        ] }),
    )
    .await;
    assert_eq!(answer.code(), "not_found");
}
