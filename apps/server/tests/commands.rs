//! Command protection, idempotency, revisions and validation.

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use common::{HOST, Harness, ORIGIN};
use serde_json::json;

fn update(harness: &Harness, key: &str, name: &str, expected: Option<i64>) -> serde_json::Value {
    let mut body = json!({
        "idempotency_key": key,
        "site": harness.sites["site_a1"].to_string(),
        "name": name,
        "domain": "roadhouse.test",
    });
    if let Some(expected) = expected {
        body["expected_revision"] = expected.into();
    }
    body
}

#[tokio::test]
async fn commands_need_the_request_marker_and_a_same_origin_origin() {
    let harness = Harness::new().await;
    let cookie = harness.session("p_member").await;
    let body = update(&harness, "k", "Renamed", None).to_string();
    let request = |origin: Option<&str>, marker: bool| {
        let mut builder = Request::post("/api/commands/sites.update")
            .header(header::HOST, HOST)
            .header(header::COOKIE, &cookie)
            .header(header::CONTENT_TYPE, "application/json");
        if let Some(origin) = origin {
            builder = builder.header(header::ORIGIN, origin);
        }
        if marker {
            builder = builder.header("x-holocron-request", "1");
        }
        builder.body(Body::from(body.clone())).unwrap()
    };

    for (origin, marker) in [
        (Some(ORIGIN), false),
        (Some("https://evil.example"), true),
        (None, true),
    ] {
        let answer = harness.send(request(origin, marker)).await;
        assert_eq!(answer.status, StatusCode::FORBIDDEN);
        assert_eq!(answer.code(), "origin_rejected");
        // The harness sets no public URL, so there is no single address to name.
        assert!(answer.json().get("expected_origin").is_none());
    }
    assert_eq!(
        harness
            .topic_head(&format!("site:{}", harness.sites["site_a1"]))
            .await,
        0
    );
    assert_eq!(
        harness.send(request(Some(ORIGIN), true)).await.status,
        StatusCode::OK
    );
}

/// AX-S001: the same key and arguments return the stored answer and record no second event.
#[tokio::test]
async fn same_key_and_arguments_replay_the_stored_answer() {
    let harness = Harness::new().await;
    let cookie = harness.session("p_member").await;
    let topic = format!("site:{}", harness.sites["site_a1"]);

    let first = harness
        .command(
            Some(&cookie),
            "sites.update",
            update(&harness, "K1", "Renamed", Some(0)),
        )
        .await;
    let second = harness
        .command(
            Some(&cookie),
            "sites.update",
            update(&harness, "K1", "Renamed", Some(0)),
        )
        .await;
    assert_eq!(first.status, StatusCode::OK, "{}", first.json());
    assert_eq!(first.json()["revision"], 1);
    assert_eq!(second.bytes, first.bytes);
    assert_eq!(harness.topic_head(&topic).await, 1);
}

/// AX-S002 and AX-S003: a key is bound to its arguments and to its owner.
#[tokio::test]
async fn reused_key_with_other_arguments_is_a_conflict() {
    let harness = Harness::new().await;
    let member = harness.session("p_member").await;
    let owner = harness.session("p_owner").await;

    harness
        .command(
            Some(&member),
            "sites.update",
            update(&harness, "K2", "One", None),
        )
        .await;
    let conflict = harness
        .command(
            Some(&member),
            "sites.update",
            update(&harness, "K2", "Two", None),
        )
        .await;
    assert_eq!(conflict.status, StatusCode::CONFLICT);
    assert_eq!(conflict.code(), "idempotency_conflict");

    let other_owner = harness
        .command(
            Some(&owner),
            "sites.update",
            update(&harness, "K2", "Two", None),
        )
        .await;
    assert_eq!(other_owner.status, StatusCode::OK);
    assert_eq!(other_owner.json()["site"]["name"], "Two");
}

