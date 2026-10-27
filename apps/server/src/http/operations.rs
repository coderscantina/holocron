//! `POST /api/commands/{name}`, `GET /api/queries/{name}` and `POST /api/queries/{name}`: the
//! only dashboard entry points. A query answers the same from a query string or a JSON body.
//!
//! A command runs: origin check (skipped for API keys), lookup, caller, admission (credential,
//! key grants, share reach, enrollment-only session), body parse, `BEGIN IMMEDIATE`, idempotency
//! lookup (a replay is authorized again, including a fresh login when the original needed one),
//! the handler, revision, stored answer with one-time secrets withheld, commit, then cookies. A
//! failed handler rolls back and stores nothing.

use std::net::SocketAddr;
use std::time::Duration;

use axum::Json;
use axum::extract::{ConnectInfo, Path, Request, State};
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderValue, header};
use axum::response::{IntoResponse, Response};
use holocron_protocol::envelope::Topic;
use holocron_protocol::errors::ErrorCode;
use holocron_protocol::limits::{
    COMMAND_BODY_MAX_BYTES, IDEMPOTENCY_KEY_MAX_CHARS, LOGIN_CHALLENGE_MS, QUERY_BODY_MAX_BYTES,
    QUERY_STRING_MAX_BYTES, REQUEST_MARKER_HEADER, REQUEST_MARKER_VALUE, REQUEST_TIMEOUT_MS,
    SESSION_LIFETIME_MS,
};
use holocron_protocol::operations;
use holocron_protocol::registry::{
    ArgsSource, DispatchError, FormArgs, JsonArgs, Kind, OperationInfo, Reauth,
};
use serde_json::{Map, Value};

use crate::config::Config;
use crate::context::{self, Authorized, CommandCtx, CommandParts, QueryCtx, SessionChange};
use crate::db::now_ms;
use crate::error::ApiError;
use crate::identity::{self, CHALLENGE_COOKIE, Caller, SESSION_COOKIE};
use crate::state::AppState;
use crate::{events, idempotency};

pub async fn command(
    State(state): State<AppState>,
    Path(name): Path<String>,
    request: Request,
) -> Response {
    match run_command(&state, &name, request).await {
        Ok(response) => response,
        Err(error) => error.into_response(),
    }
}

pub async fn query(
    State(state): State<AppState>,
    Path(name): Path<String>,
    request: Request,
) -> Response {
    match run_query(&state, &name, request).await {
        Ok(answer) => no_store(Json(Value::Object(answer)).into_response()),
        Err(error) => error.into_response(),
    }
}

pub async fn query_with_body(
    State(state): State<AppState>,
    Path(name): Path<String>,
    request: Request,
) -> Response {
    match run_query_with_body(&state, &name, request).await {
        Ok(answer) => no_store(Json(Value::Object(answer)).into_response()),
        Err(error) => error.into_response(),
    }
}

