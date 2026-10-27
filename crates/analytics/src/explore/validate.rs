//! Turns a wire exploration state into a checked [`Query`]: limits, unique predicate IDs,
//! operators matching the field and value types, supported visualizations, and the rules of
//! each report kind (0004 Report definitions). Everything refused here is refused before any
//! engine work, with the field it concerns.

use std::collections::HashSet;

use chrono_tz::Tz;
use holocron_protocol::errors::{ErrorCode, FieldErrorCode};
use holocron_protocol::exploration::{
    AttributionReport, Compare, Condition, Conversion, DimensionField, ExplorationState,
    FUNNEL_VERSION, Field, FunnelReport, GoalDefinition, GoalsReport, Granularity, Group, GroupOp,
    JourneyReport, Measure, NamedMeasure, Node, Operator, Page, Panel, PropertyScope,
    REPORT_VERSION, ReportDefinition, RetentionReport, STATE_VERSION, SessionListReport, Sort,
    SortKey, Value, Visualization,
};
use holocron_protocol::limits::{
    EXPLORATION_CONDITIONS_MAX, EXPLORATION_DIMENSIONS_MAX, EXPLORATION_FILTER_DEPTH_MAX,
    EXPLORATION_GROUP_ITEMS_MAX, EXPLORATION_LIST_ITEMS_MAX, EXPLORATION_MEASURES_MAX,
    EXPLORATION_PAGE_SIZE_DEFAULT, EXPLORATION_PANELS_MAX, EXPLORATION_PROPERTY_KEY_MAX_BYTES,
    EXPLORATION_ROWS_MAX, EXPLORATION_SORT_MAX, PAGE_SIZE_MAX, REPORT_ATTRIBUTION_CONVERSIONS_MAX,
    REPORT_ATTRIBUTION_LOOKBACK_DAYS, REPORT_FUNNEL_STEPS_MAX, REPORT_FUNNEL_STEPS_MIN,
    REPORT_FUNNEL_WINDOW_MAX_SECONDS, REPORT_FUNNEL_WINDOW_MIN_SECONDS, REPORT_GOALS_MAX,
    REPORT_JOURNEY_LENGTH_MAX, REPORT_JOURNEY_LENGTH_MIN, REPORT_QUERY_LIST_ITEMS_MAX,
    REPORT_RETENTION_DAY_INTERVALS_MAX, REPORT_RETENTION_WEEK_INTERVALS_MAX,
    REPORT_SESSION_LIST_COLUMNS_MAX,
};

use super::glob::{self, Glob};
use crate::METRIC_VERSION;

/// Limits that differ between callers of [`validate_with`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StateLimits {
    /// Values in one `in` or `not_in` list.
    pub list_items: usize,
}

impl StateLimits {
    /// An exploration state, which lives in URLs (0004 Limits).
    pub const EXPLORATION: Self = Self {
        list_items: EXPLORATION_LIST_ITEMS_MAX,
    };
    /// A report query (0022).
    pub const REPORT_QUERY: Self = Self {
        list_items: REPORT_QUERY_LIST_ITEMS_MAX,
    };
}

/// Why a state cannot run, naming the field: `measures`, `filter.f3`, `panels[1].sort`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    pub code: ErrorCode,
    pub field: String,
    pub reason: FieldErrorCode,
    pub limit: Option<u32>,
    pub message: String,
}

impl Refusal {
    pub(crate) fn invalid(field: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(
            ErrorCode::InvalidArguments,
            field,
            FieldErrorCode::InvalidValue,
            message,
        )
    }

    /// A comparison this state cannot have; never answered with another comparison instead.
    pub(crate) fn compare_unsupported(message: impl Into<String>) -> Self {
        Self::new(
            ErrorCode::CompareUnsupported,
            "compare",
            FieldErrorCode::InvalidValue,
            message,
        )
    }

    pub(crate) fn new(
        code: ErrorCode,
        field: impl Into<String>,
        reason: FieldErrorCode,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code,
            field: field.into(),
            reason,
            limit: None,
            message: message.into(),
        }
    }

    pub(crate) fn too_many(
        code: ErrorCode,
        field: impl Into<String>,
        limit: usize,
        message: impl Into<String>,
    ) -> Self {
        Self {
            limit: Some(limit as u32),
            ..Self::new(code, field, FieldErrorCode::TooMany, message)
        }
    }
}

