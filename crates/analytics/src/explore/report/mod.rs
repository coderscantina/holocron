//! Report kinds of decision 0004: funnel, journey, attribution, retention and session list.
//!
//! [`explore`] resolves what every kind needs once (range, retention cutoff, freshness, known
//! property keys) and hands it to the kind's `run` as an [`Input`]. Each kind lives in its own
//! file with the same signature: `run(&Input, &definition) -> Outcome<KindResult>`.

mod attribution;
mod funnel;
mod goals;
mod journey;
mod matching;
mod retention;
mod session_list;

use chrono::{Datelike, NaiveDate};
use holocron_protocol::errors::ErrorCode;
use holocron_protocol::exploration::{
    Compare, Coverage, Cut, ExploreResult, Field, Granularity, ReportResult, ResolvedRange,
};
use holocron_protocol::limits::REPORT_RETENTION_CELLS_MAX;
use holocron_storage::Snapshot;
use holocron_storage::duckdb;

use super::resolve::{self, Span};
use super::validate::{Query, Refusal, Report};
use super::{
    Context, policies, property_known, run, typical_weekday, visitor_label, wire_freshness,
};
use crate::explore::baseline::Windows;
use crate::freshness::freshness;
use crate::identity::SitePolicies;

/// An engine failure outside, a refusal that needs the stored data or the clock inside.
pub type Outcome<T> = duckdb::Result<Result<T, Refusal>>;

/// What the planner resolved for a report, all from one snapshot.
#[expect(
    dead_code,
    reason = "the kinds read these once milestones 07 and 08 compute them"
)]
pub struct Input<'a> {
    pub snapshot: &'a Snapshot<'a>,
    /// The checked state: timezone, the filter as a [`super::validate::Pred`], the page.
    pub query: &'a Query,
    /// The site's analytical ID, the clock and the site's hostnames.
    pub ctx: &'a Context<'a>,
    pub policies: &'a SitePolicies,
    /// The requested local dates and their instants.
    pub span: &'a Span,
    /// `[start, end)` to read: the span clipped to the retention cutoff.
    pub read: (i64, i64),
    pub freshness: &'a crate::Freshness,
}

/// Runs a checked report.
pub(super) fn explore(
    snapshot: &Snapshot,
    query: &Query,
    report: &Report,
    ctx: &Context,
) -> duckdb::Result<Result<ExploreResult, Refusal>> {
    let state = &query.state;
    let policies = policies(snapshot, ctx.site)?;
    let history = ctx.history(snapshot, query.timezone, &policies, &state.range)?;
    let span = match resolve::resolve(&state.range, query.timezone, ctx.now_ms, history) {
        Ok((current, _)) => current,
        Err(refusal) => return Ok(Err(refusal)),
    };
    if let Report::Retention(retention) = report {
        let cells = cohort_periods(&span, state.granularity) * usize::from(retention.intervals);
        if cells > REPORT_RETENTION_CELLS_MAX {
            return Ok(Err(Refusal::too_many(
                ErrorCode::ReportTooLarge,
                "report.intervals",
                REPORT_RETENTION_CELLS_MAX,
                "cohort periods in range times intervals exceed 3,000",
            )));
        }
    }
    for (path, tree) in query.trees() {
        let mut properties = Vec::new();
        tree.visit(&mut |cond| {
            if let Field::Property(property) = &cond.field {
                properties.push((cond.id.clone(), property.clone()));
            }
        });
        for (id, property) in properties {
            if !property_known(snapshot, ctx.site, property.scope, &property.key)? {
                return Ok(Err(Refusal::invalid(
                    format!("{path}.{id}"),
                    format!(
                        "no data on this site carries the property `{}`",
                        property.key
                    ),
                )));
            }
        }
    }

    if let Report::Funnel(funnel) = report
        && let Some(Field::Property(property)) = funnel.group_by()
        && !property_known(snapshot, ctx.site, property.scope, &property.key)?
    {
        return Ok(Err(Refusal::invalid(
            "report.group_by",
            format!(
                "no data on this site carries the property `{}`",
                property.key
            ),
        )));
    }

    let cutoff = ctx.cutoff_ms(&policies);
    let (start, end, retention_start) = run::clip(&span, cutoff);
    // Validation lets only the goals report compare with a typical weekday.
    let baselines = match state.compare {
        Compare::TypicalWeekday => {
            match typical_weekday(snapshot, ctx, query.timezone, &span, cutoff)? {
                Ok(days) => Some(days),
                Err(refusal) => return Ok(Err(refusal)),
            }
        }
        Compare::None | Compare::PreviousPeriod => None,
    };
    let baseline = baselines
        .as_deref()
        .map(|days| Windows::plan(query.timezone, &span, days, ctx.now_ms));
    let fresh = freshness(snapshot, ctx.site, start, end.max(start + 1))?;
    let input = Input {
        snapshot,
        query,
        ctx,
        policies: &policies,
        span: &span,
        read: (start, end),
        freshness: &fresh,
    };
    let result = match report {
        Report::Funnel(r) => funnel::run(&input, r)?.map(ReportResult::Funnel),
        Report::Journey(r) => journey::run(&input, r)?.map(ReportResult::Journey),
        Report::Attribution(r) => attribution::run(&input, r)?.map(ReportResult::Attribution),
        Report::Retention(r) => retention::run(&input, r)?.map(ReportResult::Retention),
        Report::SessionList(r) => session_list::run(&input, r)?.map(ReportResult::SessionList),
        Report::Goals(r) => goals::run(&input, r, baseline.as_ref())?.map(ReportResult::Goals),
    };
    Ok(result.map(|result| ExploreResult {
        resolved: ResolvedRange {
            start: span.start_ms,
            end: span.end_ms,
            compare_start: None,
            compare_end: None,
            retention_start,
            compare_retention_start: None,
            compare_days: baselines.as_deref().map(resolve::compare_days),
            timezone: state.timezone.clone(),
        },
        freshness: wire_freshness(fresh, ctx.now_ms),
        visitor_label: visitor_label(&policies, start, end),
        main: Cut {
            cut: query.main.panel.clone(),
            totals: Vec::new(),
            compare: None,
            projection: None,
            rows: Vec::new(),
            row_count: 0,
            series: None,
            identity_coverage: Coverage {
                with_identity: 0,
                eligible_events: 0,
            },
        },
        panels: Vec::new(),
        report: Some(result),
    }))
}

/// Local days, or Monday-based local weeks, that the span touches.
fn cohort_periods(span: &Span, granularity: Granularity) -> usize {
    let monday =
        |d: NaiveDate| d - chrono::Days::new(u64::from(d.weekday().num_days_from_monday()));
    let days = |a: NaiveDate, b: NaiveDate| (b - a).num_days() as usize;
    match granularity {
        Granularity::Week => days(monday(span.first), monday(span.last)) / 7 + 1,
        _ => days(span.first, span.last) + 1,
    }
}
