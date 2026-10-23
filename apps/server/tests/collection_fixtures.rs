//! Replays every `fixtures/collection` request case through the real router, storage and
//! identity derivation, and checks Holocron's documented status, the upstream body and headers
//! where the status is preserved, and the counted outcome.

mod common;

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::Path;

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, header};
use holocron_server::clock::Clock;
use holocron_server::config::{CollectConfig, parse_net};
use holocron_storage::Budgets;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use common::{Answer, Harness, test_budgets};

const BROWSER_UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36";
const WEBSITES: [&str; 4] = [
    "5e1a0000-0000-4000-8000-000000000001",
    "5e1a0000-0000-4000-8000-000000000002",
    "5e1a0000-0000-4000-8000-000000000003",
    "5e1a0000-0000-4000-8000-000000000004",
];
const LINK: &str = "5e1a0000-0000-4000-8000-0000000000a1";
const PIXEL: &str = "5e1a0000-0000-4000-8000-0000000000b1";
/// The harness peer, listed in `collect.trusted_proxies` as the fixture notes require.
const PEER: &str = "127.0.0.1:50000";
/// A peer outside `collect.trusted_proxies`, for cases written against the empty default.
const UNTRUSTED_PEER: &str = "198.51.100.20:50000";
const UNTRUSTED_CASES: [&str; 1] = ["send-event-ignored-ip-header"];
/// Recording policies of the harness websites, as `tools/collection-harness/seed.mjs` sets
/// them upstream: replay and heatmaps at 100 %, heatmaps only, replay only.
const RECORDING: [(&str, bool, bool); 3] = [
    ("5e1a0000-0000-4000-8000-000000000002", true, true),
    ("5e1a0000-0000-4000-8000-000000000003", false, true),
    ("5e1a0000-0000-4000-8000-000000000004", true, false),
];
/// Same status as upstream, deliberately different body: `payload.userAgent` is not trusted
/// from unauthenticated callers, so the event is stored instead of answered as a bot (0001).
const BODY_DIFFERS: [&str; 1] = ["send-event-bot-payload-user-agent"];
/// Accepted requests that carry no event, so no outcome is counted.
const NOTHING_TO_COUNT: [&str; 2] = ["batch-empty", "record-empty-events"];

/// Counters a case must raise beyond its outcome.
fn expected_counters(id: &str) -> &'static [&'static str] {
    match id {
        "send-event-timestamp-keyless-day" => &["keyless"],
        "send-event-timestamp-boundary-plus-5min-1s" | "send-event-timestamp-future-10min" => {
            &["event_time_ahead"]
        }
        "send-event-eventid-duplicate-different-payload" => &["dedup_conflict"],
        "send-identify-consent-invalid-value"
        | "send-identify-consent-none"
        | "send-identify-consent-persistent"
        | "send-identify-consent-without-id" => &["consent_ignored"],
        "send-event-properties-over-50" => &["property_dropped"],
        _ => &[],
    }
}

fn fixtures() -> Vec<Value> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/collection");
    let mut paths: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|e| e == "json"))
        .collect();
    paths.sort();
    paths
        .iter()
        .map(|path| serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap())
        .collect()
}

fn collect_config(endpoint: Option<&str>, recording_cap_bytes: u64) -> CollectConfig {
    CollectConfig {
        trusted_proxies: vec![parse_net("127.0.0.1").unwrap()],
        ignore_ips: vec![parse_net("203.0.113.7").unwrap()],
        tracker_script_names: vec!["analytics.js".into(), "stats/a.js".into()],
        collect_endpoint: endpoint.map(str::to_owned),
        recording_cap_bytes,
        ..CollectConfig::default()
    }
}

async fn harness(endpoint: Option<&str>, budgets: Budgets) -> Harness {
    harness_with_cap(
        endpoint,
        budgets,
        CollectConfig::default().recording_cap_bytes,
    )
    .await
}

