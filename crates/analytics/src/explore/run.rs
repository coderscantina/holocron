//! Executes a checked query inside one analytical snapshot.
//!
//! Per range the matching rows are materialized once into connection-local temp tables: events
//! in range (`<p>_ev`) and member events of sessions starting in range (`<p>_se`), each already
//! filtered and carrying one column per referenced field. Totals, rows and the series of every
//! cut aggregate those tables, so all of them describe the same rows. Session-scoped fields
//! come from the values correction stores on `sessions`, written with the members they describe.

use std::collections::HashMap;

use chrono::NaiveDateTime;
use chrono_tz::Tz;
use holocron_protocol::exploration::{
    Cell, Comparison, Coverage, Cut, DimensionField, Field, Granularity, Measure, Metric, Money,
    NamedMeasure, Projection, PropertyScope, Reason, Row, Series, SeriesPoint, SortDir, SortKey,
    Stat,
};
use holocron_storage::Id;
use holocron_storage::duckdb::types::{TimeUnit, Value};
use holocron_storage::duckdb::{self, Connection, params_from_iter};

use super::resolve::{Span, buckets, trunc};
use super::rollup;
use super::sql::{
    self, PERF_METRICS, PROPERTY_PARTS, SESSION_FIELDS, Source, Sql, columns, needs_session,
    predicate,
};
use super::validate::{
    CutSpec, Pred, Query, Refusal, is_money, is_percentile, is_revenue, is_session_measure,
};
use crate::traffic::local_ms_sql;

/// What materialization reads: the site, the fields, the filter, whether sessions count and
/// whether the request is served from rollups.
#[derive(Clone)]
pub struct Plan<'a> {
    pub site: Id,
    pub timezone: Tz,
    pub filter: Option<&'a Pred>,
    pub fields: Vec<Field>,
    /// Per field, whether a cut reads it. Fields only the filter reads are not materialized.
    pub read: Vec<bool>,
    /// Some cut has a session measure, so `<p>_se` is needed.
    pub sessions: bool,
    /// Event aggregates combine `<p>_ev` with the rollups of `<p>_days` ([`super::rollup`]).
    pub rollup: bool,
    /// The site's hostnames, for the internal-referrer rule of `channel`.
    pub hostnames: &'a [String],
    /// Rows are performance observations, one per reported metric of a performance event,
    /// instead of eligible events (0003 Percentiles).
    pub performance: bool,
    /// Rows carry their revenue record as `cur` and `rev`.
    pub revenue: bool,
    /// Every measure is a revenue measure, so rows are revenue records only (0003 Revenue).
    pub revenue_only: bool,
}

impl<'a> Plan<'a> {
    pub fn of(query: &'a Query, site: Id, hostnames: &'a [String], rollup: bool) -> Self {
        let fields = query.fields();
        let read = fields
            .iter()
            .map(|field| {
                query.cuts().any(|cut| {
                    cut.dimensions.contains(field)
                        || cut.measures.iter().any(|m| {
                            matches!(m, Measure::Property(s) if Field::Property(s.field.clone()) == *field)
                        })
                })
            })
            .collect();
        Self {
            site,
            timezone: query.timezone,
            filter: query.filter.as_ref(),
            fields,
            read,
            sessions: query
                .cuts()
                .any(|cut| cut.measures.iter().any(is_session_measure)),
            rollup,
            hostnames,
            performance: query
                .cuts()
                .any(|cut| cut.measures.iter().any(is_percentile))
                || query
                    .fields()
                    .contains(&Field::Dimension(DimensionField::PerfMetric)),
            revenue: query.cuts().any(|cut| cut.measures.iter().any(is_revenue)),
            revenue_only: query.cuts().all(|cut| cut.measures.iter().all(is_revenue)),
        }
    }

    fn needs_sessions(&self) -> bool {
        self.sessions
    }

    /// Events must be joined to their sessions: a session field, or session measures.
    fn needs_scope(&self) -> bool {
        self.sessions || self.fields.iter().any(needs_session)
    }
}

/// The aggregates of one cell or total. Absent parts were not requested.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Aggs {
    pub pageviews: i64,
    pub events: i64,
    pub visitors: i64,
    pub people: i64,
    pub with_identity: i64,
    pub eligible: i64,
    /// Per property field index: numeric row count, sum, min, max (scaled by 10^4).
    pub stats: HashMap<usize, (i64, i128, Option<i128>, Option<i128>)>,
    pub sessions: i64,
    pub pageview_sessions: i64,
    pub bounces: i64,
    pub span_sum: i64,
    pub span_median: Option<i64>,
    pub revenue_count: i64,
    /// Per currency in code order: gross, refunds and net minor units, and its revenue records.
    pub money: Vec<(String, [i128; 4])>,
    /// p50, p75 and p95 of the observations, `cls` scaled by 10^4.
    pub percentiles: [Option<i64>; 3],
    /// Distinct performance metrics among the observations, and the first of them.
    pub perf_metrics: i64,
    pub perf_metric: Option<String>,
}

/// One aggregate column of the generated SQL and where its value goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Agg {
    Pageviews,
    Events,
    Visitors,
    People,
    WithIdentity,
    Eligible,
    StatCount(usize),
    StatSum(usize),
    StatMin(usize),
    StatMax(usize),
    Sessions,
    PageviewSessions,
    Bounces,
    SpanSum,
    SpanMedian,
    RevenueCount,
    /// Nearest-rank percentile: index 0, 1, 2 for p50, p75, p95.
    Percentile(usize),
    PerfMetrics,
    PerfMetric,
}

/// Percent ranks of p50, p75 and p95.
const PERCENTILES: [i64; 3] = [50, 75, 95];

impl Agg {
    fn sql(self) -> String {
        let numeric = |i: usize| {
            format!(
                "(CASE WHEN f{i}_t = 2 THEN f{i}_i::HUGEINT * 10000 WHEN f{i}_t = 3 THEN f{i}_d::HUGEINT END)"
            )
        };
        match self {
            Agg::Pageviews => "count(*) FILTER (WHERE kind = 1)".into(),
            Agg::Events => "count(*) FILTER (WHERE kind IN (2, 4, 5))".into(),
            Agg::Visitors => "count(DISTINCT daily_id)".into(),
            Agg::People => "count(DISTINCT profile_id)".into(),
            Agg::WithIdentity => {
                "count(*) FILTER (WHERE daily_id IS NOT NULL OR profile_id IS NOT NULL)".into()
            }
            Agg::Eligible => "count(*)".into(),
            Agg::StatCount(i) => format!("count({})", numeric(i)),
            Agg::StatSum(i) => format!("coalesce(sum({}), 0)::HUGEINT", numeric(i)),
            Agg::StatMin(i) => format!("min({})", numeric(i)),
            Agg::StatMax(i) => format!("max({})", numeric(i)),
            Agg::Sessions => "count(*)".into(),
            Agg::PageviewSessions => "count(*) FILTER (WHERE pv > 0)".into(),
            Agg::Bounces => "count(*) FILTER (WHERE pv = 1 AND ce = 0)".into(),
            Agg::SpanSum => "coalesce(sum(span) FILTER (WHERE pv > 0), 0)::BIGINT".into(),
            Agg::SpanMedian => "quantile_disc(span, 0.5) FILTER (WHERE pv > 0)".into(),
            Agg::RevenueCount => "count(rev)".into(),
            // r = ceil(p × n) in integers, 1-based; no interpolation (0003 Percentiles).
            Agg::Percentile(i) => format!(
                "list_sort(list(pv))[({} * count(pv) + 99) // 100]",
                PERCENTILES[i]
            ),
            Agg::PerfMetrics => "count(DISTINCT pm)".into(),
            Agg::PerfMetric => "min(pm)".into(),
        }
    }

    /// The aggregate over rollup cells ([`super::rollup`]): every count there is additive,
    /// visitors too, since a daily pseudonym belongs to one cell's day.
    fn rollup_sql(self) -> String {
        let column = match self {
            Agg::Pageviews => "pageviews",
            Agg::Events => "custom_events",
            Agg::Visitors => "visitors",
            Agg::WithIdentity => "identified_events",
            Agg::Eligible => "eligible_events",
            other => unreachable!("rollup::servable keeps {other:?} off rollups"),
        };
        format!("sum({column})::BIGINT")
    }

