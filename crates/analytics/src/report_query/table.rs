//! The columns of a block and the rows an exploration result flattens into (0022 Result).
//!
//! Keys are typed cells, values are the exploration's metrics unchanged. Funnel leaks are left
//! out, so session IDs never reach a report query.

use holocron_protocol::exploration::{
    Cell, Compare, Comparison, Coverage, DimensionField, ExploreResult, Field, FunnelResult,
    FunnelStep, Granularity, Measure, Metric, NamedMeasure, PropertyScope, Reason,
    ReportDefinition, ReportResult, Stat, StepDuration, Value, VisitorLabel,
};
use holocron_protocol::report_query::{
    Block, BlockBody, Column, ColumnRole, ColumnType, CutBody, Derived, NoteCode, ReportCell,
};

use crate::explore::{instant, ratio4};

/// Funnel duration columns after `from_step`: percentiles, then the five buckets.
const DURATION_COLUMNS: [&str; 8] = [
    "p25_ms",
    "median_ms",
    "p75_ms",
    "bucket_lt_1h",
    "bucket_1h_24h",
    "bucket_1d_7d",
    "bucket_7d_30d",
    "bucket_ge_30d",
];

/// One column as planned. A type of `None` comes from the cells, else `fallback`.
pub(super) struct Spec {
    pub name: String,
    pub role: ColumnRole,
    pub kind: Option<ColumnType>,
    pub fallback: ColumnType,
}

impl Spec {
    fn new(name: impl Into<String>, role: ColumnRole, kind: ColumnType) -> Self {
        Self {
            name: name.into(),
            role,
            kind: Some(kind),
            fallback: kind,
        }
    }

    fn inferred(name: impl Into<String>, role: ColumnRole, fallback: ColumnType) -> Self {
        Self {
            name: name.into(),
            role,
            kind: None,
            fallback,
        }
    }

    fn key(field: &Field, role: ColumnRole) -> Self {
        let name = field_name(field);
        match field {
            Field::Dimension(DimensionField::Weekday | DimensionField::Hour) => {
                Self::new(name, role, ColumnType::Integer)
            }
            Field::Dimension(_) => Self::new(name, role, ColumnType::String),
            Field::Property(_) => Self::inferred(name, role, ColumnType::String),
        }
    }
}

/// The planned columns of a block, in order: `for_each`, then the body's, then derived ones.
pub(super) struct Layout {
    specs: Vec<Spec>,
}

