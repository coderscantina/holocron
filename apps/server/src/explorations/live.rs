//! `GET /api/subscribe?topic=analytics:<site>&cursor=analytics:<site>@<batch>`: tells a realtime
//! screen when newer analytical data is visible, as server-sent events of [`LiveFrame`]s.
//!
//! The topic's log is the applied-batch ledger: a cursor is the watermark batch of the snapshot
//! on screen. A cursor behind the head gets one `events` frame at once, so a reconnect resumes
//! instead of refetching; a cursor ahead of the head (a restored store) is `expired`. A
//! `summary` event says the site's summary was computed again ([`crate::summaries`]). It moves
//! no cursor and is not replayed, so a client reads the summary again after a reconnect.
//! Frames are coalesced to one per second. The session and the site scope are resolved again
//! before every frame and every 5 seconds while idle; when access is gone the stream ends with
//! `access_revoked`, and when it cannot be checked with `resync` and reason `unavailable`. The credential is the session cookie or an
//! `Authorization` header (API key or share token); browsers send the header through a
//! fetch-based reader, since `EventSource` cannot set headers.
//!
//! Server-sent events instead of the pattern's WebSocket: delivery is one-way, browsers resume
//! natively, and a cross-site page cannot read the response, so no Origin check is needed.

use std::collections::VecDeque;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::time::Duration;

use axum::extract::{ConnectInfo, Request, State};
use axum::http::HeaderMap;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use holocron_protocol::access::Capability;
use holocron_protocol::errors::ErrorCode;
use holocron_protocol::ids::SiteId;
use holocron_protocol::operations::explorations::{
    LiveEvent, LiveEventKind, LiveFrame, ResyncReason,
};
use holocron_storage::Watermark;
use serde::Deserialize;
use tokio::sync::broadcast::{self, error::RecvError};
use tokio::sync::watch;
use tokio::time::Instant;

use crate::error::ApiError;
use crate::identity::{self, Caller};
use crate::sites::scope;
use crate::state::AppState;

const PREFIX: &str = "analytics:";
/// Authorization is checked again at least this often while no frame is sent.
const RECHECK: Duration = Duration::from_secs(5);
/// At most one `events` frame per interval; a batch of changes reloads once.
const COALESCE: Duration = Duration::from_secs(1);

pub fn topic(site: &SiteId) -> String {
    format!("{PREFIX}{site}")
}

pub fn cursor(topic: &str, batch: i64) -> String {
    format!("{topic}@{batch}")
}

#[derive(Deserialize)]
struct Params {
    topic: String,
    cursor: Option<String>,
}

/// The site of an `analytics:<site>` topic and the batch of a cursor on that topic. Parsed
/// strictly: a cursor of another topic is refused.
fn parse(params: &Params) -> Option<(SiteId, Option<i64>)> {
    let site = params
        .topic
        .strip_prefix(PREFIX)
        .filter(|s| !s.is_empty())?;
    let batch = match &params.cursor {
        None => None,
        Some(cursor) => {
            let (topic, batch) = cursor.rsplit_once('@')?;
            if topic != params.topic {
                return None;
            }
            Some(batch.parse::<i64>().ok().filter(|b| *b >= 0)?)
        }
    };
    Some((SiteId::new(site), batch))
}

pub async fn subscribe(State(state): State<AppState>, request: Request) -> Response {
    let (parts, _) = request.into_parts();
    let address = parts
        .extensions
        .get::<ConnectInfo<SocketAddr>>()
        .map(|info| info.0.ip().to_string())
        .unwrap_or_else(|| "unknown".into());
    let params: Option<Params> =
        serde_html_form::from_str(parts.uri.query().unwrap_or_default()).ok();
    let Some((site, cursor)) = params.as_ref().and_then(parse) else {
        return ApiError::with_message(
            ErrorCode::MalformedRequest,
            "expected topic=analytics:<site> and a cursor on it",
        )
        .into_response();
    };
    let headers = parts.headers;
    if let Err(error) = authorize(&state, &headers, &address, &site).await {
        return error.into_response();
    }

    let topic = topic(&site);
    let watermark = state.collector.storage().watermark();
    let head = watermark.borrow().batch;
    let mut queue = VecDeque::from([LiveFrame::Subscribed {
        head: self::cursor(&topic, head),
        cursor: self::cursor(&topic, cursor.unwrap_or(head)),
    }]);
    let mut sent = cursor.unwrap_or(head);
    match cursor {
        Some(batch) if batch > head => {
            queue.push_back(LiveFrame::Expired {
                head: self::cursor(&topic, head),
            });
        }
        Some(batch) if batch < head => {
            queue.push_back(events(&topic, head));
            sent = head;
        }
        _ => {}
    }
    let expired = cursor.is_some_and(|batch| batch > head);
    let live = Live {
        summaries: state.summaries.subscribe(),
        summary: None,
        state,
        headers,
        address,
        site,
        topic,
        watermark,
        sent,
        last_frame: Instant::now(),
        queue,
        done: expired,
    };
    let stream = futures_util::stream::unfold(live, |mut live| async move {
        let frame = live.next().await?;
        let event = Event::default()
            .json_data(&frame)
            .expect("frames serialize");
        Some((Ok::<_, Infallible>(event), live))
    });
    Sse::new(stream)
        .keep_alive(KeepAlive::default())
        .into_response()
}

