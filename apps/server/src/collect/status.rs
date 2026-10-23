//! `collection.status`: what an operator needs to see whether collection is healthy.

use holocron_protocol::access::InstallRole;
use holocron_protocol::envelope::{Empty, Topic};
use holocron_protocol::errors::ErrorCode;
use holocron_protocol::operations::collection::{
    Availability, ClockMode, CollectionHandlers, CollectionStatus,
};
use holocron_protocol::registry::Snapshot;

use crate::context::QueryCtx;
use crate::error::ApiError;
use crate::identity;
use crate::state::AppState;

impl CollectionHandlers for AppState {
    async fn collection_status(
        &self,
        ctx: &mut QueryCtx,
        _args: Empty,
    ) -> Result<Snapshot<CollectionStatus>, ApiError> {
        let user = ctx.person()?;
        let account = identity::account(ctx.conn(), &user).await?;
        if account.is_none_or(|a| a.install_role != InstallRole::InstallAdmin) {
            return Err(ApiError::new(ErrorCode::Forbidden));
        }

        Ok(Snapshot {
            topic: Topic::installation(),
            data: snapshot(self).await?,
        })
    }
}

/// The status itself, which `operator.status` includes. Callers check the install admin role.
pub(crate) async fn snapshot(state: &AppState) -> Result<CollectionStatus, ApiError> {
    let c = &state.collector;
    let now = c.clock.now_ms();
    let storage = c.storage();
    let status = storage.status();
    let lag = storage.inbox_age().await.map_err(ApiError::internal)?;
    let (free, reserve) = storage.disk_headroom().map_err(ApiError::internal)?;
    let (busy, unavailable) = c.metrics.recent_refusals(now);
    let availability = if unavailable || free < reserve {
        Availability::Unavailable
    } else if busy {
        Availability::Busy
    } else {
        Availability::Available
    };
    Ok(CollectionStatus {
        availability,
        since: c.metrics.since(),
        clock: if c.clock.is_test() {
            ClockMode::Test
        } else {
            ClockMode::System
        },
        inbox_records: status.inbox_records as i64,
        inbox_bytes: status.inbox_bytes as i64,
        processing_lag_ms: lag.as_millis() as i64,
        watermark_batch: status.watermark.batch,
        watermark_sequence: status.watermark.sequence,
        disk_free_bytes: free.min(i64::MAX as u64) as i64,
        disk_reserve_bytes: reserve.min(i64::MAX as u64) as i64,
        outcomes: c.metrics.outcomes(),
        counters: c.metrics.counters(),
    })
}
