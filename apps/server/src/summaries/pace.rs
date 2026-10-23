//! Today's pace of a site: the exploration state the summary runs, and the summary it yields.
//! Everything comes from one `analytics.explore` result, so the row says what the overview
//! would show for the same state at the same snapshot.

use anyhow::{Context as _, bail};
use holocron_analytics::METRIC_VERSION;
use holocron_analytics::explore::{self, Query};
use holocron_protocol::exploration::{
    Compare, ExplorationState, ExploreResult, Granularity, Measure, Metric, NamedMeasure,
    Projection, Range, RangeUnit, Reason, STATE_VERSION, Visualization,
};
use holocron_protocol::ids::SiteId;
use holocron_protocol::operations::summaries::{MeasurePace, Summary};

/// Today by the hour against a typical weekday, as the overview opens (0015 decision 4), with
/// the two measures the summary keeps: pageviews first, then visitor-days.
pub fn state(site: &SiteId, timezone: &str) -> ExplorationState {
    ExplorationState {
        v: STATE_VERSION,
        site: site.clone(),
        measures: vec![
            Measure::Named(NamedMeasure::Pageviews),
            Measure::Named(NamedMeasure::VisitorDays),
        ],
        dimensions: Vec::new(),
        filter: None,
        range: Range::Relative {
            unit: RangeUnit::Day,
            count: 1,
        },
        timezone: timezone.to_owned(),
        granularity: Granularity::Hour,
        compare: Compare::TypicalWeekday,
        metric_version: METRIC_VERSION,
        sort: Vec::new(),
        page: explore::DEFAULT_PAGE,
        visualization: Visualization::Line,
        report: None,
    }
}

/// The checked query of [`state`].
pub fn query(site: &SiteId, timezone: &str) -> anyhow::Result<Query> {
    explore::validate(&state(site, timezone), &[])
        .map_err(|refusal| anyhow::anyhow!("summary state refused: {}", refusal.message))
}

/// The summary of a [`state`] result, at the result's watermark. Not stale: it was just
/// computed.
pub fn summary(
    site: &SiteId,
    timezone: &str,
    result: &ExploreResult,
    computed_at: i64,
) -> anyhow::Result<Summary> {
    let main = &result.main;
    let pace = |i: usize| -> anyhow::Result<MeasurePace> {
        let compare = main.compare.as_ref().and_then(|c| c.get(i));
        Ok(MeasurePace {
            today: main.totals.get(i).and_then(count).context("no total")?,
            typical: compare.and_then(|c| count(&c.previous)),
            relative_change: compare.and_then(|c| c.relative_change),
            projection: main
                .projection
                .as_ref()
                .and_then(|p| p.get(i).cloned().flatten())
                .filter(|p| matches!(p, Projection::Estimate { .. })),
        })
    };
    let Some(series) = &main.series else {
        bail!("summary result has no series");
    };
    let hourly_today = series
        .points
        .iter()
        .map(|point| point.values.first().and_then(count).unwrap_or(0))
        .collect();
    let hourly_typical = series
        .points
        .iter()
        .map(|point| {
            point
                .compare
                .as_ref()
                .and_then(|c| c.first())
                .and_then(|c| count(&c.previous))
        })
        .collect();
    let insufficient_history = main.compare.iter().flatten().any(|c| {
        c.reason == Some(Reason::InsufficientHistory)
            || c.previous
                == Metric::Unavailable {
                    reason: Reason::InsufficientHistory,
                }
    });
    Ok(Summary {
        site: site.clone(),
        computed_at,
        watermark_batch: result.freshness.watermark_batch,
        watermark_sequence: result.freshness.watermark_sequence,
        metric_version: result.freshness.metric_version,
        stale: false,
        timezone: timezone.to_owned(),
        pageviews: pace(0)?,
        visitor_days: pace(1)?,
        hourly_today,
        hourly_typical,
        insufficient_history,
        finding_count: 0,
    })
}

fn count(metric: &Metric) -> Option<i64> {
    match metric {
        Metric::Count { v } => Some(*v),
        _ => None,
    }
}
