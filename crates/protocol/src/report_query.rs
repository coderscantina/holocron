//! Report queries for agents (decision 0022): named blocks over one or more sites, each a cut
//! or a report, answered as typed tables from one analytical snapshot.
//!
//! A block compiles onto an [`ExplorationState`](crate::exploration::ExplorationState) and runs
//! through the same validator and planner, so a block and the equivalent exploration return the
//! same numbers. Parsing is strict: unknown fields are refused, and every refusal names its
//! path, such as `blocks.2.filter.f3` or `blocks.0.derive.1.b`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::exploration::{
    Cell, Compare, CondKind, Field, Granularity, GroupKind, GroupOp, Measure, Metric, Operator,
    Range, ReportDefinition, ResolvedRange, Sort, Value,
};
use crate::ids::SiteId;
use crate::operations::analytics::Freshness;

/// The only report query version this build reads.
pub const REPORT_QUERY_VERSION: u8 = 1;

/// Named blocks run in order on one snapshot. At most 16 blocks and 200 sub-queries in total,
/// counting each `for_each` value as one; a larger request is refused with `report_too_large`
/// before anything runs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReportQuery {
    /// Always 1.
    pub v: u8,
    /// Range of blocks that name none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub range: Option<Range>,
    /// IANA timezone of blocks that name none. Without either, a block reads its site's timezone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub timezone: Option<String>,
    /// 1 to 16, names unique.
    pub blocks: Vec<Block>,
}

/// One table of the answer: a cut or a report over one site.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Block {
    /// `^[a-z][a-z0-9_]{0,31}$`, unique in the query. Later blocks refer to it by this name.
    pub name: String,
    /// The site this block reads; blocks may name different sites. Needs `site.read` on it.
    pub site: SiteId,
    /// Required unless the query has a range.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub range: Option<Range>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub timezone: Option<String>,
    /// Which events the block reads, as an exploration filter. `in` and `not_in` lists hold up to
    /// 1,000 values and may take them from an earlier block with a [`Ref`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub filter: Option<BlockGroup>,
    /// Runs the block once per value with an added `eq` condition on the field, and puts the
    /// value in a leading column named after the field. Prefer a dimension or a funnel
    /// `group_by` where one exists: they run in one pass.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub for_each: Option<ForEach>,
    pub body: BlockBody,
    /// 0 to 8 columns computed from this block's measure columns. Cuts only.
    #[serde(default)]
    pub derive: Vec<Derived>,
}

/// A filter group as in an exploration state, whose list values may be a [`Ref`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BlockGroup {
    pub kind: GroupKind,
    pub op: GroupOp,
    /// 1 to 32 entries; groups nest at most three levels deep.
    pub items: Vec<BlockNode>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(untagged)]
pub enum BlockNode {
    Group(BlockGroup),
    Cond(BlockCondition),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BlockCondition {
    pub kind: CondKind,
    /// `^[a-z0-9_]{1,16}$`, unique within the block; refusals name it, as `blocks.0.filter.f3`.
    pub id: String,
    pub field: Field,
    pub cmp: Operator,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub value: Option<BlockValue>,
}

/// A literal value, or for `in` and `not_in` the values of an earlier block's column.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(untagged)]
pub enum BlockValue {
    Ref(Ref),
    Value(Value),
}

/// The distinct values of a `dimension` or `for_each` column of an earlier block, in that
/// block's row order. Not-set and null cells are skipped. A ref that finds no values skips the
/// block, which then answers no rows and the note `ref_empty`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Ref {
    #[serde(rename = "ref")]
    pub source: RefSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RefSource {
    /// The name of an earlier block.
    pub block: String,
    /// A column of that block.
    pub column: String,
    /// The first values to take: 1 to 1,000 in a filter (default 1,000), 1 to 50 in `for_each`
    /// (default 50).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ForEach {
    /// An event, session or property field; not one of the cut's dimensions.
    pub field: Field,
    /// 1 to 50 values, or a [`Ref`] with a limit of at most 50.
    pub values: ForEachValues,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(untagged)]
pub enum ForEachValues {
    Ref(Ref),
    Values(Vec<Value>),
}

/// What a block computes: one cut of measures by dimensions, or one report.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BlockBody {
    Cut(CutBody),
    Report(ReportBody),
}

/// Measures by up to three dimensions, as in an exploration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CutBody {
    /// 1 to 8, unique. Columns are named after them: `pageviews`, `sum(event.amount)`.
    pub measures: Vec<Measure>,
    /// 0 to 3, unique. Columns are named after them: `path`, `event.checkout_id`. Without
    /// dimensions or a series, the one row holds the totals.
    #[serde(default)]
    pub dimensions: Vec<Field>,
    /// Bucket size of the series, default `day`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub granularity: Option<Granularity>,
    /// Rows are the time buckets of the totals, in a leading `time` column. Needs no dimensions.
    #[serde(default)]
    pub series: bool,
    /// Adds a `<measure>_previous` column after each measure. Default `none`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub compare: Option<Compare>,
    /// 0 to 2 keys. Ties fall back to the dimension values ascending.
    #[serde(default)]
    pub sort: Vec<Sort>,
    /// Rows to return, 1 to 1,000, default 100. The note `rows_truncated` says when more exist.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub limit: Option<u32>,
}