impl Layout {
    pub fn of(block: &Block) -> Self {
        use ColumnRole::{Dimension, Measure as Value};
        use ColumnType::{Bool, Date, DurationMs, Instant, Integer, Money, Ratio, String};
        let mut specs = Vec::new();
        if let Some(for_each) = &block.for_each {
            specs.push(Spec::key(&for_each.field, ColumnRole::ForEach));
        }
        match &block.body {
            BlockBody::Cut(cut) => {
                if cut.series {
                    let kind = match cut.granularity {
                        Some(Granularity::Hour) => Instant,
                        _ => Date,
                    };
                    specs.push(Spec::new("time", ColumnRole::Time, kind));
                }
                specs.extend(cut.dimensions.iter().map(|d| Spec::key(d, Dimension)));
                for measure in &cut.measures {
                    let name = measure_name(measure);
                    let spec = |name: std::string::String| match measure_kind(measure) {
                        Ok(kind) => Spec::new(name, Value, kind),
                        Err(fallback) => Spec::inferred(name, Value, fallback),
                    };
                    if compared(cut) {
                        specs.push(spec(name.clone()));
                        specs.push(spec(format!("{name}_previous")));
                    } else {
                        specs.push(spec(name));
                    }
                }
                for derived in &block.derive {
                    specs.push(match derived {
                        Derived::Ratio(d) => Spec::new(&d.name, ColumnRole::Derived, Ratio),
                        Derived::Share(d) => Spec::new(&d.name, ColumnRole::Derived, Ratio),
                        Derived::Difference(d) => {
                            Spec::inferred(&d.name, ColumnRole::Derived, Integer)
                        }
                    });
                }
            }
            BlockBody::Report(report) => match &report.report {
                ReportDefinition::Funnel(funnel) => {
                    if let Some(field) = &funnel.group_by {
                        specs.push(Spec {
                            name: "group".into(),
                            ..Spec::key(field, Dimension)
                        });
                    }
                    specs.push(Spec::new("step", Dimension, Integer));
                    specs.push(Spec::new("from_step", Dimension, Integer));
                    specs.push(Spec::new("reached", Value, Integer));
                    specs.push(Spec::new("drop", Value, Integer));
                    specs.push(Spec::new("conversion", Value, Ratio));
                    for (i, name) in DURATION_COLUMNS.iter().enumerate() {
                        let kind = if i < 3 { DurationMs } else { Integer };
                        specs.push(Spec::new(*name, Value, kind));
                    }
                }
                ReportDefinition::Journey(journey) => {
                    for i in 1..=journey.length {
                        specs.push(Spec::new(format!("step_{i}"), Dimension, String));
                    }
                    specs.push(Spec::new("identities", Value, Integer));
                }
                ReportDefinition::Attribution(_) => {
                    specs.push(Spec::new("conversion", Dimension, Integer));
                    specs.push(Spec::new("touch", Dimension, String));
                    specs.push(Spec::new("direct", Dimension, Bool));
                    specs.push(Spec::new("conversions", Value, Integer));
                    specs.push(Spec::new("revenue", Value, Money));
                }
                ReportDefinition::Retention(_) => {
                    specs.push(Spec::new("cohort", Dimension, Date));
                    specs.push(Spec::new("offset", Dimension, Integer));
                    specs.push(Spec::new("cohort_size", Value, Integer));
                    specs.push(Spec::new("retained", Value, Integer));
                    specs.push(Spec::new("rate", Value, Ratio));
                    specs.push(Spec::new("complete", Dimension, Bool));
                }
                ReportDefinition::Goals(_) => {
                    specs.push(Spec::new("goal", Dimension, Integer));
                    specs.push(Spec::new("completions", Value, Integer));
                    specs.push(Spec::new("denominator", Value, Integer));
                    specs.push(Spec::new("rate", Value, Ratio));
                }
                ReportDefinition::SessionList(_) | ReportDefinition::Unsupported => {}
            },
        }
        Self { specs }
    }

    pub fn specs(&self) -> &[Spec] {
        &self.specs
    }

    pub fn index(&self, name: &str) -> Option<usize> {
        self.specs.iter().position(|spec| spec.name == name)
    }

    /// The result columns, with types the plan left open read from the first typed cell.
    pub fn columns(&self, rows: &[Vec<ReportCell>]) -> Vec<Column> {
        self.specs
            .iter()
            .enumerate()
            .map(|(i, spec)| Column {
                name: spec.name.clone(),
                kind: spec.kind.unwrap_or_else(|| {
                    rows.iter()
                        .find_map(|row| row.get(i).and_then(cell_type))
                        .unwrap_or(spec.fallback)
                }),
                role: spec.role,
            })
            .collect()
    }
}

/// One run's rows and totals, without the `for_each` and derived columns, and its notes.
pub(super) struct Part {
    pub rows: Vec<Vec<ReportCell>>,
    pub totals: Option<Vec<ReportCell>>,
    pub notes: Vec<(NoteCode, String)>,
}

pub(super) fn flatten(block: &Block, result: &ExploreResult) -> Part {
    match (&block.body, &result.report) {
        (BlockBody::Cut(cut), _) => flatten_cut(cut, result),
        (BlockBody::Report(_), Some(report)) => flatten_report(block, report),
        (BlockBody::Report(_), None) => unreachable!("a report state answers a report"),
    }
}

fn compared(cut: &CutBody) -> bool {
    cut.compare.is_some_and(|c| c != Compare::None)
}

