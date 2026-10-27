//! Queries sent as `POST /api/queries/{name}` with a JSON body answer like their `GET` form.

mod common;

use axum::http::StatusCode;
use common::Harness;
use holocron_protocol::limits::QUERY_BODY_MAX_BYTES;
use serde_json::json;

async fn bearer_key(harness: &Harness, operations: &[&str]) -> String {
    let session = harness.session("p_member").await;
    let created = harness
        .command(
            Some(&session),
            "apiKeys.create",
            json!({
                "idempotency_key": uuid::Uuid::new_v4().to_string(),
                "name": "agent",
                "operations": operations,
                "sites": { "kind": "all" },
            }),
        )
        .await
        .json();
    format!("Bearer {}", created["secret"].as_str().unwrap())
}

#[tokio::test]
async fn post_answers_like_get_and_admits_keys_alike() {
    let harness = Harness::new().await;
    let key = bearer_key(&harness, &["sites.get"]).await;
    let site = harness.sites["site_a1"].to_string();

    let get = harness
        .query_with(&key, "sites.get", &format!("site={site}"))
        .await;
    let post = harness
        .query_post(
            Some(&key),
            "sites.get",
            json!({ "site": site }).to_string().into_bytes(),
        )
        .await;
    assert_eq!(get.status, StatusCode::OK);
    assert_eq!(post.status, StatusCode::OK);
    assert_eq!(post.bytes, get.bytes);

    let body = json!({ "site": site }).to_string().into_bytes();
    let refused_get = harness.query_with(&key, "sites.list", "").await;
    let refused_post = harness
        .query_post(Some(&key), "sites.list", b"{}".to_vec())
        .await;
    assert_ne!(refused_post.status, StatusCode::OK);
    assert_eq!(refused_post.status, refused_get.status);
    assert_eq!(refused_post.bytes, refused_get.bytes);

    let anonymous = harness.query_post(None, "sites.get", body).await;
    assert_eq!(anonymous.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn post_refuses_a_body_that_is_not_an_object_or_too_big() {
    let harness = Harness::new().await;
    let key = bearer_key(&harness, &["sites.get"]).await;

    let array = harness
        .query_post(Some(&key), "sites.get", b"[]".to_vec())
        .await;
    assert_eq!(array.code(), "malformed_request");

    let big = format!("{{\"site\":\"{}\"}}", "x".repeat(QUERY_BODY_MAX_BYTES));
    let oversize = harness
        .query_post(Some(&key), "sites.get", big.into_bytes())
        .await;
    assert_eq!(oversize.code(), "payload_too_large");
}
