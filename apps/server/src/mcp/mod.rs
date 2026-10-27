//! `/mcp`: the Model Context Protocol over Streamable HTTP, for agents with an API key (0022).
//!
//! Stateless: every POST carries one JSON-RPC message and gets one `application/json` answer,
//! other methods answer 405. Revision 2026-07-28 reads the version and client capabilities
//! from each request's `_meta` and checks the headers mirroring them. Clients of 2025-03-26 to
//! 2025-11-25 open with `initialize` and are served by the revision they negotiated, without a
//! session ID.
//!
//! Every request needs `Authorization: Bearer <api key>`. A tool call runs one query operation
//! through the registry as that key, so admission, grants, site reach and the 30 s budget are
//! those of HTTP: MCP adds no access path of its own.

mod prompts;
mod resources;
mod tools;

use axum::extract::{Request, State};
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use holocron_protocol::errors::ErrorCode;
use holocron_protocol::limits::QUERY_BODY_MAX_BYTES;
use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::error::ApiError;
use crate::http::operations;
use crate::identity::{self, Caller};
use crate::state::AppState;

/// The revision this server implements, with per-request metadata.
const MODERN: &str = "2026-07-28";
/// Handshake revisions served after `initialize`, newest first.
const LEGACY: [&str; 3] = ["2025-11-25", "2025-06-18", "2025-03-26"];
/// A request without `MCP-Protocol-Version` or `_meta` speaks 2025-03-26, which had no header.
const LEGACY_DEFAULT: &str = "2025-03-26";

const META_VERSION: &str = "io.modelcontextprotocol/protocolVersion";
const META_CAPABILITIES: &str = "io.modelcontextprotocol/clientCapabilities";
const META_SERVER: &str = "io.modelcontextprotocol/serverInfo";

const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;
const HEADER_MISMATCH: i64 = -32020;
const UNSUPPORTED_VERSION: i64 = -32022;

/// Told to the agent on `initialize` and `server/discover`.
const INSTRUCTIONS: &str = "Holocron web analytics, read-only. Call list_sites for site IDs, \
catalog and field_values for exact property keys, event names and paths, then answer with one \
query of several named blocks. Read holocron://report-query/guide before writing a query. Every \
number is an aggregate: join external data such as ticketing or CRM yourself on the paths and \
properties in the rows, and cite the notes of each block as method notes.";

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/mcp", post(endpoint))
        .with_state(state)
}

async fn endpoint(State(state): State<AppState>, request: Request) -> Response {
    let (parts, body) = request.into_parts();
    if let Some(origin) = parts.headers.get(header::ORIGIN) {
        let own = origin
            .to_str()
            .is_ok_and(|origin| operations::is_own_origin(&state.config, &parts.headers, origin));
        if !own {
            let error = RpcError::new(INVALID_REQUEST, "The Origin header names another site.");
            return reply(StatusCode::FORBIDDEN, Some(error.into_message(None)));
        }
    }
    let caller = match authenticate(&state, &parts).await {
        Ok(caller) => caller,
        Err(error) => return challenge(error, identity::has_api_key(&parts.headers)),
    };
    let Ok(body) = axum::body::to_bytes(body, QUERY_BODY_MAX_BYTES).await else {
        let error = RpcError::new(INVALID_REQUEST, "The message is over 1 MiB.");
        return reply(
            StatusCode::PAYLOAD_TOO_LARGE,
            Some(error.into_message(None)),
        );
    };
    let Ok(message) = serde_json::from_slice::<Value>(&body) else {
        let error = RpcError::new(PARSE_ERROR, "The body is not JSON.");
        return reply(StatusCode::BAD_REQUEST, Some(error.into_message(None)));
    };
    let (status, answer) = handle(&state, caller, &parts.headers, message).await;
    reply(status, answer)
}

fn reply(status: StatusCode, body: Option<Value>) -> Response {
    let response = match body {
        Some(body) => (status, Json(body)).into_response(),
        None => status.into_response(),
    };
    operations::no_store(response)
}

/// The key's caller. Sessions and share links do not reach MCP.
async fn authenticate(state: &AppState, parts: &Parts) -> Result<Caller, ApiError> {
    let unauthenticated = || {
        ApiError::with_message(
            ErrorCode::Unauthenticated,
            "Send an API key as Authorization: Bearer <key>.",
        )
    };
    if !identity::has_api_key(&parts.headers) {
        return Err(unauthenticated());
    }
    match operations::caller(state, parts).await {
        Ok(caller @ Caller::Key(_)) => Ok(caller),
        Err(error) if error.code() != ErrorCode::Unauthenticated => Err(error),
        _ => Err(unauthenticated()),
    }
}