/// Resolves the credential and the site scope again. Persons need a full session; API keys and
/// share viewers need the site inside their grants. The watermark topic carries no analytics,
/// only that newer data is visible, so a key needs no operation grant for it (decision 0012).
async fn authorize(
    state: &AppState,
    headers: &HeaderMap,
    address: &str,
    site: &SiteId,
) -> Result<(), ApiError> {
    let caller = identity::resolve_caller(&state.db, headers, address.to_owned()).await?;
    match &caller {
        Caller::Anonymous { .. } => return Err(ApiError::new(ErrorCode::Unauthenticated)),
        Caller::Person {
            enrollment_only: true,
            ..
        } => return Err(ApiError::new(ErrorCode::SecondFactorEnrollmentRequired)),
        _ => {}
    }
    let mut conn = state.db.reader.acquire().await?;
    scope::resolve_for(&mut conn, &caller, site)
        .await?
        .require(Capability::SiteRead)
}

/// Whether a failed check is an access decision rather than a failure to decide.
fn revoked(error: &ApiError) -> bool {
    matches!(
        error.code(),
        ErrorCode::NotFound
            | ErrorCode::Unauthenticated
            | ErrorCode::Forbidden
            | ErrorCode::SecondFactorEnrollmentRequired
    )
}

fn events(topic: &str, batch: i64) -> LiveFrame {
    LiveFrame::Events {
        cursor: cursor(topic, batch),
        events: vec![LiveEvent {
            seq: batch,
            kind: LiveEventKind::Watermark,
        }],
    }
}

struct Live {
    state: AppState,
    headers: HeaderMap,
    address: String,
    site: SiteId,
    topic: String,
    watermark: watch::Receiver<Watermark>,
    /// The last batch a frame named.
    sent: i64,
    summaries: broadcast::Receiver<(SiteId, i64)>,
    /// The batch of a summary change not sent yet.
    summary: Option<i64>,
    last_frame: Instant,
    queue: VecDeque<LiveFrame>,
    done: bool,
}

impl Live {
    async fn next(&mut self) -> Option<LiveFrame> {
        if let Some(frame) = self.queue.pop_front() {
            self.last_frame = Instant::now();
            return Some(frame);
        }
        if self.done {
            return None;
        }
        loop {
            tokio::select! {
                changed = self.watermark.changed() => {
                    // Storage is closing: end the stream; the client reconnects.
                    changed.ok()?;
                    tokio::time::sleep_until(self.last_frame + COALESCE).await;
                }
                changed = self.summaries.recv() => {
                    let batch = match changed {
                        Ok((site, batch)) => (site == self.site).then_some(batch),
                        // Missed names may include this site's.
                        Err(RecvError::Lagged(_)) => Some(self.sent),
                        Err(RecvError::Closed) => return None,
                    };
                    if batch.is_some() {
                        self.summary = batch;
                        tokio::time::sleep_until(self.last_frame + COALESCE).await;
                    }
                }
                _ = tokio::time::sleep(RECHECK) => {}
            }
            if let Err(error) =
                authorize(&self.state, &self.headers, &self.address, &self.site).await
            {
                self.done = true;
                return Some(if revoked(&error) {
                    LiveFrame::AccessRevoked
                } else {
                    LiveFrame::Resync {
                        cursor: cursor(&self.topic, self.sent),
                        reason: ResyncReason::Unavailable,
                    }
                });
            }
            let head = self.watermark.borrow_and_update().batch;
            let mut events = Vec::new();
            if head > self.sent {
                self.sent = head;
                events.push(LiveEvent {
                    seq: head,
                    kind: LiveEventKind::Watermark,
                });
            }
            if let Some(batch) = self.summary.take() {
                events.push(LiveEvent {
                    seq: batch,
                    kind: LiveEventKind::Summary,
                });
            }
            if !events.is_empty() {
                self.last_frame = Instant::now();
                return Some(LiveFrame::Events {
                    cursor: cursor(&self.topic, self.sent),
                    events,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Params, parse};

    #[test]
    fn cursors_belong_to_their_topic() {
        let params = |topic: &str, cursor: Option<&str>| Params {
            topic: topic.into(),
            cursor: cursor.map(Into::into),
        };
        assert_eq!(
            parse(&params("analytics:s1", Some("analytics:s1@7")))
                .unwrap()
                .1,
            Some(7)
        );
        assert!(parse(&params("analytics:s1", Some("analytics:s2@7"))).is_none());
        assert!(parse(&params("site:s1", None)).is_none());
        assert!(parse(&params("analytics:s1", Some("analytics:s1@-1"))).is_none());
    }
}
