//! Site summaries (decision 0015 Where it runs): a background step keeps one small row per site
//! with today's pace against a typical weekday, so the site chip, site menu, workspace page and
//! sidebar read it with one query and never start an analytical computation. The row is a
//! derived record with its own watermark, not a report cache. With the row the step keeps the
//! site's findings: deterministic noticed changes, each with the measured values its template
//! needs and the exploration state that shows it. No text is stored. When a site's row
//! changes, the site's `analytics:<site>` stream sends a `summary` event.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::exploration::{Cell, DimensionField, ExplorationState, Metric, Projection};
use crate::ids::{CardId, GoalId, SiteId};

crate::domain! {
    pub trait SummariesHandlers;
    commands {}
    queries {
        /// The summaries of up to 100 sites, in request order. Sites the caller cannot read
        /// (`site.read`), unknown sites and sites without a summary yet are left out, so the
        /// answer never tells them apart.
        "summaries.forSites" => summaries_for_sites (Person, key): SummariesForSitesArgs => SummaryList;
        /// The site's current findings in rank order, at most `limit`. Needs `site.read`; an
        /// unreadable site answers `not_found` like an unknown one.
        "summaries.findings" => summaries_findings (Person, key): SummariesFindingsArgs => FindingList;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct SummariesForSitesArgs {
    /// 1 to 100 site IDs; repeated ones count once.
    pub sites: Vec<SiteId>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct SummaryList {
    pub summaries: Vec<Summary>,
}

/// Today so far against a typical weekday, from `analytics.explore` with today, hour
/// granularity and `compare: "typical_weekday"` at the stated watermark.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
pub struct Summary {
    pub site: SiteId,
    /// When the step computed it, Unix milliseconds.
    pub computed_at: i64,
    /// The analytical position it was computed at.
    pub watermark_batch: i64,
    pub watermark_sequence: i64,
    /// Formula version it was computed under (decision 0003).
    pub metric_version: u32,
    /// Older than 30 minutes or computed under another metric version: show the site's name
    /// and drop the pace.
    pub stale: bool,
    /// IANA timezone today and its hours are read in.
    pub timezone: String,
    pub pageviews: MeasurePace,
    pub visitor_days: MeasurePace,
    /// Pageviews per local hour of today, 23 to 25 entries on a daylight saving day. Hours
    /// after `computed_at` are 0.
    pub hourly_today: Vec<i64>,
    /// The typical weekday's pageviews per hour, aligned with `hourly_today`; `null` entries
    /// without enough history.
    pub hourly_typical: Vec<Option<i64>>,
    /// Fewer than 4 baseline days: no typical values, pace or projection.
    pub insufficient_history: bool,
    /// Current findings of the site.
    pub finding_count: u32,
}

/// One count measure's pace.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
pub struct MeasurePace {
    /// Exact count from the start of today up to `computed_at`.
    pub today: i64,
    /// The typical weekday up to the same local time; `null` without enough history.
    pub typical: Option<i64>,
    /// `(today − typical) / typical`, rounded to 4 places; `null` without a typical value or
    /// when it is 0.
    pub relative_change: Option<f64>,
    /// Where today ends, an estimate; `null` without enough history.
    pub projection: Option<Projection>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct SummariesFindingsArgs {
    pub site: SiteId,
    /// 1 to 20.
    pub limit: u32,
}

/// A site's findings as of its last summary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
pub struct FindingList {
    pub site: SiteId,
    /// When the summary they belong to was computed; `null` before the site's first summary,
    /// which has no findings.
    pub computed_at: Option<i64>,
    pub watermark_batch: Option<i64>,
    pub watermark_sequence: Option<i64>,
    /// As for [`Summary`]; `true` without a summary.
    pub stale: bool,
    /// In rank order.
    pub findings: Vec<Finding>,
}

/// One noticed change (decision 0015 Noticed changes): a fact with the values its template
/// needs. The step never states a cause or relates two findings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
pub struct Finding {
    /// 1 for the first. Rule families rank goal pace, performance, dimension mover, funnel
    /// step; within a family by `share`, then by the size of the change.
    pub rank: u32,
    pub rule: FindingRule,
    /// Version of the rule's thresholds and values.
    pub rule_version: u32,
    /// When the rule computed it. Performance and funnel step rules run at most hourly, so
    /// theirs can be older than the summary.
    pub computed_at: i64,
    /// Start of the measured window, Unix milliseconds.
    pub window_start: i64,
    /// The finding's share of the site's total change, 0 to 1. Only dimension movers have a
    /// site total; other families have 0.
    pub share: f64,
    /// The exploration state that shows the measured values (decision 0004, version 1).
    pub state: ExplorationState,
    pub values: FindingValues,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FindingRule {
    GoalPace,
    Performance,
    DimensionMover,
    FunnelStep,
}

/// What a finding measured, by template.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FindingValues {
    DimensionMover(MoverValues),
    GoalPace(GoalPaceValues),
    FunnelStep(FunnelStepValues),
    Performance(PerformanceValues),
}

/// One value of a dimension today so far against a typical weekday up to the same local time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
pub struct MoverValues {
    /// `path`, `referrer_domain`, `browser`, `os` or `device`.
    pub dimension: DimensionField,
    pub value: Cell,
    /// The value's visitor-days today so far.
    pub today: i64,
    /// The value's typical visitor-days so far.
    pub typical: i64,
    /// `today / typical`, rounded to 4 places.
    pub ratio: f64,
    /// The site's visitor-days today so far and typical so far.
    pub site_today: i64,
    pub site_typical: i64,
}

/// A goal's completions today so far against a typical weekday up to the same local time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
pub struct GoalPaceValues {
    pub goal: GoalId,
    pub goal_version: u32,
    pub today: i64,
    pub typical: i64,
    /// `(today − typical) / typical`, rounded to 4 places.
    pub relative_change: f64,
}

/// A funnel step's drop-off today against the pooled baseline days, both from local midnight
/// to `until_hour`:00. Funnels count daily visitors.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
pub struct FunnelStepValues {
    /// The board card holding the funnel, and its title when computed.
    pub card: CardId,
    pub title: String,
    /// The step visitors drop off after, from 1: they reach `step` but not `step + 1`.
    pub step: u32,
    pub until_hour: u32,
    /// Visitors reaching `step` today, and those of them not reaching the next step.
    pub entered: i64,
    pub dropped: i64,
    /// The same over every baseline day together.
    pub baseline_entered: i64,
    pub baseline_dropped: i64,
    /// Same local weekdays of the previous 8 weeks with history.
    pub baseline_days: u32,
    /// `dropped / entered` and `baseline_dropped / baseline_entered`, rounded to 4 places.
    pub drop_off: f64,
    pub baseline_drop_off: f64,
}

/// A performance metric's p75 on one device over the last 24 hours against the slowest daily
/// p75 of the 28 days before.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
pub struct PerformanceValues {
    /// `lcp`, `fcp`, `ttfb`, `inp` or `cls`.
    pub metric: String,
    pub device: Cell,
    /// The p75 of the last 24 whole hours: milliseconds, or a decimal for `cls`.
    pub value: Metric,
    pub observations: i64,
    /// The slowest daily p75 and its local date `YYYY-MM-DD`.
    pub slowest: Metric,
    pub slowest_day: String,
    /// Days with observations it was the slowest of.
    pub days: u32,
}