fn flatten_cut(cut: &CutBody, result: &ExploreResult) -> Part {
    let main = &result.main;
    let compared = compared(cut);
    let values = |values: &[Metric], compare: Option<&Vec<Comparison>>| {
        let mut cells = Vec::with_capacity(values.len() * 2);
        for (j, value) in values.iter().enumerate() {
            cells.push(ReportCell::Metric(value.clone()));
            if compared {
                cells.push(
                    compare
                        .and_then(|c| c.get(j))
                        .map_or(null(), |c| ReportCell::Metric(c.previous.clone())),
                );
            }
        }
        cells
    };
    let mut notes = Vec::new();
    let rows = match (&main.series, cut.series) {
        (Some(series), true) => series
            .points
            .iter()
            .map(|point| {
                let time = match series.granularity {
                    Granularity::Hour => Cell::Date(instant(point.start)),
                    _ => Cell::String(point.local[..10].to_owned()),
                };
                let mut row = vec![ReportCell::Key(time)];
                row.extend(values(&point.values, point.compare.as_ref()));
                row
            })
            .collect(),
        _ => {
            if main.row_count > main.rows.len() as i64 {
                notes.push((
                    NoteCode::RowsTruncated,
                    format!(
                        "{} of {} rows; raise `limit` (up to 1,000) or narrow the filter",
                        main.rows.len(),
                        main.row_count
                    ),
                ));
            }
            main.rows
                .iter()
                .map(|row| {
                    let mut cells: Vec<_> = row.key.iter().cloned().map(ReportCell::Key).collect();
                    cells.extend(values(&row.values, row.compare.as_ref()));
                    cells
                })
                .collect()
        }
    };
    let keys = cut.dimensions.len() + usize::from(cut.series);
    let mut totals = vec![null(); keys];
    totals.extend(values(&main.totals, main.compare.as_ref()));
    // Without dimensions or series the totals are the one row, so `for_each` runs line up.
    let rows = if keys == 0 {
        vec![totals.clone()]
    } else {
        rows
    };
    notes.extend(coverage(&main.identity_coverage));
    if cut
        .measures
        .contains(&Measure::Named(NamedMeasure::Visitors))
        && result.visitor_label == VisitorLabel::VisitorDays
    {
        notes.push((
            NoteCode::VisitorDays,
            "the range spans several identity days, so `visitors` counts visitor-days".into(),
        ));
    }
    Part {
        rows,
        totals: Some(totals),
        notes,
    }
}

fn flatten_report(block: &Block, report: &ReportResult) -> Part {
    let mut notes = Vec::new();
    let rows = match report {
        ReportResult::Funnel(funnel) => {
            if funnel.daily_limit {
                notes.push((
                    NoteCode::DailyLimit,
                    "daily scope: the pseudonym changes at local midnight, so nobody progresses across it".into(),
                ));
            }
            if funnel.groups_truncated {
                notes.push((
                    NoteCode::GroupsTruncated,
                    "more than 50 groups entered; the rest are left out".into(),
                ));
            }
            notes.extend(coverage(&funnel.coverage));
            funnel_rows(funnel)
        }
        ReportResult::Journey(journey) => {
            let length = match &block.body {
                BlockBody::Report(r) => match &r.report {
                    ReportDefinition::Journey(j) => usize::from(j.length),
                    _ => 0,
                },
                BlockBody::Cut(_) => 0,
            };
            if journey.distinct_sequences > journey.sequences.len() as i64 {
                notes.push((
                    NoteCode::RowsTruncated,
                    format!(
                        "the top {} of {} distinct sequences",
                        journey.sequences.len(),
                        journey.distinct_sequences
                    ),
                ));
            }
            notes.extend(coverage(&journey.coverage));
            journey
                .sequences
                .iter()
                .map(|sequence| {
                    let mut row: Vec<_> = (0..length)
                        .map(|i| {
                            ReportCell::Key(
                                sequence
                                    .steps
                                    .get(i)
                                    .map_or(Cell::NotSet, |s| Cell::String(s.clone())),
                            )
                        })
                        .collect();
                    row.push(count(sequence.sessions));
                    row
                })
                .collect()
        }
        ReportResult::Attribution(attribution) => {
            notes.extend(coverage(&attribution.coverage));
            let mut rows = Vec::new();
            for (k, conversion) in attribution.conversions.iter().enumerate() {
                let number = integer(k + 1);
                if conversion.touch_count > conversion.touches.len() as i64 {
                    notes.push((
                        NoteCode::RowsTruncated,
                        format!(
                            "conversion {}: the top {} of {} touch values",
                            k + 1,
                            conversion.touches.len(),
                            conversion.touch_count
                        ),
                    ));
                }
                let touches = conversion
                    .touches
                    .iter()
                    .map(|t| (t.touch.clone(), false, &t.attributed));
                let direct = std::iter::once((Cell::NotSet, true, &conversion.direct));
                for (touch, direct, attributed) in touches.chain(direct) {
                    rows.push(vec![
                        number.clone(),
                        ReportCell::Key(touch),
                        ReportCell::Key(Cell::Bool(direct)),
                        count(attributed.conversions),
                        ReportCell::Metric(Metric::Money {
                            v: attributed.revenue.clone(),
                        }),
                    ]);
                }
            }
            rows
        }
        ReportResult::Retention(retention) => {
            notes.extend(coverage(&retention.coverage));
            retention
                .cohorts
                .iter()
                .flat_map(|cohort| {
                    cohort.cells.iter().enumerate().map(|(offset, cell)| {
                        vec![
                            ReportCell::Key(Cell::String(cohort.local.clone())),
                            integer(offset),
                            count(cohort.size),
                            count(cell.active),
                            ReportCell::Metric(cell.rate.clone()),
                            ReportCell::Key(Cell::Bool(cell.complete)),
                        ]
                    })
                })
                .collect()
        }
        ReportResult::Goals(goals) => goals
            .goals
            .iter()
            .enumerate()
            .map(|(k, goal)| {
                if let Some((code, message)) = coverage(&goal.coverage) {
                    notes.push((code, format!("goal {}: {message}", k + 1)));
                }
                vec![
                    integer(k + 1),
                    count(goal.completions),
                    count(goal.denominator),
                    ReportCell::Metric(goal.rate.clone()),
                ]
            })
            .collect(),
        ReportResult::SessionList(_) => unreachable!("session lists are refused in plan"),
    };
    Part {
        rows,
        totals: None,
        notes,
    }
}

