//! Static hosting, runtime configuration and API fallbacks.

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use common::{HOST, Harness, ORIGIN};

#[tokio::test]
async fn unknown_api_routes_answer_typed_not_found_never_html() {
    let harness = Harness::new().await;
    let cookie = harness.session("p_member").await;
    let hidden_site = harness
        .query(
            Some(&cookie),
            "sites.get",
            &format!("site={}", harness.sites["site_b1"]),
        )
        .await;

    let answers = [
        harness.get("/api/nope").await,
        harness.get("/api/queries/reports.secret").await,
        harness.get("/api/commands/sites.update").await,
        harness
            .command(
                Some(&cookie),
                "sites.explode",
                serde_json::json!({ "idempotency_key": "k" }),
            )
            .await,
    ];
    for answer in answers {
        assert_eq!(answer.status, StatusCode::NOT_FOUND);
        assert!(
            answer
                .header("content-type")
                .starts_with("application/json")
        );
        assert_eq!(
            answer.bytes, hidden_site.bytes,
            "an unknown operation looks like a hidden resource"
        );
    }
}

#[tokio::test]
async fn hashed_assets_are_immutable_and_html_revalidates() {
    let harness = Harness::new().await;

    let asset = harness.get("/assets/app-3f9a1c.js").await;
    assert_eq!(asset.status, StatusCode::OK);
    assert_eq!(
        asset.header("cache-control"),
        "public, max-age=31536000, immutable"
    );

    for path in ["/", "/sites/0d2c9a6e/settings"] {
        let page = harness.get(path).await;
        assert_eq!(page.status, StatusCode::OK, "{path}");
        assert_eq!(page.header("cache-control"), "no-cache", "{path}");
        assert!(
            String::from_utf8_lossy(&page.bytes).contains("<div id=app>"),
            "{path}"
        );
    }

    let missing = harness.get("/assets/app-0000.js").await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND);
    assert_ne!(
        missing.header("cache-control"),
        "public, max-age=31536000, immutable"
    );
}

#[tokio::test]
async fn runtime_config_comes_from_the_server_and_revalidates() {
    let harness = Harness::new().await;
    let config = harness.get("/config.json").await;
    assert_eq!(config.status, StatusCode::OK);
    assert_eq!(config.header("cache-control"), "no-cache");
    assert_eq!(config.json()["instance_name"], "Test install");
    assert_eq!(config.json()["api_base"], "/api");
}

#[tokio::test]
async fn api_answers_are_not_cached_and_carry_security_headers() {
    let harness = Harness::new().await;
    let answer = harness
        .send(
            Request::get("/api/queries/auth.session")
                .header(header::HOST, HOST)
                .header(header::ORIGIN, ORIGIN)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(answer.status, StatusCode::OK);
    assert_eq!(answer.header("cache-control"), "no-store");
    assert_eq!(answer.header("x-content-type-options"), "nosniff");
    assert_eq!(answer.header("x-frame-options"), "DENY");
}
