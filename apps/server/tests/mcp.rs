//! Milestone 15 over MCP: `/mcp` negotiates the protocol revision, admits API keys only, refuses
//! foreign origins, and answers tools, resources and prompts through the operation registry.

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use holocron_protocol::operations::api_keys::READ_ANALYTICS_PRESET;
use holocron_protocol::report_query::ReportQuery;
use holocron_server::clock::Clock;
use holocron_server::config::CollectConfig;
use serde_json::{Value, json};

use common::{Answer, HOST, Harness, test_budgets};

/// 2026-10-01T10:00:00Z.
const NOW_MS: i64 = 1_790_848_800_000;
const MODERN: &str = "2026-07-28";

async fn harness() -> Harness {
    Harness::with(
        CollectConfig::default(),
        test_budgets(),
        Clock::frozen(NOW_MS),
    )
    .await
}

/// A key with the "Read analytics" preset, created as the key dialog does.
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

/// Two pageviews of `/pricing` by one visitor, applied to analytics.
async fn traffic(harness: &Harness, site: &str) {
    for minute in [0, 1] {
        let payload = json!({
            "website": site, "hostname": "example.test", "url": "/pricing", "language": "de-AT",
            "referrer": "", "timestamp": (NOW_MS - (10 - minute) * 60_000) / 1000,
        });
        let request = Request::post("/api/send")
            .header(header::HOST, HOST)
            .header(
                header::USER_AGENT,
                "Mozilla/5.0 (X11; Linux x86_64) Firefox/140.0",
            )
            .header(header::CONTENT_TYPE, "application/json")
            .extension(axum::extract::ConnectInfo(
                "203.0.113.1:9000".parse::<std::net::SocketAddr>().unwrap(),
            ))
            .body(Body::from(
                json!({ "type": "event", "payload": payload }).to_string(),
            ))
            .unwrap();
        assert_eq!(harness.send(request).await.status, 200);
    }
    harness.state.collector.storage().drain().await;
    holocron_analytics::run_until_clean(
        harness.state.collector.storage(),
        holocron_analytics::Limits::default(),
    )
    .await
    .unwrap();
}

/// POSTs `body` to `/mcp` with the given headers.
async fn post(harness: &Harness, headers: &[(&str, &str)], body: &Value) -> Answer {
    let mut request = Request::post("/mcp")
        .header(header::HOST, HOST)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ACCEPT, "application/json, text/event-stream");
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    harness
        .send(request.body(Body::from(body.to_string())).unwrap())
        .await
}

/// A 2026-07-28 request: `_meta` in the body, mirrored in the headers.
async fn rpc(harness: &Harness, key: &str, method: &str, mut params: Value) -> Answer {
    params["_meta"] = json!({
        "io.modelcontextprotocol/protocolVersion": MODERN,
        "io.modelcontextprotocol/clientCapabilities": {},
    });
    let name = params["name"]
        .as_str()
        .or(params["uri"].as_str())
        .map(str::to_owned);
    let mut headers = vec![
        ("authorization", key),
        ("mcp-protocol-version", MODERN),
        ("mcp-method", method),
    ];
    if let Some(name) = &name {
        headers.push(("mcp-name", name));
    }
    let body = json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params });
    post(harness, &headers, &body).await
}

fn result(answer: &Answer) -> Value {
    assert_eq!(
        answer.status,
        200,
        "{}",
        String::from_utf8_lossy(&answer.bytes)
    );
    let message = answer.json();
    assert!(message["error"].is_null(), "{message}");
    message["result"].clone()
}

async fn call(harness: &Harness, key: &str, tool: &str, arguments: Value) -> Value {
    result(
        &rpc(
            harness,
            key,
            "tools/call",
            json!({ "name": tool, "arguments": arguments }),
        )
        .await,
    )
}

