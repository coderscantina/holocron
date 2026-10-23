//! The versioned exploration state of decision 0004 and the results computed from it.
//!
//! URLs, saved explorations, board cards, exports and query keys all carry this one state.
//! Parsing is strict: unknown fields are refused, so a state from a later version (a link
//! `source`) fails visibly instead of computing a different report. Semantic checks (limits,
//! operator and value types, unique predicate IDs, report rules) run in the analytics planner.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::ids::{GoalId, SiteId};
use crate::operations::analytics::Freshness;

/// The only state version this build reads.
pub const STATE_VERSION: u8 = 1;

/// The only version of each report kind this build reads.
pub const REPORT_VERSION: u8 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExplorationState {
    /// Always 1.
    pub v: u8,
    pub site: SiteId,
    /// 1 to 8, unique; none with a report.
    pub measures: Vec<Measure>,
    /// 0 to 3, unique; none with a report.
    pub dimensions: Vec<Field>,
    pub filter: Option<Group>,
    pub range: Range,
    /// IANA timezone the range, buckets, weekday and hour are read in.
    pub timezone: String,
    pub granularity: Granularity,
    pub compare: Compare,
    /// Formula version the state was created with. A state pinned to another version is refused.
    pub metric_version: u32,
    /// 0 to 2 keys. Ties fall back to the dimension values ascending.
    pub sort: Vec<Sort>,
    pub page: Page,
    pub visualization: Visualization,
    /// Settings of a report kind. Absent for plain explorations, which stay byte-identical.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub report: Option<ReportDefinition>,
}

