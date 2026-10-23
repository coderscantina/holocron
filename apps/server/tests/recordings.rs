//! Milestone 09 over HTTP: recording policy and the recorder handshake, replay parts through
//! the replay store to playback, the recording cap, retention, saved replays, page exclusion,
//! session corrections, heatmaps, and SiteScope on every read.

mod common;

use std::path::Path;
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, header};
use holocron_server::background::Background;
use holocron_server::clock::Clock;
use holocron_server::config::CollectConfig;
use holocron_server::recordings::maintenance;
use holocron_storage::Budgets;
use serde_json::{Value, json};

use common::{Answer, HOST, Harness, test_budgets};

/// 2026-10-01T10:00:00Z.
const NOW_MS: i64 = 1_790_848_800_000;
const MINUTE_MS: i64 = 60_000;
const DAY_MS: i64 = 86_400_000;
const PEER: &str = "198.51.100.9:40000";
const UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36";

async fn harness_with(cap_bytes: u64) -> (Harness, Clock) {
    let clock = Clock::frozen(NOW_MS);
    let collect = CollectConfig {
        recording_cap_bytes: cap_bytes,
        ..CollectConfig::default()
    };
    (
        Harness::with(collect, test_budgets(), clock.clone()).await,
        clock,
    )
}

async fn harness() -> (Harness, Clock) {
    harness_with(CollectConfig::default().recording_cap_bytes).await
}

async fn ok(answer: Answer) -> Value {
    assert_eq!(
        answer.status,
        200,
        "{}",
        String::from_utf8_lossy(&answer.bytes)
    );
    answer.json()
}

async fn command(h: &Harness, cookie: &str, name: &str, mut body: Value) -> Answer {
    body["idempotency_key"] = json!(uuid::Uuid::new_v4().to_string());
    h.command(Some(cookie), name, body).await
}

/// Enables replay and heatmaps at 100 % with the given path exclusions.
async fn enable(h: &Harness, site: &str, excluded_paths: Value) {
    let owner = h.session("p_owner").await;
    ok(command(
        h,
        &owner,
        "recordings.updatePolicy",
        json!({ "site": site, "replay_enabled": true, "heatmap_enabled": true, "replay_sample_percent": 100,
                "heatmap_sample_percent": 100, "max_duration_minutes": 5, "excluded_elements": [".private"],
                "excluded_paths": excluded_paths }),
    )
    .await)
    .await;
}

/// The tracker's cache token after one pageview from `peer`.
async fn cache_token(h: &Harness, site: &str, peer: &str) -> String {
    let body = json!({ "type": "event", "payload": { "website": site, "url": "/", "hostname": "shop.test" } });
    ok(h.collect("POST", "/api/send", peer, Some(body)).await).await["cache"]
        .as_str()
        .unwrap()
        .to_owned()
}

async fn post_record(h: &Harness, token: &str, body: Value) -> Answer {
    let request = Request::post("/api/record")
        .header(header::HOST, HOST)
        .header(header::USER_AGENT, UA)
        .header(header::CONTENT_TYPE, "application/json")
        .header("x-umami-cache", token)
        .extension(ConnectInfo(PEER.parse::<std::net::SocketAddr>().unwrap()))
        .body(Body::from(body.to_string()))
        .unwrap();
    h.send(request).await
}

async fn record(h: &Harness, token: &str, site: &str, chunk: i64, events: Value) -> Value {
    let body = json!({ "type": "record", "payload": { "website": site, "events": events, "timestamp": chunk } });
    ok(post_record(h, token, body).await).await
}

fn meta(at: i64, path: &str) -> Value {
    json!({ "type": 4, "timestamp": at, "data": { "href": format!("https://shop.test{path}"), "width": 1280, "height": 720 } })
}

fn snapshot(at: i64, text: &str) -> Value {
    json!({ "type": 2, "timestamp": at, "data": { "node": { "type": 0, "id": 1, "childNodes": [
        { "type": 2, "tagName": "html", "id": 2, "attributes": {}, "childNodes": [
            { "type": 2, "tagName": "body", "id": 3, "attributes": {}, "childNodes": [{ "type": 3, "id": 4, "textContent": text }] }] }] },
        "initialOffset": { "left": 0, "top": 0 } } })
}

/// Letters that do not compress, so byte budgets in tests are predictable.
fn noise(seed: u64, len: usize) -> String {
    let mut state = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
    (0..len)
        .map(|_| {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            char::from(b'a' + ((state >> 33) % 26) as u8)
        })
        .collect()
}

fn click(at: i64) -> Value {
    json!({ "type": 3, "timestamp": at, "data": { "source": 2, "type": 2, "id": 4, "x": 5, "y": 5 } })
}

fn fragments(event: &Value, parts: usize) -> Vec<Value> {
    let text = event.to_string();
    let size = text.len().div_ceil(parts);
    text.as_bytes()
        .chunks(size)
        .enumerate()
        .map(|(index, piece)| {
            json!({ "type": "umami:rrweb-event-fragment", "timestamp": event["timestamp"], "data": {
                "id": "big", "index": index, "total": parts, "value": std::str::from_utf8(piece).unwrap() } })
        })
        .collect()
}

