//! `POST /api/record` and `GET /api/websites/{id}/recorder`: the unchanged recorder's
//! configuration handshake, replay parts and heatmap payloads (collection compatibility
//! contract, "POST /api/record").
//!
//! Order as upstream: size, schema, empty events, token, website, recording policy, bot, IP
//! block, kind policy, write. The token is bound to its website (0005). A replay part is
//! acknowledged only after its commit in the replay store; heatmap interactions only after
//! the inbox commit. A full recording cap answers `replay_budget_exceeded` while every other
//! collection route keeps working.

use std::net::SocketAddr;

use axum::Json;
use axum::body::Body;
use axum::extract::{ConnectInfo, Path, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use holocron_protocol::operations::collection::{
    CollectionOutcome, CollectionRoute, OutcomeReason,
};
use serde_json::{Value, json};
use uuid::Uuid;

use super::client::{Client, classify};
use super::wire::{self, Answer, BAD_REQUEST};
use crate::recordings::replay::{self, Summary};
use crate::recordings::store::{self, Part, Stored};
use crate::recordings::wire::{Payload, RECORD_MAX_BYTES};
use crate::recordings::{heatmaps, policy};
use crate::state::AppState;

const ROUTE: CollectionRoute = CollectionRoute::Record;
const ZSTD_LEVEL: i32 = 3;

/// A request body validated off the async workers. Replay events are re-joined as sent.
enum Prepared {
    Empty,
    Replay {
        website: Uuid,
        chunk: Option<i64>,
        summary: Summary,
        json: String,
    },
    Heatmap {
        website: Uuid,
        events: Vec<crate::recordings::wire::HeatmapEvent>,
    },
}

fn prepare(bytes: &[u8]) -> Option<Prepared> {
    let request = crate::recordings::wire::parse(std::str::from_utf8(bytes).ok()?).ok()?;
    if request.payload.is_empty() {
        return Some(Prepared::Empty);
    }
    Some(match request.payload {
        Payload::Replay(events) => {
            if replay::depth(bytes) > replay::MAX_DEPTH {
                return None;
            }
            let summary = replay::summarize(&events).ok()?;
            let joined: Vec<&str> = events.iter().map(|event| event.get()).collect();
            Prepared::Replay {
                website: request.website,
                chunk: request.timestamp,
                summary,
                json: format!("[{}]", joined.join(",")),
            }
        }
        Payload::Heatmap(events) => Prepared::Heatmap {
            website: request.website,
            events,
        },
    })
}

fn ok() -> Response {
    Json(json!({ "ok": true })).into_response()
}

fn declined(reason: &str) -> Response {
    Json(json!({ "ok": false, "reason": reason })).into_response()
}

pub async fn record(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Body,
) -> Response {
    let c = &state.collector;
    let now = c.clock.now_ms();
    let intake = match c.budget.read(&headers, body, RECORD_MAX_BYTES).await {
        Ok(intake) => intake,
        Err(error) => return c.body_error(ROUTE, error, RECORD_MAX_BYTES, now),
    };
    let bytes = intake.bytes.clone();
    let prepared = tokio::task::spawn_blocking(move || prepare(&bytes))
        .await
        .ok()
        .flatten();
    let count = |outcome, reason| c.metrics.outcome(ROUTE, outcome, reason, 1, now);
    let refuse = |message: &'static str, reason| {
        count(CollectionOutcome::Refused, Some(reason));
        Answer::BadRequest(message).into_response()
    };
    let (website, prepared) = match prepared {
        None => return refuse(BAD_REQUEST, OutcomeReason::Invalid),
        Some(Prepared::Empty) => return ok(),
        Some(p @ (Prepared::Replay { website, .. } | Prepared::Heatmap { website, .. })) => {
            (website, p)
        }
    };

    let Some(token) = headers.get("x-umami-cache") else {
        return refuse("Missing session token.", OutcomeReason::Invalid);
    };
    let Some(claims) = token.to_str().ok().and_then(|value| c.tokens.verify(value)) else {
        return refuse("Invalid session token.", OutcomeReason::Invalid);
    };
    let site = website.hyphenated().to_string();
    let exists = sqlx::query_scalar::<_, i64>("SELECT 1 FROM sites WHERE id = ?")
        .bind(&site)
        .fetch_optional(&c.db.reader)
        .await;
    match exists {
        Ok(Some(_)) => {}
        Ok(None) => return refuse("Website not found.", OutcomeReason::UnknownSource),
        Err(error) => return internal(error),
    }
    if claims.website_id != Some(website) {
        return refuse("Invalid session token.", OutcomeReason::Invalid);
    }
    let policy = match policy::load(&c.db.reader, &site).await {
        Ok(policy) => policy,
        Err(error) => return internal(error),
    };
    let disabled = |reason: &str| {
        count(
            CollectionOutcome::Suppressed,
            Some(OutcomeReason::RecordingDisabled),
        );
        declined(reason)
    };
    if !policy.replay_enabled && !policy.heatmap_enabled {
        return disabled("recorder_disabled");
    }
    let client = Client::resolve(peer.ip(), &headers, &c.config, &c.geoip);
    if !client.user_agent.is_empty() && c.bots.is_bot(&client.user_agent) {
        count(CollectionOutcome::Suppressed, Some(OutcomeReason::Bot));
        return Answer::Bot.into_response();
    }
    if client.blocked(&c.config) {
        count(
            CollectionOutcome::Suppressed,
            Some(OutcomeReason::IpBlocked),
        );
        return Answer::Forbidden.into_response();
    }

    let site_id = website.as_u128();
    let capture = claims.visit_id.as_u128();
    match prepared {
        Prepared::Empty => ok(),
        Prepared::Replay {
            chunk,
            summary,
            json,
            ..
        } => {
            if !policy.replay_enabled {
                return disabled("replay_disabled");
            }
            if c.storage
                .disk_headroom()
                .is_ok_and(|(free, reserve)| free < reserve)
            {
                count(CollectionOutcome::Refused, Some(OutcomeReason::Unavailable));
                return wire::refused(false);
            }
            let raw_bytes = json.len() as i64;
            let compressed = tokio::task::spawn_blocking(move || {
                let fingerprint: [u8; 16] = blake3::hash(json.as_bytes()).as_bytes()[..16]
                    .try_into()
                    .expect("16 bytes");
                zstd::bulk::compress(json.as_bytes(), ZSTD_LEVEL)
                    .map(|payload| (payload, fingerprint))
            })
            .await;
            let Ok(Ok((payload, fingerprint))) = compressed else {
                return internal("replay part did not compress");
            };
            let (browser, os, device) = classify(&client.user_agent, None);
            let source = store::Client {
                browser,
                os,
                device,
                country: client.geo.country.clone(),
            };
            let part = Part {
                site: site_id,
                capture,
                visitor: Some(claims.session_id.as_u128()),
                chunk: chunk.unwrap_or(now.div_euclid(1000)),
                received_at: now,
                summary: &summary,
                raw_bytes,
                payload,
                fingerprint,
                client: &source,
            };
            let recordings = &state.recordings;
            match recordings
                .store
                .put(part, &policy.excluded_paths, recordings.cap_bytes())
                .await
            {
                Ok(Stored::Accepted) => {
                    count(CollectionOutcome::Accepted, None);
                    ok()
                }
                Ok(Stored::Duplicate) => {
                    count(CollectionOutcome::Duplicate, None);
                    ok()
                }
                Ok(Stored::Excluded) => {
                    count(
                        CollectionOutcome::Suppressed,
                        Some(OutcomeReason::ExcludedPage),
                    );
                    declined("page_excluded")
                }
                Ok(Stored::OverCap) => {
                    recordings.refused_at(now);
                    count(
                        CollectionOutcome::Suppressed,
                        Some(OutcomeReason::ReplayBudget),
                    );
                    declined("replay_budget_exceeded")
                }
                Err(error) => {
                    tracing::error!(error = %error, "replay part not stored");
                    count(CollectionOutcome::Refused, Some(OutcomeReason::Unavailable));
                    wire::refused(false)
                }
            }
        }
        Prepared::Heatmap { events, .. } => {
            if !policy.heatmap_enabled {
                return disabled("heatmap_disabled");
            }
            let (records, excluded) =
                heatmaps::records(site_id, capture, &events, &policy.excluded_paths, now);
            if records.is_empty() {
                let reason = (excluded > 0).then_some(OutcomeReason::ExcludedPage);
                count(CollectionOutcome::Suppressed, reason);
                return ok();
            }
            match c.storage.admit(&records).await {
                Ok(_) => {
                    count(CollectionOutcome::Accepted, None);
                    ok()
                }
                Err(refusal) => {
                    let busy = refusal == holocron_storage::Refusal::Busy;
                    let reason = if busy {
                        OutcomeReason::Busy
                    } else {
                        OutcomeReason::Unavailable
                    };
                    count(CollectionOutcome::Refused, Some(reason));
                    wire::refused(busy)
                }
            }
        }
    }
}

/// The recorder's configuration: `{enabled: false}` for a site without recording, an unknown
/// site, or an ID that is not a UUID, so the answer does not reveal which sites exist.
pub async fn recorder_config(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let disabled = || Json(json!({ "enabled": false })).into_response();
    let Ok(website) = Uuid::try_parse(&id) else {
        return disabled();
    };
    match policy::load(
        &state.collector.db.reader,
        &website.hyphenated().to_string(),
    )
    .await
    {
        Ok(policy) => Json::<Value>(policy::recorder_config(&policy)).into_response(),
        Err(error) => internal(error),
    }
}

fn internal(error: impl std::fmt::Display) -> Response {
    tracing::error!(error = %error, "recording collection failed");
    wire::server_error()
}
