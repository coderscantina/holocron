//! CSV of the behavioral report kinds: funnel steps, journey sequences, attributed touch
//! values, goals and session lists. The JSON export carries the complete result instead.

use holocron_protocol::exploration::{
    Attributed, AttributionResult, Cell, Conversion, ExplorationState, FunnelResult, GoalsResult,
    JourneyResult, Metric, ReportDefinition, ReportResult, SessionColumn, SessionListResult,
};
use holocron_protocol::exploration::{AttributionReport, SessionListReport};

use super::{PROJECTION_COLUMNS, cell, line, metric, projection_cells, quote};

/// Data rows of a report's CSV.
pub fn row_count(report: &ReportResult) -> i64 {
    (match report {
        ReportResult::Funnel(r) => r.steps.len(),
        ReportResult::Journey(r) => r.sequences.len(),
        ReportResult::Attribution(r) => r.conversions.iter().map(|c| c.touches.len() + 1).sum(),
        ReportResult::Retention(r) => r.cohorts.len(),
        ReportResult::SessionList(r) => r.rows.len(),
        ReportResult::Goals(r) => r.goals.len(),
    }) as i64
}

pub fn csv(state: &ExplorationState, report: &ReportResult) -> String {
    match (report, &state.report) {
        (ReportResult::Funnel(r), _) => funnel(r),
        (ReportResult::Journey(r), _) => journey(r),
        (ReportResult::Attribution(r), Some(ReportDefinition::Attribution(d))) => attribution(d, r),
        (ReportResult::SessionList(r), Some(ReportDefinition::SessionList(d))) => {
            session_list(d, r)
        }
        (ReportResult::Goals(r), _) => goals(r),
        _ => unreachable!("a report result has the kind of its state"),
    }
}

fn rate(metric: &Metric) -> String {
    match metric {
        Metric::Ratio { v, .. } => v.to_string(),
        _ => String::new(),
    }
}

fn funnel(result: &FunnelResult) -> String {
    let mut out = line(["step", "reached", "drop"].map(String::from).into_iter());
    for (i, step) in result.steps.iter().enumerate() {
        let drop = step.drop.map(|d| d.to_string()).unwrap_or_default();
        out.push_str(&line(
            [(i + 1).to_string(), step.reached.to_string(), drop].into_iter(),
        ));
    }
    out
}

fn journey(result: &JourneyResult) -> String {
    let width = result
        .sequences
        .iter()
        .map(|s| s.steps.len())
        .max()
        .unwrap_or(1);
    let header = (1..=width)
        .map(|n| format!("step_{n}"))
        .chain(["sessions".to_owned()]);
    let mut out = line(header);
    for sequence in &result.sequences {
        let steps = (0..width).map(|i| sequence.steps.get(i).map(|s| quote(s)).unwrap_or_default());
        out.push_str(&line(steps.chain([sequence.sessions.to_string()])));
    }
    out
}

/// Net revenue per currency in one cell: `30.00 EUR; 5 JPY`.
fn revenue(attributed: &Attributed) -> String {
    attributed
        .revenue
        .iter()
        .map(|m| format!("{} {}", m.amount, m.currency))
        .collect::<Vec<_>>()
        .join("; ")
}

fn attribution(definition: &AttributionReport, result: &AttributionResult) -> String {
    let mut out = line(
        ["conversion", "touch", "conversions", "revenue"]
            .map(String::from)
            .into_iter(),
    );
    for (i, (conversion, attributed)) in definition
        .conversions
        .iter()
        .zip(&result.conversions)
        .enumerate()
    {
        let name = match conversion {
            Conversion::Goal(_) => format!("goal {}", i + 1),
            Conversion::Revenue => "revenue".to_owned(),
        };
        let rows = attributed
            .touches
            .iter()
            .map(|row| (cell(&row.touch), &row.attributed))
            .chain([("(direct)".to_owned(), &attributed.direct)]);
        for (touch, value) in rows {
            out.push_str(&line(
                [
                    name.clone(),
                    touch,
                    value.conversions.to_string(),
                    quote(&revenue(value)),
                ]
                .into_iter(),
            ));
        }
    }
    out
}

/// With `typical_weekday`, each goal also has its baselines and, on a range containing now, the
/// projection of its completions in separately named columns.
fn goals(result: &GoalsResult) -> String {
    let compared = result.goals.iter().any(|g| g.compare.is_some());
    let projected = result.goals.iter().any(|g| g.projection.is_some());
    let mut header: Vec<String> = ["goal", "completions", "denominator", "rate"]
        .map(String::from)
        .into();
    if compared {
        header.extend(["completions_previous", "rate_previous"].map(String::from));
    }
    if projected {
        header.extend(PROJECTION_COLUMNS.map(|part| format!("completions_{part}")));
    }
    let mut out = line(header.into_iter());
    for (i, goal) in result.goals.iter().enumerate() {
        let mut cells = vec![
            (i + 1).to_string(),
            goal.completions.to_string(),
            goal.denominator.to_string(),
            rate(&goal.rate),
        ];
        if compared {
            let compare = goal.compare.as_ref();
            cells.push(compare.map_or_else(String::new, |c| metric(&c.completions.previous)));
            cells.push(compare.map_or_else(String::new, |c| rate(&c.rate.previous)));
        }
        if projected {
            cells.extend(projection_cells(goal.projection.as_ref()));
        }
        out.push_str(&line(cells.into_iter()));
    }
    out
}

fn session_list(definition: &SessionListReport, result: &SessionListResult) -> String {
    let header = definition.columns.iter().map(|c| {
        serde_json::to_value(c)
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default()
    });
    let mut out = line(header);
    for row in &result.rows {
        let cells =
            definition
                .columns
                .iter()
                .zip(row)
                .map(|(column, value)| match (column, value) {
                    // Instants as RFC 3339 for spreadsheets; spans stay integer milliseconds.
                    (SessionColumn::Start | SessionColumn::End, Cell::Integer(ms)) => {
                        holocron_analytics::explore::instant(*ms)
                    }
                    _ => cell(value),
                });
        out.push_str(&line(cells));
    }
    out
}