/// A funnel, journey, attribution, retention or goals report, flattened into rows. Session
/// lists are refused: report queries answer aggregates only.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReportBody {
    pub report: ReportDefinition,
    /// The cohort period of a retention report, `day` (default) or `week`. Other kinds read `day`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub granularity: Option<Granularity>,
}

/// A column computed per row, and for the totals, from measure columns.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Derived {
    /// `a / b`, rounded half to even to 4 places; unavailable with `no_denominator` when `b` is 0.
    Ratio(BinaryDerived),
    /// `a − b`, of the operands' type. Both operands have the same column type.
    Difference(BinaryDerived),
    /// `a` over the total of the same column, a ratio. In a `for_each` block the total is that
    /// value's total.
    Share(ShareDerived),
}

/// Money operands need the same single currency in a cell; otherwise the cell is unavailable
/// with `mixed_currencies`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BinaryDerived {
    /// The column name, `^[a-z][a-z0-9_]{0,31}$`, unique in the block.
    pub name: String,
    pub a: Operand,
    pub b: Operand,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ShareDerived {
    pub name: String,
    pub a: ColumnOperand,
}

/// A measure column of this block, or the total of a measure column of an earlier cut block
/// without `for_each`. Count, duration, decimal and money columns qualify; ratios do not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(untagged)]
pub enum Operand {
    Total(TotalOperand),
    Column(ColumnOperand),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ColumnOperand {
    pub column: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TotalOperand {
    pub block: String,
    pub column: String,
    /// Always `true`.
    pub total: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct ReportQueryResult {
    /// Always 1.
    pub v: u8,
    /// The snapshot every block read.
    pub watermark: ReportWatermark,
    /// Formula version of every number (decision 0003).
    pub metric_version: u32,
    /// One per block, in request order.
    pub blocks: Vec<BlockResult>,
}

/// The analytical snapshot of a report query.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct ReportWatermark {
    /// Last applied batch and the last accepted inbox sequence it contains.
    pub batch: i64,
    pub sequence: i64,
    /// Every record received before this instant is visible, Unix milliseconds.
    pub at: i64,
    /// Analytics is over a minute behind collection.
    pub stale: bool,
}

/// One block's table and its provenance.
#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct BlockResult {
    pub name: String,
    pub site: SiteId,
    /// The range the block read; absent when it did not run (`ref_empty`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub resolved: Option<ResolvedRange>,
    pub timezone: String,
    /// Watermark, correction state, session policy and imported history of the block's range;
    /// absent when it did not run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub freshness: Option<Freshness>,
    pub columns: Vec<Column>,
    /// One cell per column. Keys are typed cells (`{"t":"string","v":"/a"}`), values are metrics
    /// (`{"t":"count","v":12}`, `{"t":"money","v":[...]}` in currency code order).
    pub rows: Vec<Vec<ReportCell>>,
    /// Cuts without `for_each`: one cell per column over every matching row, `null` for
    /// dimension and time columns.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub totals: Option<Vec<ReportCell>>,
    /// Method notes to cite: identity coverage, daily limits, truncation.
    pub notes: Vec<Note>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct Column {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: ColumnType,
    pub role: ColumnRole,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ColumnType {
    String,
    Integer,
    /// Decimal string with 4 places.
    Decimal,
    /// Amounts per currency.
    Money,
    /// Rounded half to even to 4 places, with the exact parts.
    Ratio,
    /// Integer milliseconds.
    DurationMs,
    /// Local date `YYYY-MM-DD` in the block's timezone.
    Date,
    /// RFC 3339 instant.
    Instant,
    Bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ColumnRole {
    /// The value a `for_each` run filtered on.
    ForEach,
    Dimension,
    /// The bucket of a series.
    Time,
    Measure,
    Derived,
}

/// A result cell: a typed key as in an exploration row key, or a computed metric.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(untagged)]
pub enum ReportCell {
    Key(Cell),
    Metric(Metric),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct Note {
    pub code: NoteCode,
    /// English text for the reader of the report.
    pub message: String,
    /// The block or part the note is about, as `blocks.1`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub path: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum NoteCode {
    /// Not every eligible event carries an identity, so visitor and identity counts cover less
    /// than all traffic (0003).
    Coverage,
    /// A daily-scope funnel: the pseudonym changes at local midnight, so nobody progresses across it.
    DailyLimit,
    /// A grouped funnel had more than 50 groups; the rest are left out.
    GroupsTruncated,
    /// More rows exist than the block returns.
    RowsTruncated,
    /// A ref found no values, so the block did not run.
    RefEmpty,
    /// The range spans several identity days, so `visitors` counts visitor-days.
    VisitorDays,
}