async fn harness_with_cap(
    endpoint: Option<&str>,
    budgets: Budgets,
    recording_cap_bytes: u64,
) -> Harness {
    let harness = Harness::with(
        collect_config(endpoint, recording_cap_bytes),
        budgets,
        Clock::system(),
    )
    .await;
    let workspace = harness
        .workspaces
        .values()
        .next()
        .unwrap()
        .as_str()
        .to_owned();
    for (index, id) in WEBSITES.iter().enumerate() {
        sqlx::query("INSERT INTO sites (id, workspace_id, name, domain, created_at) VALUES (?, ?, ?, 'localhost', 0)")
            .bind(id)
            .bind(&workspace)
            .bind(format!("holocron {index}"))
            .execute(&harness.state.db.writer)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO session_policies (site_id, version, epoch, effective_at, timezone, window_minutes, created_at)
             VALUES (?, 1, 1, -9223372036854775808, 'UTC', 30, 0)",
        )
        .bind(id)
        .execute(&harness.state.db.writer)
        .await
        .unwrap();
    }
    for (site, replay, heatmap) in RECORDING {
        sqlx::query(
            "INSERT INTO recording_policies (site_id, replay_enabled, heatmap_enabled, replay_sample_percent,
                 heatmap_sample_percent, max_duration_minutes, excluded_elements, excluded_paths, version, updated_at)
             VALUES (?, ?, ?, 100, 100, 5, '[]', '[]', 1, 0)",
        )
        .bind(site)
        .bind(replay)
        .bind(heatmap)
        .execute(&harness.state.db.writer)
        .await
        .unwrap();
    }
    sqlx::query("INSERT INTO links (id, workspace_id, name, slug, url, created_at) VALUES (?, ?, 'holocron link', 'holocron-link', 'https://example.com/landing?from=holocron', 0)")
        .bind(LINK)
        .bind(&workspace)
        .execute(&harness.state.db.writer)
        .await
        .unwrap();
    sqlx::query("INSERT INTO pixels (id, workspace_id, name, slug, created_at) VALUES (?, ?, 'holocron pixel', 'holocron-pixel', 0)")
        .bind(PIXEL)
        .bind(&workspace)
        .execute(&harness.state.db.writer)
        .await
        .unwrap();
    harness
}

struct Runner {
    harness: Harness,
    admin: String,
    tokens: BTreeMap<String, String>,
    run_id: String,
}

impl Runner {
    async fn new(harness: Harness) -> Self {
        let admin = harness.session("p_install_admin").await;
        Self {
            harness,
            admin,
            tokens: BTreeMap::new(),
            run_id: format!("{}", std::process::id()),
        }
    }

    async fn send(&mut self, request: &Value, peer: &str) -> Answer {
        let method = request["method"].as_str().unwrap();
        let path = request["path"].as_str().unwrap();
        let mut builder = Request::builder()
            .method(method)
            .uri(path)
            .header(header::HOST, common::HOST)
            .header(header::USER_AGENT, BROWSER_UA)
            .extension(ConnectInfo(peer.parse::<SocketAddr>().unwrap()));
        let body = if let Some(raw) = request["body_raw"].as_str() {
            Body::from(self.resolve(&json!(raw)).await.as_str().unwrap().to_owned())
        } else if !request["body"].is_null() {
            builder = builder.header(header::CONTENT_TYPE, "application/json");
            Body::from(self.resolve(&request["body"]).await.to_string())
        } else {
            Body::empty()
        };
        let mut request_built = builder.body(body).unwrap();
        if let Some(headers) = request["headers"].as_object() {
            for (name, value) in headers {
                let name = header::HeaderName::try_from(name.to_lowercase()).unwrap();
                match self.resolve(value).await {
                    Value::Null => {
                        request_built.headers_mut().remove(&name);
                    }
                    Value::String(value) => {
                        request_built
                            .headers_mut()
                            .insert(name, value.parse().unwrap());
                    }
                    other => panic!("header value {other}"),
                }
            }
        }
        self.harness.send(request_built).await
    }

