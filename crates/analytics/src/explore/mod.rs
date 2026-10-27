//! The exploration planner: compiles a typed exploration state (0004) into bounded SQL over
//! the analytical store and computes exact totals, rows, series and comparisons (0003).
//!
//! [`validate`] refuses what cannot run before any engine work. [`explore`] resolves the range
//! once, materializes the matching rows of the current range and of what it compares with (the
//! previous period, or the typical-weekday baseline days in [`baseline`]), and computes every
//! cut from them inside the caller's snapshot, so totals, rows, series and panels agree.
//! A state the identity-day rollups can serve reads them for clean whole days instead
//! ([`rollup`]), with the same result. A state with a report runs its kind in [`report`].

mod baseline;
mod glob;
mod report;
mod resolve;
pub(crate) mod rollup;
mod run;
pub mod session;
mod sql;
mod validate;

use chrono::{Days, TimeZone};
use chrono_tz::Tz;
use holocron_protocol::exploration::{
    Compare, DimensionField, ExploreResult, Field, Measure, NamedMeasure, PropertyScope, Range,
    ResolvedRange, VisitorLabel,
};
use holocron_protocol::limits::{TYPICAL_WEEKDAY_RANGE_DAYS_MAX, TYPICAL_WEEKDAY_WEEKS};
use holocron_protocol::operations::analytics::{CorrectionState, Freshness, ImportedHistory};
use holocron_protocol::retention::Retention;
use holocron_storage::duckdb::types::Value;
use holocron_storage::duckdb::{self, params};
use holocron_storage::{Id, Snapshot};

use crate::freshness::{Correction, freshness};
use crate::identity::{SessionPolicy, SitePolicies};
use crate::jobs::site_policies;
use resolve::{History, Span};
pub use rollup::servable;
pub(crate) use run::ratio4;
pub use run::{Plan, decimal, instant, record_plans};
pub(crate) use sql::session_fields_sql;
pub use validate::{
    CheckedAttribution, CheckedFunnel, CheckedGoals, DEFAULT_PAGE, Pred, Query, Refusal, Report,
    StateLimits, validate, validate_filter, validate_with,
};

/// What a request adds to the state: the site's analytical ID, the application clock, the
/// site's hostnames and its effective retention.
pub struct Context<'a> {
    pub site: Id,
    pub now_ms: i64,
    pub hostnames: &'a [String],
    pub retention: Retention,
}

impl Context<'_> {
    /// The retention cutoff queries apply as a predicate (0006, 0016).
    pub(crate) fn cutoff_ms(&self, policies: &SitePolicies) -> i64 {
        policies.retention_floor_ms(self.now_ms, self.retention)
    }

    /// How far back the state's range may reach; reads the first stored event only for `all`.
    pub(crate) fn history(
        &self,
        snapshot: &Snapshot,
        tz: chrono_tz::Tz,
        policies: &SitePolicies,
        range: &Range,
    ) -> duckdb::Result<History> {
        let cutoff = policies
            .retention_cutoff_ms(self.now_ms, self.retention)
            .map(|ms| History::cutoff_date(tz, ms));
        let first = match range {
            Range::All => snapshot
                .connection()
                .query_row(
                    "SELECT epoch_ms(min(event_time)) FROM events WHERE site = ?",
                    params![self.site],
                    |row| row.get::<_, Option<i64>>(0),
                )?
                .map(|ms| {
                    tz.timestamp_millis_opt(ms)
                        .single()
                        .expect("instant in range")
                        .date_naive()
                }),
            _ => None,
        };
        Ok(History { cutoff, first })
    }
}

/// Runs a checked query. The outer error is an engine failure; the inner one a refusal that
/// needs the stored data or the clock (range limits, series points, unknown property keys).
pub fn explore(
    snapshot: &Snapshot,
    query: &Query,
    ctx: &Context,
) -> duckdb::Result<Result<ExploreResult, Refusal>> {
    run_query(snapshot, query, ctx, servable(query))
}

/// [`explore`] from events alone, without rollups: the reference rollup results must equal.
pub fn explore_raw(
    snapshot: &Snapshot,
    query: &Query,
    ctx: &Context,
) -> duckdb::Result<Result<ExploreResult, Refusal>> {
    run_query(snapshot, query, ctx, false)
}