    fn is_session(self) -> bool {
        matches!(
            self,
            Agg::Sessions | Agg::PageviewSessions | Agg::Bounces | Agg::SpanSum | Agg::SpanMedian
        )
    }

    fn store(self, value: &Value, into: &mut Aggs) {
        let int = || match value {
            Value::Null => 0,
            Value::BigInt(n) => *n,
            Value::HugeInt(n) => *n as i64,
            Value::Int(n) => i64::from(*n),
            Value::UBigInt(n) => *n as i64,
            other => panic!("unexpected aggregate {other:?}"),
        };
        let small = || match value {
            Value::Null => None,
            Value::Int(n) => Some(i64::from(*n)),
            Value::BigInt(n) => Some(*n),
            other => panic!("unexpected aggregate {other:?}"),
        };
        let huge = || match value {
            Value::Null => None,
            Value::HugeInt(n) => Some(*n),
            Value::BigInt(n) => Some(i128::from(*n)),
            other => panic!("unexpected aggregate {other:?}"),
        };
        match self {
            Agg::Pageviews => into.pageviews = int(),
            Agg::Events => into.events = int(),
            Agg::Visitors => into.visitors = int(),
            Agg::People => into.people = int(),
            Agg::WithIdentity => into.with_identity = int(),
            Agg::Eligible => into.eligible = int(),
            Agg::StatCount(i) => into.stats.entry(i).or_default().0 = int(),
            Agg::StatSum(i) => into.stats.entry(i).or_default().1 = huge().unwrap_or(0),
            Agg::StatMin(i) => into.stats.entry(i).or_default().2 = huge(),
            Agg::StatMax(i) => into.stats.entry(i).or_default().3 = huge(),
            Agg::Sessions => into.sessions = int(),
            Agg::PageviewSessions => into.pageview_sessions = int(),
            Agg::Bounces => into.bounces = int(),
            Agg::SpanSum => into.span_sum = int(),
            Agg::SpanMedian => into.span_median = huge().map(|n| n as i64),
            Agg::RevenueCount => into.revenue_count = int(),
            Agg::Percentile(i) => into.percentiles[i] = small(),
            Agg::PerfMetrics => into.perf_metrics = int(),
            Agg::PerfMetric => {
                into.perf_metric = match value {
                    Value::Text(text) => Some(text.clone()),
                    _ => None,
                }
            }
        }
    }
}

fn aggs_for(measures: &[Measure], fields: &[Field], with_coverage: bool) -> Vec<Agg> {
    let mut out = Vec::new();
    let mut add = |agg: Agg| {
        if !out.contains(&agg) {
            out.push(agg);
        }
    };
    if with_coverage {
        add(Agg::WithIdentity);
        add(Agg::Eligible);
    }
    for measure in measures {
        match measure {
            Measure::Named(named) => match named {
                NamedMeasure::Pageviews => add(Agg::Pageviews),
                NamedMeasure::Events => add(Agg::Events),
                NamedMeasure::Visitors | NamedMeasure::VisitorDays => add(Agg::Visitors),
                NamedMeasure::IdentifiedPeople => add(Agg::People),
                NamedMeasure::Sessions => add(Agg::Sessions),
                NamedMeasure::BounceRate => {
                    add(Agg::Bounces);
                    add(Agg::PageviewSessions);
                }
                NamedMeasure::SpanMean => {
                    add(Agg::SpanSum);
                    add(Agg::PageviewSessions);
                }
                NamedMeasure::SpanMedian => add(Agg::SpanMedian),
                // Amounts come from per-currency queries ([`money`]), not one column.
                NamedMeasure::RevenueNet
                | NamedMeasure::RevenueGross
                | NamedMeasure::RevenueRefunds => {}
                NamedMeasure::RevenueCount => add(Agg::RevenueCount),
                NamedMeasure::P50 | NamedMeasure::P75 | NamedMeasure::P95 => {
                    add(Agg::Percentile(percentile_index(*named)));
                    add(Agg::PerfMetrics);
                    add(Agg::PerfMetric);
                }
            },
            Measure::Property(stat) => {
                let i = field_index(fields, &Field::Property(stat.field.clone()));
                match stat.stat {
                    Stat::Count => add(Agg::StatCount(i)),
                    Stat::Sum => add(Agg::StatSum(i)),
                    Stat::Min => add(Agg::StatMin(i)),
                    Stat::Max => add(Agg::StatMax(i)),
                    Stat::Mean => {
                        add(Agg::StatSum(i));
                        add(Agg::StatCount(i));
                    }
                }
            }
        }
    }
    out
}

fn percentile_index(measure: NamedMeasure) -> usize {
    match measure {
        NamedMeasure::P50 => 0,
        NamedMeasure::P75 => 1,
        _ => 2,
    }
}

fn field_index(fields: &[Field], field: &Field) -> usize {
    fields
        .iter()
        .position(|f| f == field)
        .expect("field is materialized")
}

/// Longest stretch one materializing statement reads. A longer range runs in chunks, so the
/// joins of its events with members, sessions and properties fit the engine memory budget.
/// Members and sessions are stored in time order, so a chunk reads its own row groups.
const CHUNK_MS: i64 = 31 * 86_400_000;

/// An identify attaches to the session open at its time or to the next one of that identity
/// on its local day, so it lies at most one local day (25 hours) before the session start.
pub(super) const IDENTIFY_LEAD_MS: i64 = 2 * 86_400_000;

/// Materializes `<prefix>_ev` and, when needed, `<prefix>_se` for one range.
pub fn materialize(
    conn: &Connection,
    plan: &Plan,
    prefix: &str,
    span: (i64, i64),
) -> duckdb::Result<()> {
    let (a, b) = span;
    let sessions = plan.needs_sessions();
    // Session measures count every member of a session starting in range, which lie in
    // [a, hi]; other reads need the range only.
    let (lo, hi) = if sessions {
        let last: Option<i64> = conn.query_row(
            "SELECT max(epoch_ms(last_time)) FROM sessions
             WHERE site = ? AND start_time >= epoch_ms(?) AND start_time < epoch_ms(?)",
            duckdb::params![plan.site, a, b],
            |r| r.get(0),
        )?;
        (a, last.map_or(b - 1, |l| l.max(b - 1)))
    } else {
        (a, b - 1)
    };
    let sources = property_sources(conn, plan, prefix, (lo, hi))?;

    // Without session measures the scope is the range itself and is written as `<p>_ev`.
    let target = if sessions { "scope" } else { "ev" };
    let mut from = lo;
    loop {
        let to = (from + CHUNK_MS - 1).min(hi);
        let mut sql = Sql::default();
        sql.push(&if from == lo {
            format!("CREATE OR REPLACE TEMP TABLE {prefix}_{target} AS ")
        } else {
            format!("INSERT INTO {prefix}_{target} ")
        });
        select_rows(plan, &sources, (from, to), &mut sql);
        execute(conn, &sql)?;
        if to >= hi {
            break;
        }
        from = to + 1;
    }
    for source in &sources {
        if let Some(Values::Sessions(table)) = source {
            conn.execute_batch(&format!("DROP TABLE {table}"))?;
        }
    }

    if sessions {
        let mut ev = Sql::default();
        ev.push(&format!(
            "CREATE OR REPLACE TEMP TABLE {prefix}_ev AS SELECT * EXCLUDE (session_id) FROM {prefix}_scope WHERE t >= "
        ))
        .bind(a)
        .push(" AND t < ")
        .bind(b);
        execute(conn, &ev)?;

        let mut se = Sql::default();
        se.push(&format!(
            "CREATE OR REPLACE TEMP TABLE {prefix}_se AS
             SELECT x.*, s.pageviews AS pv, s.custom_events AS ce,
                    epoch_ms(s.last_time) - epoch_ms(s.start_time) AS span, epoch_ms(s.start_time) AS st
             FROM {prefix}_scope x JOIN sessions s ON s.site = "
        ))
        .bind(plan.site)
        .push(" AND s.session_id = x.session_id WHERE s.start_time >= epoch_ms(")
        .bind(a)
        .push(") AND s.start_time < epoch_ms(")
        .bind(b)
        .push(")");
        execute(conn, &se)?;
        conn.execute_batch(&format!("DROP TABLE {prefix}_scope"))?;
    }
    Ok(())
}