/// A checked filter tree.
#[derive(Debug, Clone, PartialEq)]
pub enum Pred {
    All(Vec<Pred>),
    Any(Vec<Pred>),
    Cond(Cond),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Cond {
    pub id: String,
    pub field: Field,
    pub test: Test,
    /// Negative operators match exactly the rows the positive one does not (0004).
    pub negate: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Order {
    Eq,
    Gt,
    Gte,
    Lt,
    Lte,
}

/// What a condition tests, with values already parsed.
#[derive(Debug, Clone, PartialEq)]
pub enum Test {
    StrEq(String),
    StrContains(String),
    StrStartsWith(String),
    StrIn(Vec<String>),
    StrMatches(Glob),
    /// Integer fields (`weekday`, `hour`).
    Int(Order, i64),
    IntBetween(i64, i64),
    /// Integer and decimal property rows, compared as values scaled by 10^4.
    Num(Order, i128),
    NumBetween(i128, i128),
    /// Date property rows, Unix milliseconds.
    Date(Order, i64),
    DateBetween(i64, i64),
    Bool(bool),
    /// An array property holding an element equal to the typed value, given as its JSON text.
    /// For a string value, string rows containing the text match too.
    Contains {
        text: Option<String>,
        element: String,
    },
    IsSet,
    IsNull,
}

/// One cut: measures, dimensions, sort and the page window.
#[derive(Debug, Clone, PartialEq)]
pub struct CutSpec {
    pub panel: Panel,
    pub measures: Vec<Measure>,
    pub dimensions: Vec<Field>,
    pub sort: Vec<Sort>,
    pub offset: u32,
    pub size: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Query {
    pub state: ExplorationState,
    pub timezone: Tz,
    pub filter: Option<Pred>,
    /// Without measures, dimensions or sort when the state has a report.
    pub main: CutSpec,
    pub panels: Vec<CutSpec>,
    /// The state's visualization draws a series of its totals.
    pub series: bool,
    pub report: Option<Report>,
}

/// A checked report definition. Kinds with predicate trees carry them parsed.
#[derive(Debug, Clone, PartialEq)]
pub enum Report {
    Funnel(CheckedFunnel),
    Journey(JourneyReport),
    Attribution(CheckedAttribution),
    Retention(RetentionReport),
    SessionList(SessionListReport),
    Goals(CheckedGoals),
}

#[derive(Debug, Clone, PartialEq)]
pub struct CheckedFunnel {
    pub definition: FunnelReport,
    /// One per step, in step order.
    pub steps: Vec<Pred>,
}

impl CheckedFunnel {
    /// The field identities are grouped by, version 2 only.
    pub fn group_by(&self) -> Option<&Field> {
        self.definition.group_by.as_ref()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CheckedAttribution {
    pub definition: AttributionReport,
    /// One per conversion: its goal predicate, `None` for revenue.
    pub goals: Vec<Option<Pred>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CheckedGoals {
    pub definition: GoalsReport,
    /// One per goal, in definition order.
    pub goals: Vec<Pred>,
}

fn step_path(i: usize) -> String {
    format!("report.steps.{i}")
}

fn goal_path(i: usize) -> String {
    format!("report.conversions.{i}.goal.predicate")
}

impl Report {
    /// Every predicate tree of the report, with the path naming it in refusals.
    pub fn trees(&self) -> Vec<(String, &Pred)> {
        match self {
            Report::Funnel(funnel) => funnel
                .steps
                .iter()
                .enumerate()
                .map(|(i, step)| (step_path(i), step))
                .collect(),
            Report::Attribution(attribution) => attribution
                .goals
                .iter()
                .enumerate()
                .filter_map(|(i, goal)| goal.as_ref().map(|goal| (goal_path(i), goal)))
                .collect(),
            Report::Goals(goals) => goals
                .goals
                .iter()
                .enumerate()
                .map(|(i, goal)| (format!("report.goals.{i}.predicate"), goal))
                .collect(),
            Report::Journey(_) | Report::Retention(_) | Report::SessionList(_) => Vec::new(),
        }
    }
}

impl Query {
    pub fn cuts(&self) -> impl Iterator<Item = &CutSpec> {
        std::iter::once(&self.main).chain(&self.panels)
    }

    /// The filter and every report predicate tree, with the paths naming them in refusals.
    pub fn trees(&self) -> Vec<(String, &Pred)> {
        let filter = self.filter.iter().map(|f| ("filter".to_owned(), f));
        let report = self.report.iter().flat_map(Report::trees);
        filter.chain(report).collect()
    }

    /// Every field the filter, dimensions, sorts and property measures read.
    pub fn fields(&self) -> Vec<Field> {
        let mut fields = Vec::new();
        let mut add = |field: &Field| {
            if !fields.contains(field) {
                fields.push(field.clone());
            }
        };
        if let Some(filter) = &self.filter {
            filter.visit(&mut |cond| add(&cond.field));
        }
        for cut in self.cuts() {
            cut.dimensions.iter().for_each(&mut add);
            for measure in &cut.measures {
                if let Measure::Property(stat) = measure {
                    add(&Field::Property(stat.field.clone()));
                }
            }
        }
        fields
    }
}

impl Pred {
    pub fn visit(&self, f: &mut impl FnMut(&Cond)) {
        match self {
            Pred::All(items) | Pred::Any(items) => items.iter().for_each(|item| item.visit(f)),
            Pred::Cond(cond) => f(cond),
        }
    }

    /// The first condition, depth first, that `test` accepts.
    pub fn find(&self, test: &impl Fn(&Cond) -> bool) -> Option<&Cond> {
        match self {
            Pred::All(items) | Pred::Any(items) => items.iter().find_map(|item| item.find(test)),
            Pred::Cond(cond) => test(cond).then_some(cond),
        }
    }
}

/// Checks a state and its panels with the limits of an exploration state.
pub fn validate(state: &ExplorationState, panels: &[Panel]) -> Result<Query, Refusal> {
    validate_with(state, panels, StateLimits::EXPLORATION)
}

/// [`validate`] with the limits of another caller, such as a report query.
pub fn validate_with(
    state: &ExplorationState,
    panels: &[Panel],
    limits: StateLimits,
) -> Result<Query, Refusal> {
    if state.v != STATE_VERSION {
        return Err(Refusal::invalid(
            "v",
            format!("state version {} is not supported", state.v),
        ));
    }
    if state.metric_version != METRIC_VERSION {
        return Err(Refusal::invalid(
            "metric_version",
            format!(
                "the state is pinned to metric version {}; this server computes version {METRIC_VERSION}",
                state.metric_version
            ),
        ));
    }
    let timezone: Tz = state
        .timezone
        .parse()
        .map_err(|_| Refusal::invalid("timezone", "unknown IANA timezone"))?;
    match state.visualization {
        _ if state.report.is_some() => {}
        Visualization::Funnel | Visualization::Journey | Visualization::Cohort => {
            return Err(Refusal::new(
                ErrorCode::ReportMissing,
                "visualization",
                FieldErrorCode::InvalidValue,
                "this visualization needs a report definition",
            ));
        }
        Visualization::Map => {
            return Err(Refusal::invalid(
                "visualization",
                "the map visualization is not available",
            ));
        }
        Visualization::Grid
            if state.dimensions
                != [
                    Field::Dimension(DimensionField::Weekday),
                    Field::Dimension(DimensionField::Hour),
                ] =>
        {
            return Err(Refusal::invalid(
                "dimensions",
                "the grid needs exactly the dimensions weekday and hour",
            ));
        }
        _ => {}
    }

    let mut trees = Trees::new(limits);
    let filter = state
        .filter
        .as_ref()
        .map(|group| trees.check("filter", group))
        .transpose()?;
    // Percentiles need a performance metric to rank (0004 State shape).
    let perf = filter.as_ref().is_some_and(|f| {
        f.find(&|c| c.field == Field::Dimension(DimensionField::PerfMetric))
            .is_some()
    });
    let panel = Panel {
        measures: state.measures.clone(),
        dimensions: state.dimensions.clone(),
        sort: state.sort.clone(),
        page: state.page,
    };
    let (main, report) = match &state.report {
        None => (cut("", &panel, perf)?, None),
        Some(definition) => {
            let report = report(state, definition, &mut trees)?;
            if !panels.is_empty() {
                return Err(Refusal::new(
                    ErrorCode::ReportFieldUnsupported,
                    "panels",
                    FieldErrorCode::InvalidValue,
                    "a report has no panels",
                ));
            }
            let page = page_window("page", state.page)?;
            let main = CutSpec {
                panel: Panel { page, ..panel },
                measures: Vec::new(),
                dimensions: Vec::new(),
                sort: Vec::new(),
                offset: page.index * page.size,
                size: page.size,
            };
            (main, Some(report))
        }
    };
    if panels.len() > EXPLORATION_PANELS_MAX {
        return Err(Refusal::too_many(
            ErrorCode::InvalidArguments,
            "panels",
            EXPLORATION_PANELS_MAX,
            "too many panels",
        ));
    }
    let panels = panels
        .iter()
        .enumerate()
        .map(|(i, panel)| cut(&format!("panels[{i}]."), panel, perf))
        .collect::<Result<_, _>>()?;
    let query = Query {
        state: state.clone(),
        timezone,
        filter,
        main,
        panels,
        series: report.is_none()
            && matches!(
                state.visualization,
                Visualization::Line | Visualization::Bar
            ),
        report,
    };
    performance(&query)?;
    typical_weekday(&query)?;
    Ok(query)
}

/// A filter tree on its own, with the limits of a state's `filter` (0004 Limits): for lists
/// that take the exploration filter, such as replays. Refusals name `filter`. Like report
/// trees, it matches pageviews and custom events, never performance observations.
pub fn validate_filter(group: &Group) -> Result<Pred, Refusal> {
    let pred = Trees::new(StateLimits::EXPLORATION).check("filter", group)?;
    if let Some(cond) = pred.find(&|c| c.field == PERF_METRIC) {
        return Err(Refusal::invalid(
            format!("filter.{}", cond.id),
            "this filter reads pageviews and custom events, never performance observations",
        ));
    }
    Ok(pred)
}

/// Checks a report against the rest of the state (0004 "How each kind uses the rest of the
/// state") and its own limits.
fn report(
    state: &ExplorationState,
    definition: &ReportDefinition,
    trees: &mut Trees,
) -> Result<Report, Refusal> {
    let (v, visualizations, granularities): (u8, &[Visualization], &[Granularity]) =
        match definition {
            ReportDefinition::Funnel(r) => (r.v, &[Visualization::Funnel], &[Granularity::Day]),
            ReportDefinition::Journey(r) => (r.v, &[Visualization::Journey], &[Granularity::Day]),
            ReportDefinition::Attribution(r) => (
                r.v,
                &[Visualization::Table, Visualization::Bar],
                &[Granularity::Day],
            ),
            ReportDefinition::Retention(r) => (
                r.v,
                &[Visualization::Cohort],
                &[Granularity::Day, Granularity::Week],
            ),
            ReportDefinition::SessionList(r) => (r.v, &[Visualization::Table], &[Granularity::Day]),
            ReportDefinition::Goals(r) => (
                r.v,
                &[Visualization::Table, Visualization::Bar],
                &[Granularity::Day],
            ),
            ReportDefinition::Unsupported => {
                return Err(Refusal::new(
                    ErrorCode::ReportVersionUnsupported,
                    "report.kind",
                    FieldErrorCode::InvalidValue,
                    "this report kind is not supported",
                ));
            }
        };
    let known = match definition {
        ReportDefinition::Funnel(_) => (1..=FUNNEL_VERSION).contains(&v),
        _ => v == REPORT_VERSION,
    };
    if !known {
        return Err(Refusal::new(
            ErrorCode::ReportVersionUnsupported,
            "report.v",
            FieldErrorCode::InvalidValue,
            format!("report version {v} is not supported"),
        ));
    }
    let unsupported = |field: &str, message: &str| {
        Refusal::new(
            ErrorCode::ReportFieldUnsupported,
            field,
            FieldErrorCode::InvalidValue,
            message,
        )
    };
    if !state.measures.is_empty() {
        return Err(unsupported("measures", "a report defines its own output"));
    }
    if !state.dimensions.is_empty() {
        return Err(unsupported("dimensions", "a report defines its own output"));
    }
    if !visualizations.contains(&state.visualization) {
        return Err(unsupported(
            "visualization",
            "this visualization does not show this report",
        ));
    }
    if !granularities.contains(&state.granularity) {
        return Err(unsupported(
            "granularity",
            "this report does not use this granularity",
        ));
    }
    match (state.compare, definition) {
        (Compare::None, _) | (Compare::TypicalWeekday, ReportDefinition::Goals(_)) => {}
        (Compare::TypicalWeekday, _) => {
            return Err(Refusal::compare_unsupported(
                "only the goals report compares with a typical weekday",
            ));
        }
        (Compare::PreviousPeriod, _) => {
            return Err(unsupported("compare", "reports have no comparison"));
        }
    }
    if !state.sort.is_empty() {
        return Err(unsupported("sort", "a report has a fixed order"));
    }
    if !matches!(definition, ReportDefinition::SessionList(_)) && state.page != DEFAULT_PAGE {
        return Err(unsupported("page", "only session lists page"));
    }

    let invalid = |field: &str, message: &str| {
        Refusal::new(
            ErrorCode::ReportInvalid,
            field,
            FieldErrorCode::InvalidValue,
            message,
        )
    };
    Ok(match definition {
        ReportDefinition::Funnel(r) => {
            count(
                "report.steps",
                r.steps.len(),
                REPORT_FUNNEL_STEPS_MIN..=REPORT_FUNNEL_STEPS_MAX,
            )?;
            let window = REPORT_FUNNEL_WINDOW_MIN_SECONDS..=REPORT_FUNNEL_WINDOW_MAX_SECONDS;
            if !window.contains(&r.window_seconds) {
                return Err(invalid(
                    "report.window_seconds",
                    "the window is 60 seconds to 30 days",
                ));
            }
            let steps = r
                .steps
                .iter()
                .enumerate()
                .map(|(i, step)| trees.check(&step_path(i), step))
                .collect::<Result<_, _>>()?;
            if let Some(field) = &r.group_by {
                if r.v < 2 {
                    return Err(unsupported(
                        "report.group_by",
                        "grouping a funnel needs funnel version 2",
                    ));
                }
                if let Field::Property(property) = field {
                    property_key("report.group_by", &property.key)?;
                }
                if *field == PERF_METRIC {
                    return Err(Refusal::invalid(
                        "report.group_by",
                        "reports read pageviews and custom events, never performance observations",
                    ));
                }
            }
            Report::Funnel(CheckedFunnel {
                definition: r.clone(),
                steps,
            })
        }
        ReportDefinition::Journey(r) => {
            if !(REPORT_JOURNEY_LENGTH_MIN..=REPORT_JOURNEY_LENGTH_MAX).contains(&r.length) {
                return Err(invalid("report.length", "a journey has 2 to 7 steps"));
            }
            Report::Journey(r.clone())
        }
        ReportDefinition::Attribution(r) => {
            if r.lookback_days != REPORT_ATTRIBUTION_LOOKBACK_DAYS {
                return Err(invalid(
                    "report.lookback_days",
                    "the lookback is 30 days in this version",
                ));
            }
            count(
                "report.conversions",
                r.conversions.len(),
                1..=REPORT_ATTRIBUTION_CONVERSIONS_MAX,
            )?;
            let mut goals = Vec::with_capacity(r.conversions.len());
            for (i, conversion) in r.conversions.iter().enumerate() {
                goals.push(match conversion {
                    Conversion::Goal(goal) => {
                        goal_ref(&format!("report.conversions.{i}.goal.ref"), &goal.goal)?;
                        Some(trees.check(&goal_path(i), &goal.goal.predicate)?)
                    }
                    Conversion::Revenue => None,
                });
            }
            Report::Attribution(CheckedAttribution {
                definition: r.clone(),
                goals,
            })
        }
        ReportDefinition::Retention(r) => {
            let max = match state.granularity {
                Granularity::Week => REPORT_RETENTION_WEEK_INTERVALS_MAX,
                _ => REPORT_RETENTION_DAY_INTERVALS_MAX,
            };
            count(
                "report.intervals",
                usize::from(r.intervals),
                1..=usize::from(max),
            )?;
            Report::Retention(r.clone())
        }
        ReportDefinition::SessionList(r) => {
            count(
                "report.columns",
                r.columns.len(),
                1..=REPORT_SESSION_LIST_COLUMNS_MAX,
            )?;
            if !unique(&r.columns) {
                return Err(invalid("report.columns", "a column is listed twice"));
            }
            Report::SessionList(r.clone())
        }
        ReportDefinition::Goals(r) => {
            count("report.goals", r.goals.len(), 1..=REPORT_GOALS_MAX)?;
            let mut goals = Vec::with_capacity(r.goals.len());
            for (i, goal) in r.goals.iter().enumerate() {
                goal_ref(&format!("report.goals.{i}.ref"), goal)?;
                goals.push(trees.check(&format!("report.goals.{i}.predicate"), &goal.predicate)?);
            }
            Report::Goals(CheckedGoals {
                definition: r.clone(),
                goals,
            })
        }
        ReportDefinition::Unsupported => unreachable!("refused above"),
    })
}

/// Saved goal versions start at 1.
fn goal_ref(field: &str, goal: &GoalDefinition) -> Result<(), Refusal> {
    if goal.goal_ref.as_ref().is_some_and(|r| r.version == 0) {
        return Err(Refusal::new(
            ErrorCode::ReportInvalid,
            field,
            FieldErrorCode::InvalidValue,
            "goal versions start at 1",
        ));
    }
    Ok(())
}

/// Refuses a report list or number outside `range` with `report_invalid` and the crossed limit.
fn count(field: &str, n: usize, range: std::ops::RangeInclusive<usize>) -> Result<(), Refusal> {
    let (reason, limit) = if n < *range.start() {
        (FieldErrorCode::TooShort, *range.start())
    } else if n > *range.end() {
        (FieldErrorCode::TooMany, *range.end())
    } else {
        return Ok(());
    };
    Err(Refusal {
        limit: Some(limit as u32),
        ..Refusal::new(
            ErrorCode::ReportInvalid,
            field,
            reason,
            format!("allowed are {} to {}", range.start(), range.end()),
        )
    })
}

pub fn is_percentile(measure: &Measure) -> bool {
    matches!(
        measure,
        Measure::Named(NamedMeasure::P50 | NamedMeasure::P75 | NamedMeasure::P95)
    )
}

/// A measure over revenue records.
pub fn is_revenue(measure: &Measure) -> bool {
    is_money(measure) || *measure == Measure::Named(NamedMeasure::RevenueCount)
}

/// A revenue amount: a list of per-currency values, not one number.
pub fn is_money(measure: &Measure) -> bool {
    matches!(
        measure,
        Measure::Named(
            NamedMeasure::RevenueNet | NamedMeasure::RevenueGross | NamedMeasure::RevenueRefunds
        )
    )
}

const PERF_METRIC: Field = Field::Dimension(DimensionField::PerfMetric);

/// A typical-weekday baseline is a median for counts and money (per currency) and a pooled
/// population for rates and spans (0015). Percentiles, identified people and property
/// statistics have none.
fn typical_weekday(query: &Query) -> Result<(), Refusal> {
    if query.state.compare != Compare::TypicalWeekday {
        return Ok(());
    }
    let baselined = |measure: &Measure| {
        matches!(
            measure,
            Measure::Named(
                NamedMeasure::Pageviews
                    | NamedMeasure::Events
                    | NamedMeasure::Visitors
                    | NamedMeasure::VisitorDays
                    | NamedMeasure::Sessions
                    | NamedMeasure::RevenueCount
                    | NamedMeasure::RevenueNet
                    | NamedMeasure::RevenueGross
                    | NamedMeasure::RevenueRefunds
                    | NamedMeasure::BounceRate
                    | NamedMeasure::SpanMean
                    | NamedMeasure::SpanMedian
            )
        )
    };
    match query
        .cuts()
        .flat_map(|cut| &cut.measures)
        .find(|m| !baselined(m))
    {
        Some(measure) => Err(Refusal::compare_unsupported(format!(
            "{} has no typical-weekday baseline",
            serde_json::to_string(measure).expect("measures serialize")
        ))),
        None => Ok(()),
    }
}

/// Performance observations are rows of their own (one per reported metric), so a state that
/// reads them measures nothing but percentiles, and reports never read them.
fn performance(query: &Query) -> Result<(), Refusal> {
    let observations = query
        .cuts()
        .any(|cut| cut.measures.iter().any(is_percentile) || cut.dimensions.contains(&PERF_METRIC))
        || query
            .filter
            .as_ref()
            .is_some_and(|f| f.find(&|c| c.field == PERF_METRIC).is_some());
    if query.report.is_some() {
        for (path, tree) in query.trees() {
            if let Some(cond) = tree.find(&|c| c.field == PERF_METRIC) {
                return Err(Refusal::invalid(
                    format!("{path}.{}", cond.id),
                    "reports read pageviews and custom events, never performance observations",
                ));
            }
        }
        return Ok(());
    }
    if !observations {
        return Ok(());
    }
    for (i, cut) in query.cuts().enumerate() {
        if !cut.measures.iter().all(is_percentile) {
            let prefix = match i {
                0 => String::new(),
                n => format!("panels[{}].", n - 1),
            };
            return Err(Refusal::invalid(
                format!("{prefix}measures"),
                "performance observations are measured by p50, p75 and p95 only",
            ));
        }
    }
    Ok(())
}

fn cut(prefix: &str, panel: &Panel, perf: bool) -> Result<CutSpec, Refusal> {
    let field = |name: &str| format!("{prefix}{name}");
    let measures = &panel.measures;
    if measures.is_empty() {
        return Err(Refusal::new(
            ErrorCode::InvalidArguments,
            field("measures"),
            FieldErrorCode::Required,
            "choose at least one measure",
        ));
    }
    if measures.len() > EXPLORATION_MEASURES_MAX {
        return Err(Refusal::too_many(
            ErrorCode::InvalidArguments,
            field("measures"),
            EXPLORATION_MEASURES_MAX,
            "too many measures",
        ));
    }
    if !unique(measures) {
        return Err(Refusal::invalid(
            field("measures"),
            "a measure is listed twice",
        ));
    }
    let percentile = |m: &Measure| {
        matches!(
            m,
            Measure::Named(NamedMeasure::P50 | NamedMeasure::P75 | NamedMeasure::P95)
        )
    };
    if !perf && measures.iter().any(percentile) {
        return Err(Refusal::invalid(
            field("measures"),
            "percentiles need a perf_metric condition in the filter",
        ));
    }
    for measure in measures {
        if let Measure::Property(stat) = measure {
            property_key(&field("measures"), &stat.field.key)?;
            if stat.field.scope == PropertyScope::Session {
                return Err(Refusal::invalid(
                    field("measures"),
                    "statistics over session properties are not available",
                ));
            }
        }
    }
    if panel.dimensions.len() > EXPLORATION_DIMENSIONS_MAX {
        return Err(Refusal::too_many(
            ErrorCode::InvalidArguments,
            field("dimensions"),
            EXPLORATION_DIMENSIONS_MAX,
            "too many dimensions",
        ));
    }
    if !unique(&panel.dimensions) {
        return Err(Refusal::invalid(
            field("dimensions"),
            "a dimension is listed twice",
        ));
    }
    for dimension in &panel.dimensions {
        if let Field::Property(property) = dimension {
            property_key(&field("dimensions"), &property.key)?;
        }
    }
    if panel.sort.len() > EXPLORATION_SORT_MAX {
        return Err(Refusal::too_many(
            ErrorCode::InvalidArguments,
            field("sort"),
            EXPLORATION_SORT_MAX,
            "too many sort keys",
        ));
    }
    for sort in &panel.sort {
        let known = match &sort.by {
            SortKey::Measure(m) => measures.contains(m),
            SortKey::Field(f) => panel.dimensions.contains(f),
        };
        if !known {
            return Err(Refusal::invalid(
                field("sort"),
                "sort by a measure or dimension of the view",
            ));
        } // Amounts of different currencies do not compare, so each row must hold one currency.
        if matches!(&sort.by, SortKey::Measure(m) if is_money(m))
            && !panel
                .dimensions
                .contains(&Field::Dimension(DimensionField::Currency))
        {
            return Err(Refusal::invalid(
                field("sort"),
                "sorting by a revenue amount needs the currency dimension",
            ));
        }
    }
    let page = page_window(&field("page"), panel.page)?;
    let Page { size, index } = page;
    Ok(CutSpec {
        panel: Panel {
            page,
            ..panel.clone()
        },
        measures: measures.clone(),
        dimensions: panel.dimensions.clone(),
        sort: panel.sort.clone(),
        offset: index * size,
        size,
    })
}

/// The page with its size clamped to 100, refused past the addressable rows.
fn page_window(field: &str, Page { size, index }: Page) -> Result<Page, Refusal> {
    if size == 0 {
        return Err(Refusal::invalid(field, "page size must be at least 1"));
    }
    let size = size.min(PAGE_SIZE_MAX);
    let end = (u64::from(index) + 1) * u64::from(size);
    if end > u64::from(EXPLORATION_ROWS_MAX) {
        return Err(Refusal {
            limit: Some(EXPLORATION_ROWS_MAX),
            ..Refusal::new(
                ErrorCode::PageOutOfRange,
                field,
                FieldErrorCode::TooMany,
                "only the first 10,000 rows can be paged",
            )
        });
    }
    Ok(Page { size, index })
}

fn unique<T: PartialEq>(items: &[T]) -> bool {
    items
        .iter()
        .enumerate()
        .all(|(i, item)| !items[..i].contains(item))
}

fn property_key(field: &str, key: &str) -> Result<(), Refusal> {
    if key.is_empty() || key.len() > EXPLORATION_PROPERTY_KEY_MAX_BYTES {
        return Err(Refusal::invalid(field, "property keys are 1 to 500 bytes"));
    }
    Ok(())
}

/// Checks the predicate trees of one state: the filter and every report predicate. IDs are
/// unique and conditions are counted across all trees; depth applies to each tree on its own.
struct Trees {
    limits: StateLimits,
    ids: HashSet<String>,
    conditions: usize,
}

impl Trees {
    fn new(limits: StateLimits) -> Self {
        Self {
            limits,
            ids: HashSet::new(),
            conditions: 0,
        }
    }

    /// `path` names the tree in refusals: `filter`, `report.steps.2`.
    fn check(&mut self, path: &str, root: &Group) -> Result<Pred, Refusal> {
        self.group(path, root, 1)
    }

    fn group(&mut self, path: &str, g: &Group, depth: usize) -> Result<Pred, Refusal> {
        if depth > EXPLORATION_FILTER_DEPTH_MAX {
            return Err(Refusal::too_many(
                ErrorCode::FilterTooDeep,
                path,
                EXPLORATION_FILTER_DEPTH_MAX,
                "groups nest at most three levels deep",
            ));
        }
        if g.items.is_empty() || g.items.len() > EXPLORATION_GROUP_ITEMS_MAX {
            return Err(Refusal::invalid(path, "a group holds 1 to 32 entries"));
        }
        let items = g
            .items
            .iter()
            .map(|item| match item {
                Node::Group(inner) => self.group(path, inner, depth + 1),
                Node::Cond(c) => {
                    self.conditions += 1;
                    if self.conditions > EXPLORATION_CONDITIONS_MAX {
                        return Err(Refusal::too_many(
                            ErrorCode::TooManyConditions,
                            path,
                            EXPLORATION_CONDITIONS_MAX,
                            "at most 32 conditions",
                        ));
                    }
                    if !valid_id(&c.id) {
                        return Err(Refusal::invalid(
                            path,
                            "predicate IDs match ^[a-z0-9_]{1,16}$",
                        ));
                    }
                    if !self.ids.insert(c.id.clone()) {
                        return Err(Refusal::new(
                            ErrorCode::DuplicatePredicateId,
                            format!("{path}.{}", c.id),
                            FieldErrorCode::InvalidValue,
                            "two conditions share this ID",
                        ));
                    }
                    condition(path, c, self.limits).map(Pred::Cond)
                }
            })
            .collect::<Result<_, _>>()?;
        Ok(match g.op {
            GroupOp::And => Pred::All(items),
            GroupOp::Or => Pred::Any(items),
        })
    }
}

fn valid_id(id: &str) -> bool {
    (1..=16).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// Text fields a `matches` glob applies to, besides string properties.
const GLOB_FIELDS: [DimensionField; 6] = [
    DimensionField::Path,
    DimensionField::EntryPath,
    DimensionField::ReferrerPath,
    DimensionField::Title,
    DimensionField::Query,
    DimensionField::Hostname,
];

fn condition(tree: &str, c: &Condition, limits: StateLimits) -> Result<Cond, Refusal> {
    let path = format!("{tree}.{}", c.id);
    let refuse = |message: &str| Refusal::invalid(path.clone(), message);
    if let Field::Property(p) = &c.field {
        property_key(&path, &p.key)?;
    }
    let (op, negate) = match c.cmp {
        Operator::Neq => (Operator::Eq, true),
        Operator::NotContains => (Operator::Contains, true),
        Operator::NotIn => (Operator::In, true),
        Operator::IsNotSet => (Operator::IsSet, true),
        Operator::NotMatches => (Operator::Matches, true),
        other => (other, false),
    };
    let kind = FieldKind::of(&c.field);
    let test = match (op, &c.value) {
        (Operator::Matches, Some(Value::String(pattern)))
            if match &c.field {
                Field::Dimension(d) => GLOB_FIELDS.contains(d),
                Field::Property(_) => true,
            } =>
        {
            Test::StrMatches(glob::parse(pattern).map_err(refuse)?)
        }
        (Operator::IsSet, None) => Test::IsSet,
        (Operator::IsNull, None) if kind == FieldKind::Property => Test::IsNull,
        (Operator::IsSet | Operator::IsNull, _) => {
            return Err(refuse("this operator takes no value"));
        }
        (_, None) => return Err(refuse("this operator needs a value")),
        (op, Some(value)) => test(tree, op, value, kind, limits)
            .ok_or_else(|| refuse("the operator does not apply to this field and value"))??,
    };
    Ok(Cond {
        id: c.id.clone(),
        field: c.field.clone(),
        test,
        negate,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FieldKind {
    Text,
    Integer,
    Property,
}

impl FieldKind {
    fn of(field: &Field) -> Self {
        match field {
            Field::Dimension(DimensionField::Weekday | DimensionField::Hour) => Self::Integer,
            Field::Dimension(_) => Self::Text,
            Field::Property(_) => Self::Property,
        }
    }
}

fn order(op: Operator) -> Option<Order> {
    Some(match op {
        Operator::Eq => Order::Eq,
        Operator::Gt => Order::Gt,
        Operator::Gte => Order::Gte,
        Operator::Lt => Order::Lt,
        Operator::Lte => Order::Lte,
        _ => return None,
    })
}

/// `None` when the operator does not fit the field and value types; `Some(Err)` when it fits
/// but the value does not parse.
fn test(
    tree: &str,
    op: Operator,
    value: &Value,
    kind: FieldKind,
    limits: StateLimits,
) -> Option<Result<Test, Refusal>> {
    let bad = |message: &str| Some(Err(Refusal::invalid(tree, message)));
    Some(Ok(match (kind, op, value) {
        (FieldKind::Text | FieldKind::Property, Operator::Eq, Value::String(s)) => {
            Test::StrEq(s.clone())
        }
        (FieldKind::Text, Operator::Contains, Value::String(s)) => Test::StrContains(s.clone()),
        (FieldKind::Property, Operator::Contains, Value::String(s)) => Test::Contains {
            text: Some(s.clone()),
            element: serde_json::Value::from(s.as_str()).to_string(),
        },
        (FieldKind::Text | FieldKind::Property, Operator::StartsWith, Value::String(s)) => {
            Test::StrStartsWith(s.clone())
        }
        (FieldKind::Text | FieldKind::Property, Operator::In, Value::List(items)) => {
            if items.is_empty() || items.len() > limits.list_items {
                return Some(Err(Refusal::too_many(
                    ErrorCode::InvalidArguments,
                    tree,
                    limits.list_items,
                    format!("a list holds 1 to {} values", limits.list_items),
                )));
            }
            let mut strings = Vec::with_capacity(items.len());
            for item in items {
                match item {
                    Value::String(s) => strings.push(s.clone()),
                    _ => return None,
                }
            }
            Test::StrIn(strings)
        }
        (FieldKind::Integer, op, Value::Integer(n)) => Test::Int(order(op)?, *n),
        (FieldKind::Integer, Operator::Between, Value::List(items)) => match items.as_slice() {
            [Value::Integer(a), Value::Integer(b)] => Test::IntBetween(*a, *b),
            _ => return bad("between takes two integers"),
        },
        (FieldKind::Property, Operator::Contains, Value::Integer(n)) => Test::Contains {
            text: None,
            element: n.to_string(),
        },
        (FieldKind::Property, Operator::Contains, Value::Bool(b)) => Test::Contains {
            text: None,
            element: b.to_string(),
        },
        (FieldKind::Property, op, Value::Integer(_) | Value::Decimal(_)) => match scaled(value) {
            Ok(n) => Test::Num(order(op)?, n),
            Err(message) => return bad(message),
        },
        (FieldKind::Property, op, Value::Date(text)) => match date_ms(text) {
            Ok(ms) => Test::Date(order(op)?, ms),
            Err(message) => return bad(message),
        },
        (FieldKind::Property, Operator::Eq, Value::Bool(b)) => Test::Bool(*b),
        (FieldKind::Property, Operator::Between, Value::List(items)) => match items.as_slice() {
            [
                a @ (Value::Integer(_) | Value::Decimal(_)),
                b @ (Value::Integer(_) | Value::Decimal(_)),
            ] => match (scaled(a), scaled(b)) {
                (Ok(a), Ok(b)) => Test::NumBetween(a, b),
                (Err(message), _) | (_, Err(message)) => return bad(message),
            },
            [Value::Date(a), Value::Date(b)] => match (date_ms(a), date_ms(b)) {
                (Ok(a), Ok(b)) => Test::DateBetween(a, b),
                (Err(message), _) | (_, Err(message)) => return bad(message),
            },
            _ => return bad("between takes two numbers or two dates"),
        },
        _ => return None,
    }))
}

/// Integer or decimal value scaled by 10^4, the storage scale of decimals.
fn scaled(value: &Value) -> Result<i128, &'static str> {
    match value {
        Value::Integer(n) => Ok(i128::from(*n) * 10_000),
        Value::Decimal(text) => {
            parse_decimal(text).ok_or("decimals have up to 4 fractional digits")
        }
        _ => Err("expected a number"),
    }
}

fn parse_decimal(text: &str) -> Option<i128> {
    let (negative, digits) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    let (whole, fraction) = digits.split_once('.').unwrap_or((digits, ""));
    let digit = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
    if whole.is_empty()
        || whole.len() > 30
        || fraction.len() > 4
        || !digit(whole)
        || !digit(fraction)
    {
        return None;
    }
    let value =
        whole.parse::<i128>().ok()? * 10_000 + format!("{fraction:0<4}").parse::<i128>().ok()?;
    Some(if negative { -value } else { value })
}

fn date_ms(text: &str) -> Result<i64, &'static str> {
    chrono::DateTime::parse_from_rfc3339(text)
        .map(|d| d.timestamp_millis())
        .map_err(|_| "dates are RFC 3339 instants")
}

/// The default page of a new state, for callers building one.
pub const DEFAULT_PAGE: Page = Page {
    size: EXPLORATION_PAGE_SIZE_DEFAULT,
    index: 0,
};

/// Measures computed from sessions rather than events.
pub fn is_session_measure(measure: &Measure) -> bool {
    matches!(
        measure,
        Measure::Named(
            NamedMeasure::Sessions
                | NamedMeasure::BounceRate
                | NamedMeasure::SpanMean
                | NamedMeasure::SpanMedian
        )
    )
}

#[cfg(test)]
mod tests {
    use super::parse_decimal;

    #[test]
    fn decimals_scale_by_ten_thousand() {
        assert_eq!(parse_decimal("1"), Some(10_000));
        assert_eq!(parse_decimal("2.5"), Some(25_000));
        assert_eq!(parse_decimal("-0.0412"), Some(-412));
        assert_eq!(parse_decimal("1.23456"), None);
        assert_eq!(parse_decimal("1e3"), None);
        assert_eq!(parse_decimal(".5"), None);
    }
}