/// The performance observations a percentile query ranks for its totals (0003 Percentiles):
/// the `n` its results do not carry. Counts rows matching the filter for any other query.
pub fn observations(
    snapshot: &Snapshot,
    query: &Query,
    ctx: &Context,
) -> duckdb::Result<Result<i64, Refusal>> {
    let policies = policies(snapshot, ctx.site)?;
    let history = ctx.history(snapshot, query.timezone, &policies, &query.state.range)?;
    let (current, _) =
        match resolve::resolve(&query.state.range, query.timezone, ctx.now_ms, history) {
            Ok(spans) => spans,
            Err(refusal) => return Ok(Err(refusal)),
        };
    let cutoff = ctx.cutoff_ms(&policies);
    let (start, end, _) = run::clip(&current, cutoff);
    let plan = Plan::of(query, ctx.site, ctx.hostnames, false);
    let conn = snapshot.connection();
    run::materialize(conn, &plan, "o", (start, end))?;
    let n = conn.query_row("SELECT count(*) FROM o_ev", [], |r| r.get(0))?;
    run::drop_tables(conn, "o")?;
    Ok(Ok(n))
}

fn run_query(
    snapshot: &Snapshot,
    query: &Query,
    ctx: &Context,
    rollup: bool,
) -> duckdb::Result<Result<ExploreResult, Refusal>> {
    if let Some(report) = &query.report {
        return report::explore(snapshot, query, report, ctx);
    }
    let conn = snapshot.connection();
    let state = &query.state;
    let policies = policies(snapshot, ctx.site)?;
    let history = ctx.history(snapshot, query.timezone, &policies, &state.range)?;
    let (current, previous) =
        match resolve::resolve(&state.range, query.timezone, ctx.now_ms, history) {
            Ok(spans) => spans,
            Err(refusal) => return Ok(Err(refusal)),
        };
    if query.series
        && let Err(refusal) = resolve::buckets(query.timezone, &current, state.granularity)
    {
        return Ok(Err(refusal));
    }
    // Queries apply the retention cutoff as a predicate (0006, 0016).
    let cutoff = ctx.cutoff_ms(&policies);
    let (start, end, retention_start) = run::clip(&current, cutoff);
    let baselines = match state.compare {
        Compare::TypicalWeekday => {
            match typical_weekday(snapshot, ctx, query.timezone, &current, cutoff)? {
                Ok(baselines) => Some(baselines),
                Err(refusal) => return Ok(Err(refusal)),
            }
        }
        Compare::None | Compare::PreviousPeriod => None,
    };
    let compared = state.compare == Compare::PreviousPeriod;
    let (compare_start, compare_end, compare_retention_start) = run::clip(&previous, cutoff);
    // A range containing now compares its totals and rows only up to the same offset; its
    // series still compares with the whole previous span, materialized apart as `w`.
    let compare_until = resolve::same_offset(query.timezone, &current, &previous, ctx.now_ms)
        .map_or(compare_end, |until| until.clamp(compare_start, compare_end));
    let whole_previous = compared && query.series && compare_until < compare_end;
    let series_granularity = query.series.then_some(state.granularity);

    let mut plan = Plan::of(query, ctx.site, ctx.hostnames, rollup);
    for field in &plan.fields {
        if let Field::Property(property) = field
            && !property_known(snapshot, ctx.site, property.scope, &property.key)?
        {
            return Ok(Err(Refusal::invalid(
                field_path(query, field),
                format!(
                    "no data on this site carries the property `{}`",
                    property.key
                ),
            )));
        }
    }

    if rollup {
        let y_series = series_granularity.filter(|_| !whole_previous);
        plan.rollup = rollup::materialize(
            conn,
            &plan,
            &policies,
            "x",
            (start, end),
            series_granularity,
        )? && (!compared
            || rollup::materialize(
                conn,
                &plan,
                &policies,
                "y",
                (compare_start, compare_until),
                y_series,
            )?)
            && (!whole_previous
                || rollup::materialize(
                    conn,
                    &plan,
                    &policies,
                    "w",
                    (compare_start, compare_end),
                    series_granularity,
                )?);
    }
    if !plan.rollup {
        run::materialize(conn, &plan, "x", (start, end))?;
        if compared {
            run::materialize(conn, &plan, "y", (compare_start, compare_until))?;
        }
        if whole_previous {
            run::materialize(conn, &plan, "w", (compare_start, compare_end))?;
        }
    }
    let baseline = match &baselines {
        Some(days) => Some(baseline::Baseline::materialize(
            conn,
            &plan,
            &policies,
            &current,
            days,
            (series_granularity, ctx.now_ms),
        )?),
        None => None,
    };
    let mut cuts = Vec::new();
    for (n, spec) in query.cuts().enumerate() {
        let totals = run::totals(conn, &plan, "x", spec)?;
        let page = if spec.dimensions.is_empty() {
            run::Page {
                rows: Vec::new(),
                row_count: 0,
            }
        } else {
            run::rows(conn, &plan, "x", spec, None)?
        };
        let versus = if compared {
            let totals = run::totals(conn, &plan, "y", spec)?;
            let keys: Vec<Vec<Value>> = page.rows.iter().map(|(_, raw, _)| raw.clone()).collect();
            let rows: std::collections::HashMap<_, _> = if spec.dimensions.is_empty() {
                Default::default()
            } else {
                run::rows(conn, &plan, "y", spec, Some(&keys))?
                    .rows
                    .into_iter()
                    .map(|(cells, _, aggs)| (cells, aggs))
                    .collect()
            };
            Some(run::Compared::previous(
                spec,
                &plan.fields,
                &page,
                &totals,
                &rows,
            ))
        } else if let Some(baseline) = &baseline {
            Some(baseline.cut(conn, &plan, spec, &page, &totals)?)
        } else {
            None
        };
        let series = if n == 0 && query.series {
            match run::series(
                conn,
                &plan,
                "x",
                &current,
                state.granularity,
                &spec.measures,
            )? {
                Ok(mut series) => {
                    if let Some(at) = spec.measures.iter().position(is_visitor_measure) {
                        let summed: i64 = series
                            .points
                            .iter()
                            .map(|p| match p.values[at] {
                                holocron_protocol::exploration::Metric::Count { v } => v,
                                _ => 0,
                            })
                            .sum();
                        series.series_sum_differs = summed > totals.visitors;
                    }
                    let points = if compared {
                        let prefix = if whole_previous { "w" } else { "y" };
                        let starts =
                            resolve::bucket_starts(query.timezone, &previous, state.granularity);
                        let aggs = run::bucket_aggs(
                            conn,
                            &plan,
                            prefix,
                            &previous,
                            state.granularity,
                            &spec.measures,
                            &starts,
                        )?;
                        let values = |a: &run::Aggs| run::values(&spec.measures, a, &plan.fields);
                        Some(aggs.iter().map(values).collect())
                    } else if let Some(baseline) = &baseline {
                        let (points, projected) =
                            baseline.series(conn, &plan, spec, &series, &totals)?;
                        series.projection = projected;
                        Some(points)
                    } else {
                        None
                    };
                    if let Some(points) = points {
                        run::compare_points(&mut series, points);
                    }
                    Some(series)
                }
                Err(refusal) => return Ok(Err(refusal)),
            }
        } else {
            None
        };
        cuts.push(run::cut(spec, &plan.fields, &totals, page, versus, series));
    }
    if let Some(baseline) = &baseline {
        baseline.drop_tables(conn)?;
    }
    run::drop_tables(conn, "x")?;
    run::drop_tables(conn, "y")?;
    run::drop_tables(conn, "w")?;

    let fresh = freshness(snapshot, ctx.site, start, end.max(start + 1))?;
    let main = cuts.remove(0);
    Ok(Ok(ExploreResult {
        // The requested bounds; `retention_start` names the first retained instant when the
        // cutoff clipped the read.
        resolved: ResolvedRange {
            start: current.start_ms,
            end: current.end_ms,
            compare_start: compared.then_some(previous.start_ms),
            compare_end: compared.then_some(previous.end_ms),
            retention_start,
            compare_retention_start: if compared {
                compare_retention_start
            } else {
                None
            },
            compare_days: baselines.as_deref().map(resolve::compare_days),
            timezone: state.timezone.clone(),
        },
        freshness: wire_freshness(fresh, ctx.now_ms),
        visitor_label: visitor_label(&policies, start, end),
        main,
        panels: cuts,
        report: None,
    }))
}

