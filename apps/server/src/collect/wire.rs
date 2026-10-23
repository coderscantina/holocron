//! Upstream Umami response shapes for the public collection routes (collection compatibility
//! contract, "POST /api/send" and "CORS").

use axum::Json;
use axum::http::{HeaderName, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use serde_json::{Value, json};
use uuid::Uuid;

/// Seconds a refused client is told to wait. Only server integrations read it.
pub const RETRY_AFTER_SECONDS: &str = "5";

/// The body of a stored, duplicate or `outside_retention` event: unchanged from upstream.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stored {
    pub cache: String,
    pub session_id: Uuid,
    pub visit_id: Uuid,
}

/// What one `/api/send` call or batch item answers.
#[derive(Debug, Clone)]
pub enum Answer {
    Stored(Stored),
    /// `{beep: "boop"}`, 200: a bot, suppressed.
    Bot,
    /// 403 `Forbidden`: an address on `collect.ignore_ips`, suppressed.
    Forbidden,
    /// 401 `Unauthorized`: a collection key that is unknown, revoked, or for another site.
    Unauthorized,
    /// 400 with the given message, for example `Website not found.`.
    BadRequest(&'static str),
}

impl Answer {
    pub fn status(&self) -> StatusCode {
        match self {
            Answer::Stored(_) | Answer::Bot => StatusCode::OK,
            Answer::Forbidden => StatusCode::FORBIDDEN,
            Answer::Unauthorized => StatusCode::UNAUTHORIZED,
            Answer::BadRequest(_) => StatusCode::BAD_REQUEST,
        }
    }

    pub fn body(&self) -> Value {
        match self {
            Answer::Stored(stored) => json!(stored),
            Answer::Bot => json!({ "beep": "boop" }),
            Answer::Forbidden => error("Forbidden", "forbidden", 403),
            Answer::Unauthorized => error("Unauthorized", "unauthorized", 401),
            Answer::BadRequest(message) => error(message, "bad-request", 400),
        }
    }
}

impl IntoResponse for Answer {
    fn into_response(self) -> Response {
        (self.status(), Json(self.body())).into_response()
    }
}

pub const BAD_REQUEST: &str = "Bad request";

pub fn error(message: &str, code: &str, status: u16) -> Value {
    json!({ "error": { "message": message, "code": code, "status": status } })
}

/// 413 with the upstream `payloadTooLarge` envelope (0005 Request body caps).
pub fn too_large(max_bytes: usize, size: u64) -> Response {
    let mut body = error("Payload too large", "payload-too-large", 413);
    body["error"]["reason"] = json!("payload_too_large");
    body["error"]["maxBytes"] = json!(max_bytes);
    body["error"]["size"] = json!(size);
    (StatusCode::PAYLOAD_TOO_LARGE, Json(body)).into_response()
}

/// 429 when the admission budget is full, 503 when durable storage is unavailable. Both carry
/// `Retry-After` (0005 Backpressure).
pub fn refused(busy: bool) -> Response {
    let (status, body) = if busy {
        (
            StatusCode::TOO_MANY_REQUESTS,
            error("Too many requests", "too-many-requests", 429),
        )
    } else {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            error("Service unavailable", "service-unavailable", 503),
        )
    };
    let mut response = (status, Json(body)).into_response();
    response.headers_mut().insert(
        header::RETRY_AFTER,
        HeaderValue::from_static(RETRY_AFTER_SECONDS),
    );
    response
}

pub fn server_error() -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(error("Server error", "server-error", 500)),
    )
        .into_response()
}

pub fn not_found() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(error("Not found", "not-found", 404)),
    )
        .into_response()
}

/// Headers upstream sends on every `/api/*` answer (`next.config.ts:65-97`).
pub const API_CORS: [(HeaderName, &str); 5] = [
    (header::ACCESS_CONTROL_ALLOW_ORIGIN, "*"),
    (header::ACCESS_CONTROL_ALLOW_HEADERS, "*"),
    (
        header::ACCESS_CONTROL_ALLOW_METHODS,
        "GET, DELETE, POST, PUT",
    ),
    (header::ACCESS_CONTROL_MAX_AGE, "86400"),
    (header::CACHE_CONTROL, "no-cache"),
];