    /// A real cache token for a website, from one pageview.
    async fn token(&mut self, website: &str) -> String {
        if let Some(token) = self.tokens.get(website) {
            return token.clone();
        }
        let request = json!({
            "method": "POST",
            "path": "/api/send",
            "body": { "type": "event", "payload": { "website": website, "url": "/cache-bootstrap", "hostname": "localhost" } }
        });
        let answer = Box::pin(self.send(&request, PEER)).await;
        let token = answer.json()["cache"].as_str().unwrap().to_owned();
        self.tokens.insert(website.into(), token.clone());
        token
    }

    async fn resolve(&mut self, value: &Value) -> Value {
        match value {
            Value::Array(items) => {
                let mut out = Vec::new();
                for item in items {
                    out.push(Box::pin(self.resolve(item)).await);
                }
                Value::Array(out)
            }
            Value::Object(map) if map.contains_key("$repeat") => {
                let item = Box::pin(self.resolve(&map["item"])).await;
                Value::Array(vec![item; map["$repeat"].as_u64().unwrap() as usize])
            }
            Value::Object(map) => {
                let mut out = serde_json::Map::new();
                for (key, item) in map {
                    out.insert(key.clone(), Box::pin(self.resolve(item)).await);
                }
                Value::Object(out)
            }
            Value::String(text) => {
                if let Some(offset) = text
                    .strip_prefix("{{NOW_S")
                    .and_then(|rest| rest.strip_suffix("}}"))
                {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs() as i64;
                    let offset: i64 = if offset.is_empty() {
                        0
                    } else {
                        offset.trim_start_matches('+').parse().unwrap()
                    };
                    return json!(now + offset);
                }
                if let Some(website) = text
                    .strip_prefix("{{CACHE:")
                    .and_then(|rest| rest.strip_suffix("}}"))
                {
                    return json!(self.token(website).await);
                }
                let mut text = text.replace("{{RUN_ID}}", &self.run_id);
                while let Some(start) = text.find("{{FILL:") {
                    let end = start + text[start..].find("}}").unwrap();
                    let n: usize = text[start + 7..end].parse().unwrap();
                    text.replace_range(start..end + 2, &"x".repeat(n));
                }
                Value::String(text)
            }
            other => other.clone(),
        }
    }

    async fn status(&self) -> Value {
        let answer = self
            .harness
            .query(Some(&self.admin), "collection.status", "")
            .await;
        assert_eq!(
            answer.status,
            200,
            "{}",
            String::from_utf8_lossy(&answer.bytes)
        );
        answer.json()
    }
}

/// Totals per `outcome` and per `outcome:reason`, plus every counter.
fn tally(status: &Value) -> BTreeMap<String, i64> {
    let mut totals = BTreeMap::new();
    for row in status["outcomes"].as_array().unwrap() {
        let count = row["count"].as_i64().unwrap();
        let outcome = row["outcome"].as_str().unwrap();
        *totals.entry(outcome.to_owned()).or_default() += count;
        if let Some(reason) = row["reason"].as_str() {
            *totals.entry(format!("{outcome}:{reason}")).or_default() += count;
        }
    }
    for row in status["counters"].as_array().unwrap() {
        totals.insert(
            row["counter"].as_str().unwrap().to_owned(),
            row["count"].as_i64().unwrap(),
        );
    }
    totals
}

fn grew(before: &BTreeMap<String, i64>, after: &BTreeMap<String, i64>, key: &str) -> bool {
    after.get(key).copied().unwrap_or(0) > before.get(key).copied().unwrap_or(0)
}

/// Upstream's zod detail (`errors`, `properties`) is not part of Holocron's contract.
fn without_zod_detail(mut expected: Value) -> Value {
    fn strip(value: &mut Value) {
        match value {
            Value::Object(map) => {
                if map.contains_key("code") {
                    map.remove("errors");
                    map.remove("properties");
                }
                map.values_mut().for_each(strip);
            }
            Value::Array(items) => items.iter_mut().for_each(strip),
            _ => {}
        }
    }
    strip(&mut expected);
    expected
}

