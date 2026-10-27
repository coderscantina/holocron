//! The router: collection routes, dashboard API under `/api`, health, runtime config and the
//! web app.

pub(crate) mod operations;
pub(crate) mod web;

use axum::http::{HeaderName, HeaderValue, header};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::json;
use tower::ServiceBuilder;
use tower_http::compression::CompressionLayer;
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::trace::TraceLayer;

use crate::error::ApiError;
use crate::state::AppState;

async fn health() -> impl IntoResponse {
    Json(json!({ "status": "ok", "version": env!("CARGO_PKG_VERSION") }))
}

/// Unknown API paths and methods get the typed `not_found` body, never the web app.
async fn api_not_found() -> ApiError {
    ApiError::not_found()
}

pub fn router(state: AppState) -> Router {
    let api = Router::new()
        .route("/commands/{name}", post(operations::command))
        .route(
            "/queries/{name}",
            get(operations::query).post(operations::query_with_body),
        )
        .route("/subscribe", get(crate::explorations::live::subscribe))
        .fallback(api_not_found)
        .method_not_allowed_fallback(api_not_found)
        .with_state(state.clone());

    let web_dir = state.config.web_dir.clone();
    Router::new()
        .route("/health", get(health))
        .merge(crate::maintenance::health::router(state.clone()))
        .merge(crate::collect::router(state.clone()))
        .merge(crate::recordings::frame::router(web_dir.as_deref()))
        .nest("/api", api)
        .merge(web::router(state, web_dir.as_deref()))
        .layer(
            ServiceBuilder::new()
                .layer(TraceLayer::new_for_http())
                .layer(CompressionLayer::new())
                .layer(SetResponseHeaderLayer::if_not_present(
                    header::X_CONTENT_TYPE_OPTIONS,
                    HeaderValue::from_static("nosniff"),
                ))
                .layer(SetResponseHeaderLayer::if_not_present(
                    header::X_FRAME_OPTIONS,
                    HeaderValue::from_static("DENY"),
                ))
                .layer(SetResponseHeaderLayer::if_not_present(
                    HeaderName::from_static("referrer-policy"),
                    HeaderValue::from_static("same-origin"),
                )),
        )
}
