//! Collection routes: tracker and recorder assets, `/api/send` and its alias, `/api/batch`,
//! the recorder configuration and `/api/record`, and link and pixel hits.

use std::net::SocketAddr;

use axum::body::Body;
use axum::extract::{ConnectInfo, Path, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use holocron_protocol::operations::collection::{
    CollectionOutcome, CollectionRoute, OutcomeReason,
};
use serde_json::value::RawValue;
use serde_json::{Map, Value, json};
use tower_http::set_header::SetResponseHeaderLayer;
use uuid::Uuid;

use super::client::Client;
use super::intake::{BATCH_MAX_BYTES, BodyError, SEND_MAX_BYTES};
use super::keys::Trust;
use super::payload::{self, Item, Kind, Source};
use super::token::Claims;
use super::wire::{self, API_CORS, Answer, BAD_REQUEST};
use super::{Collector, Step, record, script};
use crate::state::AppState;

/// Upstream's 43-byte transparent GIF.
const PIXEL: [u8; 43] = [
    0x47, 0x49, 0x46, 0x38, 0x39, 0x61, 0x01, 0x00, 0x01, 0x00, 0x80, 0x00, 0x00, 0xff, 0xff, 0xff,
    0x00, 0x00, 0x00, 0x21, 0xf9, 0x04, 0x01, 0x00, 0x00, 0x00, 0x00, 0x2c, 0x00, 0x00, 0x00, 0x00,
    0x01, 0x00, 0x01, 0x00, 0x00, 0x02, 0x02, 0x44, 0x01, 0x00, 0x3b,
];
const MAX_BATCH_ITEMS: usize = 500;

pub fn router(state: AppState) -> Router {
    let config = &state.collector.config;
    let mut api = Router::new()
        .route("/api/send", post(send).options(preflight))
        .route("/api/batch", post(batch).options(preflight))
        .route(
            "/api/websites/{id}/recorder",
            get(record::recorder_config).options(preflight),
        )
        .route("/api/record", post(record::record).options(preflight));
    if let Some(endpoint) = &config.collect_endpoint {
        api = api.route(endpoint, post(send).options(preflight));
    }
    for (name, value) in API_CORS {
        api = api.layer(SetResponseHeaderLayer::overriding(
            name,
            HeaderValue::from_static(value),
        ));
    }

    let mut assets = Router::new()
        .route("/script.js", get(tracker))
        .route("/recorder.js", get(recorder));
    for name in &config.tracker_script_names {
        assets = assets.route(&format!("/{name}"), get(tracker));
    }

    Router::new()
        .merge(api)
        .merge(assets)
        .route("/q/{slug}", get(link))
        .route("/p/{slug}", get(pixel))
        .with_state(state)
}

async fn tracker(State(state): State<AppState>) -> Response {
    script::respond(state.collector.tracker.clone())
}

async fn recorder() -> Response {
    script::respond(script::RECORDER.to_vec())
}

async fn preflight() -> StatusCode {
    StatusCode::NO_CONTENT
}

impl Collector {
    fn token(&self, headers: &HeaderMap) -> Option<Claims> {
        let value = headers.get("x-umami-cache")?.to_str().ok()?;
        self.tokens.verify(value)
    }

    pub(super) fn body_error(
        &self,
        route: CollectionRoute,
        error: BodyError,
        cap: usize,
        now: i64,
    ) -> Response {
        let (reason, response) = match error {
            BodyError::TooLarge { size } => {
                (OutcomeReason::PayloadTooLarge, wire::too_large(cap, size))
            }
            BodyError::Busy => (OutcomeReason::Busy, wire::refused(true)),
            BodyError::Aborted => (
                OutcomeReason::Invalid,
                Answer::BadRequest(BAD_REQUEST).into_response(),
            ),
        };
        self.metrics
            .outcome(route, CollectionOutcome::Refused, Some(reason), 1, now);
        response
    }

    /// Upstream's 404 for an unknown or deleted slug.
    fn unknown_slug(&self, route: CollectionRoute) -> Response {
        self.metrics.outcome(
            route,
            CollectionOutcome::Refused,
            Some(OutcomeReason::UnknownSource),
            1,
            self.clock.now_ms(),
        );
        wire::not_found()
    }

    /// The site of the request's collection key, `None` without one, or the 401 answer for a
    /// key that is empty, unknown or revoked.
    async fn key_site(
        &self,
        route: CollectionRoute,
        headers: &HeaderMap,
        now: i64,
    ) -> Result<Option<Uuid>, Box<Response>> {
        match self.trust(headers).await {
            Ok(Trust::Connection) => Ok(None),
            Ok(Trust::Site(site)) => Ok(Some(site)),
            Ok(Trust::Refused) => {
                self.metrics.outcome(
                    route,
                    CollectionOutcome::Refused,
                    Some(OutcomeReason::InvalidKey),
                    1,
                    now,
                );
                Err(Box::new(Answer::Unauthorized.into_response()))
            }
            Err(error) => Err(Box::new(internal(error))),
        }
    }

    pub(super) fn invalid(&self, route: CollectionRoute, now: i64) -> Answer {
        self.metrics.outcome(
            route,
            CollectionOutcome::Refused,
            Some(OutcomeReason::Invalid),
            1,
            now,
        );
        Answer::BadRequest(BAD_REQUEST)
    }
}

/// 500, or 503 with `Retry-After` when the failure is a full disk, which clears on its own
/// once space is freed (0005 Backpressure).
fn internal(error: anyhow::Error) -> Response {
    tracing::error!(error = %error, "collection failed");
    if disk_full(&error) {
        wire::refused(false)
    } else {
        wire::server_error()
    }
}

/// SQLite's `SQLITE_FULL` or the OS's ENOSPC anywhere in the error chain.
fn disk_full(error: &anyhow::Error) -> bool {
    const SQLITE_FULL: &str = "13";
    const ENOSPC: i32 = 28;
    error.chain().any(|cause| {
        if let Some(sqlx::Error::Database(db)) = cause.downcast_ref::<sqlx::Error>() {
            return db.code().is_some_and(|code| code == SQLITE_FULL);
        }
        cause
            .downcast_ref::<std::io::Error>()
            .is_some_and(|io| io.raw_os_error() == Some(ENOSPC))
    })
}

async fn send(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Body,
) -> Response {
    let c = &state.collector;
    let route = CollectionRoute::Send;
    let now = c.clock.now_ms();
    let intake = match c.budget.read(&headers, body, SEND_MAX_BYTES).await {
        Ok(intake) => intake,
        Err(error) => return c.body_error(route, error, SEND_MAX_BYTES, now),
    };
    let key_site = match c.key_site(route, &headers, now).await {
        Ok(site) => site,
        Err(response) => return *response,
    };
    let Some(Ok(item)) = std::str::from_utf8(&intake.bytes).ok().map(payload::parse) else {
        return c.invalid(route, now).into_response();
    };
    let client = Client::resolve(peer.ip(), &headers, &c.config, &c.geoip);
    let token = c.token(&headers);
    match c
        .prepare(&item, &client, key_site, token.as_ref(), now)
        .await
    {
        Err(error) => internal(error),
        Ok(Step::Answer(answer, (outcome, reason))) => {
            c.metrics.outcome(route, outcome, reason, 1, now);
            answer.into_response()
        }
        Ok(Step::Store {
            record,
            revocation,
            stored,
            accounting,
        }) => {
            let records: Vec<_> = std::iter::once(record).chain(revocation).collect();
            match c.admit(route, &records, &[accounting], now).await {
                Ok(_) => Answer::Stored(stored).into_response(),
                Err(refusal) => wire::refused(refusal == holocron_storage::Refusal::Busy),
            }
        }
    }
}

/// Upstream's `cache`: `null` until the first successful item, then that item's token, or
/// absent when it had none (a bot answer).
enum BatchCache {
    Null,
    Absent,
    Token(String),
}

async fn batch(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Body,
) -> Response {
    let c = &state.collector;
    let route = CollectionRoute::Batch;
    let now = c.clock.now_ms();
    let intake = match c.budget.read(&headers, body, BATCH_MAX_BYTES).await {
        Ok(intake) => intake,
        Err(error) => return c.body_error(route, error, BATCH_MAX_BYTES, now),
    };
    let key_site = match c.key_site(route, &headers, now).await {
        Ok(site) => site,
        Err(response) => return *response,
    };
    let items: Vec<&RawValue> = match std::str::from_utf8(&intake.bytes)
        .ok()
        .and_then(|text| serde_json::from_str(text).ok())
    {
        Some(items) => items,
        None => return c.invalid(route, now).into_response(),
    };
    if items.len() > MAX_BATCH_ITEMS
        || items
            .iter()
            .any(|item| !item.get().trim_start().starts_with('{'))
    {
        return c.invalid(route, now).into_response();
    }

    let client = Client::resolve(peer.ip(), &headers, &c.config, &c.geoip);
    let token = c.token(&headers);
    let mut answers: Vec<Option<Answer>> = vec![None; items.len()];
    let (mut records, mut accounting, mut pending) = (Vec::new(), Vec::new(), Vec::new());
    let mut revocations = Vec::new();
    for (index, raw) in items.iter().enumerate() {
        let item = match payload::parse(raw.get()) {
            Ok(item) => item,
            Err(_) => {
                answers[index] = Some(c.invalid(route, now));
                continue;
            }
        };
        match c
            .prepare(&item, &client, key_site, token.as_ref(), now)
            .await
        {
            Err(error) => return internal(error),
            Ok(Step::Answer(answer, (outcome, reason))) => {
                c.metrics.outcome(route, outcome, reason, 1, now);
                answers[index] = Some(answer);
            }
            Ok(Step::Store {
                record,
                revocation,
                stored,
                accounting: a,
            }) => {
                records.push(record);
                revocations.extend(revocation);
                accounting.push(a);
                pending.push((index, stored));
            }
        }
    }
    // One durable commit for every accepted item; if it fails, none is stored (0005).
    records.extend(revocations);
    if !records.is_empty()
        && let Err(refusal) = c.admit(route, &records, &accounting, now).await
    {
        return wire::refused(refusal == holocron_storage::Refusal::Busy);
    }
    for (index, stored) in pending {
        answers[index] = Some(Answer::Stored(stored));
    }

    let mut cache = BatchCache::Null;
    let mut details = Vec::new();
    for (index, answer) in answers.into_iter().enumerate() {
        let answer = answer.expect("every item answered");
        if answer.status().is_success() {
            if !matches!(cache, BatchCache::Token(_)) {
                cache = match &answer {
                    Answer::Stored(stored) => BatchCache::Token(stored.cache.clone()),
                    _ => BatchCache::Absent,
                };
            }
        } else {
            details.push(json!({ "index": index, "response": answer.body() }));
        }
    }
    let mut body = Map::new();
    body.insert("size".into(), json!(items.len()));
    body.insert("processed".into(), json!(items.len() - details.len()));
    body.insert("errors".into(), json!(details.len()));
    body.insert("details".into(), Value::Array(details));
    match cache {
        BatchCache::Null => {
            body.insert("cache".into(), Value::Null);
        }
        BatchCache::Token(token) => {
            body.insert("cache".into(), Value::String(token));
        }
        BatchCache::Absent => {}
    }
    Json(Value::Object(body)).into_response()
}

async fn link(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Path(slug): Path<String>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    let found: Result<Option<(String, String)>, sqlx::Error> =
        sqlx::query_as("SELECT id, url FROM links WHERE slug = ? AND deleted_at IS NULL")
            .bind(&slug)
            .fetch_optional(&state.db.reader)
            .await;
    let (id, target) = match found {
        Ok(Some(row)) => row,
        Ok(None) => return state.collector.unknown_slug(CollectionRoute::Link),
        Err(error) => return internal(error.into()),
    };
    hit(&state, CollectionRoute::Link, &id, peer, &headers, &uri).await;
    match HeaderValue::from_str(&target) {
        Ok(location) => (
            StatusCode::TEMPORARY_REDIRECT,
            [(header::LOCATION, location)],
        )
            .into_response(),
        Err(_) => internal(anyhow::anyhow!("link target is not a valid header value")),
    }
}

async fn pixel(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Path(slug): Path<String>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    let found: Result<Option<String>, sqlx::Error> =
        sqlx::query_scalar("SELECT id FROM pixels WHERE slug = ? AND deleted_at IS NULL")
            .bind(&slug)
            .fetch_optional(&state.db.reader)
            .await;
    let id = match found {
        Ok(Some(id)) => id,
        Ok(None) => return state.collector.unknown_slug(CollectionRoute::Pixel),
        Err(error) => return internal(error.into()),
    };
    hit(&state, CollectionRoute::Pixel, &id, peer, &headers, &uri).await;
    (
        [
            (header::CONTENT_TYPE, "image/gif"),
            (header::CACHE_CONTROL, "no-cache, no-store, must-revalidate"),
            (header::PRAGMA, "no-cache"),
            (header::EXPIRES, "0"),
        ],
        PIXEL.to_vec(),
    )
        .into_response()
}

/// Collects one link or pixel hit. The visitor gets the redirect or image whatever happens; a
/// hit that cannot be stored is counted in `lost_hits` (0005 Links and pixels).
async fn hit(
    state: &AppState,
    route: CollectionRoute,
    id: &str,
    peer: SocketAddr,
    headers: &HeaderMap,
    uri: &Uri,
) {
    let c = &state.collector;
    let now = c.clock.now_ms();
    let Ok(source_id) = Uuid::try_parse(id) else {
        tracing::error!("link or pixel has an invalid id");
        return;
    };
    let source = match route {
        CollectionRoute::Link => Source::Link(source_id),
        _ => Source::Pixel(source_id),
    };
    let origin = match &state.config.public_origin {
        Some(origin) => origin.clone(),
        None => format!(
            "http://{}",
            headers
                .get(header::HOST)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("localhost")
        ),
    };
    let item = Item::hit(
        source,
        format!("{origin}{uri}"),
        headers
            .get(header::REFERER)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned),
    );
    let client = Client::resolve(peer.ip(), headers, &c.config, &c.geoip);
    let lost = match c.prepare(&item, &client, None, None, now).await {
        Ok(Step::Answer(_, (outcome, reason))) => {
            c.metrics.outcome(route, outcome, reason, 1, now);
            false
        }
        Ok(Step::Store {
            record, accounting, ..
        }) => c.admit(route, &[record], &[accounting], now).await.is_err(),
        Err(error) => {
            tracing::error!(error = %error, "collection failed");
            c.metrics.outcome(
                route,
                CollectionOutcome::Refused,
                Some(OutcomeReason::Unavailable),
                1,
                now,
            );
            true
        }
    };
    if lost {
        let sql = match route {
            CollectionRoute::Link => "UPDATE links SET lost_hits = lost_hits + 1 WHERE id = ?",
            _ => "UPDATE pixels SET lost_hits = lost_hits + 1 WHERE id = ?",
        };
        // Best effort: when storage is down this write may fail too.
        let _ = sqlx::query(sql).bind(id).execute(&state.db.writer).await;
    }
}

impl Item<'static> {
    /// The send payload upstream builds for a link or pixel hit.
    fn hit(source: Source, url: String, referrer: Option<String>) -> Self {
        Item {
            kind: Kind::Event,
            source,
            data: None,
            hostname: None,
            language: None,
            screen: None,
            title: None,
            url: Some(url),
            referrer,
            name: None,
            tag: None,
            browser: None,
            os: None,
            device: None,
            ip: None,
            user_agent: None,
            timestamp_ms: None,
            distinct_id: None,
            event_id: None,
            consent: None,
            performance: Default::default(),
            fingerprint: 0,
        }
    }
}