/// Per group with `group_by`, else once: a row per step with its duration from the previous
/// step, then a row from step 1 to the last step with the time to convert.
fn funnel_rows(funnel: &FunnelResult) -> Vec<Vec<ReportCell>> {
    if funnel.groups.is_empty() {
        return step_rows(None, &funnel.steps, &funnel.conversion, &funnel.durations);
    }
    funnel
        .groups
        .iter()
        .flat_map(|g| step_rows(Some(&g.value), &g.steps, &g.conversion, &g.durations))
        .collect()
}

fn step_rows(
    group: Option<&Cell>,
    steps: &[FunnelStep],
    conversion: &Metric,
    durations: &[StepDuration],
) -> Vec<Vec<ReportCell>> {
    let Some(first) = steps.first() else {
        return Vec::new();
    };
    let row = |step: usize, from: Option<usize>, cells: [ReportCell; 3], duration| {
        let mut row: Vec<_> = group.cloned().map(ReportCell::Key).into_iter().collect();
        row.push(integer(step));
        row.push(from.map_or(null(), integer));
        row.extend(cells);
        row.extend(duration_cells(duration));
        row
    };
    let mut rows = Vec::with_capacity(steps.len() + 1);
    for (i, step) in steps.iter().enumerate() {
        let cells = [
            count(step.reached),
            step.drop.map_or(null(), count),
            ReportCell::Metric(rate(step.reached, first.reached)),
        ];
        let duration = i.checked_sub(1).and_then(|d| durations.get(d));
        rows.push(row(i + 1, i.checked_sub(1).map(|d| d + 1), cells, duration));
    }
    let last = steps.len();
    let cells = [
        count(steps[last - 1].reached),
        null(),
        ReportCell::Metric(conversion.clone()),
    ];
    rows.push(row(last, Some(1), cells, durations.get(last - 1)));
    rows
}

fn duration_cells(duration: Option<&StepDuration>) -> Vec<ReportCell> {
    match duration {
        None => vec![null(); DURATION_COLUMNS.len()],
        Some(d) => [&d.p25, &d.median, &d.p75]
            .into_iter()
            .map(|m| ReportCell::Metric(m.clone()))
            .chain(d.buckets.iter().map(|&n| count(n)))
            .collect(),
    }
}

fn rate(numerator: i64, denominator: i64) -> Metric {
    match denominator {
        0 => Metric::Unavailable {
            reason: Reason::NoDenominator,
        },
        _ => Metric::Ratio {
            v: ratio4(i128::from(numerator), i128::from(denominator)),
            numerator,
            denominator,
        },
    }
}

fn coverage(coverage: &Coverage) -> Option<(NoteCode, String)> {
    (coverage.with_identity < coverage.eligible_events).then(|| {
        (
            NoteCode::Coverage,
            format!(
                "{} of {} eligible events carry a visitor identity; visitor and identity counts cover those only",
                coverage.with_identity, coverage.eligible_events
            ),
        )
    })
}

fn null() -> ReportCell {
    ReportCell::Key(Cell::Null)
}

