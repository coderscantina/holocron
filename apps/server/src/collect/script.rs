//! The pinned, unchanged Umami 3.4.0 tracker and recorder (collection compatibility contract,
//! "Script assets and aliases"). `collect.endpoint` rewrites `/api/send` in the served tracker
//! as upstream's `update-tracker.js` does; nothing else touches the bytes.

use axum::http::{HeaderValue, header};
use axum::response::{IntoResponse, Response};

pub const TRACKER: &[u8] = include_bytes!("../../../../sources/umami-3.4.0/script.js");
pub const RECORDER: &[u8] = include_bytes!("../../../../sources/umami-3.4.0/recorder.js");

/// The tracker as served: pinned bytes, or the endpoint rewrite of them.
pub fn tracker(endpoint: Option<&str>) -> Vec<u8> {
    match endpoint {
        Some(endpoint) => String::from_utf8_lossy(TRACKER)
            .replace("/api/send", endpoint)
            .into_bytes(),
        None => TRACKER.to_vec(),
    }
}

pub fn respond(bytes: Vec<u8>) -> Response {
    (
        [
            (
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/javascript; charset=UTF-8"),
            ),
            (
                header::CACHE_CONTROL,
                HeaderValue::from_static("public, max-age=86400, must-revalidate"),
            ),
            (
                header::ACCESS_CONTROL_ALLOW_ORIGIN,
                HeaderValue::from_static("*"),
            ),
        ],
        bytes,
    )
        .into_response()
}