/// The baseline days of a typical-weekday comparison (0015) for a range of at most 31 days,
/// clipped at the retention cutoff and the site's first stored event, before today.
fn typical_weekday(
    snapshot: &Snapshot,
    ctx: &Context,
    tz: Tz,
    current: &Span,
    cutoff: i64,
) -> duckdb::Result<Result<Vec<Vec<Span>>, Refusal>> {
    if current.days() > u64::from(TYPICAL_WEEKDAY_RANGE_DAYS_MAX) {
        return Ok(Err(Refusal::compare_unsupported(format!(
            "a typical-weekday comparison covers at most {TYPICAL_WEEKDAY_RANGE_DAYS_MAX} days"
        ))));
    }
    let earliest = current.first - Days::new(7 * u64::from(TYPICAL_WEEKDAY_WEEKS));
    let from = resolve::local_start(tz, earliest.and_hms_opt(0, 0, 0).expect("midnight"));
    let first = first_event(snapshot, ctx.site, cutoff, (from, current.end_ms))?;
    let today = resolve::local(tz, ctx.now_ms).date();
    let until = resolve::local_start(tz, today.and_hms_opt(0, 0, 0).expect("midnight"));
    Ok(Ok(resolve::baseline_days(
        tz, current, cutoff, first, until,
    )))
}