fn integer(n: usize) -> ReportCell {
    ReportCell::Key(Cell::Integer(n as i64))
}

fn count(v: i64) -> ReportCell {
    ReportCell::Metric(Metric::Count { v })
}

fn cell_type(cell: &ReportCell) -> Option<ColumnType> {
    Some(match cell {
        ReportCell::Key(key) => match key {
            Cell::String(_) => ColumnType::String,
            Cell::Integer(_) => ColumnType::Integer,
            Cell::Decimal(_) => ColumnType::Decimal,
            Cell::Date(_) => ColumnType::Instant,
            Cell::Bool(_) => ColumnType::Bool,
            Cell::Null | Cell::NotSet => return None,
        },
        ReportCell::Metric(metric) => match metric {
            Metric::Count { .. } => ColumnType::Integer,
            Metric::Ratio { .. } => ColumnType::Ratio,
            Metric::Millis { .. } => ColumnType::DurationMs,
            Metric::Decimal { .. } => ColumnType::Decimal,
            Metric::Money { .. } => ColumnType::Money,
            Metric::Unavailable { .. } => return None,
        },
    })
}

/// The column type of a measure, or `Err` with the fallback when it depends on the data:
/// percentiles are milliseconds except for CLS, property statistics decimals except counts.
fn measure_kind(measure: &Measure) -> Result<ColumnType, ColumnType> {
    match measure {
        Measure::Named(named) => match named {
            NamedMeasure::BounceRate => Ok(ColumnType::Ratio),
            NamedMeasure::SpanMean | NamedMeasure::SpanMedian => Ok(ColumnType::DurationMs),
            NamedMeasure::RevenueNet
            | NamedMeasure::RevenueGross
            | NamedMeasure::RevenueRefunds => Ok(ColumnType::Money),
            NamedMeasure::P50 | NamedMeasure::P75 | NamedMeasure::P95 => {
                Err(ColumnType::DurationMs)
            }
            _ => Ok(ColumnType::Integer),
        },
        Measure::Property(stat) if stat.stat == Stat::Count => Ok(ColumnType::Integer),
        Measure::Property(_) => Err(ColumnType::Decimal),
    }
}

/// The wire name of a field: `path`, `event.checkout_id`.
pub(super) fn field_name(field: &Field) -> String {
    match field {
        Field::Dimension(d) => wire_name(d),
        Field::Property(p) => {
            let scope = match p.scope {
                PropertyScope::Event => "event",
                PropertyScope::Session => "session",
            };
            format!("{scope}.{}", p.key)
        }
    }
}

/// The column name of a measure: `pageviews`, `sum(event.amount)`.
pub(super) fn measure_name(measure: &Measure) -> String {
    match measure {
        Measure::Named(named) => wire_name(named),
        Measure::Property(stat) => format!(
            "{}({})",
            wire_name(&stat.stat),
            field_name(&Field::Property(stat.field.clone()))
        ),
    }
}

fn wire_name(unit: &impl serde::Serialize) -> String {
    match serde_json::to_value(unit) {
        Ok(serde_json::Value::String(name)) => name,
        _ => unreachable!("unit variants serialize to their name"),
    }
}

pub(super) fn cell_of(value: &Value) -> Cell {
    match value {
        Value::String(s) => Cell::String(s.clone()),
        Value::Integer(n) => Cell::Integer(*n),
        Value::Decimal(s) => Cell::Decimal(s.clone()),
        Value::Date(s) => Cell::Date(s.clone()),
        Value::Bool(b) => Cell::Bool(*b),
        Value::List(_) => Cell::Null,
    }
}

/// The value a ref takes from a key cell; none for null and not-set cells.
pub(super) fn value_of(cell: &Cell) -> Option<Value> {
    Some(match cell {
        Cell::String(s) => Value::String(s.clone()),
        Cell::Integer(n) => Value::Integer(*n),
        Cell::Decimal(s) => Value::Decimal(s.clone()),
        Cell::Date(s) => Value::Date(s.clone()),
        Cell::Bool(b) => Value::Bool(*b),
        Cell::Null | Cell::NotSet => return None,
    })
}

pub(super) fn value_text(value: &Value) -> String {
    match value {
        Value::String(s) | Value::Decimal(s) | Value::Date(s) => s.clone(),
        Value::Integer(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::List(items) => format!("{} values", items.len()),
    }
}