fn matches(expected: &Value, actual: &Value, exact: bool, at: &str, problems: &mut Vec<String>) {
    let token = |t: &str, v: &Value| match t {
        "<any>" => Some(true),
        "<string>" => Some(v.is_string()),
        "<number>" => Some(v.is_number()),
        "<boolean>" => Some(v.is_boolean()),
        "<uuid>" => Some(
            v.as_str()
                .is_some_and(|s| uuid::Uuid::try_parse(s).is_ok() && s.len() == 36),
        ),
        "<jwt>" => Some(v.as_str().is_some_and(|s| s.split('.').count() == 3)),
        "<null>" => Some(v.is_null()),
        "<array>" => Some(v.is_array()),
        "<object>" => Some(v.is_object()),
        _ => None,
    };
    match expected {
        Value::String(t) if token(t, actual).is_some() => {
            if token(t, actual) != Some(true) {
                problems.push(format!("{at}: expected {t}, got {actual}"));
            }
        }
        Value::Array(items) => match actual.as_array() {
            Some(actual) if actual.len() == items.len() => {
                for (i, (e, a)) in items.iter().zip(actual).enumerate() {
                    matches(e, a, exact, &format!("{at}[{i}]"), problems);
                }
            }
            _ => problems.push(format!(
                "{at}: expected array of {}, got {actual}",
                items.len()
            )),
        },
        Value::Object(map) => {
            let Some(actual_map) = actual.as_object() else {
                return problems.push(format!("{at}: expected object, got {actual}"));
            };
            for (key, e) in map {
                match (e.as_str() == Some("<absent>"), actual_map.get(key)) {
                    (true, Some(_)) => problems.push(format!("{at}.{key}: expected absent")),
                    (true, None) => {}
                    (false, Some(a)) => matches(e, a, exact, &format!("{at}.{key}"), problems),
                    (false, None) => problems.push(format!("{at}.{key}: missing")),
                }
            }
            if exact {
                for key in actual_map.keys().filter(|k| !map.contains_key(*k)) {
                    problems.push(format!("{at}.{key}: unexpected key"));
                }
            }
        }
        _ if expected != actual => {
            problems.push(format!("{at}: expected {expected}, got {actual}"))
        }
        _ => {}
    }
}

fn compare_upstream(expected: &Value, answer: &Answer, problems: &mut Vec<String>) {
    for (name, want) in expected["headers"].as_object().into_iter().flatten() {
        let got = answer.headers.get(name).and_then(|v| v.to_str().ok());
        let ok = match want.as_str().unwrap() {
            "<absent>" => got.is_none(),
            "<present>" => got.is_some(),
            want => got == Some(want),
        };
        if !ok {
            problems.push(format!("header {name}: expected {want}, got {got:?}"));
        }
    }
    if let Some(sha) = expected["body_sha256"].as_str() {
        let got = format!("{:x}", Sha256::digest(&answer.bytes));
        if got != sha {
            problems.push(format!("body sha256 {got}"));
        }
    }
    if let Some(bytes) = expected["body_bytes"].as_u64()
        && answer.bytes.len() as u64 != bytes
    {
        problems.push(format!("body bytes {}", answer.bytes.len()));
    }
    for needle in expected["body_contains"].as_array().into_iter().flatten() {
        if !String::from_utf8_lossy(&answer.bytes).contains(needle.as_str().unwrap()) {
            problems.push(format!("body lacks {needle}"));
        }
    }
    if !expected["body"].is_null() {
        let actual: Value = serde_json::from_slice(&answer.bytes)
            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&answer.bytes).into()));
        let exact = expected["body_match"] == "exact";
        matches(
            &without_zod_detail(expected["body"].clone()),
            &actual,
            exact,
            "body",
            problems,
        );
    }
}