/// The selection of one chunk `[lo, hi]`: matching eligible events with one column per field.
/// A link or pixel has only its hits, which count as its events (decision 0020); a site has
/// none, so its selection is the same with or without them.
fn select_rows(plan: &Plan, sources: &[Option<Values>], (lo, hi): (i64, i64), sql: &mut Sql) {
    let (kinds, role) = if plan.performance {
        ("e.kind = 3", crate::correction::role::PERFORMANCE)
    } else if plan.revenue_only {
        (
            "e.kind = 2 AND e.revenue_minor IS NOT NULL",
            crate::correction::role::ELIGIBLE,
        )
    } else {
        ("e.kind IN (1, 2, 4, 5)", crate::correction::role::ELIGIBLE)
    };
    let window = |sql: &mut Sql| {
        sql.bind(plan.site)
            .push(&format!(" AND {kinds} AND e.event_time >= epoch_ms("))
            .bind(lo)
            .push(") AND e.event_time <= epoch_ms(")
            .bind(hi)
            .push(")");
    };
    let needs = Needs::of(&plan.fields);
    sql.push("WITH r AS (SELECT e.seq, e.kind, e.event_time, e.daily_id, e.profile_id, ");
    sql.push(if plan.needs_scope() {
        "m.session_id"
    } else {
        "NULL::BIGINT AS session_id"
    });
    for column in &needs.event {
        sql.push(&format!(", e.{column}"));
    }
    if plan.performance {
        for metric in PERF_METRICS {
            sql.push(&format!(", e.{metric}"));
        }
    }
    if plan.revenue {
        sql.push(", e.currency AS cur, e.revenue_minor AS rev");
    }
    // Session fields come from the stored session; events without one use their own (0004).
    for (column, stored) in &needs.session {
        sql.push(&format!(
            ", CASE WHEN m.session_id IS NULL THEN e.{column} ELSE s.{stored} END AS {stored}"
        ));
    }
    sql.push(" FROM events e");
    if plan.needs_scope() {
        // Sequences are unique across sites, so members and sessions join on them alone once
        // restricted to the site and the chunk.
        sql.push(" LEFT JOIN (SELECT seq, session_id FROM session_members WHERE site = ")
            .bind(plan.site)
            .push(&format!(" AND role = {role} AND event_time >= epoch_ms("))
            .bind(lo)
            .push(") AND event_time <= epoch_ms(")
            .bind(hi)
            .push(")) m ON m.seq = e.seq");
        if !needs.session.is_empty() {
            sql.push(" LEFT JOIN (SELECT * FROM sessions WHERE site = ")
                .bind(plan.site)
                .push(" AND start_time <= epoch_ms(")
                .bind(hi)
                .push(") AND last_time >= epoch_ms(")
                .bind(lo)
                .push(")) s ON s.session_id = m.session_id");
        }
    }
    sql.push(" WHERE e.site = ");
    window(sql);
    sql.push(") ");
    // The filter applies to the projected rows, so the engine pushes what it can into the
    // event and property scans instead of materializing the whole range first, and joins
    // only the property columns a cut reads.
    let mut kept = vec![
        "seq".to_owned(),
        "kind".into(),
        "t".into(),
        "daily_id".into(),
        "profile_id".into(),
    ];
    if plan.needs_sessions() {
        kept.push("session_id".into());
    }
    if plan.performance {
        kept.extend(["pm".to_owned(), "pv".into()]);
    }
    if plan.revenue {
        kept.extend(["cur".to_owned(), "rev".into()]);
    }
    for (i, field) in plan.fields.iter().enumerate() {
        if plan.read[i] {
            kept.extend(columns(i, field));
        }
    }
    sql.push(&format!("SELECT {} FROM (", kept.join(", ")));
    select_fields(plan, (lo, hi), sql);
    sql.push(" FROM r");
    if plan.performance {
        let metrics: Vec<String> = PERF_METRICS.iter().map(|m| format!("('{m}')")).collect();
        sql.push(&format!(
            " CROSS JOIN (VALUES {}) u(pm)",
            metrics.join(", ")
        ));
    }
    property_joins(plan, sources, (lo, hi), sql);
    sql.push(")");
    // A missing metric is no observation; an explicit zero is one (0003 Percentiles).
    let observed = plan.performance.then_some("pv IS NOT NULL");
    match (observed, plan.filter) {
        (None, None) => {}
        (Some(observed), None) => {
            sql.push(" WHERE ").push(observed);
        }
        (observed, Some(pred)) => {
            sql.push(" WHERE ");
            if let Some(observed) = observed {
                sql.push(observed).push(" AND ");
            }
            predicate(pred, &plan.fields, sql);
        }
    }
}

/// The table a property field's values are read from.
enum Values {
    /// The partition of an event property key (0008 Property partitions).
    Events(String),
    /// `<p>_sp<i>`: the latest identify value of each session (0002 Identify).
    Sessions(String),
}