async fn run_command(state: &AppState, name: &str, request: Request) -> Result<Response, ApiError> {
    let (parts, body) = request.into_parts();
    if !identity::has_api_key(&parts.headers) {
        check_origin(&state.config, &parts.headers)?;
    }
    let op = operations::find(Kind::Command, name).ok_or_else(ApiError::not_found)?;
    let caller = caller(state, &parts).await?;
    caller.admit(op)?;

    let body = axum::body::to_bytes(body, COMMAND_BODY_MAX_BYTES)
        .await
        .map_err(|_| ApiError::new(ErrorCode::PayloadTooLarge))?;
    let (key, args) = split_body(&body)?;
    let fingerprint = idempotency::fingerprint(&args);
    let owner = caller.key_owner();
    let now = now_ms();

    let mut tx = state.db.writer.begin_with("BEGIN IMMEDIATE").await?;

    if let Some(stored) = idempotency::find(&mut tx, &owner, &key, now).await? {
        if stored.operation != op.name || stored.fingerprint != fingerprint {
            return Err(ApiError::new(ErrorCode::IdempotencyConflict));
        }
        let authorized: Vec<Authorized> =
            serde_json::from_str(&stored.authorized).map_err(ApiError::internal)?;
        context::reauthorize(&mut tx, &caller, &authorized).await?;
        if stored.fresh_login {
            caller.require_fresh_login(now)?;
        }
        tx.commit().await?;
        let answer: Value = serde_json::from_str(&stored.response).map_err(ApiError::internal)?;
        return Ok(no_store(Json(answer).into_response()));
    }

    let mut ctx = CommandCtx::new(tx, caller, now, op);
    let mut answer = operations::dispatch_command(state, &mut ctx, op.name, &JsonArgs(args))
        .await
        .map_err(dispatch_error)?;
    let CommandParts {
        mut tx,
        last_event,
        authorized,
        session,
        fresh_login,
    } = ctx.into_parts();
    if op.policy.reauth == Reauth::Always && !fresh_login {
        return Err(ApiError::internal(format!(
            "{} committed without its reauthentication check",
            op.name
        )));
    }

    let revision = match last_event {
        Some(seq) => seq,
        None => events::head(&mut tx, &Topic::installation()).await?,
    };
    answer.insert("revision".into(), revision.into());
    let withheld = if op.policy.secret {
        Some(withhold_secret(answer.clone(), op.name)?)
    } else {
        None
    };
    let response = Value::Object(answer).to_string();
    let stored_response = withheld.unwrap_or_else(|| response.clone());

    let stored = idempotency::Stored {
        operation: op.name.to_owned(),
        fingerprint,
        response: stored_response,
        authorized: serde_json::to_string(&authorized).map_err(ApiError::internal)?,
        fresh_login,
    };
    idempotency::store(&mut tx, &owner, &key, &stored, now).await?;
    tx.commit().await?;

    let mut response = no_store(
        (
            [(
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/json"),
            )],
            response,
        )
            .into_response(),
    );
    if let Some(change) = session {
        for cookie in session_cookies(&state.config, &change) {
            if let Ok(value) = HeaderValue::from_str(&cookie) {
                response.headers_mut().append(header::SET_COOKIE, value);
            }
        }
    }
    Ok(response)
}

async fn run_query(
    state: &AppState,
    name: &str,
    request: Request,
) -> Result<Map<String, Value>, ApiError> {
    let op = operations::find(Kind::Query, name).ok_or_else(ApiError::not_found)?;
    let (parts, _) = request.into_parts();
    let raw = parts.uri.query().unwrap_or_default();
    if raw.len() > QUERY_STRING_MAX_BYTES {
        return Err(ApiError::new(ErrorCode::PayloadTooLarge));
    }
    let caller = caller(state, &parts).await?;
    caller.admit(op)?;
    read(state, op, caller, &FormArgs(raw)).await
}

/// The same query with its arguments as a JSON object body, for reports too big for a URL.
async fn run_query_with_body(
    state: &AppState,
    name: &str,
    request: Request,
) -> Result<Map<String, Value>, ApiError> {
    let op = operations::find(Kind::Query, name).ok_or_else(ApiError::not_found)?;
    let (parts, body) = request.into_parts();
    let caller = caller(state, &parts).await?;
    caller.admit(op)?;
    let body = axum::body::to_bytes(body, QUERY_BODY_MAX_BYTES)
        .await
        .map_err(|_| ApiError::new(ErrorCode::PayloadTooLarge))?;
    let Ok(args @ Value::Object(_)) = serde_json::from_slice::<Value>(&body) else {
        return Err(ApiError::with_message(
            ErrorCode::MalformedRequest,
            "The body must be a JSON object.",
        ));
    };
    read(state, op, caller, &JsonArgs(args)).await
}

/// Runs an admitted query in one read transaction within the request budget.
async fn read(
    state: &AppState,
    op: &'static OperationInfo,
    caller: Caller,
    args: &impl ArgsSource,
) -> Result<Map<String, Value>, ApiError> {
    let work = async {
        let tx = state.db.reader.begin().await?;
        let mut ctx = QueryCtx::new(tx, caller);
        let (mut answer, topic) = operations::dispatch_query(state, &mut ctx, op.name, args)
            .await
            .map_err(dispatch_error)?;
        let mut tx = ctx.into_tx();
        let revision = events::head(&mut tx, &topic).await?;
        tx.rollback().await?;
        answer.insert("revision".into(), revision.into());
        answer.insert("cursor".into(), topic.cursor(revision).into());
        Ok(answer)
    };
    // Dropping the future on timeout rolls the read transaction back.
    tokio::time::timeout(Duration::from_millis(REQUEST_TIMEOUT_MS), work)
        .await
        .unwrap_or_else(|_| Err(ApiError::new(ErrorCode::QueryTimeout)))
}