async fn run(runner: &mut Runner, fixture: &Value) -> Vec<String> {
    let id = fixture["id"].as_str().unwrap();
    let peer = if UNTRUSTED_CASES.contains(&id) {
        UNTRUSTED_PEER
    } else {
        PEER
    };
    for step in fixture["setup"].as_array().into_iter().flatten() {
        let answer = runner.send(step, peer).await;
        assert!(
            answer.status.is_success(),
            "{id}: setup answered {}",
            answer.status
        );
    }
    let before = tally(&runner.status().await);
    let answer = runner.send(&fixture["request"], peer).await;
    let after = tally(&runner.status().await);

    let holocron = &fixture["expected_holocron"];
    let upstream = &fixture["expected_upstream"];
    let mut problems = Vec::new();
    let status = holocron["status"].as_u64().unwrap() as u16;
    if answer.status != status {
        problems.push(format!(
            "status: expected {status}, got {} {}",
            answer.status,
            String::from_utf8_lossy(&answer.bytes)
                .chars()
                .take(200)
                .collect::<String>()
        ));
    }
    if fixture["validated_against_upstream"] == true
        && upstream["status"].as_u64() == Some(status as u64)
        && !BODY_DIFFERS.contains(&id)
    {
        compare_upstream(upstream, &answer, &mut problems);
    }
    if matches!(status, 429 | 503) && answer.header("retry-after").is_empty() {
        problems.push("no Retry-After".into());
    }

    let outcome = holocron["outcome"].as_str().unwrap();
    let reason = holocron["reason"].as_str().map(|r| match r {
        "storage_unavailable" => "unavailable",
        other => other,
    });
    match outcome {
        _ if NOTHING_TO_COUNT.contains(&id) => {}
        "served" | "not_found" => {
            if before != after {
                problems.push(format!("counted an outcome for a {outcome} answer"));
            }
        }
        _ => {
            let key = match reason {
                Some(reason) => format!("{outcome}:{reason}"),
                None => outcome.to_owned(),
            };
            if !grew(&before, &after, &key) {
                problems.push(format!("outcome {key} not counted: {after:?}"));
            }
        }
    }
    for counter in expected_counters(id) {
        if !grew(&before, &after, counter) {
            problems.push(format!("counter {counter} not raised"));
        }
    }
    problems
}

#[tokio::test(flavor = "multi_thread")]
async fn every_collection_fixture_gets_its_documented_outcome() {
    let mut default = Runner::new(harness(None, test_budgets()).await).await;
    let mut alias = Runner::new(harness(Some("/api/collect"), test_budgets()).await).await;
    let mut unavailable = Runner::new(
        harness(
            None,
            Budgets {
                disk_reserve_bytes: u64::MAX,
                ..test_budgets()
            },
        )
        .await,
    )
    .await;
    let mut busy = Runner::new(
        harness(
            None,
            Budgets {
                inbox_bytes: 1,
                ..test_budgets()
            },
        )
        .await,
    )
    .await;
    let mut full_recording = Runner::new(harness_with_cap(None, test_budgets(), 1).await).await;

    let mut failures = Vec::new();
    let mut ran = 0;
    for fixture in fixtures() {
        let id = fixture["id"].as_str().unwrap();
        let runner = if id.contains("storage-unavailable") || id.contains("commit-failure") {
            &mut unavailable
        } else if id.contains("admission-budget-full") {
            &mut busy
        } else if id.contains("record-budget-exceeded") {
            &mut full_recording
        } else if fixture["requires_pass"] == "collect-alias" {
            &mut alias
        } else {
            &mut default
        };
        let problems = run(runner, &fixture).await;
        ran += 1;
        if !problems.is_empty() {
            failures.push(format!("{id}:\n    {}", problems.join("\n    ")));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {ran} failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
    println!("{ran} collection fixtures passed");
}