/// A refused credential gets 401 with a Bearer challenge (RFC 6750).
fn challenge(error: ApiError, presented: bool) -> Response {
    let unauthenticated = error.code() == ErrorCode::Unauthenticated;
    let mut response = error.into_response();
    if unauthenticated {
        let challenge = if presented {
            r#"Bearer realm="holocron", error="invalid_token""#
        } else {
            r#"Bearer realm="holocron""#
        };
        response.headers_mut().insert(
            header::WWW_AUTHENTICATE,
            HeaderValue::from_static(challenge),
        );
    }
    response
}

/// One JSON-RPC message: the HTTP status and the answer, none for a notification.
async fn handle(
    state: &AppState,
    caller: Caller,
    headers: &HeaderMap,
    message: Value,
) -> (StatusCode, Option<Value>) {
    let bad = |id: Option<Value>, error: RpcError| {
        (StatusCode::BAD_REQUEST, Some(error.into_message(id)))
    };
    let Value::Object(message) = message else {
        let error = RpcError::new(INVALID_REQUEST, "Send one JSON-RPC message per POST.");
        return bad(None, error);
    };
    let id = message.get("id").cloned();
    let method = message
        .get("method")
        .and_then(Value::as_str)
        .filter(|_| message.get("jsonrpc").and_then(Value::as_str) == Some("2.0"));
    let Some(method) = method else {
        let error = RpcError::new(INVALID_REQUEST, "Expected a JSON-RPC 2.0 request.");
        return bad(id, error);
    };
    let Some(id) = id else {
        return (StatusCode::ACCEPTED, None);
    };
    if !(id.is_string() || id.is_i64() || id.is_u64()) {
        let error = RpcError::new(INVALID_REQUEST, "The id must be a string or an integer.");
        return bad(None, error);
    }
    let params = match message.get("params") {
        None => Map::new(),
        Some(Value::Object(params)) => params.clone(),
        Some(_) => {
            return bad(
                Some(id),
                RpcError::new(INVALID_PARAMS, "params must be an object."),
            );
        }
    };

    if method == "initialize" {
        return answer(id, Ok(initialize(&params)), None);
    }
    let era = match era(headers, method, &params) {
        Ok(era) => era,
        Err(error) => return bad(Some(id), error),
    };
    let result = call(state, caller, era, method, &params).await;
    answer(id, result, Some(era))
}

fn answer(
    id: Value,
    result: Result<Value, RpcError>,
    era: Option<Era>,
) -> (StatusCode, Option<Value>) {
    let modern = era.is_some_and(Era::modern);
    match result {
        Ok(mut result) => {
            if let (true, Some(fields)) = (modern, result.as_object_mut()) {
                fields.insert("resultType".into(), "complete".into());
                fields.insert("_meta".into(), json!({ META_SERVER: server_info() }));
            }
            let message = json!({ "jsonrpc": "2.0", "id": id, "result": result });
            (StatusCode::OK, Some(message))
        }
        Err(error) => {
            let status = if modern && error.code == METHOD_NOT_FOUND {
                StatusCode::NOT_FOUND
            } else {
                StatusCode::OK
            };
            (status, Some(error.into_message(Some(id))))
        }
    }
}

/// The revision a request is served by.
#[derive(Debug, Clone, Copy)]
struct Era {
    version: &'static str,
}

impl Era {
    fn modern(self) -> bool {
        self.version == MODERN
    }

    /// `structuredContent` arrived in 2025-06-18.
    fn structured(self) -> bool {
        self.version >= "2025-06-18"
    }
}

/// A 2026-07-28 request names its version in `_meta`, a legacy one in `MCP-Protocol-Version`.
fn era(headers: &HeaderMap, method: &str, params: &Map<String, Value>) -> Result<Era, RpcError> {
    let meta = params.get("_meta").and_then(Value::as_object);
    let declared = meta
        .and_then(|meta| meta.get(META_VERSION))
        .and_then(Value::as_str);
    let header = header_text(headers, "mcp-protocol-version");
    let requested = declared.or(header.as_deref()).unwrap_or(LEGACY_DEFAULT);
    if requested == MODERN {
        let complete = declared.is_some()
            && meta
                .and_then(|meta| meta.get(META_CAPABILITIES))
                .is_some_and(Value::is_object);
        if !complete {
            return Err(RpcError::new(
                INVALID_PARAMS,
                format!("_meta needs {META_VERSION} and {META_CAPABILITIES}."),
            ));
        }
        check_headers(headers, method, params)?;
        return Ok(Era { version: MODERN });
    }
    match LEGACY.into_iter().find(|version| *version == requested) {
        Some(version) => Ok(Era { version }),
        None => Err(RpcError {
            code: UNSUPPORTED_VERSION,
            message: "Unsupported protocol version".into(),
            data: Some(json!({ "supported": supported(), "requested": requested })),
        }),
    }
}

