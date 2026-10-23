//! Site clustering of closed months (0008 Analytical layout; [`Storage::compact`]), run beside
//! collection. A pass starts shortly after the server starts and then hourly, and moves one
//! slice per step until every closed month is clustered.
//!
//! Bounds: a step moves at most [`STEP_EVENTS`] events with their properties in one analytical
//! transaction, so transfer, correction, backups and erasure wait at most one step for the
//! writer, and the step's sort stays far below the engine's memory budget. Collection never
//! waits: admission only needs the inbox. A pass stops early while transfer lags behind or
//! free space is short, and leaves a pause between steps so transfer and reads get their turn.

use std::time::Duration;

use holocron_storage::{Error, Storage};
use tokio::sync::watch;

use super::CompactionPass;
use crate::state::AppState;

/// Events moved per step, with their property and identify rows.
pub const STEP_EVENTS: u64 = 250_000;
const FIRST_PASS: Duration = Duration::from_secs(10 * 60);
const EVERY: Duration = Duration::from_secs(60 * 60);
const PAUSE: Duration = Duration::from_millis(500);
/// A month counts as closed one day after it ended, so late events still land before it is
/// clustered.
const SETTLE_MS: i64 = 86_400_000;
/// A pass stops while the oldest record in the inbox waits longer than this.
const TRANSFER_LAG: Duration = Duration::from_secs(10);

/// Whether transfer keeps up: no record waits in the inbox longer than [`TRANSFER_LAG`].
/// Background work that takes engine time yields while it does not.
pub async fn transfer_current(storage: &Storage) -> Result<bool, Error> {
    Ok(storage.inbox_age().await? <= TRANSFER_LAG)
}

pub async fn follow(state: AppState, mut stop: watch::Receiver<bool>) {
    let mut wait = FIRST_PASS;
    loop {
        tokio::select! {
            _ = tokio::time::sleep(wait) => {}
            _ = stop.changed() => return,
        }
        wait = EVERY;
        let now = state.collector.clock().now_ms();
        let result = run(state.collector.storage(), now, &stop).await;
        state.maintenance.compacted(CompactionPass {
            started_at: now,
            finished_at: state.collector.clock().now_ms(),
            failed: result.is_err(),
        });
        match result {
            Ok(events) if events > 0 => tracing::info!(events, "compaction"),
            Ok(_) => {}
            Err(error) => tracing::error!(error = %error, "compaction failed; retrying next hour"),
        }
    }
}

/// Runs steps until every month closed at `now_ms` is clustered, `stop` changes, or a bound
/// says wait. Returns the events moved.
pub async fn run(
    storage: &Storage,
    now_ms: i64,
    stop: &watch::Receiver<bool>,
) -> Result<u64, Error> {
    let mut moved = 0;
    while !*stop.borrow() && room(storage).await? {
        let Some(step) = storage.compact(now_ms - SETTLE_MS, STEP_EVENTS).await? else {
            break;
        };
        moved += step.events;
        tokio::time::sleep(PAUSE).await;
    }
    Ok(moved)
}

/// Whether a step may run now. A step writes its slice before the rows it replaces free their
/// blocks, and a month's old row groups are only freed once all its slices moved, so a pass
/// needs about one month of space. A month of a retained year is under a tenth of the analytics
/// file; a quarter above the disk reserve leaves room for an uneven month.
async fn room(storage: &Storage) -> Result<bool, Error> {
    if !transfer_current(storage).await? {
        return Ok(false);
    }
    let disk = storage.disk()?;
    let needed = disk.reserve_bytes + disk.analytics_bytes / 4;
    if disk.free_bytes < needed {
        tracing::warn!(
            free = disk.free_bytes,
            needed,
            "compaction waits for free disk space"
        );
        return Ok(false);
    }
    Ok(true)
}