async fn list(h: &Harness, cookie: &str, site: &str, extra: &str) -> Answer {
    let query = format!(
        "site={site}&start={}&end={}{extra}",
        NOW_MS - 30 * DAY_MS,
        NOW_MS + 30 * DAY_MS
    );
    h.query(Some(cookie), "recordings.list", &query).await
}

async fn captures(h: &Harness, cookie: &str, site: &str) -> Vec<Value> {
    h.state.collector.storage().drain().await;
    ok(list(h, cookie, site, "").await).await["captures"]
        .as_array()
        .unwrap()
        .clone()
}

async fn playback(
    h: &Harness,
    cookie: &str,
    site: &str,
    capture: &str,
    after: Option<&str>,
) -> Value {
    let mut query = format!("site={site}&capture={capture}");
    if let Some(after) = after {
        query.push_str(&format!("&after={after}"));
    }
    ok(h.query(Some(cookie), "recordings.get", &query).await).await
}

fn events(page: &Value) -> Vec<Value> {
    serde_json::from_str(page["events"].as_str().unwrap()).unwrap()
}

async fn chunk_rows(h: &Harness) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM chunks")
        .fetch_one(&recordings_db(h).await)
        .await
        .unwrap()
}

async fn recordings_db(h: &Harness) -> sqlx::SqlitePool {
    sqlx::SqlitePool::connect(&format!(
        "sqlite://{}?mode=ro",
        h.data_dir().join("recordings.sqlite").display()
    ))
    .await
    .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn recording_is_off_until_enabled_and_then_strict_at_ten_percent() {
    let (h, _) = harness().await;
    let site = h.site("UTC").await;
    let config = ok(h.get(&format!("/api/websites/{site}/recorder")).await).await;
    assert_eq!(config, json!({ "enabled": false }));
    let token = cache_token(&h, &site, PEER).await;
    let answer = record(&h, &token, &site, 1, json!([meta(NOW_MS, "/")])).await;
    assert_eq!(
        answer,
        json!({ "ok": false, "reason": "recorder_disabled" })
    );
    assert_eq!(chunk_rows(&h).await, 0);

    let owner = h.session("p_owner").await;
    let policy = ok(h
        .query(Some(&owner), "recordings.policy", &format!("site={site}"))
        .await)
    .await;
    assert_eq!(policy["policy"]["replay_enabled"], false);
    assert_eq!(policy["policy"]["replay_sample_percent"], 10);
    let mut args = policy["policy"].clone();
    args["site"] = json!(site);
    args["replay_enabled"] = json!(true);
    args.as_object_mut().unwrap().remove("version");
    ok(command(&h, &owner, "recordings.updatePolicy", args).await).await;
    let config = ok(h.get(&format!("/api/websites/{site}/recorder")).await).await;
    assert_eq!(config["maskLevel"], "strict");
    assert_eq!(config["sampleRate"], 0.1);
    assert_eq!(config["heatmapEnabled"], false);

    let member = h.session("p_viewer").await;
    let denied = command(
        &h,
        &member,
        "recordings.updatePolicy",
        json!({ "site": site, "replay_enabled": false,
        "heatmap_enabled": false, "replay_sample_percent": 10, "heatmap_sample_percent": 10,
        "max_duration_minutes": 5, "excluded_elements": [], "excluded_paths": [] }),
    )
    .await;
    assert_eq!(denied.code(), "forbidden");
}

#[tokio::test(flavor = "multi_thread")]
async fn reordered_duplicated_and_split_parts_play_back_once_in_recording_order() {
    let (h, _) = harness().await;
    let site = h.site("UTC").await;
    enable(&h, &site, json!([])).await;
    let token = cache_token(&h, &site, PEER).await;
    let big = snapshot(NOW_MS + 10, &"*".repeat(3000));
    let pieces = fragments(&big, 3);

    // Recording order: meta (100), the snapshot in three parts (101-103), a click (104).
    // Arrival order is shuffled and one part comes twice.
    record(&h, &token, &site, 104, json!([click(NOW_MS + 500)])).await;
    record(&h, &token, &site, 102, json!([pieces[1]])).await;
    record(
        &h,
        &token,
        &site,
        100,
        json!([meta(NOW_MS, "/cart?email=a@b.test")]),
    )
    .await;
    record(&h, &token, &site, 103, json!([pieces[2]])).await;
    record(&h, &token, &site, 101, json!([pieces[0]])).await;
    record(&h, &token, &site, 102, json!([pieces[1]])).await;
    assert_eq!(chunk_rows(&h).await, 5);

    let viewer = h.session("p_viewer").await;
    let listed = captures(&h, &viewer, &site).await;
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0]["entry_path"], "/cart");
    assert_eq!(listed[0]["events"], 3);
    let capture = listed[0]["id"].as_str().unwrap();
    let page = playback(&h, &viewer, &site, capture, None).await;
    assert_eq!(
        events(&page),
        [
            meta(NOW_MS, "/cart?email=a@b.test"),
            big,
            click(NOW_MS + 500)
        ]
    );
    assert_eq!(page["gaps"], 0);
    assert_eq!(
        page["timeline"][0],
        json!({ "at": NOW_MS, "kind": "page", "path": "/cart" })
    );

    // A split event whose middle part never arrives is reported, not played half.
    let other = cache_token(&h, &site, "198.51.100.10:40000").await;
    let lost = fragments(&snapshot(NOW_MS, "x"), 3);
    record(&h, &other, &site, 200, json!([meta(NOW_MS, "/"), lost[0]])).await;
    record(&h, &other, &site, 202, json!([lost[2]])).await;
    let second = captures(&h, &viewer, &site)
        .await
        .into_iter()
        .find(|c| c["id"] != capture)
        .unwrap();
    let page = playback(&h, &viewer, &site, second["id"].as_str().unwrap(), None).await;
    assert_eq!((events(&page).len(), page["gaps"].as_i64()), (1, Some(1)));
}

