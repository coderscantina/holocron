//! Ordered erasure across the engines (briefing, Reliability strategy; decision 0014):
//!
//! 1. Fence: from this commit on, transfer leaves out every record the erasure covers, and
//!    imports check the same fence.
//! 2. Replays and raw heatmap interactions in `recordings.sqlite` and DuckDB.
//! 3. Analytics: canonical rows, derived rows, heatmap cells.
//! 4. Control rows the erasure covers: day keys and import inventory of a deleted site, the
//!    inventoried history of a reset site, the summary and findings of either, consents and
//!    imported identity links of a profile.
//! 5. Done once transfer moved past every record admitted before the fence, so none of them
//!    is left in the inbox either.
//!
//! Every step finds nothing to do when it already ran, so an interrupted job starts over.

use holocron_storage::{Erasure, Fence, Lane, duckdb};
use serde_json::{Value, json};
use uuid::Uuid;

use super::internal;
use crate::imports;
use crate::init::Failure;
use crate::state::AppState;
use crate::summaries;

/// Captures removed per replay store transaction.
const CAPTURES_PER_STEP: i64 = 256;

/// Runs the erasure for job `job`, fencing records admitted up to `through_seq` or up to now.
/// `None` while records the fence covers still wait for transfer; the job runs again later.
pub async fn run(
    state: &AppState,
    job: &str,
    erasure: Erasure,
    through_seq: Option<i64>,
) -> Result<Option<Value>, Failure> {
    let storage = state.collector.storage();
    let fence = storage
        .fence(job, erasure, through_seq)
        .await
        .map_err(internal)?;
    let site = erasure.site();
    let mut captures = remove_replays(state, fence).await?;
    let erased = storage.erase(fence).await.map_err(internal)?;
    if let Erasure::Site(_) | Erasure::SiteHistory(_) = erasure {
        storage
            .write(move |tx| tx.execute("DELETE FROM heatmap_cells WHERE site = ?", [site]))
            .await
            .map_err(internal)?;
    }
    let key = Uuid::from_u128(site).hyphenated().to_string();
    if let Erasure::Site(_) | Erasure::SiteHistory(_) = erasure {
        summaries::forget(&state.db.writer, &key)
            .await
            .map_err(internal)?;
    }
    match erasure {
        // Live keys would still let someone with the database test a pseudonym of a site
        // that no longer exists.
        Erasure::Site(_) => {
            sqlx::query("DELETE FROM identity_day_keys WHERE site_id = ?")
                .bind(&key)
                .execute(&state.db.writer)
                .await
                .map_err(internal)?;
            imports::forget_site(&state.db.writer, &key, false)
                .await
                .map_err(internal)?;
        }
        Erasure::Profile { profile, .. } => {
            sqlx::query("DELETE FROM consents WHERE site_id = ? AND profile_id = ?")
                .bind(&key)
                .bind(&profile.to_be_bytes()[..])
                .execute(&state.db.writer)
                .await
                .map_err(internal)?;
            remove_imported_identity(state, site, profile).await?;
        }
        Erasure::SiteHistory(_) => {
            imports::forget_site(&state.db.writer, &key, true)
                .await
                .map_err(internal)?;
        }
    }
    if storage.watermark().borrow().sequence < fence.through_seq {
        return Ok(None);
    }
    // A replay part a request validated before the deletion may have landed since.
    if let Erasure::Site(_) = erasure {
        captures += remove_replays(state, fence).await?;
    }
    Ok(Some(json!({
        "events": erased.events,
        "captures": captures,
        "through_sequence": fence.through_seq,
    })))
}

/// Removes what an import kept of a profile's distinct ID: the restricted provenance of
/// imported events and the inventoried Umami `session_link` rows (0013). Imported history has
/// no profile, so they are found by deriving the profile ID of each stored distinct ID.
async fn remove_imported_identity(
    state: &AppState,
    site: u128,
    profile: u128,
) -> Result<(), Failure> {
    let storage = state.collector.storage();
    let pseudonyms = state.collector.pseudonyms();
    let theirs = |distinct_id: &str| pseudonyms.profile_id(site, distinct_id) == profile;
    let distinct_ids: Vec<String> = storage
        .read(Lane::Live, move |s| {
            s.connection()
                .prepare(
                    "SELECT DISTINCT distinct_id FROM import_provenance
                     WHERE site = ? AND distinct_id IS NOT NULL",
                )?
                .query_map([site], |r| r.get(0))?
                .collect::<duckdb::Result<Vec<String>>>()
        })
        .await
        .map_err(internal)?
        .value
        .into_iter()
        .filter(|id| theirs(id))
        .collect();
    if !distinct_ids.is_empty() {
        storage
            .write(move |tx| {
                for id in &distinct_ids {
                    tx.execute(
                        "DELETE FROM import_provenance WHERE site = ? AND distinct_id = ?",
                        duckdb::params![site, id],
                    )?;
                }
                Ok(())
            })
            .await
            .map_err(internal)?;
    }
    let key = Uuid::from_u128(site).hyphenated().to_string();
    let links: Vec<String> = imports::session_links(&state.db.reader, &key)
        .await
        .map_err(internal)?
        .into_iter()
        .filter(|(_, distinct_id)| theirs(distinct_id))
        .map(|(source_key, _)| source_key)
        .collect();
    imports::forget_session_links(&state.db.writer, &links)
        .await
        .map_err(internal)
}

/// Removes the captures an erasure covers, with their raw heatmap interactions.
async fn remove_replays(state: &AppState, fence: Fence) -> Result<u64, Failure> {
    let store = &state.recordings.store;
    let storage = state.collector.storage();
    let (site, before) = match fence.erasure {
        Erasure::Site(site) => (site, i64::MAX),
        Erasure::SiteHistory(site) => (site, fence.created_at_ms),
        Erasure::Profile { site, profile } => {
            // A profile's replays are the captures recorded under its daily pseudonyms.
            let through = fence.through_seq;
            let visitors = storage
                .read(Lane::Live, move |s| {
                    s.connection()
                        .prepare(
                            "SELECT DISTINCT daily_id FROM events
                             WHERE site = ? AND profile_id = ? AND seq <= ? AND daily_id IS NOT NULL",
                        )?
                        .query_map(duckdb::params![site, profile, through], |r| r.get(0))?
                        .collect::<duckdb::Result<Vec<u128>>>()
                })
                .await
                .map_err(internal)?
                .value;
            let captures = store.captures_of(site, &visitors).await.map_err(internal)?;
            if captures.is_empty() {
                return Ok(0);
            }
            let list = captures.clone();
            storage
                .write(move |tx| {
                    for capture in &list {
                        tx.execute(
                            "DELETE FROM heatmap_interactions WHERE site = ? AND capture_id = ?",
                            duckdb::params![site, capture],
                        )?;
                    }
                    Ok(())
                })
                .await
                .map_err(internal)?;
            return store.remove(site, &captures).await.map_err(internal);
        }
    };
    let mut removed = 0;
    loop {
        let step = store
            .expire(site, before, true, CAPTURES_PER_STEP)
            .await
            .map_err(internal)?;
        removed += step;
        if step < CAPTURES_PER_STEP as u64 {
            return Ok(removed);
        }
    }
}