/// The site's first stored event as a baseline over `[from, to)` sees it: `from` itself when a
/// retained event lies before `from`, else the first event in `[from, to)`.
fn first_event(
    snapshot: &Snapshot,
    site: Id,
    cutoff: i64,
    (from, to): (i64, i64),
) -> duckdb::Result<Option<i64>> {
    let conn = snapshot.connection();
    let earlier = cutoff < from
        && conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM events
                            WHERE site = ? AND event_time >= epoch_ms(?) AND event_time < epoch_ms(?))",
            params![site, cutoff, from],
            |r| r.get::<_, bool>(0),
        )?;
    if earlier {
        return Ok(Some(from));
    }
    conn.query_row(
        "SELECT epoch_ms(min(event_time)) FROM events
         WHERE site = ? AND event_time >= epoch_ms(?) AND event_time < epoch_ms(?)",
        params![site, from.max(cutoff), to],
        |r| r.get(0),
    )
}

/// "visitors" when `[start, end)` lies inside one identity day, else "visitor-days" (0003).
fn visitor_label(policies: &SitePolicies, start: i64, end: i64) -> VisitorLabel {
    if policies.identity_day(start) == policies.identity_day((end - 1).max(start)) {
        VisitorLabel::Visitors
    } else {
        VisitorLabel::VisitorDays
    }
}

fn is_visitor_measure(measure: &Measure) -> bool {
    matches!(
        measure,
        Measure::Named(NamedMeasure::Visitors | NamedMeasure::VisitorDays)
    )
}

/// Sessions starting in `[start, end)`, clipped to the retention cutoff, with at least one
/// eligible event matching a filter from [`validate_filter`], in any order. Refused when the
/// filter names a property no data on the site carries, as an exploration would be.
pub fn matching_sessions(
    snapshot: &Snapshot,
    filter: &Pred,
    ctx: &Context,
    (start, end): (i64, i64),
) -> duckdb::Result<Result<Vec<i64>, Refusal>> {
    let mut fields: Vec<Field> = Vec::new();
    let mut properties = Vec::new();
    filter.visit(&mut |cond| {
        if let Field::Property(property) = &cond.field {
            properties.push((cond.id.clone(), property.clone()));
        }
        if !fields.contains(&cond.field) {
            fields.push(cond.field.clone());
        }
    });
    for (id, property) in properties {
        if !property_known(snapshot, ctx.site, property.scope, &property.key)? {
            return Ok(Err(Refusal::invalid(
                format!("filter.{id}"),
                format!(
                    "no data on this site carries the property `{}`",
                    property.key
                ),
            )));
        }
    }
    let policies = policies(snapshot, ctx.site)?;
    let start = start.max(ctx.cutoff_ms(&policies));
    if start >= end {
        return Ok(Ok(Vec::new()));
    }
    let plan = Plan {
        site: ctx.site,
        timezone: policies.at(ctx.now_ms).policy.timezone(),
        filter: Some(filter),
        read: vec![false; fields.len()],
        fields,
        sessions: true,
        rollup: false,
        hostnames: ctx.hostnames,
        performance: false,
        revenue: false,
        revenue_only: false,
    };
    let conn = snapshot.connection();
    run::materialize(conn, &plan, "ms", (start, end))?;
    let sessions = conn
        .prepare("SELECT DISTINCT session_id FROM ms_se")?
        .query_map([], |row| row.get(0))?
        .collect::<duckdb::Result<Vec<i64>>>()?;
    run::drop_tables(conn, "ms")?;
    Ok(Ok(sessions))
}

