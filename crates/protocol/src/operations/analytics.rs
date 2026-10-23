//! Correction state of derived analytics: freshness of a range and explicit recalculations.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::ids::{RecalculationId, SiteId};

crate::domain! {
    pub trait AnalyticsHandlers;
    commands {
        /// Rebuilds sessions and day aggregates of the identity days `first_day` to
        /// `last_day` under the current session policy. Needs `site.configure`. At most 366
        /// days per request.
        "recalculations.start" => recalculations_start (Person, key): RecalculationsStartArgs => RecalculationResult;
    }
    queries {
        /// Whether a recalculation has finished. Needs `site.configure`.
        "recalculations.status" => recalculations_status (Person, key): RecalculationsStatusArgs => RecalculationResult;
        /// What a result over `[start, end)` can claim: the ingestion watermark, whether
        /// identity days that can change the range still wait for correction, the metric
        /// version and the session policy versions in effect. Needs `site.read`.
        "analytics.freshness" => analytics_freshness (Person, key, share_read): FreshnessArgs => Freshness;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct RecalculationsStartArgs {
    pub site: SiteId,
    /// Local date `YYYY-MM-DD` in the site timezone, inclusive.
    pub first_day: String,
    /// Local date `YYYY-MM-DD`, inclusive, not before `first_day`.
    pub last_day: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct RecalculationsStatusArgs {
    pub site: SiteId,
    pub recalculation: RecalculationId,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct RecalculationResult {
    pub recalculation: Recalculation,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct Recalculation {
    pub id: RecalculationId,
    pub first_day: String,
    pub last_day: String,
    /// Unix milliseconds.
    pub requested_at: i64,
    pub state: CorrectionState,
    /// Identity days of the range still waiting for correction.
    pub pending_days: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CorrectionState {
    /// Derived rows are final for the current ingestion watermark.
    Corrected,
    /// Dirty identity days can still change the result; it must say so.
    Pending,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct FreshnessArgs {
    pub site: SiteId,
    /// Range start, Unix milliseconds, inclusive.
    pub start: i64,
    /// Range end, Unix milliseconds, exclusive.
    pub end: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct Freshness {
    /// Last applied batch and the last accepted inbox sequence it contains: every record
    /// acknowledged up to it is visible.
    pub watermark_batch: i64,
    pub watermark_sequence: i64,
    /// Every record received before this instant is visible: the receive time of the oldest
    /// record the watermark does not reach, or now when it reaches all. Unix milliseconds on
    /// the application clock.
    pub watermark_at: i64,
    /// Analytics is over a minute behind collection (readiness problem `analytics_stale`).
    /// Views then show data until `watermark_at` rather than until now.
    pub stale: bool,
    /// Every batch up to this one has had its dirty identity days corrected.
    pub corrected_through: i64,
    pub correction: CorrectionState,
    /// Identity days that can still change the range.
    pub dirty_days: i64,
    pub metric_version: u32,
    /// Session policy versions in effect anywhere in the range, ascending.
    pub session_policy_versions: Vec<i32>,
    /// Present when the range reaches history imported from Umami. Imported visitors are Umami
    /// session IDs per local day, sessions are recomputed from event times, and imported events
    /// never count toward identified people or retention (decision 0013).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub imported: Option<ImportedHistory>,
}

/// The imported snapshot a result reaches and the event times it spans, Unix milliseconds.
#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct ImportedHistory {
    /// The snapshot, like `umami-3.4.0-postgresql`.
    pub source: String,
    pub first: i64,
    pub last: i64,
}
