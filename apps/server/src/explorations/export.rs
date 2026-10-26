//! Export files of an exploration. JSON carries the state, resolved range, freshness and the
//! complete result, so it reproduces exactly. CSV carries the rows for spreadsheets; its file
//! name names the site and the resolved dates.

use holocron_protocol::exploration::{
    Cell, Comparison, ExplorationState, ExploreResult, Field, Granularity, Measure, Metric,
    NamedMeasure, Projection, ReportResult, RetentionResult, Stat,
};
use holocron_protocol::operations::explorations::{ExportFile, ExportFormat};
use serde_json::json;

use super::scope_name;

mod reports;

pub fn file(
    state: &ExplorationState,
    result: &ExploreResult,
    format: ExportFormat,
    domain: &str,
    now_ms: i64,
) -> Result<ExportFile, serde_json::Error> {
    let day = |ms: i64| {
        chrono::DateTime::from_timestamp_millis(ms)
            .map(|d| d.format("%Y%m%d").to_string())
            .unwrap_or_default()
    };
    let stem = format!(
        "{}-{}-{}",
        domain.replace(
            |c: char| !c.is_ascii_alphanumeric() && c != '.' && c != '-',
            "_"
        ),
        day(result.resolved.start),
        day(result.resolved.end - 1)
    );
    let row_count = match &result.report {
        Some(report) => reports::row_count(report),
        None => result.main.rows.len() as i64,
    };
    Ok(match format {
        ExportFormat::Json => ExportFile {
            filename: format!("{stem}.json"),
            content_type: "application/json".into(),
            content: serde_json::to_string_pretty(&json!({
                "exported_at": now_ms,
                "state": state,
                "result": result,
            }))?,
            row_count,
        },
        ExportFormat::Csv => ExportFile {
            filename: format!("{stem}.csv"),
            content_type: "text/csv".into(),
            content: csv(state, result),
            row_count,
        },
    })
}

fn csv(state: &ExplorationState, result: &ExploreResult) -> String {
    match &result.report {
        Some(ReportResult::Retention(retention)) => return retention_csv(state, retention),
        Some(report) => return reports::csv(state, report),
        None => {}
    }
    let main = &result.main;
    let compared = main.compare.is_some();
    // Only the totals row has a projection, and it is an estimate: never a measured column.
    let projection = main
        .projection
        .as_ref()
        .filter(|_| state.dimensions.is_empty());
    let mut header: Vec<String> = state.dimensions.iter().map(field_name).collect();
    for measure in &state.measures {
        let name = measure_name(measure);
        header.push(name.clone());
        if compared {
            header.extend(
                ["previous", "change", "relative_change"].map(|part| format!("{name}_{part}")),
            );
        }
        if projection.is_some() {
            header.extend(PROJECTION_COLUMNS.map(|part| format!("{name}_{part}")));
        }
    }
    let mut out = line(header.iter().map(|h| quote(h)));
    let mut add_row = |key: Vec<String>,
                       values: &[Metric],
                       compare: Option<&Vec<Comparison>>,
                       projection: Option<&Vec<Option<Projection>>>| {
        let mut cells = key;
        for (n, value) in values.iter().enumerate() {
            cells.push(metric(value));
            if let Some(compare) = compare {
                let c = &compare[n];
                cells.push(metric(&c.previous));
                cells.push(c.change.map(|v| v.to_string()).unwrap_or_default());
                cells.push(c.relative_change.map(|v| v.to_string()).unwrap_or_default());
            }
            if let Some(projection) = projection {
                cells.extend(projection_cells(projection[n].as_ref()));
            }
        }
        out.push_str(&line(cells.into_iter()));
    };
    if state.dimensions.is_empty() {
        add_row(Vec::new(), &main.totals, main.compare.as_ref(), projection);
    }
    for row in &main.rows {
        add_row(
            row.key.iter().map(cell).collect(),
            &row.values,
            row.compare.as_ref(),
            None,
        );
    }
    out
}

/// Column suffixes of a projection: its value and the 25th to 75th percentile band.
const PROJECTION_COLUMNS: [&str; 3] = ["projection", "projection_low", "projection_high"];

/// A projection's cells, empty when there is none or it is unavailable. Money lists one
/// amount per currency, as its measured cell does, rounded to the currency's minor unit.
fn projection_cells(projection: Option<&Projection>) -> [String; 3] {
    match projection {
        Some(Projection::Estimate {
            value, low, high, ..
        }) => [value, low, high].map(|v| v.to_string()),
        Some(Projection::MoneyEstimate { v, .. }) => [0, 1, 2].map(|i| {
            v.iter()
                .map(|m| {
                    let minor = [m.value, m.low, m.high][i];
                    let digits = usize::from(m.exponent);
                    let amount = minor / 10_f64.powi(i32::from(m.exponent));
                    format!("{amount:.digits$} {}", m.currency)
                })
                .collect::<Vec<_>>()
                .join("; ")
        }),
        Some(Projection::Unavailable { .. }) | None => Default::default(),
    }
}

