//! The replay frame: the one page that renders captured documents. Its content security policy
//! allows only its own script and inline styles and no network at all; the browser applies the
//! same policy to the scriptless iframe rrweb renders the captured page into. Its sandbox
//! forbids top navigation, popups, forms and dialogs for everything inside. The dashboard hands
//! it events through `postMessage` (decision 0011).
//!
//! The frame keeps the dashboard's origin: rrweb's Replayer must reach into its own iframe, which
//! an opaque-origin sandbox would put on yet another origin. Captured content never runs script,
//! so it cannot use that origin.

use std::path::{Path, PathBuf};

use axum::Router;
use axum::extract::State;
use axum::http::{HeaderName, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;

pub const PATH: &str = "/replay-frame.html";

/// Captured pages keep their inline styles and data URLs. Remote images, fonts, styles,
/// frames, forms and fetches are all blocked, so replayed content cannot reach the network.
pub const CONTENT_SECURITY_POLICY: &str = "default-src 'none'; script-src 'self'; \
     style-src 'self' 'unsafe-inline'; img-src data: blob:; font-src data:; media-src 'none'; \
     connect-src 'none'; object-src 'none'; frame-src 'none'; worker-src 'none'; \
     form-action 'none'; base-uri 'none'; frame-ancestors 'self'; sandbox allow-scripts allow-same-origin";

pub fn router(web_dir: Option<&Path>) -> Router {
    Router::new()
        .route(PATH, get(frame))
        .with_state(web_dir.map(|dir| dir.join("replay-frame.html")))
}

async fn frame(State(file): State<Option<PathBuf>>) -> Response {
    let Some(file) = file else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Ok(html) = tokio::fs::read(&file).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    (
        [
            (
                header::CONTENT_TYPE,
                HeaderValue::from_static("text/html; charset=utf-8"),
            ),
            (
                header::CONTENT_SECURITY_POLICY,
                HeaderValue::from_static(CONTENT_SECURITY_POLICY),
            ),
            (
                header::X_FRAME_OPTIONS,
                HeaderValue::from_static("SAMEORIGIN"),
            ),
            (
                HeaderName::from_static("referrer-policy"),
                HeaderValue::from_static("no-referrer"),
            ),
            (header::CACHE_CONTROL, HeaderValue::from_static("no-cache")),
        ],
        html,
    )
        .into_response()
}