/// Per field, the source of its property values. Session values are computed once per range,
/// from the identifies that can attach to a session overlapping it.
fn property_sources(
    conn: &Connection,
    plan: &Plan,
    prefix: &str,
    (lo, hi): (i64, i64),
) -> duckdb::Result<Vec<Option<Values>>> {
    let mut sources = Vec::new();
    for (i, field) in plan.fields.iter().enumerate() {
        let Field::Property(property) = field else {
            sources.push(None);
            continue;
        };
        if property.scope == PropertyScope::Event {
            sources.push(Some(Values::Events(
                holocron_storage::event_property_table(conn, &property.key)?,
            )));
            continue;
        }
        let (first, last): (Option<i64>, Option<i64>) = conn.query_row(
            "SELECT epoch_ms(min(start_time)), epoch_ms(max(ends_at)) FROM sessions
             WHERE site = ? AND start_time <= epoch_ms(?) AND last_time >= epoch_ms(?)",
            duckdb::params![plan.site, hi, lo],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let (from, to) = match (first, last) {
            (Some(first), Some(last)) => (first - IDENTIFY_LEAD_MS, last),
            _ => (lo, lo),
        };
        let latest = |column: &str| format!("arg_max_null({column}, (p.event_time, p.seq))");
        let mut sql = Sql::default();
        sql.push(&format!(
            "CREATE OR REPLACE TEMP TABLE {prefix}_sp{i} AS
             SELECT m.session_id, {} AS t, {} AS s, {} AS i, {} AS d, {} AS b, epoch_ms({}) AS dt, {} AS a
             FROM session_members m JOIN session_properties p ON p.seq = m.seq
             WHERE m.site = ",
            latest("p.type"),
            latest("p.v_string"),
            latest("p.v_integer"),
            latest("p.v_decimal"),
            latest("p.v_bool"),
            latest("p.v_date"),
            latest("p.v_array"),
        ))
        .bind(plan.site);
        for alias in ["m", "p"] {
            sql.push(&format!(" AND {alias}.event_time >= epoch_ms("))
                .bind(from)
                .push(&format!(") AND {alias}.event_time < epoch_ms("))
                .bind(to)
                .push(")");
        }
        sql.push(" AND m.role = 3 AND p.site = ")
            .bind(plan.site)
            .push(" AND p.key = ")
            .bind(property.key.clone())
            .push(" GROUP BY m.session_id");
        execute(conn, &sql)?;
        sources.push(Some(Values::Sessions(format!("{prefix}_sp{i}"))));
    }
    Ok(sources)
}

pub fn drop_tables(conn: &Connection, prefix: &str) -> duckdb::Result<()> {
    conn.execute_batch(&format!(
        "DROP TABLE IF EXISTS {prefix}_ev; DROP TABLE IF EXISTS {prefix}_se; DROP TABLE IF EXISTS {prefix}_days;"
    ))
}

pub(super) fn execute(conn: &Connection, sql: &Sql) -> duckdb::Result<usize> {
    note_plan(conn, sql)?;
    conn.execute(&sql.text, params_from_iter(sql.params.iter()))
}

thread_local! {
    /// Physical plans of the statements run while [`record_plans`] is active on this thread.
    static PLANS: std::cell::RefCell<Option<Vec<String>>> = const { std::cell::RefCell::new(None) };
}

/// Runs `f` and returns the `EXPLAIN` output of every planner statement it ran, in order. For
/// measurement tools: each statement is planned once more with the same bindings, not run twice.
pub fn record_plans<T>(f: impl FnOnce() -> T) -> (T, Vec<String>) {
    PLANS.with(|plans| *plans.borrow_mut() = Some(Vec::new()));
    let value = f();
    let plans = PLANS.with(|plans| plans.borrow_mut().take().unwrap_or_default());
    (value, plans)
}

pub(super) fn note_plan(conn: &Connection, sql: &Sql) -> duckdb::Result<()> {
    if PLANS.with(|plans| plans.borrow().is_none()) {
        return Ok(());
    }
    let mut statement = conn.prepare(&format!("EXPLAIN {}", sql.text))?;
    let lines: Vec<String> = statement
        .query_map(params_from_iter(sql.params.iter()), |row| {
            row.get::<_, String>(1)
        })?
        .collect::<duckdb::Result<_>>()?;
    PLANS.with(|plans| {
        if let Some(plans) = plans.borrow_mut().as_mut() {
            plans.push(format!("{}\n\n{}", sql.text, lines.join("\n")));
        }
    });
    Ok(())
}

/// Event columns the fields read: the event's own, and session-scoped ones as (event column,
/// stored session column) from [`SESSION_FIELDS`].
struct Needs {
    event: Vec<&'static str>,
    session: Vec<(&'static str, &'static str)>,
}

impl Needs {
    fn of(fields: &[Field]) -> Self {
        let mut needs = Needs {
            event: Vec::new(),
            session: Vec::new(),
        };
        let mut session = |stored: &str| {
            let (column, _, stored) = SESSION_FIELDS
                .into_iter()
                .find(|(_, _, s)| *s == stored)
                .expect("session fields are stored");
            if !needs.session.contains(&(column, stored)) {
                needs.session.push((column, stored));
            }
        };
        let mut event: Vec<&'static str> = Vec::new();
        for field in fields {
            if let Field::Dimension(d) = field {
                match sql::source(*d) {
                    Source::Event(c) => {
                        if !event.contains(&c) {
                            event.push(c);
                        }
                    }
                    Source::SessionFirst(c) | Source::SessionTouch(c) => session(c),
                    Source::Channel => {
                        for stored in ["touch_query", "referrer_domain", "utm_medium", "utm_source"]
                        {
                            session(stored);
                        }
                    }
                    Source::Weekday | Source::Hour | Source::PerfMetric => {}
                }
            }
        }
        needs.event = event;
        needs
    }
}

fn select_fields(plan: &Plan, (lo, hi): (i64, i64), out: &mut Sql) {
    out.push(
        "SELECT r.seq, r.kind, epoch_ms(r.event_time) AS t, r.daily_id, r.profile_id, r.session_id",
    );
    if plan.performance {
        let values: Vec<String> = PERF_METRICS
            .iter()
            .map(|m| format!("WHEN '{m}' THEN r.{m}"))
            .collect();
        out.push(&format!(", u.pm, CASE u.pm {} END AS pv", values.join(" ")));
    }
    if plan.revenue {
        out.push(", r.cur, r.rev");
    }
    let local = local_ms_sql(plan.timezone, lo, hi + 1, "epoch_ms(r.event_time)");
    for (i, field) in plan.fields.iter().enumerate() {
        match field {
            Field::Dimension(d) => {
                out.push(", ");
                match sql::source(*d) {
                    Source::Event(c) | Source::SessionFirst(c) | Source::SessionTouch(c) => {
                        out.push(&format!("r.{c}"));
                    }
                    Source::Channel => sql::channel(
                        "lower(r.touch_query)",
                        "lower(r.referrer_domain)",
                        "lower(r.utm_medium)",
                        "lower(r.utm_source)",
                        plan.hostnames,
                        out,
                    ),
                    Source::Weekday => {
                        out.push(&format!("isodow(epoch_ms({local}))"));
                    }
                    Source::Hour => {
                        out.push(&format!("hour(epoch_ms({local}))"));
                    }
                    Source::PerfMetric => {
                        out.push("u.pm");
                    }
                }
                out.push(&format!(" AS f{i}"));
            }
            Field::Property(_) => {
                for part in PROPERTY_PARTS {
                    out.push(&format!(", p{i}.{part} AS f{i}_{part}"));
                }
            }
        }
    }
}

fn property_joins(plan: &Plan, sources: &[Option<Values>], (lo, hi): (i64, i64), sql: &mut Sql) {
    for (i, field) in plan.fields.iter().enumerate() {
        let Field::Property(property) = field else {
            continue;
        };
        sql.push(" LEFT JOIN (SELECT * FROM (");
        let on = match &sources[i] {
            Some(Values::Events(table)) => {
                sql.push(&format!(
                    "SELECT seq AS id, type AS t, v_string AS s, v_integer AS i, v_decimal AS d, v_bool AS b,
                            epoch_ms(v_date) AS dt, v_array AS a
                     FROM {table} WHERE site = "
                ))
                .bind(plan.site)
                .push(" AND key = ")
                .bind(property.key.clone())
                .push(" AND event_time >= epoch_ms(")
                .bind(lo)
                .push(") AND event_time <= epoch_ms(")
                .bind(hi)
                .push(")");
                "seq"
            }
            Some(Values::Sessions(table)) => {
                sql.push(&format!(
                    "SELECT session_id AS id, t, s, i, d, b, dt, a FROM {table}"
                ));
                "session_id"
            }
            None => unreachable!("property fields have a source"),
        };
        sql.push(")");
        // A property only the filter reads keeps the rows one of its conditions can match.
        // Every other row fails those conditions as a missing row does, and the filter has
        // no negation outside a condition, so the matched events are the same.
        if !plan.read[i]
            && let Some(filter) = plan.filter
        {
            let mut tests = Vec::new();
            filter.visit(&mut |cond| {
                if cond.field == *field {
                    tests.push(cond.test.clone());
                }
            });
            sql.push(" WHERE ");
            for (n, test) in tests.iter().enumerate() {
                sql.push(if n == 0 { "(" } else { " OR (" });
                sql::property_test("", test, sql);
                sql.push(")");
            }
        }
        sql.push(&format!(") p{i} ON p{i}.id = r.{on}"));
    }
}

/// Totals of one cut over a materialized range.
pub fn totals(conn: &Connection, plan: &Plan, prefix: &str, cut: &CutSpec) -> duckdb::Result<Aggs> {
    Ok(totals_by(conn, plan, prefix, cut, false)?
        .remove(&0)
        .unwrap_or_default())
}

/// Totals of one cut per slot: per value of the `g` column of the materialized tables when
/// `grouped`, else all of them as slot 0. A slot without rows is absent.
pub fn totals_by(
    conn: &Connection,
    plan: &Plan,
    prefix: &str,
    cut: &CutSpec,
    grouped: bool,
) -> duckdb::Result<HashMap<i64, Aggs>> {
    let aggs = aggs_for(&cut.measures, &plan.fields, true);
    let mut out: HashMap<i64, Aggs> = HashMap::new();
    let (event, session): (Vec<Agg>, Vec<Agg>) = aggs.into_iter().partition(|a| !a.is_session());
    let list = |aggs: &[Agg]| aggs.iter().map(|a| a.sql()).collect::<Vec<_>>().join(", ");
    let (g, by, distinct) = if grouped {
        ("g", " GROUP BY g", "g, ")
    } else {
        ("0", "", "")
    };
    let mut read = |sql: String, aggs: &[Agg]| -> duckdb::Result<()> {
        let mut statement = conn.prepare(&sql)?;
        let mut result = statement.query([])?;
        while let Some(row) = result.next()? {
            let into = out.entry(row.get(0)?).or_default();
            for (n, agg) in aggs.iter().enumerate() {
                agg.store(&row.get::<_, Value>(n + 1)?, into);
            }
        }
        Ok(())
    };
    let sql = if plan.rollup {
        let sums: Vec<String> = event.iter().map(|a| a.rollup_sql()).collect();
        format!(
            "SELECT {g}::BIGINT, {} FROM ({}){by}",
            sums.join(", "),
            rollup::total_cells(prefix, grouped)
        )
    } else {
        format!("SELECT {g}::BIGINT, {} FROM {prefix}_ev{by}", list(&event))
    };
    read(sql, &event)?;
    if !session.is_empty() {
        read(
            format!(
                "SELECT {g}::BIGINT, {} FROM (SELECT DISTINCT {distinct}session_id, pv, ce, span FROM {prefix}_se){by}",
                list(&session)
            ),
            &session,
        )?;
    }
    if cut.measures.iter().any(is_money) {
        let mut statement = conn.prepare(&format!(
            "SELECT {g}::BIGINT, cur, {MONEY_SUMS} FROM {prefix}_ev WHERE rev IS NOT NULL GROUP BY ALL ORDER BY ALL"
        ))?;
        let mut result = statement.query([])?;
        while let Some(row) = result.next()? {
            let parts = (row.get(1)?, money_parts(row, 2)?);
            out.entry(row.get(0)?).or_default().money.push(parts);
        }
    }
    Ok(out)
}

/// Gross, refunds and net of the revenue records of a group, exactly (0003 Revenue), and their
/// count.
const MONEY_SUMS: &str = "coalesce(sum(rev) FILTER (WHERE rev > 0), 0)::HUGEINT AS g, \
     coalesce(sum(rev) FILTER (WHERE rev < 0), 0)::HUGEINT AS r, sum(rev)::HUGEINT AS n, \
     count(rev)::HUGEINT AS k";

fn money_parts(row: &duckdb::Row, from: usize) -> duckdb::Result<[i128; 4]> {
    let part = |i: usize| -> duckdb::Result<i128> {
        Ok(match row.get::<_, Value>(from + i)? {
            Value::HugeInt(n) => n,
            other => panic!("unexpected sum {other:?}"),
        })
    };
    Ok([part(0)?, part(1)?, part(2)?, part(3)?])
}

/// The rows of one page: their cells, raw key values (to look the same cells up in another
/// range) and aggregates. With `keys`, only those cells are read and paging is off.
pub struct Page {
    pub rows: Vec<(Vec<Cell>, Vec<Value>, Aggs)>,
    pub row_count: i64,
}

pub fn rows(
    conn: &Connection,
    plan: &Plan,
    prefix: &str,
    cut: &CutSpec,
    keys: Option<&[Vec<Value>]>,
) -> duckdb::Result<Page> {
    rows_by(conn, plan, prefix, cut, keys, false)
}

/// [`rows`], per slot when `grouped`: each row's raw key values then lead with its `g`, and
/// `keys` match the cut's own key columns.
pub fn rows_by(
    conn: &Connection,
    plan: &Plan,
    prefix: &str,
    cut: &CutSpec,
    keys: Option<&[Vec<Value>]>,
    grouped: bool,
) -> duckdb::Result<Page> {
    let fields = &plan.fields;
    let dims: Vec<(usize, Vec<String>)> = cut
        .dimensions
        .iter()
        .map(|d| {
            let i = field_index(fields, d);
            (i, columns(i, d))
        })
        .collect();
    let lead = usize::from(grouped);
    let key_columns: Vec<String> = grouped
        .then(|| "g".to_owned())
        .into_iter()
        .chain(dims.iter().flat_map(|(_, c)| c.clone()))
        .collect();
    let keys_sql = key_columns.join(", ");
    let aggs = aggs_for(&cut.measures, fields, false);
    let (event, session): (Vec<Agg>, Vec<Agg>) = aggs.iter().partition(|a| !a.is_session());
    let named = |aggs: &[Agg], offset: usize, sql: fn(Agg) -> String| {
        aggs.iter()
            .enumerate()
            .map(|(n, a)| format!("{} AS a{}", sql(*a), n + offset))
            .collect::<Vec<_>>()
            .join(", ")
    };

    let mut parts = Vec::new();
    if !event.is_empty() {
        let source = match (plan.rollup, cut.dimensions.as_slice()) {
            (true, [Field::Dimension(field)]) => {
                format!(
                    "({})",
                    rollup::field_cells(prefix, dims[0].0, *field, grouped)
                )
            }
            (true, _) => unreachable!("rollup::servable allows one dimension per cut"),
            (false, _) => format!("{prefix}_ev"),
        };
        let sql = if plan.rollup {
            Agg::rollup_sql
        } else {
            Agg::sql
        };
        parts.push(format!(
            "ea AS (SELECT {keys_sql}, {} FROM {source} GROUP BY {keys_sql})",
            named(&event, 0, sql)
        ));
    }
    if !session.is_empty() {
        parts.push(format!(
            "sa AS (SELECT {keys_sql}, {} FROM (SELECT DISTINCT {keys_sql}, session_id, pv, ce, span FROM {prefix}_se) GROUP BY {keys_sql})",
            named(&session, event.len(), Agg::sql)
        ));
    }
    let agg_names: Vec<String> = (0..aggs.len()).map(|n| format!("a{n}")).collect();
    let joined = match (event.is_empty(), session.is_empty()) {
        (false, true) => "SELECT * FROM ea".to_owned(),
        (true, false) => "SELECT * FROM sa".to_owned(),
        _ => {
            let on: Vec<String> = key_columns
                .iter()
                .map(|c| format!("ea.{c} IS NOT DISTINCT FROM sa.{c}"))
                .collect();
            let select: Vec<String> = key_columns
                .iter()
                .map(|c| format!("coalesce(ea.{c}, sa.{c}) AS {c}"))
                .chain(agg_names.iter().cloned())
                .collect();
            format!(
                "SELECT {} FROM ea FULL JOIN sa ON {}",
                select.join(", "),
                on.join(" AND ")
            )
        }
    };
    let money = cut.measures.iter().any(is_money);
    if money {
        // Per row, one list entry per currency, in code order.
        let on: Vec<String> = key_columns
            .iter()
            .map(|c| format!("j0.{c} IS NOT DISTINCT FROM ma.{c}"))
            .collect();
        parts.push(format!("j0 AS ({joined})"));
        parts.push(format!(
            "ma AS (SELECT {keys_sql}, list(cur ORDER BY cur) AS mc, list(g ORDER BY cur) AS mg,
                    list(r ORDER BY cur) AS mr, list(n ORDER BY cur) AS mn,
                    list(k ORDER BY cur) AS mk
             FROM (SELECT {keys_sql}, cur, {} FROM {prefix}_ev WHERE rev IS NOT NULL GROUP BY ALL)
             GROUP BY {keys_sql})",
            MONEY_SUMS
        ));
        parts.push(format!(
            "j AS (SELECT j0.*, ma.mc, ma.mg, ma.mr, ma.mn, ma.mk FROM j0 LEFT JOIN ma ON {})",
            on.join(" AND ")
        ));
    } else {
        parts.push(format!("j AS ({joined})"));
    }
    let money_columns = if money { ", mc, mg, mr, mn, mk" } else { "" };
    // The common table expressions carry no parameters, so the row count can reuse them.
    let with = format!("WITH {}", parts.join(", "));
    let mut sql = Sql::default();
    sql.push(&with);
    let selected: Vec<&str> = key_columns
        .iter()
        .chain(&agg_names)
        .map(String::as_str)
        .collect();
    sql.push(&format!(
        " SELECT {}{money_columns}, count(*) OVER () AS n FROM j",
        selected.join(", ")
    ));
    if let Some(keys) = keys {
        sql.push(" WHERE ");
        if keys.is_empty() {
            sql.push("false");
        }
        for (k, key) in keys.iter().enumerate() {
            sql.push(if k == 0 { "(" } else { " OR (" });
            for (c, (column, value)) in key_columns[lead..].iter().zip(key).enumerate() {
                if c > 0 {
                    sql.push(" AND ");
                }
                sql.push(&format!("{column} IS NOT DISTINCT FROM "))
                    .bind(value.clone());
            }
            sql.push(")");
        }
    }
    sql.push(" ORDER BY ");
    let mut order = Vec::new();
    for sort in &cut.sort {
        let dir = match sort.dir {
            SortDir::Asc => "ASC",
            SortDir::Desc => "DESC",
        };
        match &sort.by {
            // Validation keeps one currency per row; amounts order within their currency.
            SortKey::Measure(Measure::Named(named)) if is_money(&Measure::Named(*named)) => {
                let currency = field_index(fields, &Field::Dimension(DimensionField::Currency));
                order.push(format!("f{currency} ASC NULLS LAST"));
                order.push(format!("{}[1] {dir} NULLS LAST", money_list(*named)));
            }
            SortKey::Measure(m) => order.push(format!(
                "{} {dir} NULLS LAST",
                measure_order(m, &aggs, fields)
            )),
            SortKey::Field(f) => {
                let i = field_index(fields, f);
                order.extend(cell_order(i, f, dir));
            }
        }
    }
    for (i, d) in cut.dimensions.iter().map(|d| (field_index(fields, d), d)) {
        order.extend(cell_order(i, d, "ASC"));
    }
    sql.push(&order.join(", "));
    if keys.is_none() {
        sql.push(" LIMIT ")
            .bind(i64::from(cut.size))
            .push(" OFFSET ")
            .bind(i64::from(cut.offset));
    }

    note_plan(conn, &sql)?;
    let mut statement = conn.prepare(&sql.text)?;
    let mut result = statement.query(params_from_iter(sql.params.iter()))?;
    let mut rows = Vec::new();
    let mut row_count = 0;
    while let Some(row) = result.next()? {
        let raw: Vec<Value> = (0..key_columns.len())
            .map(|c| row.get(c))
            .collect::<duckdb::Result<_>>()?;
        let mut values = Aggs::default();
        for (n, agg) in aggs.iter().enumerate() {
            agg.store(&row.get::<_, Value>(key_columns.len() + n)?, &mut values);
        }
        let mut next = key_columns.len() + aggs.len();
        if money {
            values.money = money_lists(row, next)?;
            next += 5;
        }
        row_count = row.get(next)?;
        let mut offset = lead;
        let cells = cut
            .dimensions
            .iter()
            .map(|d| {
                let width = if matches!(d, Field::Property(_)) {
                    PROPERTY_PARTS.len()
                } else {
                    1
                };
                let cell = decode_cell(&raw[offset..offset + width]);
                offset += width;
                cell
            })
            .collect();
        rows.push((cells, raw, values));
    }
    drop(result);
    if rows.is_empty() && keys.is_none() && cut.offset > 0 {
        row_count = conn.query_row(&format!("{with} SELECT count(*) FROM j"), [], |r| r.get(0))?;
    }
    Ok(Page { rows, row_count })
}