/// AX-S024: a failed command stores nothing, so a retry with the same key runs again.
#[tokio::test]
async fn failed_command_stores_nothing_under_its_key() {
    let harness = Harness::new().await;
    let cookie = harness.session("p_member").await;

    let failed = harness
        .command(
            Some(&cookie),
            "sites.update",
            update(&harness, "K3", "Renamed", Some(5)),
        )
        .await;
    assert_eq!(failed.code(), "stale_revision");
    let retried = harness
        .command(
            Some(&cookie),
            "sites.update",
            update(&harness, "K3", "Renamed", Some(5)),
        )
        .await;
    assert_eq!(retried.code(), "stale_revision");

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM idempotency_keys")
        .fetch_one(&harness.state.db.reader)
        .await
        .unwrap();
    assert_eq!(count, 0);
}

/// AX-S004: a stored answer is not returned after the caller lost access.
#[tokio::test]
async fn replay_is_authorized_again_after_membership_removal() {
    let harness = Harness::new().await;
    let cookie = harness.session("p_removed").await;

    let first = harness
        .command(
            Some(&cookie),
            "sites.update",
            update(&harness, "K4", "Renamed", None),
        )
        .await;
    assert_eq!(first.status, StatusCode::OK);
    harness.remove_member("p_removed", "ws_a").await;
    let replay = harness
        .command(
            Some(&cookie),
            "sites.update",
            update(&harness, "K4", "Renamed", None),
        )
        .await;
    assert_eq!(replay.status, StatusCode::NOT_FOUND);
    assert_eq!(replay.code(), "not_found");
}

/// AX-S007 and AX-S008: only a caller allowed to write learns the current revision.
#[tokio::test]
async fn stale_revision_carries_the_head_only_for_writers() {
    let harness = Harness::new().await;
    let member = harness.session("p_member").await;
    let viewer = harness.session("p_viewer").await;
    let outsider = harness.session("p_other_owner").await;

    harness
        .command(
            Some(&member),
            "sites.update",
            update(&harness, "a", "One", Some(0)),
        )
        .await;
    let stale = harness
        .command(
            Some(&member),
            "sites.update",
            update(&harness, "b", "Two", Some(0)),
        )
        .await;
    assert_eq!(stale.status, StatusCode::CONFLICT);
    assert_eq!(stale.json()["current"], 1);

    let read = harness
        .query(
            Some(&member),
            "sites.get",
            &format!("site={}", harness.sites["site_a1"]),
        )
        .await;
    assert_eq!(read.json()["revision"], 1);
    assert_eq!(
        read.json()["cursor"],
        format!("site:{}@1", harness.sites["site_a1"])
    );

    assert_eq!(
        harness
            .command(
                Some(&viewer),
                "sites.update",
                update(&harness, "c", "Two", Some(0))
            )
            .await
            .code(),
        "forbidden"
    );
    let hidden = harness
        .command(
            Some(&outsider),
            "sites.update",
            update(&harness, "d", "Two", Some(0)),
        )
        .await;
    assert_eq!(hidden.code(), "not_found");
    assert!(hidden.json().get("current").is_none());
}

#[tokio::test]
async fn invalid_fields_are_named() {
    let harness = Harness::new().await;
    let cookie = harness.session("p_member").await;
    let mut body = update(&harness, "k", "   ", None);
    body["domain"] = "https://roadhouse.test/".into();

    let answer = harness.command(Some(&cookie), "sites.update", body).await;
    assert_eq!(answer.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        answer.json()["errors"],
        json!({ "name": [{ "code": "required" }], "domain": [{ "code": "invalid_format" }] })
    );
}

#[tokio::test]
async fn malformed_and_oversized_bodies_are_typed_errors() {
    let harness = Harness::new().await;
    let cookie = harness.session("p_member").await;

    let mut no_key = update(&harness, "k", "Renamed", None);
    no_key.as_object_mut().unwrap().remove("idempotency_key");
    assert_eq!(
        harness
            .command(Some(&cookie), "sites.update", no_key)
            .await
            .code(),
        "malformed_request"
    );

    let wrong_type =
        json!({ "idempotency_key": "k", "site": 7, "name": "x", "domain": "x.example" });
    assert_eq!(
        harness
            .command(Some(&cookie), "sites.update", wrong_type)
            .await
            .code(),
        "malformed_request"
    );

    let huge = update(&harness, "k", &"x".repeat(70 * 1024), None);
    let answer = harness.command(Some(&cookie), "sites.update", huge).await;
    assert_eq!(answer.status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(answer.code(), "payload_too_large");
}
