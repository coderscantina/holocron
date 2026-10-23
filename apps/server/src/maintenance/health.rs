//! Liveness and collection readiness (CAP-OPS-02). `/health` answers while the process
//! serves. `/health/ready` answers 200 while collection can store events and 503 when it
//! cannot, and names every problem, so a full disk, a failing transfer, stale analytics and a
//! recorder at its budget read differently. Problem names only: no counts, paths or bodies.

use std::time::Duration;

use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json};
use axum::routing::get;
use holocron_protocol::operations::analytics::Freshness;
use holocron_protocol::operations::operator::ReadinessProblem;
use holocron_storage::{Storage, Watermark};
use serde::Serialize;
use serde_json::json;

use crate::state::AppState;

/// Analytics this far behind the oldest waiting record is stale. Visibility normally takes
/// under 2 s (performance validation); a minute means transfer is stuck or overloaded.
pub const STALE_AFTER: Duration = Duration::from_secs(60);
/// Recording counts as near its budget from this share of the cap.
const RECORDING_NEAR_CAP_PERCENT: u64 = 90;

#[derive(Debug, Clone, Serialize)]
pub struct Health {
    pub ready: bool,
    pub problems: Vec<ReadinessProblem>,
}

pub async fn check(state: &AppState) -> Health {
    let storage = state.collector.storage();
    let now = state.collector.clock().now_ms();
    let mut problems = Vec::new();
    if state.maintenance.stopping() {
        problems.push(ReadinessProblem::ShuttingDown);
    }
    match storage.disk_headroom() {
        Ok((free, reserve)) if free < reserve => problems.push(ReadinessProblem::DiskFull),
        Ok(_) if state.collector.metrics().recent_refusals(now).1 => {
            problems.push(ReadinessProblem::StorageUnavailable)
        }
        Ok(_) => {}
        Err(_) => problems.push(ReadinessProblem::StorageUnavailable),
    }
    if storage.status().transfer_failing_since_ms.is_some() {
        problems.push(ReadinessProblem::TransferFailing);
    } else if storage
        .inbox_age()
        .await
        .map_or(true, |age| age > STALE_AFTER)
    {
        problems.push(ReadinessProblem::AnalyticsStale);
    }
    let recordings = &state.recordings;
    let near_cap = recordings.store.usage().await.map_or(true, |usage| {
        usage.bytes.max(0) as u64 * 100 >= recordings.cap_bytes() * RECORDING_NEAR_CAP_PERCENT
    });
    if recordings.refusing(now) || near_cap {
        problems.push(ReadinessProblem::RecorderBudget);
    }
    Health {
        ready: !problems.iter().any(|p| p.blocks_collection()),
        problems,
    }
}

/// Sets how far a result read at `watermark` is behind collection: `watermark_at` is the
/// receive time of the oldest record it lacks, moved onto the application clock at `now_ms`,
/// and `stale` follows the `analytics_stale` rule.
pub async fn mark_behind(
    storage: &Storage,
    watermark: Watermark,
    now_ms: i64,
    freshness: &mut Freshness,
) -> Result<(), holocron_storage::Error> {
    let age = storage.inbox_age_after(watermark).await?;
    freshness.watermark_at = now_ms - age.as_millis() as i64;
    freshness.stale = age > STALE_AFTER;
    Ok(())
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health/ready", get(ready))
        .with_state(state)
}

async fn ready(State(state): State<AppState>) -> impl IntoResponse {
    let health = check(&state).await;
    let status = if health.ready {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (
        status,
        Json(json!({
            "status": if health.ready { "ready" } else { "unavailable" },
            "problems": health.problems,
        })),
    )
}