/// The list column of a revenue amount in the joined row set.
fn money_list(measure: NamedMeasure) -> &'static str {
    match measure {
        NamedMeasure::RevenueGross => "mg",
        NamedMeasure::RevenueRefunds => "mr",
        _ => "mn",
    }
}

/// Decodes the `mc, mg, mr, mn, mk` lists of a row from column `from`.
fn money_lists(row: &duckdb::Row, from: usize) -> duckdb::Result<Vec<(String, [i128; 4])>> {
    let list = |i: usize| -> duckdb::Result<Vec<Value>> {
        Ok(match row.get::<_, Value>(from + i)? {
            Value::List(items) => items,
            Value::Null => Vec::new(),
            other => panic!("unexpected list {other:?}"),
        })
    };
    let huge = |v: &Value| match v {
        Value::HugeInt(n) => *n,
        other => panic!("unexpected sum {other:?}"),
    };
    let (codes, gross, refunds, net, records) = (list(0)?, list(1)?, list(2)?, list(3)?, list(4)?);
    Ok(codes
        .iter()
        .enumerate()
        .map(|(i, code)| {
            let Value::Text(code) = code else {
                panic!("unexpected currency {code:?}")
            };
            (
                code.clone(),
                [
                    huge(&gross[i]),
                    huge(&refunds[i]),
                    huge(&net[i]),
                    huge(&records[i]),
                ],
            )
        })
        .collect())
}