/// A measure: a named formula of decision 0003 or a statistic over one numeric property.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS, JsonSchema)]
#[serde(untagged)]
pub enum Measure {
    Named(NamedMeasure),
    Property(PropertyStat),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum NamedMeasure {
    Pageviews,
    Events,
    /// Distinct daily pseudonyms; labelled "visitors" only inside one identity day.
    Visitors,
    VisitorDays,
    IdentifiedPeople,
    Sessions,
    BounceRate,
    SpanMean,
    SpanMedian,
    /// Revenue per currency in minor units: all records, positive ones, negative ones.
    RevenueNet,
    RevenueGross,
    RevenueRefunds,
    /// Revenue records with a non-zero amount.
    RevenueCount,
    /// Nearest-rank percentiles of one performance metric; need a `perf_metric` condition.
    P50,
    P75,
    P95,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PropertyStat {
    pub stat: Stat,
    pub field: PropertyField,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Stat {
    Count,
    Sum,
    Min,
    Max,
    Mean,
}

/// A dimension or a typed custom property.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS, JsonSchema)]
#[serde(untagged)]
pub enum Field {
    Dimension(DimensionField),
    Property(PropertyField),
}

/// Built-in fields. Event fields come from the event; session fields from the event's session
/// (0004 Field scope).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum DimensionField {
    Path,
    Hostname,
    Title,
    Query,
    EventName,
    Tag,
    /// The performance metric of a performance event: `lcp`, `fcp`, `ttfb`, `inp` or `cls`.
    PerfMetric,
    /// ISO 4217 currency of a revenue record.
    Currency,
    ReferrerDomain,
    ReferrerPath,
    UtmSource,
    UtmMedium,
    UtmCampaign,
    UtmContent,
    UtmTerm,
    ClickId,
    Channel,
    Country,
    Region,
    City,
    Browser,
    Os,
    Device,
    Language,
    Screen,
    /// ISO weekday 1 (Monday) to 7, wall clock in the state's timezone.
    Weekday,
    /// 0 to 23, wall clock in the state's timezone.
    Hour,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PropertyField {
    pub scope: PropertyScope,
    /// 1 to 500 bytes.
    pub key: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PropertyScope {
    Event,
    Session,
}

/// One entry of a group: a nested group or a condition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(untagged)]
pub enum Node {
    Group(Group),
    Cond(Condition),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Group {
    pub kind: GroupKind,
    pub op: GroupOp,
    /// 1 to 32 entries.
    pub items: Vec<Node>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GroupKind {
    Group,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GroupOp {
    And,
    Or,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Condition {
    pub kind: CondKind,
    /// `^[a-z0-9_]{1,16}$`, unique within the state, never renumbered.
    pub id: String,
    pub field: Field,
    pub cmp: Operator,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub value: Option<Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CondKind {
    Cond,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Operator {
    Eq,
    Neq,
    Contains,
    NotContains,
    StartsWith,
    In,
    NotIn,
    Gt,
    Gte,
    Lt,
    Lte,
    /// Both ends inclusive.
    Between,
    IsSet,
    IsNotSet,
    IsNull,
}

/// A typed comparison value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(tag = "t", content = "v", rename_all = "snake_case")]
pub enum Value {
    String(String),
    Integer(i64),
    /// Decimal string with up to 4 fractional digits.
    Decimal(String),
    /// RFC 3339 instant.
    Date(String),
    Bool(bool),
    /// For `in`, `not_in` (up to 50 items) and `between` (exactly 2).
    List(Vec<Value>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Range {
    /// The last `count` units ending with the current one, in the state's timezone.
    Relative { unit: RangeUnit, count: u16 },
    /// Inclusive local dates `YYYY-MM-DD`.
    Days { start: String, end: String },
    /// Every retained day through today: from the retention cutoff or the site's first stored
    /// event, whichever is later (0016).
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RangeUnit {
    Day,
    Week,
    Month,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Granularity {
    Hour,
    Day,
    Week,
    Month,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Compare {
    None,
    PreviousPeriod,
    /// The median of the same local weekday over the previous 8 weeks (0015).
    TypicalWeekday,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Sort {
    pub by: SortKey,
    pub dir: SortDir,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(untagged)]
pub enum SortKey {
    Measure(Measure),
    Field(Field),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SortDir {
    Asc,
    Desc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Page {
    /// 1 to 100, default 50.
    pub size: u32,
    pub index: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Visualization {
    Table,
    Line,
    Bar,
    Map,
    /// Needs exactly the dimensions `weekday` and `hour`.
    Grid,
    Funnel,
    Journey,
    Cohort,
}

/// Another cut of the same rows and range: its own measures, dimensions, sort and page.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Panel {
    pub measures: Vec<Measure>,
    pub dimensions: Vec<Field>,
    pub sort: Vec<Sort>,
    pub page: Page,
}

/// The range a result covers, resolved once per request. Unix milliseconds, half-open.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct ResolvedRange {
    pub start: i64,
    pub end: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub compare_start: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub compare_end: Option<i64>,
    /// Set when the range starts before the site's retention cutoff (0016): the first retained
    /// instant.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub retention_start: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub compare_retention_start: Option<i64>,
    /// The local dates `YYYY-MM-DD` a `typical_weekday` baseline reads, ascending, after
    /// clipping at the retention cutoff and the site's first stored event. Replaces
    /// `compare_start` and `compare_end` for that comparison.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub compare_days: Option<Vec<String>>,
    pub timezone: String,
}

/// "visitors" when the range lies inside one identity day, else "visitor_days" (0003).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum VisitorLabel {
    Visitors,
    VisitorDays,
}

/// One computed value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Metric {
    Count {
        v: i64,
    },
    /// Rounded half to even to 4 places, with the exact parts it came from.
    Ratio {
        v: f64,
        numerator: i64,
        denominator: i64,
    },
    /// Integer milliseconds.
    Millis {
        v: i64,
    },
    /// Decimal string with 4 places.
    Decimal {
        v: String,
    },
    /// Revenue per currency, by currency code; never summed across currencies (0003 Revenue).
    Money {
        v: Vec<Money>,
    },
    Unavailable {
        reason: Reason,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    /// A rate whose denominator is 0.
    NoDenominator,
    /// A statistic over no numeric rows.
    NoValues,
    /// Relative change against a previous value of 0.
    ZeroBaseline,
    /// A percentile over observations of several performance metrics, whose units differ.
    MixedMetrics,
    /// Fewer than 4 baseline days for a typical-weekday baseline or projection (0015).
    InsufficientHistory,
}

/// The previous period's or the typical weekday's value and the change to it (0003 Comparison
/// and zero baselines).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
pub struct Comparison {
    pub previous: Metric,
    /// `current − previous`; absent when either side is unavailable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub change: Option<f64>,
    /// `(current − previous) / previous`, rounded to 4 places.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub relative_change: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub reason: Option<Reason>,
}

/// A result cell key: a typed value, a present JSON `null`, or no value.
#[derive(
    Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS, JsonSchema,
)]
#[serde(tag = "t", content = "v", rename_all = "snake_case")]
pub enum Cell {
    String(String),
    Integer(i64),
    Decimal(String),
    /// RFC 3339 instant.
    Date(String),
    Bool(bool),
    Null,
    NotSet,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
pub struct Row {
    /// One cell per dimension, in dimension order.
    pub key: Vec<Cell>,
    /// One value per measure, in measure order.
    pub values: Vec<Metric>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub compare: Option<Vec<Comparison>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
pub struct SeriesPoint {
    /// Bucket start, Unix milliseconds.
    pub start: i64,
    /// Bucket start as local wall time `YYYY-MM-DDTHH:MM` in the state's timezone.
    pub local: String,
    /// One value per measure.
    pub values: Vec<Metric>,
    /// One comparison per measure when compared: previous-period buckets align by offset,
    /// typical-weekday buckets by local weekday and wall-clock hour.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub compare: Option<Vec<Comparison>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
pub struct Series {
    pub granularity: Granularity,
    pub points: Vec<SeriesPoint>,
    /// The visitor series adds up to more than the headline, which counts each pseudonym once.
    pub series_sum_differs: bool,
    /// Projected buckets from now to the end of the range, kept apart from `points`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub projection: Option<Vec<ProjectedPoint>>,
}

/// A labelled estimate of where a count measure ends (0015 Pace projection). Never a measured
/// value: it stays out of `totals`, `rows` and `points`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Projection {
    /// The median projection over the basis days, with its 25th to 75th percentile band.
    Estimate {
        value: f64,
        low: f64,
        high: f64,
        basis_days: u32,
    },
    Unavailable {
        reason: Reason,
    },
}

/// One projected series bucket.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
pub struct ProjectedPoint {
    /// Bucket start, Unix milliseconds.
    pub start: i64,
    /// Bucket start as local wall time `YYYY-MM-DDTHH:MM` in the state's timezone.
    pub local: String,
    /// One entry per measure; `null` for a measure that is not a count.
    pub values: Vec<Option<Projection>>,
}

/// Eligible events with a daily pseudonym or profile, out of all eligible events (0003).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct Coverage {
    pub with_identity: i64,
    pub eligible_events: i64,
}

/// The totals, rows and series of one cut, all from the same snapshot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
pub struct Cut {
    pub cut: Panel,
    /// One value per measure over every matching row; paging does not change it.
    pub totals: Vec<Metric>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub compare: Option<Vec<Comparison>>,
    /// Where each total ends when the range ends after now: one entry per measure, `null` for a
    /// measure that is not a count.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub projection: Option<Vec<Option<Projection>>>,
    /// The requested page of rows. Empty without dimensions.
    pub rows: Vec<Row>,
    /// Rows across all pages.
    pub row_count: i64,
    /// Present for `line` and `bar` visualizations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub series: Option<Series>,
    pub identity_coverage: Coverage,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct ExploreResult {
    pub resolved: ResolvedRange,
    pub freshness: Freshness,
    pub visitor_label: VisitorLabel,
    /// The state's own cut. With a report it has no totals, rows or series.
    pub main: Cut,
    /// The requested panels, in request order. Empty with a report.
    pub panels: Vec<Cut>,
    /// The output of the state's report, which defines its own output (0004 Report definitions).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub report: Option<ReportResult>,
}

/// Settings of one report kind (0004 Report definitions). Each kind carries its own version `v`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReportDefinition {
    Funnel(FunnelReport),
    Journey(JourneyReport),
    Attribution(AttributionReport),
    Retention(RetentionReport),
    SessionList(SessionListReport),
    Goals(GoalsReport),
    /// A kind this build does not know. Parsed so the planner can refuse it with
    /// `report_version_unsupported`; never sent.
    #[serde(other)]
    #[ts(skip)]
    #[schemars(skip)]
    Unsupported,
}

/// Which identity a report follows across events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum IdentityScope {
    /// The daily pseudonym: one local day, no consent needed.
    Daily,
    /// The persistent profile: consented population only.
    Person,
}

/// What a report counts: sessions, or the persistent profiles of consented people.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PopulationScope {
    Session,
    Person,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FunnelReport {
    pub v: u8,
    /// 2 to 8 ordered step predicates.
    pub steps: Vec<Group>,
    /// 60 seconds to 30 days from the step-1 event, half-open.
    pub window_seconds: u32,
    pub scope: IdentityScope,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct JourneyReport {
    pub v: u8,
    pub value: JourneyValue,
    /// 2 to 7 steps, counted after loop compression.
    pub length: u8,
    pub scope: PopulationScope,
}

/// The step value of a journey.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum JourneyValue {
    /// Page path of pageviews.
    Path,
    /// Event name of custom events.
    EventName,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AttributionReport {
    pub v: u8,
    pub model: AttributionModel,
    /// Always 30 in version 1.
    pub lookback_days: u16,
    pub scope: IdentityScope,
    /// The session field a touch is grouped by.
    pub touch_field: TouchField,
    /// 1 to 4.
    pub conversions: Vec<Conversion>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AttributionModel {
    LastTouch,
    FirstTouch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TouchField {
    Channel,
    ReferrerDomain,
    UtmSource,
    UtmMedium,
    UtmCampaign,
    UtmContent,
    UtmTerm,
    ClickId,
}

/// What counts as a conversion: a goal completion or a revenue event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Conversion {
    Goal(GoalConversion),
    /// Each event with a revenue record; currencies stay apart.
    Revenue,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GoalConversion {
    pub goal: GoalDefinition,
}

/// A goal stored inline, so a saved report keeps the exact goal it was built with (0003 Goals).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GoalDefinition {
    pub predicate: Group,
    pub scope: PopulationScope,
    pub counting: GoalCounting,
    /// The saved goal version this definition was copied from.
    #[serde(rename = "ref", default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub goal_ref: Option<GoalRef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GoalCounting {
    /// The first matching event per session or profile.
    Once,
    /// Every matching event.
    Every,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GoalRef {
    pub id: GoalId,
    /// 1 or higher.
    pub version: u32,
}

/// Conversion of one or more goals over the range (0003 Goals).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GoalsReport {
    pub v: u8,
    /// 1 to 8, each stored inline.
    pub goals: Vec<GoalDefinition>,
}

/// Cohorts by the state's `granularity`, `day` or `week`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RetentionReport {
    pub v: u8,
    /// Offsets 0 to `intervals − 1`: 1 to 32 for day cohorts, 1 to 53 for week cohorts.
    pub intervals: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SessionListReport {
    pub v: u8,
    /// 1 to 16, unique.
    pub columns: Vec<SessionColumn>,
    pub order: SessionOrder,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SessionColumn {
    SessionId,
    IdentityKind,
    Start,
    End,
    /// Integer milliseconds.
    Span,
    Pageviews,
    CustomEvents,
    EntryPath,
    ExitPath,
    ReferrerDomain,
    Channel,
    UtmSource,
    UtmCampaign,
    Country,
    City,
    Device,
    Browser,
    Os,
}

/// Session start order; ties by session ID ascending.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SessionOrder {
    StartDesc,
    StartAsc,
}

/// The output of one report kind, shaped after decision 0003.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReportResult {
    Funnel(FunnelResult),
    Journey(JourneyResult),
    Attribution(AttributionResult),
    Retention(RetentionResult),
    SessionList(SessionListResult),
    Goals(GoalsResult),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
pub struct FunnelResult {
    /// One per step, in step order.
    pub steps: Vec<FunnelStep>,
    /// Identities reaching the last step out of those entering step 1.
    pub conversion: Metric,
    /// Daily scope: the pseudonym changes at local midnight, so no identity progresses across it.
    pub daily_limit: bool,
    /// Daily identity coverage, or consented coverage for person scope.
    pub coverage: Coverage,
    /// Where the identities dropping after each step went: one per step but the last.
    #[serde(default)]
    pub leaks: Vec<FunnelLeak>,
}

/// The identities that reached a step but not the next, by the page their session showed
/// next within the funnel's read span (0003 Funnels).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct FunnelLeak {
    /// No further page in the session: they left.
    pub exited: i64,
    /// Next page was one the session had already shown.
    pub back: i64,
    /// The three most common other next pages, most identities first.
    pub next: Vec<FunnelNext>,
    /// The rest: other next pages, and step events without a session.
    pub other: i64,
    /// The latest sessions that dropped here, newest first, at most 20.
    pub sessions: Vec<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct FunnelNext {
    pub path: String,
    pub identities: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct FunnelStep {
    /// Identities reaching this step.
    pub reached: i64,
    /// `reached` minus the next step's; absent for the last step.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub drop: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
pub struct JourneyResult {
    /// The top 100 by sessions, then by sequence element by element, a prefix first.
    pub sequences: Vec<JourneySequence>,
    /// Distinct sequences, including those beyond the top 100.
    pub distinct_sequences: i64,
    /// Entering sessions. The sessions of all sequences add up to it.
    pub entry_sessions: i64,
    pub coverage: Coverage,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct JourneySequence {
    /// Compressed step values, 1 to `length`.
    pub steps: Vec<String>,
    pub sessions: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
pub struct AttributionResult {
    /// One per conversion, in definition order.
    pub conversions: Vec<AttributedConversion>,
    pub coverage: Coverage,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct AttributedConversion {
    /// The top 100 touch values by conversions descending, then value ascending.
    pub touches: Vec<TouchRow>,
    /// Distinct touch values, including those beyond the top 100.
    pub touch_count: i64,
    /// Conversions without a non-direct touch.
    pub direct: Attributed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct TouchRow {
    /// The value of the report's `touch_field`; not set when the touch lacks it.
    pub touch: Cell,
    pub attributed: Attributed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct Attributed {
    pub conversions: i64,
    /// Revenue conversions only: net amount per currency, by currency code. Never converted.
    pub revenue: Vec<Money>,
}

/// An exact amount of one currency (0002 Currencies and revenue).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct Money {
    /// ISO 4217 code.
    pub currency: String,
    /// Minor-unit digits of the currency: 2 for EUR, 0 for JPY.
    pub exponent: u8,
    /// Integer minor units: a JSON number up to 2^53 − 1 in magnitude, else a decimal string
    /// (0004 Encoding conventions).
    #[serde(with = "big_integer")]
    #[ts(type = "number | string")]
    #[schemars(schema_with = "big_integer::schema")]
    pub minor_units: i128,
    /// The amount as a decimal string with `exponent` fractional digits: `"49.91"`, `"4500"`.
    pub amount: String,
}

impl Money {
    /// The amount of `minor_units` in `currency`; `None` for a code that is not an active
    /// ISO 4217 currency.
    pub fn new(currency: &str, minor_units: i128) -> Option<Self> {
        let exponent = currency_exponent(currency)?;
        let scale = 10_i128.pow(u32::from(exponent));
        let sign = if minor_units < 0 { "-" } else { "" };
        let (whole, fraction) = (
            minor_units.unsigned_abs() / scale as u128,
            minor_units.unsigned_abs() % scale as u128,
        );
        let amount = match exponent {
            0 => format!("{sign}{whole}"),
            e => format!("{sign}{whole}.{fraction:0width$}", width = usize::from(e)),
        };
        Some(Self {
            currency: currency.to_owned(),
            exponent,
            minor_units,
            amount,
        })
    }
}

/// ISO 4217 minor-unit exponent of an active alphabetic code.
pub fn currency_exponent(code: &str) -> Option<u8> {
    const ZERO: &[&str] = &[
        "BIF", "CLP", "DJF", "GNF", "ISK", "JPY", "KMF", "KRW", "PYG", "RWF", "UGX", "UYI", "VND",
        "VUV", "XAF", "XOF", "XPF",
    ];
    const THREE: &[&str] = &["BHD", "IQD", "JOD", "KWD", "LYD", "OMR", "TND"];
    const FOUR: &[&str] = &["CLF", "UYW"];
    const TWO: &[&str] = &[
        "AED", "AFN", "ALL", "AMD", "ANG", "AOA", "ARS", "AUD", "AWG", "AZN", "BAM", "BBD", "BDT",
        "BGN", "BMD", "BND", "BOB", "BOV", "BRL", "BSD", "BTN", "BWP", "BYN", "BZD", "CAD", "CDF",
        "CHE", "CHF", "CHW", "CNY", "COP", "COU", "CRC", "CUC", "CUP", "CVE", "CZK", "DKK", "DOP",
        "DZD", "EGP", "ERN", "ETB", "EUR", "FJD", "FKP", "GBP", "GEL", "GHS", "GIP", "GMD", "GTQ",
        "GYD", "HKD", "HNL", "HTG", "HUF", "IDR", "ILS", "INR", "IRR", "JMD", "KES", "KGS", "KHR",
        "KPW", "KYD", "KZT", "LAK", "LBP", "LKR", "LRD", "LSL", "MAD", "MDL", "MGA", "MKD", "MMK",
        "MNT", "MOP", "MRU", "MUR", "MVR", "MWK", "MXN", "MXV", "MYR", "MZN", "NAD", "NGN", "NIO",
        "NOK", "NPR", "NZD", "PAB", "PEN", "PGK", "PHP", "PKR", "PLN", "QAR", "RON", "RSD", "RUB",
        "SAR", "SBD", "SCR", "SDG", "SEK", "SGD", "SHP", "SLE", "SLL", "SOS", "SRD", "SSP", "STN",
        "SVC", "SYP", "SZL", "THB", "TJS", "TMT", "TOP", "TRY", "TTD", "TWD", "TZS", "UAH", "USD",
        "USN", "UYU", "UZS", "VED", "VES", "WST", "XCD", "XCG", "YER", "ZAR", "ZMW", "ZWG", "ZWL",
    ];
    [(ZERO, 0), (TWO, 2), (THREE, 3), (FOUR, 4)]
        .into_iter()
        .find(|(codes, _)| codes.contains(&code))
        .map(|(_, exponent)| exponent)
}

/// Integers as JSON numbers while JavaScript reads them exactly, else as decimal strings.
mod big_integer {
    use serde::{Deserialize, Deserializer, Serializer, de::Error};

    const SAFE: i128 = (1 << 53) - 1;

    pub fn serialize<S: Serializer>(n: &i128, s: S) -> Result<S::Ok, S::Error> {
        if n.abs() <= SAFE {
            s.serialize_i64(*n as i64)
        } else {
            s.serialize_str(&n.to_string())
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<i128, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Wire {
            Number(i64),
            Text(String),
        }
        match Wire::deserialize(d)? {
            Wire::Number(n) => Ok(i128::from(n)),
            Wire::Text(text) => text.parse().map_err(D::Error::custom),
        }
    }

    pub fn schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "anyOf": [
                { "type": "integer" },
                { "type": "string", "pattern": "^-?[0-9]+$" }
            ]
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
pub struct RetentionResult {
    /// Every cohort period in range, by start, also with size 0.
    pub cohorts: Vec<CohortRow>,
    /// One per offset: active over cohort size, summed over cohorts whose interval is complete.
    pub averages: Vec<Metric>,
    /// Consented eligible events out of all eligible events in the cohort periods.
    pub coverage: Coverage,
    /// Intervals ending after this instant are incomplete: the application clock of the
    /// request, Unix milliseconds.
    pub watermark: i64,
    /// The first retained instant (0006 Retention cutoff). Cohort entry is the first eligible
    /// event since then, so a profile last active before it counts as new. Absent when the
    /// site keeps every event (0016).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub history_start: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
pub struct CohortRow {
    /// Cohort period start, Unix milliseconds.
    pub start: i64,
    /// Cohort period start as local date `YYYY-MM-DD` in the state's timezone.
    pub local: String,
    /// Profiles entering in the period; the denominator of every rate in the row.
    pub size: i64,
    /// One per offset, 0 to `intervals − 1`.
    pub cells: Vec<RetentionCell>,
    pub coverage: Coverage,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
pub struct RetentionCell {
    /// Interval bounds, Unix milliseconds, half-open.
    pub start: i64,
    pub end: i64,
    pub active: i64,
    /// `active / size`; unavailable with `no_denominator` for a size-0 cohort.
    pub rate: Metric,
    /// False when the interval ends after the result watermark. Incomplete cells keep their
    /// current value and stay out of the averages.
    pub complete: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
pub struct SessionListResult {
    /// The requested page. One cell per requested column, in column order; instants and spans
    /// are integer milliseconds.
    pub rows: Vec<Vec<Cell>>,
    /// Sessions across all pages.
    pub row_count: i64,
    pub coverage: Coverage,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
pub struct GoalsResult {
    /// One per goal, in definition order.
    pub goals: Vec<GoalResult>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
pub struct GoalResult {
    /// Sessions or profiles with a matching event (once), or matching events (every).
    pub completions: i64,
    /// Sessions in range, or profiles with an eligible event in range.
    pub denominator: i64,
    /// `completions / denominator`; unavailable with `no_denominator` when nothing is in range.
    pub rate: Metric,
    /// Identity coverage for session scope, consented coverage for person scope.
    pub coverage: Coverage,
    /// With `typical_weekday`: completions against their median baseline, the rate against
    /// the pooled baseline days (0015).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub compare: Option<GoalComparison>,
    /// With `typical_weekday` on a range containing now: where completions end (0015 Pace
    /// projection). Never part of `completions`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub projection: Option<Projection>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
pub struct GoalComparison {
    pub completions: Comparison,
    pub rate: Comparison,
}