#[tokio::test(flavor = "multi_thread")]
async fn playback_pages_are_bounded_and_continue_from_their_cursor() {
    let (h, _) = harness().await;
    let site = h.site("UTC").await;
    enable(&h, &site, json!([])).await;
    let token = cache_token(&h, &site, PEER).await;
    // Six parts of about 900 KB: a 4 MiB page holds five.
    for chunk in 0..6 {
        let text: String = (0..900_000)
            .map(|i| char::from(b'a' + ((i * 7 + chunk) % 26) as u8))
            .collect();
        record(&h, &token, &site, chunk as i64, json!([{ "type": 3, "timestamp": NOW_MS, "data": { "source": 0, "texts": [{ "id": 1, "value": text }] } }])).await;
    }
    let viewer = h.session("p_viewer").await;
    let capture = captures(&h, &viewer, &site).await[0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let first = playback(&h, &viewer, &site, &capture, None).await;
    let cursor = first["next"].as_str().unwrap().to_owned();
    assert_eq!(events(&first).len(), 5);
    let second = playback(&h, &viewer, &site, &capture, Some(&cursor)).await;
    assert_eq!(events(&second).len(), 1);
    assert!(second.get("next").is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_full_cap_refuses_replay_while_events_and_heatmaps_continue() {
    let (h, clock) = harness_with(2_000).await;
    let site = h.site("UTC").await;
    enable(&h, &site, json!([])).await;
    let token = cache_token(&h, &site, PEER).await;
    let filler = [snapshot(NOW_MS, &noise(1, 4000))];
    assert_eq!(
        record(&h, &token, &site, 1, json!([meta(NOW_MS, "/")])).await,
        json!({ "ok": true })
    );
    let refused = record(&h, &token, &site, 2, json!(filler)).await;
    assert_eq!(
        refused,
        json!({ "ok": false, "reason": "replay_budget_exceeded" })
    );

    let event = json!({ "type": "event", "payload": { "website": site, "url": "/after-cap", "hostname": "shop.test" } });
    ok(h.collect("POST", "/api/send", PEER, Some(event)).await).await;
    let heat = json!({ "type": "heatmap", "payload": { "website": site, "events": [
        { "type": "click", "url": "https://shop.test/", "pageX": 10, "pageY": 10, "viewportW": 1280 }] } });
    assert_eq!(
        ok(post_record(&h, &token, heat).await).await,
        json!({ "ok": true })
    );

    let owner = h.session("p_owner").await;
    let budget = ok(h
        .query(Some(&owner), "recordings.budget", &format!("site={site}"))
        .await)
    .await;
    assert_eq!(budget["refusing"], true);
    assert_eq!(budget["cap_bytes"], 2_000);
    let listed = captures(&h, &owner, &site).await;
    assert_eq!(listed[0]["truncated"], true);

    // Retention frees the cap and recording resumes.
    clock.set(NOW_MS + 15 * DAY_MS);
    let report = maintenance::run(&h.state).await.unwrap();
    assert_eq!(report.captures_removed, 1);
    let budget = ok(h
        .query(Some(&owner), "recordings.budget", &format!("site={site}"))
        .await)
    .await;
    assert_eq!(budget["used_bytes"], 0);
    let fresh = token_at(&h, &site).await;
    assert_eq!(
        record(
            &h,
            &fresh,
            &site,
            3,
            json!([meta(NOW_MS + 15 * DAY_MS, "/")])
        )
        .await,
        json!({ "ok": true })
    );
}

async fn token_at(h: &Harness, site: &str) -> String {
    cache_token(h, site, "198.51.100.11:40000").await
}

#[tokio::test(flavor = "multi_thread")]
async fn retention_removes_whole_unsaved_captures_and_saved_ones_share_a_fifth_of_the_cap() {
    let (h, clock) = harness_with(50_000).await;
    let site = h.site("Europe/Vienna").await;
    enable(&h, &site, json!([])).await;
    let viewer = h.session("p_viewer").await;
    for (index, peer) in ["198.51.100.20:1", "198.51.100.21:1", "198.51.100.22:1"]
        .into_iter()
        .enumerate()
    {
        let token = cache_token(&h, &site, peer).await;
        let text = noise(index as u64, 10_000);
        record(
            &h,
            &token,
            &site,
            1,
            json!([meta(NOW_MS, "/"), snapshot(NOW_MS, &text)]),
        )
        .await;
        record(&h, &token, &site, 2, json!([click(NOW_MS + 1)])).await;
    }
    let listed = captures(&h, &viewer, &site).await;
    let ids: Vec<&str> = listed.iter().map(|c| c["id"].as_str().unwrap()).collect();
    let bytes: i64 = listed[0]["bytes"].as_i64().unwrap();
    assert!(
        bytes * 2 > 10_000 && bytes < 10_000,
        "a capture of {bytes} bytes fits a 10,000-byte saved part once"
    );

    let save = |capture: &str| {
        let body = json!({ "site": site, "capture": capture });
        let viewer = viewer.clone();
        let h = &h;
        async move { command(h, &viewer, "recordings.save", body).await }
    };
    assert_eq!(ok(save(ids[0]).await).await["capture"]["saved"], true);
    assert_eq!(ok(save(ids[0]).await).await["capture"]["saved"], true);
    let over = save(ids[1]).await;
    assert_eq!(
        (over.status.as_u16(), over.code().as_str()),
        (409, "saved_replay_budget_exceeded")
    );

    // Day 15 in Vienna: unsaved captures are gone from reads before removal runs, then removed.
    clock.set(NOW_MS + 15 * DAY_MS);
    assert_eq!(captures(&h, &viewer, &site).await.len(), 1);
    let gone = h
        .query(
            Some(&viewer),
            "recordings.get",
            &format!("site={site}&capture={}", ids[1]),
        )
        .await;
    assert_eq!(gone.code(), "not_found");
    maintenance::run(&h.state).await.unwrap();
    assert_eq!(chunk_rows(&h).await, 2);
    let kept = playback(&h, &viewer, &site, ids[0], None).await;
    assert_eq!(events(&kept).len(), 3);
    ok(command(
        &h,
        &viewer,
        "recordings.unsave",
        json!({ "site": site, "capture": ids[0] }),
    )
    .await)
    .await;
    maintenance::run(&h.state).await.unwrap();
    assert_eq!(chunk_rows(&h).await, 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn reaching_an_excluded_page_drops_the_capture_and_its_heatmap_clicks() {
    let (h, _) = harness().await;
    let site = h.site("UTC").await;
    enable(&h, &site, json!(["/account*"])).await;
    let token = cache_token(&h, &site, PEER).await;
    assert_eq!(
        record(
            &h,
            &token,
            &site,
            1,
            json!([meta(NOW_MS, "/shop"), snapshot(NOW_MS, "***")])
        )
        .await,
        json!({ "ok": true })
    );
    let navigation = json!({ "type": 5, "timestamp": NOW_MS + 5, "data": { "tag": "url-change", "payload": { "url": "https://shop.test/account/orders" } } });
    let answer = record(&h, &token, &site, 2, json!([navigation])).await;
    assert_eq!(answer, json!({ "ok": false, "reason": "page_excluded" }));
    let later = record(&h, &token, &site, 3, json!([click(NOW_MS + 9)])).await;
    assert_eq!(later["reason"], "page_excluded");
    assert_eq!(chunk_rows(&h).await, 0);
    let viewer = h.session("p_viewer").await;
    assert!(captures(&h, &viewer, &site).await.is_empty());

    let heat = json!({ "type": "heatmap", "payload": { "website": site, "events": [
        { "type": "click", "url": "https://shop.test/account", "pageX": 1, "pageY": 1, "viewportW": 1280 },
        { "type": "click", "url": "https://shop.test/shop", "pageX": 1, "pageY": 1, "viewportW": 1280 }] } });
    ok(post_record(&h, &token, heat).await).await;
    h.state.collector.storage().drain().await;
    let pages = ok(h
        .query(
            Some(&viewer),
            "heatmaps.pages",
            &format!(
                "site={site}&start={}&end={}",
                NOW_MS - DAY_MS,
                NOW_MS + DAY_MS
            ),
        )
        .await)
    .await;
    assert_eq!(
        pages["pages"],
        json!([{ "path": "/shop", "clicks": 1, "scroll_visits": 0 }])
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn session_corrections_leave_the_replay_intact_and_move_its_link() {
    let (h, clock) = harness().await;
    let site = h.site("UTC").await;
    enable(&h, &site, json!([])).await;
    let background = Background::start(&h.state);
    let viewer = h.session("p_viewer").await;

    let token = cache_token(&h, &site, PEER).await;
    for chunk in 1..=3 {
        record(
            &h,
            &token,
            &site,
            chunk,
            json!([
                meta(NOW_MS + chunk, &format!("/p{chunk}")),
                click(NOW_MS + chunk * 10)
            ]),
        )
        .await;
    }
    clock.set(NOW_MS + MINUTE_MS);
    let before = linked(&h, &viewer, &site).await;
    let page_before = playback(&h, &viewer, &site, before.0.as_str(), None).await;

    // A late pageview ten minutes earlier becomes the session's first event: the session is
    // derived again and gets a new ID.
    let late = json!({ "type": "event", "payload": {
        "website": site, "url": "/late", "hostname": "shop.test", "timestamp": (NOW_MS - 10 * MINUTE_MS) / 1000 } });
    ok(h.collect("POST", "/api/send", PEER, Some(late)).await).await;
    let after = linked(&h, &viewer, &site).await;
    assert_eq!(after.0, before.0);
    assert_ne!(after.1, before.1, "the link follows the corrected session");
    let page_after = playback(&h, &viewer, &site, after.0.as_str(), None).await;
    assert_eq!(page_after["events"], page_before["events"]);
    let filtered = ok(list(&h, &viewer, &site, &format!("&session={}", after.1)).await).await;
    assert_eq!(filtered["captures"].as_array().unwrap().len(), 1);
    background.stop().await;
}

/// The only capture and its session link, once correction has caught up.
async fn linked(h: &Harness, cookie: &str, site: &str) -> (String, i64) {
    h.state.collector.storage().drain().await;
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let fresh = ok(h
            .query(
                Some(cookie),
                "analytics.freshness",
                &format!(
                    "site={site}&start={}&end={}",
                    NOW_MS - DAY_MS,
                    NOW_MS + DAY_MS
                ),
            )
            .await)
        .await;
        let listed = captures(h, cookie, site).await;
        if fresh["correction"] == "corrected" && listed[0].get("session").is_some() {
            return (
                listed[0]["id"].as_str().unwrap().to_owned(),
                listed[0]["session"].as_i64().unwrap(),
            );
        }
        assert!(Instant::now() < deadline, "not corrected: {fresh}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn path_is(id: &str, path: &str) -> Value {
    json!({ "kind": "cond", "id": id, "field": "path", "cmp": "eq", "value": { "t": "string", "v": path } })
}

fn any_of(items: Vec<Value>) -> Value {
    json!({ "kind": "group", "op": "or", "items": items })
}

async fn filtered(h: &Harness, cookie: &str, site: &str, filter: &Value, extra: &str) -> Answer {
    let filter: String =
        url::form_urlencoded::byte_serialize(filter.to_string().as_bytes()).collect();
    list(h, cookie, site, &format!("&filter={filter}{extra}")).await
}

/// IDs of a list page, and its total.
async fn listed(answer: Answer) -> (Vec<String>, Option<i64>, Option<String>) {
    let page = ok(answer).await;
    let ids = page["captures"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_str().unwrap().to_owned())
        .collect();
    (
        ids,
        page["total"].as_i64(),
        page["next"].as_str().map(str::to_owned),
    )
}

/// A pageview of `path` from `peer`, then a replay of that visit. Returns the replay's ID.
async fn replay_of(h: &Harness, cookie: &str, site: &str, peer: &str, path: &str) -> String {
    let body = json!({ "type": "event", "payload": { "website": site, "url": path, "hostname": "shop.test" } });
    let token = ok(h.collect("POST", "/api/send", peer, Some(body)).await).await["cache"]
        .as_str()
        .unwrap()
        .to_owned();
    let before = captures(h, cookie, site).await;
    record(
        h,
        &token,
        site,
        1,
        json!([meta(NOW_MS + 1, path), click(NOW_MS + 10)]),
    )
    .await;
    let after = captures(h, cookie, site).await;
    after
        .iter()
        .map(|c| c["id"].as_str().unwrap().to_owned())
        .find(|id| !before.iter().any(|b| b["id"] == id.as_str()))
        .unwrap()
}

/// Waits until correction caught up and every replay links to a session.
async fn all_linked(h: &Harness, cookie: &str, site: &str) -> Vec<Value> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        h.state.collector.storage().drain().await;
        let fresh = ok(h
            .query(
                Some(cookie),
                "analytics.freshness",
                &format!(
                    "site={site}&start={}&end={}",
                    NOW_MS - DAY_MS,
                    NOW_MS + DAY_MS
                ),
            )
            .await)
        .await;
        let listed = captures(h, cookie, site).await;
        if fresh["correction"] == "corrected" && listed.iter().all(|c| c.get("session").is_some()) {
            return listed;
        }
        assert!(Instant::now() < deadline, "not corrected: {fresh}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_filter_lists_the_replays_whose_session_has_a_matching_event() {
    let (h, _) = harness().await;
    let site = h.site("UTC").await;
    enable(&h, &site, json!([])).await;
    let background = Background::start(&h.state);
    let viewer = h.session("p_viewer").await;
    let pricing = replay_of(&h, &viewer, &site, PEER, "/pricing").await;
    let blog = replay_of(&h, &viewer, &site, "198.51.100.10:40000", "/blog").await;
    all_linked(&h, &viewer, &site).await;

    let (ids, total, _) = listed(
        filtered(
            &h,
            &viewer,
            &site,
            &any_of(vec![path_is("f1", "/pricing")]),
            "",
        )
        .await,
    )
    .await;
    assert_eq!((ids, total), (vec![pricing.clone()], Some(1)));
    let (ids, total, _) = listed(
        filtered(
            &h,
            &viewer,
            &site,
            &any_of(vec![path_is("f1", "/nowhere")]),
            "",
        )
        .await,
    )
    .await;
    assert_eq!((ids, total), (vec![], Some(0)));

    // Both match: the total counts every page, later pages carry no total.
    let both = any_of(vec![path_is("f1", "/pricing"), path_is("f2", "/blog")]);
    let (first, total, next) = listed(filtered(&h, &viewer, &site, &both, "&limit=1").await).await;
    assert_eq!((first.len(), total), (1, Some(2)));
    let after = format!("&limit=1&after={}", next.unwrap());
    let (second, total, next) = listed(filtered(&h, &viewer, &site, &both, &after).await).await;
    assert_eq!((second.len(), total, next), (1, None, None));
    let mut all = [first, second].concat();
    all.sort();
    let mut expected = vec![pricing.clone(), blog];
    expected.sort();
    assert_eq!(all, expected);

    // A late pageview re-derives the pricing session: its replay now matches `/late`.
    let late = json!({ "type": "event", "payload": {
        "website": site, "url": "/late", "hostname": "shop.test", "timestamp": (NOW_MS - 10 * MINUTE_MS) / 1000 } });
    ok(h.collect("POST", "/api/send", PEER, Some(late)).await).await;
    let late_only = any_of(vec![path_is("f1", "/late")]);
    let deadline = Instant::now() + Duration::from_secs(10);
    let page = loop {
        all_linked(&h, &viewer, &site).await;
        let page = ok(filtered(&h, &viewer, &site, &late_only, "").await).await;
        if page["total"] == 1 {
            break page;
        }
        assert!(
            Instant::now() < deadline,
            "the late pageview never matched: {page}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    assert_eq!(page["captures"][0]["id"], pricing.as_str());
    let link = all_linked(&h, &viewer, &site)
        .await
        .into_iter()
        .find(|c| c["id"] == pricing.as_str())
        .unwrap()["session"]
        .clone();
    assert_eq!(page["captures"][0]["session"], link);
    background.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_refused_replay_filter_lists_nothing() {
    let (h, _) = harness().await;
    let site = h.site("UTC").await;
    enable(&h, &site, json!([])).await;
    let viewer = h.session("p_viewer").await;
    replay_of(&h, &viewer, &site, PEER, "/pricing").await;

    let refused = |answer: Answer| {
        assert_ne!(answer.status, 200);
        let body = answer.json();
        assert!(body.get("captures").is_none(), "{body}");
        body
    };
    let many = any_of(
        [0, 17]
            .map(|from| {
                any_of(
                    (from..from + 17)
                        .map(|i| path_is(&format!("f{i}"), "/pricing"))
                        .collect(),
                )
            })
            .to_vec(),
    );
    let body = refused(filtered(&h, &viewer, &site, &many, "").await);
    assert_eq!(body["error_code"], "too_many_conditions");
    assert_eq!(body["errors"]["filter"][0]["limit"], 32);

    let mut deep = any_of(vec![path_is("f1", "/pricing")]);
    for _ in 0..3 {
        deep = any_of(vec![deep]);
    }
    assert_eq!(
        refused(filtered(&h, &viewer, &site, &deep, "").await)["error_code"],
        "filter_too_deep"
    );

    let unknown = any_of(vec![
        json!({ "kind": "cond", "id": "f1", "field": { "scope": "event", "key": "plan" },
        "cmp": "eq", "value": { "t": "string", "v": "pro" } }),
    ]);
    let body = refused(filtered(&h, &viewer, &site, &unknown, "").await);
    assert_eq!(body["error_code"], "invalid_arguments");
    assert!(body["errors"].get("filter.f1").is_some(), "{body}");

    let perf = any_of(vec![
        path_is("f1", "/pricing"),
        json!({ "kind": "cond", "id": "f2", "field": "perf_metric",
        "cmp": "eq", "value": { "t": "string", "v": "lcp" } }),
    ]);
    assert!(
        refused(filtered(&h, &viewer, &site, &perf, "").await)["errors"]
            .get("filter.f2")
            .is_some()
    );

    let answer = list(&h, &viewer, &site, "&filter=%7Bnot%20json").await;
    assert_eq!(refused(answer)["error_code"], "malformed_request");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_replay_filter_past_the_query_timeout_is_refused() {
    let clock = Clock::frozen(NOW_MS);
    let budgets = Budgets {
        query_timeout: Duration::ZERO,
        ..test_budgets()
    };
    let h = Harness::with(CollectConfig::default(), budgets, clock).await;
    let site = h.site("UTC").await;
    enable(&h, &site, json!([])).await;
    let viewer = h.session("p_viewer").await;

    let answer = filtered(
        &h,
        &viewer,
        &site,
        &any_of(vec![path_is("f1", "/pricing")]),
        "",
    )
    .await;
    assert_eq!(answer.status, 503);
    assert_eq!(answer.json()["error_code"], "query_timeout");
}

#[tokio::test(flavor = "multi_thread")]
async fn heatmaps_count_clicks_by_cell_and_scroll_depth_once_per_visit_before_and_after_folding() {
    let (h, clock) = harness().await;
    let site = h.site("UTC").await;
    enable(&h, &site, json!([])).await;
    let viewer = h.session("p_viewer").await;
    for peer in ["198.51.100.30:1", "198.51.100.31:1"] {
        let token = cache_token(&h, &site, peer).await;
        // Scroll depth arrives as an increasing series per visit, as the recorder sends it.
        let heat = json!({ "type": "heatmap", "payload": { "website": site, "events": [
            { "type": "click", "url": "https://shop.test/pricing?a=1", "pageX": 640, "pageY": 900, "viewportW": 1280, "viewportH": 720 },
            { "type": "scroll", "url": "https://shop.test/pricing", "scrollPct": 30, "viewportW": 1280 },
            { "type": "scroll", "url": "https://shop.test/pricing", "scrollPct": 75, "viewportW": 1280 },
            { "type": "click", "url": "https://shop.test/pricing", "pageX": 100, "pageY": 100, "viewportW": 390 }] } });
        assert_eq!(
            ok(post_record(&h, &token, heat).await).await,
            json!({ "ok": true })
        );
    }
    h.state.collector.storage().drain().await;
    let query = format!(
        "site={site}&path=/pricing&viewport=desktop&start={}&end={}",
        NOW_MS - DAY_MS,
        NOW_MS + DAY_MS
    );
    let live = ok(h.query(Some(&viewer), "heatmaps.cells", &query).await).await;
    assert_eq!(
        live["clicks"],
        json!([{ "column": 32, "row": 28, "count": 2 }])
    );
    assert_eq!(live["total_clicks"], 2);
    assert_eq!(live["scroll_visits"], 2);
    assert_eq!(
        live["scroll"][7],
        json!({ "depth_percent": 70, "visits": 2 })
    );
    assert_eq!(
        live["scroll"][8],
        json!({ "depth_percent": 80, "visits": 0 })
    );

    clock.set(NOW_MS + 2 * 60 * MINUTE_MS);
    let report = maintenance::run(&h.state).await.unwrap();
    assert_eq!(report.heatmap_captures_folded, 2);
    let raw: i64 = h
        .analytics(|c| {
            c.query_row("SELECT count(*) FROM heatmap_interactions", [], |r| {
                r.get(0)
            })
        })
        .await;
    assert_eq!(raw, 0);
    let folded = ok(h.query(Some(&viewer), "heatmaps.cells", &query).await).await;
    assert_eq!(
        (folded["clicks"].clone(), folded["scroll"].clone()),
        (live["clicks"].clone(), live["scroll"].clone())
    );

    clock.set(NOW_MS + 92 * DAY_MS);
    maintenance::run(&h.state).await.unwrap();
    let cells: i64 = h
        .analytics(|c| c.query_row("SELECT count(*) FROM heatmap_cells", [], |r| r.get(0)))
        .await;
    assert_eq!(cells, 0);
}

/// `fixtures/access` rows for `recordings.list` and `recordings.get` with signed-in principals,
/// plus a capture ID of another site, which answers like a missing one.
#[tokio::test(flavor = "multi_thread")]
async fn replay_reads_follow_the_access_fixture() {
    let (h, _) = harness().await;
    let rows: Value = serde_json::from_str(
        &std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/access/cases.json"),
        )
        .unwrap(),
    )
    .unwrap();
    let site_a1 = h.sites["site_a1"].to_string();
    let site_b1 = h.sites["site_b1"].to_string();
    enable(&h, &site_a1, json!([])).await;
    let owner_b = h.session("p_other_owner").await;
    ok(command(
        &h,
        &owner_b,
        "recordings.updatePolicy",
        json!({ "site": site_b1, "replay_enabled": true,
        "heatmap_enabled": false, "replay_sample_percent": 100, "heatmap_sample_percent": 10,
        "max_duration_minutes": 5, "excluded_elements": [], "excluded_paths": [] }),
    )
    .await)
    .await;
    let token_a = cache_token(&h, &site_a1, PEER).await;
    record(&h, &token_a, &site_a1, 1, json!([meta(NOW_MS, "/")])).await;
    let token_b = cache_token(&h, &site_b1, PEER).await;
    record(&h, &token_b, &site_b1, 1, json!([meta(NOW_MS, "/")])).await;
    let owner = h.session("p_owner").await;
    let rec_a1 = captures(&h, &owner, &site_a1).await[0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let rec_b1: String = captures(&h, &owner_b, &site_b1).await[0]["id"]
        .as_str()
        .unwrap()
        .to_owned();

    let principals = [
        "p_owner",
        "p_admin",
        "p_member",
        "p_viewer",
        "p_other_owner",
        "p_install_admin",
        "p_removed",
        "p_signed_out",
    ];
    let mut ran = 0;
    for row in rows["cases"].as_array().unwrap() {
        let op = row["op"].as_str().unwrap();
        let principal = row["principal"].as_str().unwrap();
        if !matches!(op, "recordings.list" | "recordings.get") || !principals.contains(&principal) {
            continue;
        }
        if row["state"] == "after_removal" {
            h.remove_member(principal, "ws_a").await;
        }
        let cookie = match principal {
            "p_signed_out" => None,
            alias => Some(h.session(alias).await),
        };
        let query = match op {
            "recordings.list" => format!(
                "site={site_a1}&start={}&end={}",
                NOW_MS - DAY_MS,
                NOW_MS + DAY_MS
            ),
            _ if row["resource"] == "rec_a1" => format!("site={site_a1}&capture={rec_a1}"),
            _ => format!("site={site_a1}&capture={}", uuid::Uuid::new_v4()),
        };
        let answer = h.query(cookie.as_deref(), op, &query).await;
        let id = row["id"].as_str().unwrap();
        match row["expect"]["result"].as_str().unwrap() {
            "allow" => assert_eq!(answer.status, 200, "{id}: {}", answer.json()),
            _ => {
                assert_eq!(
                    answer.status.as_u16() as u64,
                    row["expect"]["status"].as_u64().unwrap(),
                    "{id}"
                );
                assert_eq!(
                    answer.code(),
                    row["expect"]["code"].as_str().unwrap(),
                    "{id}"
                );
            }
        }
        ran += 1;
    }
    assert_eq!(ran, 18);

    let viewer = h.session("p_viewer").await;
    let crossed = h
        .query(
            Some(&viewer),
            "recordings.get",
            &format!("site={site_a1}&capture={rec_b1}"),
        )
        .await;
    let missing = h
        .query(
            Some(&viewer),
            "recordings.get",
            &format!("site={site_a1}&capture={}", uuid::Uuid::new_v4()),
        )
        .await;
    assert_eq!(
        (crossed.code(), crossed.bytes.clone()),
        (missing.code(), missing.bytes.clone())
    );
    let heat = h
        .query(
            Some(&viewer),
            "heatmaps.pages",
            &format!("site={site_b1}&start=0&end=1"),
        )
        .await;
    assert_eq!(heat.code(), "not_found");
}

/// AX-T039, AX-T040 and AX-T080 to AX-T082: a share's recordings grant lists and plays only
/// replays inside its range limit and pinned filter; others answer like missing ones.
#[tokio::test(flavor = "multi_thread")]
async fn share_viewers_reach_only_replays_inside_the_share() {
    let (h, clock) = harness().await;
    let site = h.sites["site_a1"].to_string();
    enable(&h, &site, json!([])).await;
    let token = cache_token(&h, &site, "198.51.100.30:1").await;
    record(&h, &token, &site, 1, json!([meta(NOW_MS, "/")])).await;
    let later = NOW_MS + 10 * DAY_MS;
    clock.set(later);
    let token = cache_token(&h, &site, "198.51.100.31:1").await;
    record(&h, &token, &site, 1, json!([meta(later, "/")])).await;
    let all = captures(&h, &h.session("p_owner").await, &site).await;
    let (recent, old) = (
        all[0]["id"].as_str().unwrap(),
        all[1]["id"].as_str().unwrap(),
    );
    let device = all[0]["device"].as_str().unwrap();

    let share = |device: &str, recordings: bool| {
        let mut state = common::share_state(&site, 7);
        state["filter"]["items"][0]["value"]["v"] = json!(device);
        let target = common::exploration_share(&state, 7);
        let h = &h;
        async move {
            h.share(
                "ws_a",
                target,
                false,
                recordings,
                holocron_server::db::now_ms() + DAY_MS,
            )
            .await
        }
    };
    let inside = share(device, true).await;
    let elsewhere = share("unknown-device", true).await;
    let without = share(device, false).await;
    let range = format!(
        "site={site}&start={}&end={}",
        NOW_MS - DAY_MS,
        later + DAY_MS
    );
    let get = |share: &str, capture: &str| {
        let query = format!("site={site}&capture={capture}");
        let h = &h;
        let share = share.to_owned();
        async move { h.query_with(&share, "recordings.get", &query).await }
    };

    let listed = ok(h.query_with(&inside, "recordings.list", &range).await).await;
    let ids: Vec<&str> = listed["captures"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, [recent]);
    ok(get(&inside, recent).await).await;
    let outside = get(&inside, old).await;
    let missing = get(&inside, &uuid::Uuid::new_v4().to_string()).await;
    assert_eq!(
        (outside.code(), outside.bytes),
        (missing.code(), missing.bytes)
    );

    let filtered = ok(h.query_with(&elsewhere, "recordings.list", &range).await).await;
    assert_eq!(filtered["captures"], json!([]));
    assert_eq!(get(&elsewhere, recent).await.code(), "not_found");
    assert_eq!(
        h.query_with(&without, "recordings.list", &range)
            .await
            .code(),
        "not_found"
    );
}
