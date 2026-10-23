//! Freshness and correction state of a result (milestone 05 work item 5).
//!
//! The ingestion watermark says which accepted records are visible. It does not
//! say derived data is corrected: that holds only when no dirty identity day can
//! still change the range. Read inside the same snapshot as the result.

use chrono::DateTime;
use holocron_storage::duckdb::{self, OptionalExt, params};
use holocron_storage::{Id, Snapshot, Watermark};

use crate::METRIC_VERSION;
use crate::jobs::{date_value, site_policies};

/// Whether derived rows a result reads are final for the current ingestion watermark.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Correction {
    Corrected,
    /// Dirty identity days can still change the result; it must say so.
    Pending {
        dirty_days: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Freshness {
    /// Highest contiguous applied batch and sequence.
    pub watermark: Watermark,
    /// Every batch up to this one has had its dirty days corrected, on every site.
    pub corrected_through: i64,
    pub correction: Correction,
    pub metric_version: u32,
    /// Session policy versions in effect anywhere in the range, ascending.
    pub session_policy_versions: Vec<i32>,
    /// Imported history the range reaches, whose visitors and sessions carry the limitations
    /// of decision 0013.
    pub imported: Option<ImportedHistory>,
}

/// History a site brought from an imported snapshot: the source and the event times it spans.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedHistory {
    pub source: String,
    pub first_ms: i64,
    pub last_ms: i64,
}

/// Freshness of a result over `[start_ms, end_ms)` of one site.
///
/// A dirty day can change the range when it lies within one day of it (local
/// dates differ from UTC dates by less than a day, and a gap is at most 24 h),
/// or when it has events of a profile whose sessions start in the range, since
/// correction rebuilds that profile's whole history.
pub fn freshness(
    snapshot: &Snapshot,
    site: Id,
    start_ms: i64,
    end_ms: i64,
) -> duckdb::Result<Freshness> {
    let conn = snapshot.connection();
    let watermark = snapshot.watermark();
    let corrected_through: i64 = conn.query_row(
        "SELECT coalesce(min(batch_id) - 1, ?) FROM dirty_days",
        [watermark.batch],
        |r| r.get(0),
    )?;
    let utc_date = |ms: i64| {
        DateTime::from_timestamp_millis(ms)
            .expect("instant in range")
            .date_naive()
    };
    let first = utc_date(start_ms).pred_opt().expect("date in range");
    let last = utc_date(end_ms - 1).succ_opt().expect("date in range");
    let dirty_days: i64 = conn.query_row(
        "SELECT count(DISTINCT d.local_day) FROM dirty_days d
         WHERE d.site = ?1 AND (
            d.local_day BETWEEN ?2 AND ?3
            OR EXISTS (
                SELECT 1 FROM sessions s
                WHERE s.site = d.site AND s.population = 2
                  AND s.start_time >= epoch_ms(?4) AND s.start_time < epoch_ms(?5)
                  AND s.identity IN (
                      SELECT profile_id FROM events e WHERE e.site = d.site AND e.local_day = d.local_day
                      UNION
                      SELECT profile_id FROM session_properties p WHERE p.site = d.site AND p.local_day = d.local_day)))",
        params![site, date_value(first), date_value(last), start_ms, end_ms],
        |r| r.get(0),
    )?;
    let session_policy_versions = match site_policies(conn, site)? {
        None => vec![1],
        Some(policies) => {
            let versions = policies.versions();
            versions
                .iter()
                .enumerate()
                .filter(|(i, v)| {
                    let until = versions
                        .get(i + 1)
                        .map_or(i64::MAX, |next| next.effective_ms);
                    v.effective_ms < end_ms && until > start_ms
                })
                .map(|(_, v)| v.version)
                .collect()
        }
    };
    let imported = conn
        .query_row(
            "SELECT source, epoch_ms(first_time), epoch_ms(last_time) FROM imported_history
             WHERE site = ? AND first_time < epoch_ms(?) AND last_time >= epoch_ms(?)",
            params![site, end_ms, start_ms],
            |r| {
                Ok(ImportedHistory {
                    source: r.get(0)?,
                    first_ms: r.get(1)?,
                    last_ms: r.get(2)?,
                })
            },
        )
        .optional()?;
    Ok(Freshness {
        watermark,
        corrected_through,
        correction: match dirty_days {
            0 => Correction::Corrected,
            n => Correction::Pending {
                dirty_days: n as u64,
            },
        },
        metric_version: METRIC_VERSION,
        session_policy_versions,
        imported,
    })
}