#[tokio::test(flavor = "multi_thread")]
async fn negotiates_the_revision_per_request_and_by_handshake() {
    let harness = harness().await;
    let key = read_key(&harness).await;

    let discovered = result(&rpc(&harness, &key, "server/discover", json!({})).await);
    assert_eq!(discovered["resultType"], "complete");
    assert_eq!(discovered["supportedVersions"][0], MODERN);
    assert!(discovered["capabilities"]["tools"].is_object());
    assert_eq!(
        discovered["_meta"]["io.modelcontextprotocol/serverInfo"]["name"],
        "holocron"
    );

    // A legacy client keeps its version, an unknown one gets the newest legacy revision.
    for (asked, served) in [("2025-06-18", "2025-06-18"), ("2024-11-05", "2025-11-25")] {
        let body = json!({ "jsonrpc": "2.0", "id": 0, "method": "initialize", "params": {
            "protocolVersion": asked, "capabilities": {},
            "clientInfo": { "name": "test", "version": "1" } } });
        let initialized = result(&post(&harness, &[("authorization", &key)], &body).await);
        assert_eq!(initialized["protocolVersion"], served);
        assert!(initialized["resultType"].is_null());
    }
    let notification = json!({ "jsonrpc": "2.0", "method": "notifications/initialized" });
    let accepted = post(&harness, &[("authorization", &key)], &notification).await;
    assert_eq!(accepted.status, StatusCode::ACCEPTED);
    assert!(accepted.bytes.is_empty());

    // Legacy requests name their version in the header and get no structured content.
    let legacy = json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/call",
        "params": { "name": "list_sites", "arguments": {} } });
    let answered = result(
        &post(
            &harness,
            &[
                ("authorization", &key),
                ("mcp-protocol-version", "2025-03-26"),
            ],
            &legacy,
        )
        .await,
    );
    assert_eq!(answered["isError"], false);
    assert!(answered["structuredContent"].is_null());

    let unknown = json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/list", "params": {} });
    let refused = post(
        &harness,
        &[
            ("authorization", &key),
            ("mcp-protocol-version", "2099-01-01"),
        ],
        &unknown,
    )
    .await;
    assert_eq!(refused.status, StatusCode::BAD_REQUEST);
    assert_eq!(refused.json()["error"]["code"], -32022);
    assert_eq!(refused.json()["error"]["data"]["supported"][0], MODERN);

    // A modern request whose headers do not mirror the body is a header mismatch.
    let body = json!({ "jsonrpc": "2.0", "id": 4, "method": "tools/list", "params": { "_meta": {
        "io.modelcontextprotocol/protocolVersion": MODERN,
        "io.modelcontextprotocol/clientCapabilities": {} } } });
    let mismatched = post(
        &harness,
        &[
            ("authorization", &key),
            ("mcp-protocol-version", MODERN),
            ("mcp-method", "prompts/list"),
        ],
        &body,
    )
    .await;
    assert_eq!(mismatched.status, StatusCode::BAD_REQUEST);
    assert_eq!(mismatched.json()["error"]["code"], -32020);

    let missing = rpc(&harness, &key, "sampling/unknown", json!({})).await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND);
    assert_eq!(missing.json()["error"]["code"], -32601);

    let get = harness
        .send(
            Request::get("/mcp")
                .header(header::HOST, HOST)
                .header(header::AUTHORIZATION, &key)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(get.status, StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test(flavor = "multi_thread")]
async fn lists_five_tools_with_input_schemas() {
    let harness = harness().await;
    let key = read_key(&harness).await;
    let listed = result(&rpc(&harness, &key, "tools/list", json!({})).await);
    let tools = listed["tools"].as_array().unwrap();
    let names: Vec<_> = tools
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        ["list_sites", "catalog", "field_values", "query", "export"]
    );
    for tool in tools {
        assert_eq!(tool["inputSchema"]["type"], "object", "{}", tool["name"]);
        assert!(!tool["description"].as_str().unwrap().is_empty());
    }
    let query = &tools[3]["inputSchema"];
    assert!(query["properties"]["blocks"].is_object());
    assert_eq!(query["required"], json!(["v", "blocks"]));
    assert!(tools[2]["inputSchema"]["properties"]["field"].is_object());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_tool_call_runs_the_query_as_the_key() {
    let harness = harness().await;
    let site = harness.site("Europe/Vienna").await;
    traffic(&harness, &site).await;
    let key = read_key(&harness).await;

    let query = json!({ "v": 1, "range": { "kind": "relative", "unit": "day", "count": 1 },
        "blocks": [{ "name": "pages", "site": site, "body": {
            "kind": "cut", "measures": ["pageviews"], "dimensions": ["path"] } }] });
    let answered = call(&harness, &key, "query", query).await;
    assert_eq!(answered["isError"], false);
    let block = &answered["structuredContent"]["blocks"][0];
    assert_eq!(
        block["rows"],
        json!([[{ "t": "string", "v": "/pricing" }, { "t": "count", "v": 2 }]])
    );
    assert!(answered["structuredContent"]["cursor"].is_null());
    let text: Value =
        serde_json::from_str(answered["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(text, answered["structuredContent"]);

    // A refusal comes back as a tool error naming the path to fix.
    let bad = json!({ "v": 1, "range": { "kind": "relative", "unit": "day", "count": 1 },
        "blocks": [{ "name": "pages", "site": site, "body": {
            "kind": "cut", "measures": ["pageviews"], "colour": "red" } }] });
    let refused = call(&harness, &key, "query", bad).await;
    assert_eq!(refused["isError"], true);
    let text = refused["content"][0]["text"].as_str().unwrap();
    assert!(
        text.starts_with("malformed_request: blocks.0.body"),
        "{text}"
    );
    assert_eq!(
        refused["structuredContent"]["error_code"],
        "malformed_request"
    );

    // field_values takes the field as in a report query.
    let values = call(
        &harness,
        &key,
        "field_values",
        json!({ "site": site, "field": "path" }),
    )
    .await;
    assert_eq!(values["isError"], false, "{values}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_key_without_the_grant_gets_a_tool_error() {
    let harness = harness().await;
    let key = harness.api_key("p_owner", &["sites.list"], None).await;
    let site = harness.sites["site_a1"].as_str().to_owned();
    let query = json!({ "v": 1, "range": { "kind": "relative", "unit": "day", "count": 1 },
        "blocks": [{ "name": "pages", "site": site, "body": {
            "kind": "cut", "measures": ["pageviews"] } }] });
    let refused = call(&harness, &key, "query", query).await;
    assert_eq!(refused["isError"], true);
    let text = refused["content"][0]["text"].as_str().unwrap();
    assert!(text.starts_with("operation_not_granted: "), "{text}");

    let listed = call(&harness, &key, "list_sites", json!({})).await;
    assert_eq!(listed["isError"], false);
}

#[tokio::test(flavor = "multi_thread")]
async fn requests_without_a_valid_key_get_a_bearer_challenge() {
    let harness = harness().await;
    let body = json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" });

    let missing = post(&harness, &[], &body).await;
    assert_eq!(missing.status, StatusCode::UNAUTHORIZED);
    assert_eq!(
        missing.header("www-authenticate"),
        r#"Bearer realm="holocron""#
    );

    let wrong = post(&harness, &[("authorization", "Bearer nope")], &body).await;
    assert_eq!(wrong.status, StatusCode::UNAUTHORIZED);
    assert!(wrong.header("www-authenticate").contains("invalid_token"));

    let session = harness.session("p_owner").await;
    let cookie = post(&harness, &[("cookie", &session)], &body).await;
    assert_eq!(cookie.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_foreign_origin_is_refused() {
    let harness = harness().await;
    let key = read_key(&harness).await;
    let body = json!({ "jsonrpc": "2.0", "id": 1, "method": "ping" });
    let foreign = post(
        &harness,
        &[("authorization", &key), ("origin", "https://evil.test")],
        &body,
    )
    .await;
    assert_eq!(foreign.status, StatusCode::FORBIDDEN);
    assert!(foreign.json()["id"].is_null());

    let own = post(
        &harness,
        &[("authorization", &key), ("origin", common::ORIGIN)],
        &body,
    )
    .await;
    assert_eq!(own.status, StatusCode::OK);
}

#[tokio::test(flavor = "multi_thread")]
async fn resources_hold_the_schema_and_the_guide() {
    let harness = harness().await;
    let key = read_key(&harness).await;
    let listed = result(&rpc(&harness, &key, "resources/list", json!({})).await);
    assert_eq!(listed["resources"].as_array().unwrap().len(), 2);

    let schema = result(
        &rpc(
            &harness,
            &key,
            "resources/read",
            json!({ "uri": "holocron://report-query/schema" }),
        )
        .await,
    );
    let text = schema["contents"][0]["text"].as_str().unwrap();
    let schema: Value = serde_json::from_str(text).unwrap();
    assert!(schema["properties"]["blocks"].is_object());

    let guide = result(
        &rpc(
            &harness,
            &key,
            "resources/read",
            json!({ "uri": "holocron://report-query/guide" }),
        )
        .await,
    );
    assert_eq!(guide["contents"][0]["mimeType"], "text/markdown");
    assert!(
        guide["contents"][0]["text"]
            .as_str()
            .unwrap()
            .contains("group_by")
    );

    let unknown = rpc(
        &harness,
        &key,
        "resources/read",
        json!({ "uri": "holocron://nope" }),
    )
    .await;
    assert_eq!(unknown.json()["error"]["code"], -32602);
}

/// Every complete example in the guide is a report query this build reads.
#[test]
fn the_guide_examples_parse() {
    let guide = include_str!("../src/mcp/guide.md");
    let examples: Vec<&str> = guide
        .split("```json\n")
        .skip(1)
        .filter_map(|rest| rest.split("\n```").next())
        .filter(|example| example.starts_with("{ \"v\": 1") || example.starts_with("{\n  \"v\": 1"))
        .collect();
    assert_eq!(examples.len(), 5);
    for example in examples {
        let parsed: Result<ReportQuery, _> = serde_json::from_str(example);
        assert!(parsed.is_ok(), "{example}: {:?}", parsed.err());
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_prompt_fills_in_its_arguments() {
    let harness = harness().await;
    let key = read_key(&harness).await;
    let listed = result(&rpc(&harness, &key, "prompts/list", json!({})).await);
    assert_eq!(listed["prompts"].as_array().unwrap().len(), 4);

    let site = "94db1cb1-74f4-4a40-ad6c-962362670409";
    let prompt = result(
        &rpc(
            &harness,
            &key,
            "prompts/get",
            json!({ "name": "event_sales_report", "arguments": {
                "site": site, "pages": "/*/events/**", "range": "2026-09-01..2026-09-30" } }),
        )
        .await,
    );
    let text = prompt["messages"][0]["content"]["text"].as_str().unwrap();
    assert!(text.contains(site));
    let query = text
        .split("```json\n")
        .nth(1)
        .and_then(|rest| rest.split("\n```").next())
        .unwrap();
    let query: Value = serde_json::from_str(query).unwrap();
    assert_eq!(query["range"]["start"], "2026-09-01");
    assert_eq!(
        query["blocks"][0]["filter"]["items"][0]["value"]["v"],
        "/*/events/**"
    );
    assert_eq!(query["blocks"][1]["body"]["report"]["group_by"], "path");

    let incomplete = rpc(
        &harness,
        &key,
        "prompts/get",
        json!({ "name": "event_sales_report", "arguments": { "site": site } }),
    )
    .await;
    assert_eq!(incomplete.json()["error"]["code"], -32602);
}
