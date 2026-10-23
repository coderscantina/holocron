//! The built web app and its runtime configuration, served without Node.

use std::path::{Path, PathBuf};

use axum::extract::{Request, State};
use axum::handler::HandlerWithoutStateExt;
use axum::http::{HeaderValue, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use holocron_protocol::config::RuntimeConfig;
use tower_http::services::{ServeDir, ServeFile};

use crate::state::AppState;

const IMMUTABLE: &str = "public, max-age=31536000, immutable";
const REVALIDATE: &str = "no-cache";

/// Read at boot by the web app. Uncached, so a changed setting needs a restart, not a rebuild.
async fn runtime_config(State(state): State<AppState>) -> impl IntoResponse {
    let config = RuntimeConfig {
        api_base: "/api".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        instance_name: state.config.instance_name.clone(),
        dbip_attribution: state.collector.geoip().dbip(),
        setup: false,
    };
    ([(header::CACHE_CONTROL, REVALIDATE)], Json(config))
}

/// Hashed files under `/assets/` never change; HTML and everything else revalidates. Assets are
/// readable cross-origin because the sandboxed replay frame loads them from an opaque origin.
async fn cache_control(request: Request, next: Next) -> Response {
    let hashed = request.uri().path().starts_with("/assets/");
    let mut response = next.run(request).await;
    if !response.headers().contains_key(header::CACHE_CONTROL) {
        let value = if hashed && response.status().is_success() {
            IMMUTABLE
        } else {
            REVALIDATE
        };
        response
            .headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static(value));
    }
    if hashed {
        response.headers_mut().insert(
            header::ACCESS_CONTROL_ALLOW_ORIGIN,
            HeaderValue::from_static("*"),
        );
    }
    response
}

async fn no_web_app() -> impl IntoResponse {
    (
        StatusCode::NOT_FOUND,
        "No web app configured. Set HOLOCRON_WEB_DIR.",
    )
}

/// `index.html` for client routes. A missing file such as `/umami.js` is a 404, as upstream
/// answers unknown script names, never the app under a file name. Share links are always the
/// app: an imported Umami 2.x link ends in the website name, which often has a dot.
async fn client_route(index: PathBuf, request: Request) -> Response {
    let path = request.uri().path();
    let last = path.rsplit('/').next().unwrap_or_default();
    if last.contains('.') && !path.starts_with("/share/") {
        return StatusCode::NOT_FOUND.into_response();
    }
    match ServeFile::new(index).try_call(request).await {
        Ok(response) => response.into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

/// `/config.json`, hashed assets, other static files, and `index.html` for client routes.
pub fn router(state: AppState, web_dir: Option<&Path>) -> Router {
    files(
        Router::new()
            .route("/config.json", get(runtime_config))
            .with_state(state),
        web_dir,
    )
}

/// The web app's files after `routes`. A missing hashed asset is a 404, never `index.html`
/// under an immutable cache header.
pub(crate) fn files(router: Router, web_dir: Option<&Path>) -> Router {
    let router = match web_dir {
        Some(dir) => {
            let index = dir.join("index.html");
            let fallback = move |request: Request| client_route(index.clone(), request);
            router
                .nest_service("/assets", ServeDir::new(dir.join("assets")))
                .fallback_service(ServeDir::new(dir).fallback(fallback.into_service()))
        }
        None => router.fallback(no_web_app),
    };
    router.layer(middleware::from_fn(cache_control))
}