/// The headers a 2026-07-28 request mirrors from its body must match it.
fn check_headers(
    headers: &HeaderMap,
    method: &str,
    params: &Map<String, Value>,
) -> Result<(), RpcError> {
    let mismatch = |header: &str| {
        RpcError::new(
            HEADER_MISMATCH,
            format!("Header mismatch: {header} is missing or differs from the body."),
        )
    };
    if header_text(headers, "mcp-protocol-version").as_deref() != Some(MODERN) {
        return Err(mismatch("MCP-Protocol-Version"));
    }
    if header_text(headers, "mcp-method").as_deref() != Some(method) {
        return Err(mismatch("Mcp-Method"));
    }
    let name = match method {
        "tools/call" | "prompts/get" => params.get("name"),
        "resources/read" => params.get("uri"),
        _ => None,
    };
    if let Some(name) = name.and_then(Value::as_str) {
        let header = header_text(headers, "mcp-name").and_then(decode_sentinel);
        if header.as_deref() != Some(name) {
            return Err(mismatch("Mcp-Name"));
        }
    }
    Ok(())
}

fn header_text(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
}

/// A header value as sent, or decoded from `=?base64?...?=`.
fn decode_sentinel(value: String) -> Option<String> {
    match value
        .strip_prefix("=?base64?")
        .and_then(|rest| rest.strip_suffix("?="))
    {
        Some(encoded) => STANDARD
            .decode(encoded)
            .ok()
            .and_then(|bytes| String::from_utf8(bytes).ok()),
        None => Some(value),
    }
}

async fn call(
    state: &AppState,
    caller: Caller,
    era: Era,
    method: &str,
    params: &Map<String, Value>,
) -> Result<Value, RpcError> {
    match method {
        "server/discover" if era.modern() => Ok(json!({
            "supportedVersions": supported(),
            "capabilities": capabilities(),
            "instructions": INSTRUCTIONS,
        })),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(tools::list()),
        "tools/call" => tools::call(state, caller, params, era.structured()).await,
        "resources/list" => Ok(resources::list()),
        "resources/templates/list" => Ok(json!({ "resourceTemplates": [] })),
        "resources/read" => resources::read(params, era),
        "prompts/list" => Ok(prompts::list()),
        "prompts/get" => prompts::get(params),
        _ => Err(RpcError::new(
            METHOD_NOT_FOUND,
            format!("Unknown method: {method}"),
        )),
    }
}

/// A legacy client's handshake: its version when this server speaks it, else the newest.
fn initialize(params: &Map<String, Value>) -> Value {
    let requested = params.get("protocolVersion").and_then(Value::as_str);
    let version = LEGACY
        .into_iter()
        .find(|version| Some(*version) == requested)
        .unwrap_or(LEGACY[0]);
    json!({
        "protocolVersion": version,
        "capabilities": capabilities(),
        "serverInfo": server_info(),
        "instructions": INSTRUCTIONS,
    })
}

fn supported() -> Vec<&'static str> {
    std::iter::once(MODERN).chain(LEGACY).collect()
}

fn capabilities() -> Value {
    json!({ "tools": {}, "resources": {}, "prompts": {} })
}

fn server_info() -> Value {
    json!({ "name": "holocron", "title": "Holocron", "version": env!("CARGO_PKG_VERSION") })
}

/// A JSON-RPC error answer.
#[derive(Debug)]
struct RpcError {
    code: i64,
    message: String,
    data: Option<Value>,
}

impl RpcError {
    fn new(code: i64, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }

    fn into_message(self, id: Option<Value>) -> Value {
        let mut error = json!({ "code": self.code, "message": self.message });
        if let Some(data) = self.data {
            error["data"] = data;
        }
        let mut message = json!({ "jsonrpc": "2.0", "error": error });
        if let Some(id) = id {
            message["id"] = id;
        }
        message
    }
}

/// A text content block of a tool result or prompt message.
#[derive(Debug, Serialize)]
struct Text {
    #[serde(rename = "type")]
    kind: &'static str,
    text: String,
}

impl Text {
    fn new(text: impl Into<String>) -> Self {
        Self {
            kind: "text",
            text: text.into(),
        }
    }
}