/// SQL ordering a measure in the joined row set: counts directly, rates and means as values.
fn measure_order(measure: &Measure, aggs: &[Agg], fields: &[Field]) -> String {
    let col = |agg: Agg| {
        format!(
            "a{}",
            aggs.iter()
                .position(|a| *a == agg)
                .expect("aggregate present")
        )
    };
    match measure {
        Measure::Named(named) => match named {
            NamedMeasure::Pageviews => col(Agg::Pageviews),
            NamedMeasure::Events => col(Agg::Events),
            NamedMeasure::Visitors | NamedMeasure::VisitorDays => col(Agg::Visitors),
            NamedMeasure::IdentifiedPeople => col(Agg::People),
            NamedMeasure::Sessions => col(Agg::Sessions),
            NamedMeasure::BounceRate => format!(
                "{}::DOUBLE / nullif({}, 0)",
                col(Agg::Bounces),
                col(Agg::PageviewSessions)
            ),
            NamedMeasure::SpanMean => format!(
                "{}::DOUBLE / nullif({}, 0)",
                col(Agg::SpanSum),
                col(Agg::PageviewSessions)
            ),
            NamedMeasure::SpanMedian => col(Agg::SpanMedian),
            NamedMeasure::RevenueCount => col(Agg::RevenueCount),
            NamedMeasure::P50 | NamedMeasure::P75 | NamedMeasure::P95 => {
                col(Agg::Percentile(percentile_index(*named)))
            }
            NamedMeasure::RevenueNet
            | NamedMeasure::RevenueGross
            | NamedMeasure::RevenueRefunds => {
                unreachable!("amounts are ordered by their currency list")
            }
        },
        Measure::Property(stat) => {
            let i = field_index(fields, &Field::Property(stat.field.clone()));
            match stat.stat {
                Stat::Count => col(Agg::StatCount(i)),
                Stat::Sum => col(Agg::StatSum(i)),
                Stat::Min => col(Agg::StatMin(i)),
                Stat::Max => col(Agg::StatMax(i)),
                Stat::Mean => format!(
                    "{}::DOUBLE / nullif({}, 0)",
                    col(Agg::StatSum(i)),
                    col(Agg::StatCount(i))
                ),
            }
        }
    }
}

/// Cell ordering: values in UTF-8 byte order, then `(null)`, then not set (0004).
fn cell_order(index: usize, field: &Field, dir: &str) -> Vec<String> {
    match field {
        Field::Dimension(_) => vec![format!("f{index} {dir} NULLS LAST")],
        Field::Property(_) => {
            let mut order = vec![format!("(f{index}_t IS NULL)"), format!("(f{index}_t = 0)")];
            order.extend(
                PROPERTY_PARTS
                    .iter()
                    .map(|part| format!("f{index}_{part} {dir} NULLS LAST")),
            );
            order
        }
    }
}

pub(super) fn decode_cell(raw: &[Value]) -> Cell {
    match raw {
        [Value::Null] => Cell::NotSet,
        [Value::Text(s)] => Cell::String(s.clone()),
        [Value::BigInt(n)] => Cell::Integer(*n),
        [Value::Int(n)] => Cell::Integer(i64::from(*n)),
        [t, s, i, d, b, dt, a] => match t {
            Value::Null => Cell::NotSet,
            Value::UTinyInt(0) => Cell::Null,
            Value::UTinyInt(1) => Cell::Bool(matches!(b, Value::Boolean(true))),
            Value::UTinyInt(2) => Cell::Integer(as_i64(i)),
            Value::UTinyInt(3) => Cell::Decimal(decimal(i128::from(as_i64(d)))),
            Value::UTinyInt(4) => Cell::Date(instant(as_i64(dt))),
            Value::UTinyInt(5) => Cell::String(text(s)),
            // Arrays group by their stored JSON text.
            _ => Cell::String(text(a)),
        },
        other => panic!("unexpected cell {other:?}"),
    }
}

fn as_i64(value: &Value) -> i64 {
    match value {
        Value::BigInt(n) => *n,
        Value::Int(n) => i64::from(*n),
        other => panic!("expected an integer, got {other:?}"),
    }
}

fn text(value: &Value) -> String {
    match value {
        Value::Text(s) => s.clone(),
        other => panic!("expected text, got {other:?}"),
    }
}

/// A value scaled by 10^4 as a decimal string with 4 places.
pub fn decimal(scaled: i128) -> String {
    let sign = if scaled < 0 { "-" } else { "" };
    let abs = scaled.unsigned_abs();
    format!("{sign}{}.{:04}", abs / 10_000, abs % 10_000)
}

pub fn instant(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms)
        .expect("instant in range")
        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
        .to_string()
}

