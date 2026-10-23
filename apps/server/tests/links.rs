//! Links and pixels managed from the dashboard (decision 0020): what `/q/<slug>` and
//! `/p/<slug>` answer after create, rename and delete, slug and URL validation, and reports.

mod common;

use axum::http::StatusCode;
use serde_json::{Value, json};

use common::{Answer, Harness};

const PEER: &str = "198.51.100.7:40000";

async fn command(harness: &Harness, cookie: &str, name: &str, mut body: Value) -> Answer {
    body["idempotency_key"] = json!(uuid::Uuid::new_v4().to_string());
    harness.command(Some(cookie), name, body).await
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

/// The first field error code of `field` in an `invalid_arguments` answer.
fn field_error(answer: &Answer, field: &str) -> String {
    assert_eq!(answer.code(), "invalid_arguments");
    answer.json()["errors"][field][0]["code"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

async fn create_link(harness: &Harness, cookie: &str, slug: &str, url: &str) -> Answer {
    let workspace = harness.workspaces["ws_a"].as_str();
    command(
        harness,
        cookie,
        "links.create",
        json!({ "workspace": workspace, "name": "Spring sale", "url": url, "slug": slug }),
    )
    .await
}

fn encode(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_created_link_redirects_and_its_hits_show_in_its_report() {
    let harness = Harness::new().await;
    let member = harness.session("p_member").await;
    let created = ok(&create_link(&harness, &member, "spring", "https://shop.example/sale").await);
    let link = created["link"]["id"].as_str().unwrap().to_owned();

    let hit = harness.collect("GET", "/q/spring", PEER, None).await;
    assert_eq!(hit.status, StatusCode::TEMPORARY_REDIRECT);
    assert_eq!(hit.header("location"), "https://shop.example/sale");

    harness.state.collector.storage().drain().await;
    let state = json!({
        "v": 1, "site": link, "measures": ["events", "visitors"], "dimensions": [],
        "range": { "kind": "relative", "unit": "day", "count": 1 },
        "timezone": "UTC", "granularity": "day", "compare": "none", "metric_version": 1,
        "sort": [], "page": { "size": 50, "index": 0 }, "visualization": "table"
    });
    let viewer = harness.session("p_viewer").await;
    let query = format!("state={}", encode(&state.to_string()));
    let report = ok(&harness.query(Some(&viewer), "links.explore", &query).await);
    assert_eq!(
        report["main"]["totals"],
        json!([{ "t": "count", "v": 1 }, { "t": "count", "v": 1 }])
    );

    // A pixel without a slug gets a generated one that answers with the image.
    let workspace = harness.workspaces["ws_a"].as_str();
    let pixel = ok(&command(
        &harness,
        &member,
        "pixels.create",
        json!({ "workspace": workspace, "name": "Newsletter" }),
    )
    .await);
    let slug = pixel["pixel"]["slug"].as_str().unwrap();
    assert_eq!(slug.len(), 8);
    let image = harness
        .collect("GET", &format!("/p/{slug}"), PEER, None)
        .await;
    assert_eq!(image.status, StatusCode::OK);
    assert_eq!(image.header("content-type"), "image/gif");
}

#[tokio::test(flavor = "multi_thread")]
async fn slugs_are_unique_per_kind_and_targets_must_be_http() {
    let harness = Harness::new().await;
    let member = harness.session("p_member").await;
    ok(&create_link(&harness, &member, "spring", "https://shop.example/").await);

    let taken = create_link(&harness, &member, "spring", "https://other.example/").await;
    assert_eq!(field_error(&taken, "slug"), "invalid_value");
    let workspace = harness.workspaces["ws_a"].as_str();
    ok(&command(
        &harness,
        &member,
        "pixels.create",
        json!({ "workspace": workspace, "name": "Spring", "slug": "spring" }),
    )
    .await);

    let bad_slug = create_link(&harness, &member, "spring/sale", "https://shop.example/").await;
    assert_eq!(field_error(&bad_slug, "slug"), "invalid_format");
    let script = create_link(&harness, &member, "script", "javascript:alert(1)").await;
    assert_eq!(field_error(&script, "url"), "invalid_format");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_changed_slug_answers_at_once_and_frees_the_old_one() {
    let harness = Harness::new().await;
    let member = harness.session("p_member").await;
    let created = ok(&create_link(&harness, &member, "spring", "https://shop.example/").await);
    let link = created["link"]["id"].as_str().unwrap();

    ok(&command(
        &harness,
        &member,
        "links.update",
        json!({ "link": link, "name": "Summer sale", "url": "https://shop.example/summer", "slug": "summer" }),
    )
    .await);
    let old = harness.collect("GET", "/q/spring", PEER, None).await;
    assert_eq!(old.status, StatusCode::NOT_FOUND);
    let new = harness.collect("GET", "/q/summer", PEER, None).await;
    assert_eq!(new.header("location"), "https://shop.example/summer");
    ok(&create_link(&harness, &member, "spring", "https://other.example/").await);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_deleted_link_stops_answering_and_frees_its_slug() {
    let harness = Harness::new().await;
    let member = harness.session("p_member").await;
    let created = ok(&create_link(&harness, &member, "spring", "https://shop.example/").await);
    let link = created["link"]["id"].as_str().unwrap();

    let refused = command(&harness, &member, "links.delete", json!({ "link": link })).await;
    assert_eq!(refused.code(), "forbidden");

    let admin = harness.session("p_admin").await;
    ok(&command(&harness, &admin, "links.delete", json!({ "link": link })).await);
    let gone = harness.collect("GET", "/q/spring", PEER, None).await;
    assert_eq!(gone.status, StatusCode::NOT_FOUND);
    let workspace = harness.workspaces["ws_a"].as_str();
    let list = ok(&harness
        .query(
            Some(&member),
            "links.list",
            &format!("workspace={workspace}"),
        )
        .await);
    assert_eq!(list["links"], json!([]));
    ok(&create_link(&harness, &member, "spring", "https://other.example/").await);
}