fn line(cells: impl Iterator<Item = String>) -> String {
    let mut text = cells.collect::<Vec<_>>().join(",");
    text.push_str("\r\n");
    text
}

/// RFC 4180 quoting. Text that a spreadsheet would read as a formula gets a leading quote mark.
fn quote(text: &str) -> String {
    let guarded = if text.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        format!("'{text}")
    } else {
        text.to_owned()
    };
    if guarded.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", guarded.replace('"', "\"\""))
    } else {
        guarded
    }
}

fn cell(cell: &Cell) -> String {
    match cell {
        Cell::String(s) | Cell::Decimal(s) | Cell::Date(s) => quote(s),
        Cell::Integer(n) => n.to_string(),
        Cell::Bool(b) => b.to_string(),
        Cell::Null => "(null)".into(),
        Cell::NotSet => "(not set)".into(),
    }
}

fn metric(metric: &Metric) -> String {
    match metric {
        Metric::Count { v } | Metric::Millis { v } => v.to_string(),
        Metric::Ratio { v, .. } => v.to_string(),
        Metric::Decimal { v } => v.clone(),
        // Exact amounts, one per currency; currencies are never added up.
        Metric::Money { v } => v
            .iter()
            .map(|m| format!("{} {}", m.amount, m.currency))
            .collect::<Vec<_>>()
            .join("; "),
        Metric::Unavailable { .. } => String::new(),
    }
}

/// One row per cohort with its size and coverage, then per offset the active profiles, the
/// rate and whether the interval is complete; the size-weighted averages close the table.
fn retention_csv(state: &ExplorationState, retention: &RetentionResult) -> String {
    let unit = match state.granularity {
        Granularity::Week => "week",
        _ => "day",
    };
    let offsets = retention.averages.len();
    let mut header: Vec<String> = ["cohort", "size", "consented_events", "eligible_events"]
        .map(str::to_owned)
        .to_vec();
    for k in 0..offsets {
        header.extend(["active", "rate", "complete"].map(|part| format!("{unit}_{k}_{part}")));
    }
    let mut out = line(header.into_iter());
    for row in &retention.cohorts {
        let mut cells = vec![
            row.local.clone(),
            row.size.to_string(),
            row.coverage.with_identity.to_string(),
            row.coverage.eligible_events.to_string(),
        ];
        for cell in &row.cells {
            cells.push(cell.active.to_string());
            cells.push(metric(&cell.rate));
            cells.push(cell.complete.to_string());
        }
        out.push_str(&line(cells.into_iter()));
    }
    let mut average = vec![
        "average".to_owned(),
        String::new(),
        retention.coverage.with_identity.to_string(),
        retention.coverage.eligible_events.to_string(),
    ];
    for value in &retention.averages {
        average.extend([String::new(), metric(value), String::new()]);
    }
    out.push_str(&line(average.into_iter()));
    out
}

fn field_name(field: &Field) -> String {
    match field {
        Field::Dimension(d) => serde_json::to_value(d)
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default(),
        Field::Property(p) => format!("{}.{}", scope_name(p.scope), p.key),
    }
}

fn measure_name(measure: &Measure) -> String {
    match measure {
        Measure::Named(named) => match named {
            NamedMeasure::Pageviews => "pageviews",
            NamedMeasure::Events => "events",
            NamedMeasure::Visitors => "visitors",
            NamedMeasure::VisitorDays => "visitor_days",
            NamedMeasure::IdentifiedPeople => "identified_people",
            NamedMeasure::Sessions => "sessions",
            NamedMeasure::BounceRate => "bounce_rate",
            NamedMeasure::SpanMean => "span_mean_ms",
            NamedMeasure::SpanMedian => "span_median_ms",
            NamedMeasure::RevenueNet => "revenue_net",
            NamedMeasure::RevenueGross => "revenue_gross",
            NamedMeasure::RevenueRefunds => "revenue_refunds",
            NamedMeasure::RevenueCount => "revenue_count",
            NamedMeasure::P50 => "p50",
            NamedMeasure::P75 => "p75",
            NamedMeasure::P95 => "p95",
        }
        .to_owned(),
        Measure::Property(stat) => {
            let stat_name = match stat.stat {
                Stat::Count => "count",
                Stat::Sum => "sum",
                Stat::Min => "min",
                Stat::Max => "max",
                Stat::Mean => "mean",
            };
            format!(
                "{stat_name}({}.{})",
                scope_name(stat.field.scope),
                stat.field.key
            )
        }
    }
}