/// The series of the main cut's totals, one point per wall-clock bucket.
pub fn series(
    conn: &Connection,
    plan: &Plan,
    prefix: &str,
    span: &Span,
    granularity: Granularity,
    measures: &[Measure],
) -> duckdb::Result<Result<Series, Refusal>> {
    let starts = match buckets(plan.timezone, span, granularity) {
        Ok(starts) => starts,
        Err(refusal) => return Ok(Err(refusal)),
    };
    let per_bucket = bucket_aggs(conn, plan, prefix, span, granularity, measures, &starts)?;
    let points = starts
        .iter()
        .zip(&per_bucket)
        .map(|((local, start), aggs)| SeriesPoint {
            start: *start,
            local: local.format("%Y-%m-%dT%H:%M").to_string(),
            values: values(measures, aggs, &plan.fields),
            compare: None,
        })
        .collect();
    Ok(Ok(Series {
        granularity,
        points,
        series_sum_differs: false,
        projection: None,
    }))
}

/// The aggregates of a materialized span per wall-clock bucket of `starts`, in their order.
pub fn bucket_aggs(
    conn: &Connection,
    plan: &Plan,
    prefix: &str,
    span: &Span,
    granularity: Granularity,
    measures: &[Measure],
    starts: &[(NaiveDateTime, i64)],
) -> duckdb::Result<Vec<Aggs>> {
    let mut by = buckets_by(conn, plan, prefix, span, granularity, measures, false)?;
    Ok(starts
        .iter()
        .map(|(local, _)| by.remove(&(0, *local)).unwrap_or_default())
        .collect())
}

/// The aggregates of a materialized span per slot and local bucket start: per value of the
/// `g` column when `grouped`, else all of them as slot 0. Local times read the offsets of
/// `span`, which must cover every instant read.
pub fn buckets_by(
    conn: &Connection,
    plan: &Plan,
    prefix: &str,
    span: &Span,
    granularity: Granularity,
    measures: &[Measure],
    grouped: bool,
) -> duckdb::Result<HashMap<(i64, NaiveDateTime), Aggs>> {
    let mut out: HashMap<(i64, NaiveDateTime), Aggs> = HashMap::new();
    let aggs = aggs_for(measures, &plan.fields, false);
    let (event, session): (Vec<Agg>, Vec<Agg>) = aggs.into_iter().partition(|a| !a.is_session());
    let (g, distinct) = if grouped { ("g", "g, ") } else { ("0", "") };
    let bucket = |column: &str| {
        format!(
            "date_trunc('{}', epoch_ms({}))",
            trunc(granularity),
            local_ms_sql(plan.timezone, span.start_ms, span.end_ms, column)
        )
    };
    let local = |value: Value| match value {
        Value::Timestamp(TimeUnit::Microsecond, us) => chrono::DateTime::from_timestamp_micros(us)
            .expect("bucket")
            .naive_utc(),
        other => panic!("unexpected bucket {other:?}"),
    };
    let mut read = |sql: String, aggs: &[Agg]| -> duckdb::Result<()> {
        let mut statement = conn.prepare(&sql)?;
        let mut result = statement.query([])?;
        while let Some(row) = result.next()? {
            let into = out.entry((row.get(0)?, local(row.get(1)?))).or_default();
            for (n, agg) in aggs.iter().enumerate() {
                agg.store(&row.get::<_, Value>(n + 2)?, into);
            }
        }
        Ok(())
    };
    if !event.is_empty() {
        let sql = if plan.rollup {
            let sums: Vec<String> = event.iter().map(|a| a.rollup_sql()).collect();
            let cells = rollup::bucket_cells(
                prefix,
                bucket,
                granularity,
                event.contains(&Agg::Visitors),
                grouped,
            );
            format!(
                "SELECT {g}::BIGINT, bucket, {} FROM ({cells}) GROUP BY ALL",
                sums.join(", ")
            )
        } else {
            let list = event.iter().map(|a| a.sql()).collect::<Vec<_>>().join(", ");
            format!(
                "SELECT {g}::BIGINT, {} AS bucket, {list} FROM {prefix}_ev GROUP BY ALL",
                bucket("t")
            )
        };
        read(sql, &event)?;
    }
    if !session.is_empty() {
        let list = session
            .iter()
            .map(|a| a.sql())
            .collect::<Vec<_>>()
            .join(", ");
        read(
            format!(
                "SELECT {g}::BIGINT, {} AS bucket, {list} FROM (SELECT DISTINCT {distinct}session_id, pv, ce, span, st FROM {prefix}_se) GROUP BY ALL",
                bucket("st")
            ),
            &session,
        )?;
    }
    if measures.iter().any(is_money) {
        let sql = format!(
            "SELECT {g}::BIGINT, {} AS bucket, cur, {MONEY_SUMS} FROM {prefix}_ev WHERE rev IS NOT NULL GROUP BY ALL ORDER BY ALL",
            bucket("t")
        );
        let mut statement = conn.prepare(&sql)?;
        let mut result = statement.query([])?;
        while let Some(row) = result.next()? {
            let parts = (row.get(2)?, money_parts(row, 3)?);
            let key = (row.get(0)?, local(row.get(1)?));
            out.entry(key).or_default().money.push(parts);
        }
    }
    Ok(out)
}

/// One measure's value from aggregates (0003 Formulas).
pub fn metric(measure: &Measure, aggs: &Aggs, fields: &[Field]) -> Metric {
    let count = |v: i64| Metric::Count { v };
    match measure {
        Measure::Named(named) => match named {
            NamedMeasure::Pageviews => count(aggs.pageviews),
            NamedMeasure::Events => count(aggs.events),
            NamedMeasure::Visitors | NamedMeasure::VisitorDays => count(aggs.visitors),
            NamedMeasure::IdentifiedPeople => count(aggs.people),
            NamedMeasure::Sessions => count(aggs.sessions),
            NamedMeasure::BounceRate => match aggs.pageview_sessions {
                0 => Metric::Unavailable {
                    reason: Reason::NoDenominator,
                },
                denominator => Metric::Ratio {
                    v: ratio4(i128::from(aggs.bounces), i128::from(denominator)),
                    numerator: aggs.bounces,
                    denominator,
                },
            },
            NamedMeasure::SpanMean => match aggs.pageview_sessions {
                0 => Metric::Unavailable {
                    reason: Reason::NoDenominator,
                },
                n => Metric::Millis {
                    v: div_half_even(i128::from(aggs.span_sum), i128::from(n)) as i64,
                },
            },
            NamedMeasure::SpanMedian => match aggs.span_median {
                Some(v) => Metric::Millis { v },
                None => Metric::Unavailable {
                    reason: Reason::NoDenominator,
                },
            },
            NamedMeasure::RevenueCount => count(aggs.revenue_count),
            NamedMeasure::RevenueGross
            | NamedMeasure::RevenueRefunds
            | NamedMeasure::RevenueNet => {
                let part = match named {
                    NamedMeasure::RevenueGross => 0,
                    NamedMeasure::RevenueRefunds => 1,
                    _ => 2,
                };
                Metric::Money {
                    v: aggs
                        .money
                        .iter()
                        .filter_map(|(code, parts)| Money::new(code, parts[part]))
                        .collect(),
                }
            }
            NamedMeasure::P50 | NamedMeasure::P75 | NamedMeasure::P95 => {
                match aggs.percentiles[percentile_index(*named)] {
                    _ if aggs.perf_metrics > 1 => Metric::Unavailable {
                        reason: Reason::MixedMetrics,
                    },
                    None => Metric::Unavailable {
                        reason: Reason::NoValues,
                    },
                    Some(v) if aggs.perf_metric.as_deref() == Some("cls") => Metric::Decimal {
                        v: decimal(i128::from(v)),
                    },
                    Some(v) => Metric::Millis { v },
                }
            }
        },
        Measure::Property(stat) => {
            let i = field_index(fields, &Field::Property(stat.field.clone()));
            let (n, sum, min, max) = aggs.stats.get(&i).copied().unwrap_or_default();
            let no_values = Metric::Unavailable {
                reason: Reason::NoValues,
            };
            let dec = |v: i128| Metric::Decimal { v: decimal(v) };
            match stat.stat {
                Stat::Count => count(n),
                Stat::Sum => dec(sum),
                Stat::Min => min.map_or(no_values, dec),
                Stat::Max => max.map_or(no_values, dec),
                Stat::Mean if n == 0 => no_values,
                Stat::Mean => dec(div_half_even(sum, i128::from(n))),
            }
        }
    }
}