/// Mutations need the request marker and an Origin naming this server, which a cross-site
/// form, image or plain link cannot produce. Browser collection uses other routes.
pub(crate) fn check_origin(config: &Config, headers: &HeaderMap) -> Result<(), ApiError> {
    let rejected = || ApiError::new(ErrorCode::OriginRejected);
    let marker = headers
        .get(REQUEST_MARKER_HEADER)
        .and_then(|v| v.to_str().ok());
    if marker != Some(REQUEST_MARKER_VALUE) {
        return Err(rejected());
    }
    let origin = headers
        .get(header::ORIGIN)
        .and_then(|v| v.to_str().ok())
        .map(str::to_lowercase)
        .ok_or_else(rejected)?;
    let allowed = match &config.public_origin {
        Some(public) => origin == *public,
        None => {
            let host = headers
                .get(header::HOST)
                .and_then(|v| v.to_str().ok())
                .ok_or_else(rejected)?;
            let host = host.to_lowercase();
            origin == format!("http://{host}") || origin == format!("https://{host}")
        }
    };
    if allowed { Ok(()) } else { Err(rejected()) }
}

async fn caller(state: &AppState, parts: &Parts) -> Result<Caller, ApiError> {
    let address = parts
        .extensions
        .get::<ConnectInfo<SocketAddr>>()
        .map(|info| info.0.ip().to_string())
        .unwrap_or_else(|| "unknown".into());
    identity::resolve_caller(&state.db, &parts.headers, address).await
}

/// Splits `{ idempotency_key, ...args }`.
fn split_body(body: &[u8]) -> Result<(String, Value), ApiError> {
    let malformed = |message: &str| ApiError::with_message(ErrorCode::MalformedRequest, message);
    let Ok(Value::Object(mut map)) = serde_json::from_slice::<Value>(body) else {
        return Err(malformed("The body must be a JSON object."));
    };
    let key = match map.remove("idempotency_key") {
        Some(Value::String(key))
            if !key.is_empty() && key.chars().count() <= IDEMPOTENCY_KEY_MAX_CHARS =>
        {
            key
        }
        _ => {
            return Err(malformed(
                "idempotency_key must be a string of 1 to 128 characters.",
            ));
        }
    };
    Ok((key, Value::Object(map)))
}

/// The answer as stored for replay: the one-time `secret` replaced by `null` and marked withheld
/// (R-IDEM-SECRET). An operation declared `secret` without the field fails closed.
fn withhold_secret(mut answer: Map<String, Value>, name: &str) -> Result<String, ApiError> {
    if !answer.contains_key("secret") {
        return Err(ApiError::internal(format!(
            "{name} declares a one-time secret but its result has no secret field"
        )));
    }
    answer.insert("secret".into(), Value::Null);
    answer.insert("secret_withheld".into(), Value::Bool(true));
    Ok(Value::Object(answer).to_string())
}

fn dispatch_error(error: DispatchError<ApiError>) -> ApiError {
    match error {
        DispatchError::Unknown => ApiError::not_found(),
        DispatchError::Args(message) => {
            ApiError::with_message(ErrorCode::MalformedRequest, message)
        }
        DispatchError::Handler(error) => error,
    }
}

pub(crate) fn session_cookies(config: &Config, change: &SessionChange) -> Vec<String> {
    let secure = if config.secure_cookies() {
        "; Secure"
    } else {
        ""
    };
    let cookie = |name: &str, value: &str, max_age_ms: i64| {
        format!(
            "{name}={value}; Path=/; HttpOnly; SameSite=Lax; Max-Age={}{secure}",
            max_age_ms / 1000
        )
    };
    match change {
        SessionChange::Start(token) => vec![
            cookie(SESSION_COOKIE, token, SESSION_LIFETIME_MS),
            cookie(CHALLENGE_COOKIE, "", 0),
        ],
        SessionChange::End => vec![cookie(SESSION_COOKIE, "", 0)],
        SessionChange::Challenge(token) => {
            vec![cookie(CHALLENGE_COOKIE, token, LOGIN_CHALLENGE_MS)]
        }
    }
}

pub(crate) fn no_store(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}
