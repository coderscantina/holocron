//! Retention (0006 Retention cutoff): analytical records after each source's retention (0016,
//! 12 calendar months by default) by the site-local date, identity day-key tombstones with
//! their days, client event IDs after their 48-hour window, raw replay after 14 days, heatmaps
//! after 90 days and storage samples after 400 days. Queries already hide what lies before a
//! cutoff; this job gives the space back. One bounded transaction per site.

use std::time::{Duration, Instant};

use chrono::TimeZone;
use chrono_tz::Tz;
use holocron_analytics::identity::days_since_epoch;
use serde_json::{Value, json};
use uuid::Uuid;

use super::{internal, samples};
use crate::init::Failure;
use crate::retention;
use crate::state::AppState;

/// Finished jobs are kept this long for diagnostics.
const JOB_HISTORY_MS: i64 = 30 * 86_400_000;

pub async fn run(state: &AppState) -> Result<Value, Failure> {
    let now = state.collector.clock().now_ms();
    let storage = state.collector.storage();
    let mut events = 0;
    let mut day_keys = 0;
    let mut slowest = Duration::ZERO;
    let (default, sources) = {
        let mut conn = state.db.reader.acquire().await.map_err(internal)?;
        let default = retention::installation_default(&mut *conn)
            .await
            .map_err(internal)?;
        (
            default,
            retention::sources(&mut conn).await.map_err(internal)?,
        )
    };
    for source in &sources {
        let key = &source.id;
        let policies = &source.policies;
        let Some(cutoff) = policies.retention_cutoff_ms(now, source.retention(default)) else {
            continue;
        };
        let site = Uuid::parse_str(key).map_err(internal)?.as_u128();
        let cutoff_day = local_day(policies.at(cutoff).policy.timezone(), cutoff);
        let started = Instant::now();
        events += storage
            .expire(site, cutoff, cutoff_day)
            .await
            .map_err(internal)?;
        slowest = slowest.max(started.elapsed());
        day_keys += sqlx::query(
            "DELETE FROM identity_day_keys WHERE site_id = ? AND key IS NULL AND local_day < ?",
        )
        .bind(key)
        .bind(cutoff_day)
        .execute(&state.db.writer)
        .await
        .map_err(internal)?
        .rows_affected();
    }
    let event_ids = storage.prune_event_ids(now).await.map_err(internal)?;
    let recordings = crate::recordings::maintenance::run(state)
        .await
        .map_err(internal)?;
    sqlx::query(
        "DELETE FROM maintenance_jobs WHERE state IN ('done', 'failed') AND finished_at < ?",
    )
    .bind(now - JOB_HISTORY_MS)
    .execute(&state.db.writer)
    .await
    .map_err(internal)?;
    sqlx::query("DELETE FROM storage_samples WHERE taken_at < ?")
        .bind(now - samples::KEEP_MS)
        .execute(&state.db.writer)
        .await
        .map_err(internal)?;
    tracing::info!(
        sources = sources.len(),
        events,
        captures = recordings.captures_removed,
        "retention"
    );
    Ok(json!({
        "sources": sources.len(),
        "events_removed": events,
        "slowest_source_ms": slowest.as_millis() as u64,
        "day_key_tombstones_removed": day_keys,
        "event_ids_pruned": event_ids,
        "captures_removed": recordings.captures_removed,
        "heatmap_captures_folded": recordings.heatmap_captures_folded,
    }))
}

/// The local date of `ms` in `tz`, in days since 1970-01-01.
fn local_day(tz: Tz, ms: i64) -> i32 {
    let date = tz
        .timestamp_millis_opt(ms)
        .single()
        .expect("UTC instants map to one local time")
        .date_naive();
    days_since_epoch(date)
}
