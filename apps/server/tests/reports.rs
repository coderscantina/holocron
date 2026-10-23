//! Milestone 07 over HTTP: versioned goals, behavioral reports computed from tracker payloads,
//! and their states surviving save, pin, export and reopening, plus refusals naming the
//! report field.

mod common;

use axum::body::Body;
use axum::http::{Request, header};
use holocron_server::clock::Clock;
use holocron_server::config::{CollectConfig, parse_net};
use serde_json::{Value, json};

use common::{HOST, Harness, test_budgets};

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

/// One tracker event: a pageview of `url`, or a custom event with `name`.
async fn send(harness: &Harness, site: &str, ip: &str, minute: i64, url: &str, name: Option<&str>) {
    let mut payload = json!({
        "website": site, "hostname": "example.test", "url": url, "language": "de-AT",
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

/// Two visitors view `/pricing`; the first signs up from a campaign link.
async fn traffic(harness: &Harness, site: &str) {
    send(
        harness,
        site,
        "203.0.113.1",
        0,
        "/pricing?utm_source=news&utm_campaign=autumn",
        None,
    )
    .await;
    send(
        harness,
        site,
        "203.0.113.1",
        2,
        "/pricing?utm_source=news&utm_campaign=autumn",
        Some("signup"),
    )
    .await;
    send(harness, site, "203.0.113.2", 5, "/pricing", None).await;
    harness.state.collector.storage().drain().await;
    holocron_analytics::run_until_clean(
        harness.state.collector.storage(),
        holocron_analytics::Limits::default(),
    )
    .await
    .unwrap();
}

fn cond(id: &str, field: &str, value: &str) -> Value {
    json!({ "kind": "cond", "id": id, "field": field, "cmp": "eq", "value": { "t": "string", "v": value } })
}

fn group(items: Vec<Value>) -> Value {
    json!({ "kind": "group", "op": "and", "items": items })
}

fn state(site: &str, visualization: &str, report: Value) -> Value {
    json!({
        "v": 1, "site": site, "measures": [], "dimensions": [], "filter": null,
        "range": { "kind": "relative", "unit": "day", "count": 7 }, "timezone": "Europe/Vienna",
        "granularity": "day", "compare": "none", "metric_version": 1, "sort": [],
        "page": { "size": 50, "index": 0 }, "visualization": visualization, "report": report
    })
}

fn funnel(site: &str) -> Value {
    state(
        site,
        "funnel",
        json!({ "kind": "funnel", "v": 1, "window_seconds": 3600, "scope": "daily", "steps": [
            group(vec![cond("s1", "path", "/pricing")]),
            group(vec![cond("s2", "event_name", "signup")]),
        ] }),
    )
}

fn signup() -> Value {
    json!({ "predicate": group(vec![cond("g1", "event_name", "signup")]), "scope": "session", "counting": "once" })
}

fn encode(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

async fn explore(harness: &Harness, cookie: &str, state: &Value) -> common::Answer {
    let query = format!("state={}", encode(&state.to_string()));
    harness
        .query(Some(cookie), "analytics.explore", &query)
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
async fn goal_edits_are_versions_and_saved_reports_keep_theirs() {
    let harness = harness().await;
    let site = harness.site("Europe/Vienna").await;
    traffic(&harness, &site).await;
    let owner = harness.session("p_owner").await;
    let viewer = harness.session("p_viewer").await;

    let saved = ok(&command(
        &harness,
        &owner,
        "goals.save",
        json!({ "site": site, "name": " Signup ", "definition": signup() }),
    )
    .await);
    let goal = saved["goal"]["id"].as_str().unwrap().to_owned();
    assert_eq!(
        (
            saved["goal"]["name"].as_str(),
            saved["goal"]["version"].as_u64()
        ),
        (Some("Signup"), Some(1))
    );

    // A new name alone is not a new version; a changed definition is.
    let renamed = ok(&command(
        &harness,
        &owner,
        "goals.save",
        json!({ "site": site, "goal": goal, "name": "Signups", "definition": signup() }),
    )
    .await);
    assert_eq!(renamed["goal"]["version"], 1);
    let mut every = signup();
    every["counting"] = json!("every");
    let edited = ok(&command(
        &harness,
        &owner,
        "goals.save",
        json!({ "site": site, "goal": goal, "name": "Signups", "definition": every }),
    )
    .await);
    assert_eq!(
        (
            edited["goal"]["version"].as_u64(),
            edited["goal"]["latest_version"].as_u64()
        ),
        (Some(2), Some(2))
    );
    let first = ok(&harness
        .query(
            Some(&viewer),
            "goals.get",
            &format!("site={site}&goal={goal}&version=1"),
        )
        .await);
    assert_eq!(
        (
            first["goal"]["definition"].clone(),
            first["goal"]["latest_version"].as_u64()
        ),
        (signup(), Some(2))
    );

    // A report saved with version 1 inline reopens with version 1 and computes it.
    let mut inline = signup();
    inline["ref"] = json!({ "id": goal, "version": 1 });
    let report = state(
        &site,
        "table",
        json!({ "kind": "goals", "v": 1, "goals": [inline] }),
    );
    let view = ok(&command(
        &harness,
        &owner,
        "explorations.save",
        json!({ "site": site, "name": "Signup goal", "state": report }),
    )
    .await);
    let id = view["exploration"]["id"].as_str().unwrap();
    let reopened = ok(&harness
        .query(
            Some(&owner),
            "explorations.get",
            &format!("site={site}&exploration={id}"),
        )
        .await);
    assert_eq!(reopened["exploration"]["state"], report);
    let result = ok(&explore(&harness, &owner, &report).await);
    let computed = &result["report"]["goals"][0];
    assert_eq!(
        (
            computed["completions"].as_i64(),
            computed["denominator"].as_i64()
        ),
        (Some(1), Some(2))
    );

    // Viewers read goals but cannot write them; names stay unique among active goals.
    let list = ok(&harness
        .query(Some(&viewer), "goals.list", &format!("site={site}"))
        .await);
    assert_eq!(list["goals"].as_array().unwrap().len(), 1);
    let refused = command(
        &harness,
        &viewer,
        "goals.save",
        json!({ "site": site, "name": "Mine", "definition": signup() }),
    )
    .await;
    assert_eq!(refused.code(), "forbidden");
    let taken = command(
        &harness,
        &owner,
        "goals.save",
        json!({ "site": site, "name": "SIGNUPS", "definition": signup() }),
    )
    .await;
    assert!(
        taken.json()["errors"]["name"].is_array(),
        "{}",
        taken.json()
    );

    // Archiving leaves the list; every version stays readable for saved reports.
    ok(&command(
        &harness,
        &owner,
        "goals.archive",
        json!({ "site": site, "goal": goal }),
    )
    .await);
    let list = ok(&harness
        .query(Some(&viewer), "goals.list", &format!("site={site}"))
        .await);
    assert_eq!(list["goals"], json!([]));
    let archived = ok(&harness
        .query(
            Some(&viewer),
            "goals.get",
            &format!("site={site}&goal={goal}&version=1"),
        )
        .await);
    assert_eq!(archived["goal"]["archived"], true);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_funnel_leak_tells_where_the_leavers_went() {
    let harness = harness().await;
    let site = harness.site("Europe/Vienna").await;
    traffic(&harness, &site).await;
    // Beside the one who signed up and the one who left: one moves on, one goes back.
    send(&harness, &site, "203.0.113.3", 10, "/pricing", None).await;
    send(&harness, &site, "203.0.113.3", 11, "/about", None).await;
    send(&harness, &site, "203.0.113.4", 20, "/about", None).await;
    send(&harness, &site, "203.0.113.4", 21, "/pricing", None).await;
    send(&harness, &site, "203.0.113.4", 22, "/about", None).await;
    harness.state.collector.storage().drain().await;
    holocron_analytics::run_until_clean(
        harness.state.collector.storage(),
        holocron_analytics::Limits::default(),
    )
    .await
    .unwrap();
    let owner = harness.session("p_owner").await;

    let result = ok(&explore(&harness, &owner, &funnel(&site)).await);
    let leak = &result["report"]["leaks"][0];
    assert_eq!(leak["exited"], 1);
    assert_eq!(leak["back"], 1);
    assert_eq!(leak["next"], json!([{ "path": "/about", "identities": 1 }]));
    assert_eq!(leak["other"], 0);
    assert_eq!(leak["sessions"].as_array().map(Vec::len), Some(3));
    assert_eq!(result["report"]["leaks"].as_array().map(Vec::len), Some(1));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_funnel_keeps_its_state_through_save_pin_and_export() {
    let harness = harness().await;
    let site = harness.site("Europe/Vienna").await;
    traffic(&harness, &site).await;
    let owner = harness.session("p_owner").await;
    let funnel = funnel(&site);

    let result = ok(&explore(&harness, &owner, &funnel).await);
    let report = &result["report"];
    assert_eq!(report["kind"], "funnel");
    assert_eq!(
        report["steps"],
        json!([{ "reached": 2, "drop": 1 }, { "reached": 1 }])
    );
    assert_eq!(report["conversion"]["v"], 0.5);
    assert_eq!(report["daily_limit"], true);

    let saved = ok(&command(
        &harness,
        &owner,
        "explorations.save",
        json!({ "site": site, "name": "Pricing to signup", "state": funnel }),
    )
    .await);
    let id = saved["exploration"]["id"].as_str().unwrap();
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
        json!({ "board": board_id, "title": "Funnel", "state": funnel, "exploration": id }),
    )
    .await);
    let board = ok(&harness
        .query(Some(&owner), "boards.get", &format!("board={board_id}"))
        .await);
    let card = &board["board"]["cards"][0]["state"];
    assert_eq!(*card, funnel);
    // The reopened card computes the same report on the same snapshot.
    assert_eq!(
        ok(&explore(&harness, &owner, card).await)["report"],
        *report
    );

    let export = |format: &str| format!("format={format}&state={}", encode(&funnel.to_string()));
    let json_file = ok(&harness
        .query(Some(&owner), "analytics.export", &export("json"))
        .await);
    let file: Value = serde_json::from_str(json_file["content"].as_str().unwrap()).unwrap();
    assert_eq!(
        (&file["state"], &file["result"]["report"]),
        (&funnel, report)
    );
    let csv = ok(&harness
        .query(Some(&owner), "analytics.export", &export("csv"))
        .await);
    assert_eq!(csv["content"], "step,reached,drop\r\n1,2,1\r\n2,1,\r\n");
    assert_eq!(csv["row_count"], 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn attribution_and_session_lists_export_their_rows() {
    let harness = harness().await;
    let site = harness.site("Europe/Vienna").await;
    traffic(&harness, &site).await;
    let owner = harness.session("p_owner").await;

    let attribution = state(
        &site,
        "table",
        json!({ "kind": "attribution", "v": 1, "model": "last_touch", "lookback_days": 30, "scope": "daily",
            "touch_field": "utm_campaign", "conversions": [{ "kind": "goal", "goal": signup() }] }),
    );
    let result = ok(&explore(&harness, &owner, &attribution).await);
    let conversion = &result["report"]["conversions"][0];
    assert_eq!(
        conversion["touches"],
        json!([{ "touch": { "t": "string", "v": "autumn" }, "attributed": { "conversions": 1, "revenue": [] } }])
    );
    assert_eq!(conversion["direct"]["conversions"], 0);
    let csv = ok(&harness
        .query(
            Some(&owner),
            "analytics.export",
            &format!("format=csv&state={}", encode(&attribution.to_string())),
        )
        .await);
    assert_eq!(
        csv["content"],
        "conversion,touch,conversions,revenue\r\ngoal 1,autumn,1,\r\ngoal 1,(direct),0,\r\n"
    );

    // The drill target: sessions with a matching event, every one of them in the export.
    let mut sessions = state(
        &site,
        "table",
        json!({ "kind": "session_list", "v": 1, "columns": ["entry_path", "pageviews", "utm_campaign"], "order": "start_asc" }),
    );
    sessions["filter"] = group(vec![cond("f1", "event_name", "signup")]);
    sessions["page"] = json!({ "size": 1, "index": 0 });
    let page = ok(&explore(&harness, &owner, &sessions).await);
    assert_eq!(page["report"]["row_count"], 1);
    let csv = ok(&harness
        .query(
            Some(&owner),
            "analytics.export",
            &format!("format=csv&state={}", encode(&sessions.to_string())),
        )
        .await);
    assert_eq!(
        csv["content"],
        "entry_path,pageviews,utm_campaign\r\n/pricing,1,autumn\r\n"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn report_refusals_name_the_report_field() {
    let harness = harness().await;
    let site = harness.site("Europe/Vienna").await;
    traffic(&harness, &site).await;
    let owner = harness.session("p_owner").await;
    let funnel = funnel(&site);
    let with = |mut state: Value, path: &str, value: Value| {
        *state.pointer_mut(path).unwrap() = value;
        state
    };
    let unknown = json!({ "kind": "cond", "id": "p", "field": { "scope": "event", "key": "nope" }, "cmp": "eq", "value": { "t": "string", "v": "x" } });
    let cases = [
        (
            with(
                funnel.clone(),
                "/report/steps",
                json!([group(vec![cond("s1", "path", "/")])]),
            ),
            "report_invalid",
            "report.steps",
        ),
        (
            with(funnel.clone(), "/compare", json!("previous_period")),
            "report_field_unsupported",
            "compare",
        ),
        (
            with(funnel.clone(), "/report/steps/1/items/0/id", json!("s1")),
            "duplicate_predicate_id",
            "report.steps.1.s1",
        ),
        (
            with(funnel.clone(), "/report/steps/1/items/0", unknown),
            "invalid_arguments",
            "report.steps.1.p",
        ),
        (
            with(funnel.clone(), "/report/v", json!(2)),
            "report_version_unsupported",
            "report.v",
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
    // A goal is checked like a report predicate and named after the definition.
    let bad = json!({ "predicate": group(vec![json!({ "kind": "cond", "id": "w", "field": "weekday", "cmp": "contains", "value": { "t": "integer", "v": 1 } })]), "scope": "session", "counting": "once" });
    let answer = command(
        &harness,
        &owner,
        "goals.save",
        json!({ "site": site, "name": "Bad", "definition": bad }),
    )
    .await;
    assert!(
        answer.json()["errors"]["definition.predicate.w"].is_array(),
        "{}",
        answer.json()
    );
}