/// Exact value of a metric as a fraction, for change and relative change.
fn fraction(metric: &Metric) -> Option<(i128, i128)> {
    match metric {
        Metric::Count { v } | Metric::Millis { v } => Some((i128::from(*v), 1)),
        Metric::Ratio {
            numerator,
            denominator,
            ..
        } => Some((i128::from(*numerator), i128::from(*denominator))),
        Metric::Decimal { v } => {
            let (whole, frac) = v.split_once('.')?;
            let negative = whole.starts_with('-');
            let magnitude = whole.trim_start_matches('-').parse::<i128>().ok()? * 10_000
                + frac.parse::<i128>().ok()?;
            Some((if negative { -magnitude } else { magnitude }, 10_000))
        }
        // One currency compares in minor units; several never do (0003 Revenue).
        Metric::Money { v } => match v.as_slice() {
            [] => Some((0, 1)),
            [one] => Some((one.minor_units, 1)),
            _ => None,
        },
        Metric::Unavailable { .. } => None,
    }
}

/// Whether two revenue amounts hold at most one currency between them.
fn one_currency(current: &Metric, previous: &Metric) -> bool {
    match (current, previous) {
        (Metric::Money { v: a }, Metric::Money { v: b }) => {
            let mut codes: Vec<&str> = a.iter().chain(b).map(|m| m.currency.as_str()).collect();
            codes.dedup();
            codes.len() <= 1
        }
        _ => true,
    }
}

/// Change and relative change against the previous period (0003 Comparison and zero baselines).
pub fn compare(current: &Metric, previous: Metric) -> Comparison {
    let (Some((cn, cd)), Some((pn, pd)), true) = (
        fraction(current),
        fraction(&previous),
        one_currency(current, &previous),
    ) else {
        // A baseline without enough history says so on the comparison, like `zero_baseline`.
        let reason = matches!(
            previous,
            Metric::Unavailable {
                reason: Reason::InsufficientHistory
            }
        )
        .then_some(Reason::InsufficientHistory);
        return Comparison {
            previous,
            change: None,
            relative_change: None,
            reason,
        };
    };
    // current − previous = (cn·pd − pn·cd) / (cd·pd); relative to previous = that / (pn / pd).
    let delta = cn * pd - pn * cd;
    let change = ratio4(delta, cd * pd);
    let (relative_change, reason) = if pn == 0 {
        (None, Some(Reason::ZeroBaseline))
    } else {
        (Some(ratio4(delta, pn * cd)), None)
    };
    Comparison {
        previous,
        change: Some(change),
        relative_change,
        reason,
    }
}

/// `num / den` rounded half to even to 4 places (0004 Encoding conventions).
pub fn ratio4(num: i128, den: i128) -> f64 {
    div_half_even(num * 10_000, den) as f64 / 10_000.0
}

pub fn div_half_even(num: i128, den: i128) -> i128 {
    let (num, den) = if den < 0 { (-num, -den) } else { (num, den) };
    let (q, r) = (num.div_euclid(den), num.rem_euclid(den));
    match (2 * r).cmp(&den) {
        std::cmp::Ordering::Less => q,
        std::cmp::Ordering::Greater => q + 1,
        std::cmp::Ordering::Equal => q + (q & 1),
    }
}

/// One value per measure from aggregates.
pub fn values(measures: &[Measure], aggs: &Aggs, fields: &[Field]) -> Vec<Metric> {
    measures.iter().map(|m| metric(m, aggs, fields)).collect()
}

/// What a cut compares with: one value per measure for the totals and for each page row, in
/// page order, and where the totals end when the range contains now.
pub struct Compared {
    pub totals: Vec<Metric>,
    pub rows: Vec<Vec<Metric>>,
    pub projection: Option<Vec<Option<Projection>>>,
}

impl Compared {
    /// The previous period's values of a page's totals and rows; a row missing there is 0.
    pub fn previous(
        spec: &CutSpec,
        fields: &[Field],
        page: &Page,
        totals: &Aggs,
        rows: &HashMap<Vec<Cell>, Aggs>,
    ) -> Self {
        let empty = Aggs::default();
        Self {
            totals: values(&spec.measures, totals, fields),
            rows: page
                .rows
                .iter()
                .map(|(key, _, _)| values(&spec.measures, rows.get(key).unwrap_or(&empty), fields))
                .collect(),
            projection: None,
        }
    }
}

/// One comparison per measure.
pub fn comparisons(current: &[Metric], previous: Vec<Metric>) -> Vec<Comparison> {
    current
        .iter()
        .zip(previous)
        .map(|(c, p)| compare(c, p))
        .collect()
}

/// Sets each series point's comparison from the values of the bucket it compares with, by
/// position; a point past the last of them keeps none.
pub fn compare_points(series: &mut Series, previous: Vec<Vec<Metric>>) {
    for (point, previous) in series.points.iter_mut().zip(previous) {
        point.compare = Some(comparisons(&point.values, previous));
    }
}

/// Assembles one cut from its totals and page, with what it compares with when compared.
pub fn cut(
    spec: &CutSpec,
    fields: &[Field],
    totals: &Aggs,
    page: Page,
    compared: Option<Compared>,
    series: Option<Series>,
) -> Cut {
    let totals_values = values(&spec.measures, totals, fields);
    let (compare_totals, mut compare_rows, projection) = match compared {
        Some(c) => (
            Some(comparisons(&totals_values, c.totals)),
            Some(c.rows.into_iter()),
            c.projection,
        ),
        None => (None, None, None),
    };
    let rows = page
        .rows
        .into_iter()
        .map(|(key, _, aggs)| {
            let current = values(&spec.measures, &aggs, fields);
            let compare = compare_rows
                .as_mut()
                .map(|rows| comparisons(&current, rows.next().expect("one per page row")));
            Row {
                key,
                values: current,
                compare,
            }
        })
        .collect();
    Cut {
        cut: spec.panel.clone(),
        totals: totals_values,
        compare: compare_totals,
        projection,
        rows,
        row_count: if spec.dimensions.is_empty() {
            0
        } else {
            page.row_count
        },
        series,
        identity_coverage: Coverage {
            with_identity: totals.with_identity,
            eligible_events: totals.eligible,
        },
    }
}

/// Retention-clipped instants of a span.
pub fn clip(span: &Span, cutoff: i64) -> (i64, i64, Option<i64>) {
    let start = span.start_ms.max(cutoff);
    (
        start,
        span.end_ms.max(start),
        (cutoff > span.start_ms).then_some(cutoff),
    )
}

/// Every condition of a filter that reads `field`, for naming refusals.
pub fn first_condition(filter: Option<&Pred>, field: &Field) -> Option<String> {
    let mut found = None;
    if let Some(pred) = filter {
        pred.visit(&mut |cond| {
            if found.is_none() && cond.field == *field {
                found = Some(format!("filter.{}", cond.id));
            }
        });
    }
    found
}

/// The `limit` most frequent cells of field 0 in `v_ev`.
pub fn top_values(
    conn: &Connection,
    field: &Field,
    prefix: Option<&str>,
    limit: usize,
) -> duckdb::Result<Vec<(Cell, i64)>> {
    let cols = columns(0, field).join(", ");
    let mut sql = Sql::default();
    sql.push(&format!("SELECT {cols}, count(*) AS n FROM v_ev"));
    if let Some(prefix) = prefix.filter(|p| !p.is_empty()) {
        let text = match field {
            Field::Dimension(_) => "f0::VARCHAR",
            Field::Property(_) => "f0_s",
        };
        sql.push(&format!(" WHERE starts_with(lower({text}), "))
            .bind(prefix.to_lowercase())
            .push(")");
    }
    sql.push(&format!(
        " GROUP BY ALL ORDER BY n DESC, {cols} LIMIT {limit}"
    ));
    let width = columns(0, field).len();
    note_plan(conn, &sql)?;
    let mut statement = conn.prepare(&sql.text)?;
    let mut result = statement.query(params_from_iter(sql.params.iter()))?;
    let mut out = Vec::new();
    while let Some(row) = result.next()? {
        let raw: Vec<Value> = (0..width)
            .map(|c| row.get(c))
            .collect::<duckdb::Result<_>>()?;
        out.push((decode_cell(&raw), row.get(width)?));
    }
    Ok(out)
}
