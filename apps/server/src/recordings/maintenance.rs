//! Recording upkeep beside request handling: 14-day replay removal, giving freed pages back,
//! folding settled heatmap interactions into cells, and 90-day heatmap removal. Every step is
//! a bounded transaction, so ingest and reads interleave with a long cleanup.

use std::time::{Duration, Instant};

use tokio::sync::watch;
use uuid::Uuid;

use super::{RETENTION_DAYS, heatmaps, replay_cutoff_ms};
use crate::sites::policy::session_policies;
use crate::state::AppState;

const INTERVAL: Duration = Duration::from_secs(5 * 60);
/// Whole captures removed per transaction.
const CAPTURES_PER_STEP: i64 = 32;
/// Pages returned to the file system per step, and steps per run.
const VACUUM_PAGES: u32 = 1024;
const VACUUM_STEPS: usize = 64;
/// Captures whose heatmap interactions fold per transaction.
const FOLD_CAPTURES: usize = 10_000;
const DAY_MS: i64 = 86_400_000;

/// What one run did, for logs and measurements.
#[derive(Debug, Default, Clone, Copy)]
pub struct Report {
    pub captures_removed: u64,
    pub removal_steps: u64,
    pub slowest_removal: Duration,
    pub heatmap_captures_folded: u64,
    pub fold_steps: u64,
    pub slowest_fold: Duration,
}

/// Runs now and then every five minutes until `stop` changes.
pub async fn follow(state: AppState, mut stop: watch::Receiver<bool>) {
    while !*stop.borrow() {
        if let Err(error) = run(&state).await {
            tracing::error!(error = %error, "recording maintenance failed; retrying later");
        }
        tokio::select! {
            _ = tokio::time::sleep(INTERVAL) => {}
            _ = stop.changed() => {}
        }
    }
}

pub async fn run(state: &AppState) -> anyhow::Result<Report> {
    let now = state.collector.clock().now_ms();
    let store = &state.recordings.store;
    let mut report = Report::default();

    // No timezone puts the cutoff later than one day after UTC's.
    for site in store
        .sites_before(now - (RETENTION_DAYS as i64 - 1) * DAY_MS)
        .await?
    {
        let key = Uuid::from_u128(site).hyphenated().to_string();
        // A site that no longer exists loses every capture, saved ones too (0006).
        let (cutoff, saved) = match session_policies(&state.db.reader, &key).await? {
            Some(policies) => (
                replay_cutoff_ms(policies.at(now).policy.timezone(), now),
                false,
            ),
            None => (i64::MAX, true),
        };
        loop {
            let started = Instant::now();
            let removed = store.expire(site, cutoff, saved, CAPTURES_PER_STEP).await?;
            report.slowest_removal = report.slowest_removal.max(started.elapsed());
            report.captures_removed += removed;
            report.removal_steps += 1;
            if removed < CAPTURES_PER_STEP as u64 {
                break;
            }
            tokio::task::yield_now().await;
        }
    }
    for _ in 0..VACUUM_STEPS {
        if !store.vacuum(VACUUM_PAGES).await? {
            break;
        }
    }

    let storage = state.collector.storage();
    loop {
        let started = Instant::now();
        let folded = heatmaps::fold(storage, now - heatmaps::SETTLE_MS, FOLD_CAPTURES).await?;
        report.slowest_fold = report.slowest_fold.max(started.elapsed());
        report.heatmap_captures_folded += folded as u64;
        report.fold_steps += 1;
        if folded < FOLD_CAPTURES {
            break;
        }
    }
    heatmaps::expire(storage, now).await?;
    if report.captures_removed > 0 || report.heatmap_captures_folded > 0 {
        tracing::info!(
            captures_removed = report.captures_removed,
            heatmap_captures_folded = report.heatmap_captures_folded,
            "recording maintenance"
        );
    }
    Ok(report)
}
