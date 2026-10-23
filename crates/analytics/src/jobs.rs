//! Settings-change and recalculation jobs (0002 Calculation and policy versions).
//! Each job writes the analytical copy of a decision and marks the identity days it
//! affects dirty in one transaction; correction does the rebuilding.

use std::ops::RangeInclusive;

use chrono::NaiveDate;
use chrono_tz::Tz;
use holocron_storage::duckdb::types::{Type, Value};
use holocron_storage::duckdb::{self, Connection, params};
use holocron_storage::{Id, Storage};

use crate::identity::{PolicyVersion, SessionPolicy, SitePolicies, days_since_epoch};

const DAY_MS: i64 = 86_400_000;

/// Makes the analytical copy of a site's session policy versions equal to `policies`, the
/// versions control storage owns (0008: control storage owns sites and settings). Versions
/// from the first difference on are replaced, and the identity days they can affect are
/// marked dirty so correction rebuilds them. Returns whether anything changed.
///
/// Idempotent: the server runs it after every policy change and for every site at startup,
/// which also repairs a copy left behind by a crash between the two stores.
pub async fn record_policies(
    storage: &Storage,
    site: Id,
    policies: SitePolicies,
) -> Result<bool, holocron_storage::Error> {
    storage
        .write(move |tx| {
            let stored = site_policies(tx, site)?;
            let stored = stored.as_ref().map_or(&[][..], SitePolicies::versions);
            let wanted = policies.versions();
            let same = stored
                .iter()
                .zip(wanted)
                .take_while(|(a, b)| a == b)
                .count();
            let Some(from) = stored[same..]
                .iter()
                .chain(&wanted[same..])
                .map(|v| v.effective_ms)
                .min()
            else {
                return Ok(false);
            };
            tx.execute(
                "DELETE FROM session_policies WHERE site = ? AND version > ?",
                params![site, same as i64],
            )?;
            for version in &wanted[same..] {
                insert_version(tx, site, version)?;
            }
            // A gap can reach back at most one maximum window (24 h) across the instant.
            // Version 1 takes effect at the smallest instant, so a new site rebuilds every day.
            mark_events_from(tx, site, from.saturating_sub(DAY_MS))?;
            Ok(true)
        })
        .await
}

/// A recalculation job as recorded: how many identity days it marked dirty, and the
/// applied batch they were marked at. It is done when none of those rows is left.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Recalculation {
    pub days: u64,
    pub through_batch: i64,
}

/// Marks every identity day in `days` dirty, so correction rebuilds them under the current
/// versions.
pub async fn recalculate(
    storage: &Storage,
    site: Id,
    days: RangeInclusive<NaiveDate>,
) -> Result<Recalculation, holocron_storage::Error> {
    let (first, last) = (
        days_since_epoch(*days.start()),
        days_since_epoch(*days.end()),
    );
    storage
        .write(move |tx| {
            let through_batch: i64 = tx.query_row(
                "SELECT coalesce(max(batch_id), 0) FROM applied_batches",
                [],
                |r| r.get(0),
            )?;
            let marked = tx.execute(
                "INSERT INTO dirty_days
                 SELECT ?, DATE '1970-01-01' + d::INTEGER, ? FROM range(?, ?) t(d)",
                params![site, through_batch, i64::from(first), i64::from(last) + 1],
            )?;
            Ok(Recalculation {
                days: marked as u64,
                through_batch,
            })
        })
        .await
}

/// Identity days of a recalculation over `days` still waiting for correction.
pub fn pending_days(
    conn: &Connection,
    site: Id,
    days: RangeInclusive<NaiveDate>,
    through_batch: i64,
) -> duckdb::Result<u64> {
    let pending: i64 = conn.query_row(
        "SELECT count(DISTINCT local_day) FROM dirty_days
         WHERE site = ? AND local_day BETWEEN ? AND ? AND batch_id <= ?",
        params![
            site,
            date_value(*days.start()),
            date_value(*days.end()),
            through_batch
        ],
        |r| r.get(0),
    )?;
    Ok(pending as u64)
}

/// The recorded session policy versions of a site, if it has any.
pub fn site_policies(conn: &Connection, site: Id) -> duckdb::Result<Option<SitePolicies>> {
    let mut statement = conn.prepare(
        "SELECT version, epoch, effective_ms, timezone, window_ms
         FROM session_policies WHERE site = ? ORDER BY version",
    )?;
    let versions = statement
        .query_map([site], |row| {
            let timezone: String = row.get(3)?;
            let window_ms: i64 = row.get(4)?;
            let invalid = |error: Box<dyn std::error::Error + Send + Sync>| {
                duckdb::Error::FromSqlConversionFailure(3, Type::Text, error)
            };
            let tz: Tz = timezone.parse().map_err(|e| invalid(Box::new(e)))?;
            let policy = SessionPolicy::new(tz, (window_ms / 60_000) as u16)
                .map_err(|e| invalid(Box::new(e)))?;
            Ok(PolicyVersion {
                version: row.get(0)?,
                epoch: row.get(1)?,
                effective_ms: row.get(2)?,
                policy,
            })
        })?
        .collect::<duckdb::Result<Vec<_>>>()?;
    Ok(SitePolicies::from_versions(versions))
}

fn insert_version(conn: &Connection, site: Id, version: &PolicyVersion) -> duckdb::Result<()> {
    conn.execute(
        "INSERT INTO session_policies (site, version, epoch, effective_ms, timezone, window_ms) VALUES (?, ?, ?, ?, ?, ?)",
        params![
            site,
            version.version,
            version.epoch,
            version.effective_ms,
            version.policy.timezone().name(),
            version.policy.window_ms()
        ],
    )?;
    Ok(())
}

/// Stored events of `site` before `cutoff_ms`: what retention at that cutoff removes (0016).
pub fn events_before(conn: &Connection, site: Id, cutoff_ms: i64) -> duckdb::Result<u64> {
    let count: i64 = conn.query_row(
        "SELECT count(*) FROM events WHERE site = ? AND epoch_ms(event_time) < ?",
        params![site, cutoff_ms],
        |r| r.get(0),
    )?;
    Ok(count as u64)
}

/// Marks the identity days of the site's events at or after `from_ms` dirty.
fn mark_events_from(conn: &Connection, site: Id, from_ms: i64) -> duckdb::Result<()> {
    conn.execute(
        "INSERT INTO dirty_days
         SELECT DISTINCT site, local_day, (SELECT coalesce(max(batch_id), 0) FROM applied_batches)
         FROM events WHERE site = ? AND epoch_ms(event_time) >= ?",
        params![site, from_ms],
    )?;
    Ok(())
}

pub(crate) fn date_value(date: NaiveDate) -> Value {
    Value::Date32(days_since_epoch(date))
}