/// The site's session policies, or the default one for a site analytics has not seen yet.
pub fn policies(snapshot: &Snapshot, site: Id) -> duckdb::Result<SitePolicies> {
    Ok(
        site_policies(snapshot.connection(), site)?.unwrap_or_else(|| {
            SitePolicies::new(SessionPolicy::new(chrono_tz::UTC, 30).expect("default policy"))
        }),
    )
}

fn property_known(
    snapshot: &Snapshot,
    site: Id,
    scope: PropertyScope,
    key: &str,
) -> duckdb::Result<bool> {
    let table = match scope {
        PropertyScope::Event => holocron_storage::event_property_table(snapshot.connection(), key)?,
        PropertyScope::Session => "session_properties".to_owned(),
    };
    snapshot.connection().query_row(
        &format!("SELECT EXISTS (SELECT 1 FROM {table} WHERE site = ? AND key = ?)"),
        params![site, key],
        |r| r.get(0),
    )
}

/// Where a field is used, for a refusal: its first condition, else the cut that reads it.
fn field_path(query: &Query, field: &Field) -> String {
    if let Some(path) = run::first_condition(query.filter.as_ref(), field) {
        return path;
    }
    for (i, spec) in query.cuts().enumerate() {
        let reads = spec.dimensions.contains(field)
            || spec.measures.iter().any(
                |m| matches!(m, Measure::Property(s) if Field::Property(s.field.clone()) == *field),
            );
        if reads {
            let part = if spec.dimensions.contains(field) {
                "dimensions"
            } else {
                "measures"
            };
            return if i == 0 {
                part.to_owned()
            } else {
                format!("panels[{}].{part}", i - 1)
            };
        }
    }
    "filter".to_owned()
}

/// The wire form of a result's freshness, read as current at `now_ms`. Whether analytics is
/// behind needs the inbox, which a snapshot does not see: the caller sets `watermark_at` and
/// `stale` from it.
pub fn wire_freshness(fresh: crate::Freshness, now_ms: i64) -> Freshness {
    let (correction, dirty_days) = match fresh.correction {
        Correction::Corrected => (CorrectionState::Corrected, 0),
        Correction::Pending { dirty_days } => (CorrectionState::Pending, dirty_days as i64),
    };
    Freshness {
        watermark_batch: fresh.watermark.batch,
        watermark_sequence: fresh.watermark.sequence,
        watermark_at: now_ms,
        stale: false,
        corrected_through: fresh.corrected_through,
        correction,
        dirty_days,
        metric_version: fresh.metric_version,
        session_policy_versions: fresh.session_policy_versions,
        imported: fresh.imported.map(wire_imported),
    }
}

/// The wire form of a site's imported history.
pub fn wire_imported(history: crate::ImportedHistory) -> ImportedHistory {
    ImportedHistory {
        source: history.source,
        first: history.first_ms,
        last: history.last_ms,
    }
}

/// Most frequent values of one field among eligible events in `[start, end)`, with an optional
/// case-insensitive prefix, for value suggestions.
pub fn top_values(
    snapshot: &Snapshot,
    site: Id,
    field: &Field,
    prefix: Option<&str>,
    (start, end): (i64, i64),
    hostnames: &[String],
    limit: usize,
) -> duckdb::Result<Vec<(holocron_protocol::exploration::Cell, i64)>> {
    let plan = Plan {
        site,
        timezone: chrono_tz::UTC,
        filter: None,
        fields: vec![field.clone()],
        read: vec![true],
        sessions: false,
        rollup: false,
        hostnames,
        performance: *field == Field::Dimension(DimensionField::PerfMetric),
        revenue: false,
        revenue_only: false,
    };
    run::materialize(snapshot.connection(), &plan, "v", (start, end))?;
    let values = run::top_values(snapshot.connection(), field, prefix, limit);
    run::drop_tables(snapshot.connection(), "v")?;
    values
}
