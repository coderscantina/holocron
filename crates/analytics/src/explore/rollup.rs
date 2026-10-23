//! Exact identity-day rollups (0008 Rollups). A daily pseudonym exists for one identity day,
//! so distinct visitor-days over a range are the sum of per-day distinct counts. A request
//! reads the rollups of every clean whole day inside its range and counts the rest from
//! events, so its result equals the raw planner's and never lags ingestion.
//!
//! One rule picks the source: a state is served from rollups when [`servable`] holds, and then
//! every cut, total and series of the request is. Otherwise the raw planner runs alone.
//!
//! A day is read from rollups when it lies whole inside the range in one timezone epoch, has
//! no dirty row, has current rollups (`day_aggregates` at this metric version) or none at all,
//! and, for the series, its rollup cells fall into single buckets. Every other instant of the
//! range is materialized from events into `<p>_ev` as the raw planner does. A count of the
//! range's eligible events confirms the split; when it differs the request counts from events
//! alone. Session measures read `sessions` directly: the raw planner counts the same rows.

use std::collections::{HashMap, HashSet};

use chrono::{Datelike, Days, NaiveDate, NaiveDateTime, TimeZone, Timelike};
use chrono_tz::Tz;
use holocron_protocol::exploration::{DimensionField, Field, Granularity, Measure, NamedMeasure};
use holocron_storage::duckdb::types::Value;
use holocron_storage::duckdb::{self, Connection, params};

use super::resolve::step;
use super::run::{self, Plan};
use super::sql::{Source, source};
use super::validate::{Query, is_session_measure};
use crate::METRIC_VERSION;
use crate::correction::role;
use crate::identity::{SitePolicies, date_of_local_day};

const HOUR_MS: i64 = 3_600_000;

/// Dimensions with a per-day rollup: the overview's panels.
pub const FIELDS: [DimensionField; 5] = [
    DimensionField::Path,
    DimensionField::ReferrerDomain,
    DimensionField::Browser,
    DimensionField::Os,
    DimensionField::Device,
];

/// Whether the rollups can serve every cut of a state: no filter, and per cut either no
/// dimension with any measure but identified people, or one rollup dimension with event
/// counts and visitors.
pub fn servable(query: &Query) -> bool {
    let event_measure = |m: &Measure| {
        matches!(
            m,
            Measure::Named(
                NamedMeasure::Pageviews
                    | NamedMeasure::Events
                    | NamedMeasure::Visitors
                    | NamedMeasure::VisitorDays
            )
        )
    };
    query.filter.is_none()
        && query.cuts().all(|cut| match cut.dimensions.as_slice() {
            [] => cut
                .measures
                .iter()
                .all(|m| event_measure(m) || is_session_measure(m)),
            [Field::Dimension(field)] => {
                FIELDS.contains(field) && cut.measures.iter().all(event_measure)
            }
            _ => false,
        })
}

/// Where a rollup dimension's value comes from (0004 Field scope).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Scope {
    Event,
    /// The session's first event, whose sequence is the session ID.
    First,
    /// The session's first pageview, else its first event.
    Touch,
}

/// The event column a rollup dimension reads, and where its value comes from.
fn column(field: DimensionField) -> (&'static str, Scope) {
    match source(field) {
        Source::Event(c) => (c, Scope::Event),
        Source::SessionFirst(c) => (c, Scope::First),
        Source::SessionTouch(c) => (c, Scope::Touch),
        _ => unreachable!("rollup fields are column fields"),
    }
}

/// Correction SQL rebuilding the rollups of every `(site, local_day)` in the temp table
/// `rollup_days`. Session-scoped fields take the value of the event's session, wherever the
/// session started, as the planner does (0004 Field scope).
pub(crate) fn refresh_sql() -> String {
    let fields: Vec<_> = FIELDS.iter().map(|f| column(*f)).collect();
    let with = |scopes: &[Scope], prefix: &str| -> String {
        fields
            .iter()
            .filter(|(_, scope)| scopes.contains(scope))
            .map(|(c, _)| format!("{prefix}{c}"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let raw = with(&[Scope::Event, Scope::First, Scope::Touch], "e.");
    let session = with(&[Scope::First, Scope::Touch], "e.");
    let late: Vec<String> = fields
        .iter()
        .filter(|(_, scope)| *scope == Scope::Touch)
        .map(|(c, _)| format!("arg_min_null(e.{c}, (e.event_time, e.seq)) AS {c}"))
        .collect();
    let touch: Vec<String> = fields
        .iter()
        .filter(|(_, scope)| *scope != Scope::Event)
        .map(|(c, scope)| match scope {
            Scope::Touch => {
                format!("CASE WHEN p.session_id IS NULL THEN f.{c} ELSE p.{c} END AS {c}")
            }
            _ => format!("f.{c}"),
        })
        .collect();
    let resolved: Vec<String> = fields
        .iter()
        .map(|(c, scope)| match scope {
            Scope::Event => format!("ev.{c}"),
            _ => format!("CASE WHEN ev.session_id IS NULL THEN ev.{c} ELSE t.{c} END AS {c}"),
        })
        .collect();
    let dimensions: Vec<String> = fields
        .iter()
        .map(|(c, _)| {
            // Grouping by pseudonym first keeps the distinct count spillable.
            format!(
                "SELECT site, local_day, '{c}', {c}, sum(pageviews), sum(custom_events), count(daily_id)
                 FROM (SELECT site, local_day, {c}, daily_id, count(*) FILTER (WHERE kind = 1) AS pageviews,
                              count(*) FILTER (WHERE kind = 2) AS custom_events
                       FROM day_rows GROUP BY site, local_day, {c}, daily_id)
                 GROUP BY site, local_day, {c}"
            )
        })
        .collect();
    let mut sql = String::new();
    for table in ["hour_aggregates", "dimension_aggregates", "visitor_days"] {
        sql += &format!(
            "DELETE FROM {table} a USING rollup_days r WHERE a.site = r.site AND a.local_day = r.local_day;\n"
        );
    }
    sql += &format!(
        "CREATE OR REPLACE TEMP TABLE day_events AS
        SELECT e.site, e.local_day, e.kind, e.event_time, e.daily_id, m.session_id, {raw}
        FROM events e JOIN rollup_days r ON e.site = r.site AND e.local_day = r.local_day
        LEFT JOIN session_members m ON m.site = e.site AND m.seq = e.seq AND m.role = {eligible}
        WHERE e.kind IN (1, 2);

        CREATE OR REPLACE TEMP TABLE day_sessions AS
        SELECT s.site, s.session_id, s.pageviews FROM sessions s
        SEMI JOIN day_events d ON d.site = s.site AND d.session_id = s.session_id;

        CREATE OR REPLACE TEMP TABLE day_first AS
        SELECT e.site, e.seq AS session_id, e.kind, {session}
        FROM events e SEMI JOIN day_sessions s ON s.site = e.site AND s.session_id = e.seq;

        -- Sessions that open with a custom event take touch fields from their first pageview.
        CREATE OR REPLACE TEMP TABLE day_touch AS
        WITH late AS (
            SELECT m.site, m.session_id, {late}
            FROM session_members m
            JOIN day_first f ON f.site = m.site AND f.session_id = m.session_id AND f.kind <> 1
            JOIN day_sessions s ON s.site = f.site AND s.session_id = f.session_id AND s.pageviews > 0
            JOIN events e ON e.site = m.site AND e.seq = m.seq AND e.kind = 1
            WHERE m.role = {eligible}
            GROUP BY m.site, m.session_id
        )
        SELECT f.site, f.session_id, {touch}
        FROM day_first f LEFT JOIN late p ON p.site = f.site AND p.session_id = f.session_id;

        CREATE OR REPLACE TEMP TABLE day_rows AS
        SELECT ev.site, ev.local_day, ev.kind, ev.event_time, ev.daily_id, {resolved}
        FROM day_events ev LEFT JOIN day_touch t ON t.site = ev.site AND t.session_id = ev.session_id;

        INSERT INTO hour_aggregates
        SELECT site, local_day, date_trunc('hour', event_time), count(*) FILTER (WHERE kind = 1),
               count(*) FILTER (WHERE kind = 2), count(DISTINCT daily_id), min(event_time), max(event_time)
        FROM day_rows GROUP BY ALL;

        INSERT INTO dimension_aggregates {dimensions};

        INSERT INTO visitor_days
        SELECT site, local_day, daily_id, min(event_time), max(event_time)
        FROM day_rows WHERE daily_id IS NOT NULL GROUP BY ALL;

        DROP TABLE day_events; DROP TABLE day_sessions; DROP TABLE day_first; DROP TABLE day_touch;
        DROP TABLE day_rows;
        ",
        late = late.join(", "),
        touch = touch.join(", "),
        resolved = resolved.join(", "),
        dimensions = dimensions.join(" UNION ALL "),
        eligible = role::ELIGIBLE,
    );
    sql
}

/// One local day served from rollups and its instants `[start, end)`.
#[derive(Debug, Clone, Copy)]
struct Day {
    local_day: i32,
    start: i64,
    end: i64,
}

/// Materializes one range for a servable state: `<p>_days` (served days), `<p>_ev` (events of
/// every other instant, as the raw planner materializes them) and, for session measures,
/// `<p>_se` (sessions starting in the range). `series` restricts days to those whose cells
/// fall into single buckets of the main cut's series.
///
/// Returns whether the range is exact: its eligible events are exactly the served days' and
/// those of `<p>_ev`. When not, a day with events lacks rollups or local days were stamped
/// under another timezone than the policy copy here, and the caller counts the request from
/// events alone.
pub fn materialize(
    conn: &Connection,
    plan: &Plan,
    policies: &SitePolicies,
    prefix: &str,
    bounds: (i64, i64),
    series: Option<Granularity>,
) -> duckdb::Result<bool> {
    materialize_days(conn, plan, policies, prefix, bounds, series, |_, _, _| true)
}

/// [`materialize`] serving only the days `keep` accepts by their date and instants
/// `[start, end)`; the others are read from events.
pub fn materialize_days(
    conn: &Connection,
    plan: &Plan,
    policies: &SitePolicies,
    prefix: &str,
    (a, b): (i64, i64),
    series: Option<Granularity>,
    keep: impl Fn(NaiveDate, i64, i64) -> bool,
) -> duckdb::Result<bool> {
    let mut days = whole_days(policies, (a, b));
    days.retain(|day| keep(date_of_local_day(day.local_day), day.start, day.end));
    if let Some(granularity) = series {
        days.retain(|day| fits(plan.timezone, granularity, day));
    }
    let days = current(conn, plan.site, days)?;
    let list: Vec<String> = days.iter().map(|d| d.local_day.to_string()).collect();
    conn.execute(
        &format!(
            "CREATE OR REPLACE TEMP TABLE {prefix}_days AS
             SELECT ?::UHUGEINT AS site, (DATE '1970-01-01' + d)::DATE AS local_day
             FROM unnest([{}]::INTEGER[]) t(d)",
            list.join(", ")
        ),
        params![plan.site],
    )?;

    let mut raw = Vec::new();
    let mut from = a;
    for day in &days {
        if day.start > from {
            raw.push((from, day.start));
        }
        from = day.end;
    }
    if from < b || raw.is_empty() {
        // An empty range still creates `<p>_ev` with its columns.
        raw.push((from.min(b), b));
    }
    let events = Plan {
        site: plan.site,
        timezone: plan.timezone,
        filter: None,
        fields: plan.fields.clone(),
        read: plan.read.clone(),
        sessions: false,
        rollup: false,
        hostnames: plan.hostnames,
        performance: false,
        revenue: false,
        revenue_only: false,
    };
    let mut parts = Vec::new();
    for (k, span) in raw.into_iter().enumerate() {
        let part = format!("{prefix}{k}");
        run::materialize(conn, &events, &part, span)?;
        parts.push(part);
    }
    let union: Vec<String> = parts
        .iter()
        .map(|p| format!("SELECT * FROM {p}_ev"))
        .collect();
    conn.execute_batch(&format!(
        "CREATE OR REPLACE TEMP TABLE {prefix}_ev AS {};",
        union.join(" UNION ALL ")
    ))?;
    for part in &parts {
        run::drop_tables(conn, part)?;
    }

    if plan.sessions {
        conn.execute(
            &format!(
                "CREATE OR REPLACE TEMP TABLE {prefix}_se AS
                 SELECT session_id, pageviews AS pv, custom_events AS ce,
                        epoch_ms(last_time) - epoch_ms(start_time) AS span, epoch_ms(start_time) AS st
                 FROM sessions WHERE site = ? AND start_time >= epoch_ms(?) AND start_time < epoch_ms(?)"
            ),
            params![plan.site, a, b],
        )?;
    }
    conn.query_row(
        &format!(
            "SELECT (SELECT count(*) FROM events
                     WHERE site = ? AND kind IN (1, 2) AND event_time >= epoch_ms(?) AND event_time < epoch_ms(?))
                  = (SELECT count(*) FROM {prefix}_ev)
                    + (SELECT coalesce(sum(a.eligible_events), 0) FROM day_aggregates a
                       JOIN {prefix}_days d ON a.site = d.site AND a.local_day = d.local_day
                       WHERE a.metric_version = ?)"
        ),
        params![plan.site, a, b, METRIC_VERSION],
        |r| r.get(0),
    )
}

/// Identity days lying whole inside `[a, b)` whose local day no other timezone epoch shares.
fn whole_days(policies: &SitePolicies, (a, b): (i64, i64)) -> Vec<Day> {
    let shared = shared_dates(policies);
    let mut days = Vec::new();
    let mut t = a;
    while t < b {
        let day = policies.identity_day(t);
        let end = policies.day_end_ms(day);
        let starts_here = policies.identity_day(t - 1).date != day.date;
        if starts_here && end <= b && !shared.contains(&day.date) {
            days.push(Day {
                local_day: day.local_day(),
                start: t,
                end,
            });
        }
        t = end;
    }
    days
}

/// Local dates two timezone epochs both have: their stored local day mixes both, without the
/// epoch (0001 Timezone change).
fn shared_dates(policies: &SitePolicies) -> HashSet<NaiveDate> {
    let starts: Vec<i64> = policies
        .versions()
        .windows(2)
        .filter(|pair| pair[1].epoch != pair[0].epoch)
        .map(|pair| pair[1].effective_ms)
        .collect();
    // Each epoch's dates, from its first instant to its last.
    let ranges: Vec<(Option<NaiveDate>, Option<NaiveDate>)> = (0..=starts.len())
        .map(|i| {
            let first = i
                .checked_sub(1)
                .map(|p| policies.identity_day(starts[p]).date);
            let last = starts.get(i).map(|&s| policies.identity_day(s - 1).date);
            (first, last)
        })
        .collect();
    let mut shared = HashSet::new();
    for (i, x) in ranges.iter().enumerate() {
        for y in &ranges[i + 1..] {
            let first = x.0.max(y.0).expect("a later epoch has a first date");
            if let Some(last) = [x.1, y.1].into_iter().flatten().min() {
                shared.extend(first.iter_days().take_while(|d| *d <= last));
            }
        }
    }
    shared
}

/// Whether a day's rollup cells fit the series buckets: every UTC-hour cell lies in one bucket,
/// an hourly bucket holds one cell of the day (its visitors are that cell's), and a coarser
/// bucket sees a visitor-day in the buckets of its first and last event only.
fn fits(tz: Tz, granularity: Granularity, day: &Day) -> bool {
    let bucket = |ms: i64| trunc(tz, granularity, ms);
    let mut seen = HashSet::new();
    let mut hour = day.start - day.start.rem_euclid(HOUR_MS);
    while hour < day.end {
        let (start, end) = (hour.max(day.start), (hour + HOUR_MS).min(day.end));
        let first = bucket(start);
        if first != bucket(end - 1) || (granularity == Granularity::Hour && !seen.insert(first)) {
            return false;
        }
        hour += HOUR_MS;
    }
    granularity == Granularity::Hour || bucket(day.end - 1) <= step(granularity, bucket(day.start))
}

/// The wall-clock bucket start of an instant, as `date_trunc` computes it on local time.
fn trunc(tz: Tz, granularity: Granularity, ms: i64) -> NaiveDateTime {
    let local = tz
        .timestamp_millis_opt(ms)
        .single()
        .expect("UTC instants map to one local time")
        .naive_local();
    let date = local.date();
    let date = match granularity {
        Granularity::Hour => {
            return date
                .and_hms_opt(local.hour(), 0, 0)
                .expect("start of the hour");
        }
        Granularity::Day => date,
        Granularity::Week => date - Days::new(u64::from(date.weekday().num_days_from_monday())),
        Granularity::Month => date.with_day(1).expect("first of month"),
    };
    date.and_hms_opt(0, 0, 0).expect("midnight")
}

/// The days without a dirty row that have current rollups whose eligible events lie inside
/// their instants, or no rollups at all: such a day has no events, which the count in
/// [`materialize`] confirms. The instants check catches a local day stamped under a timezone
/// the policy copy here does not have (0008: control storage owns settings).
fn current(conn: &Connection, site: u128, days: Vec<Day>) -> duckdb::Result<Vec<Day>> {
    let (Some(first), Some(last)) = (days.first(), days.last()) else {
        return Ok(days);
    };
    let mut statement = conn.prepare(
        "WITH a AS (
             SELECT a.local_day, any_value(a.metric_version) AS version,
                    min(epoch_ms(h.first_time)) AS earliest, max(epoch_ms(h.last_time)) AS latest
             FROM day_aggregates a
             LEFT JOIN hour_aggregates h ON h.site = a.site AND h.local_day = a.local_day
             WHERE a.site = ?1 AND a.local_day BETWEEN ?2 AND ?3
             GROUP BY a.local_day
         ), d AS (
             SELECT DISTINCT local_day FROM dirty_days WHERE site = ?1 AND local_day BETWEEN ?2 AND ?3
         )
         SELECT coalesce(a.local_day, d.local_day), d.local_day IS NULL AND a.version = ?4,
                a.earliest, a.latest
         FROM a FULL JOIN d ON a.local_day = d.local_day",
    )?;
    let known: HashMap<i32, (bool, Option<i64>, Option<i64>)> = statement
        .query_map(
            params![
                site,
                Value::Date32(first.local_day),
                Value::Date32(last.local_day),
                METRIC_VERSION
            ],
            |r| match r.get::<_, Value>(0)? {
                Value::Date32(day) => Ok((
                    day,
                    (
                        r.get::<_, Option<bool>>(1)? == Some(true),
                        r.get(2)?,
                        r.get(3)?,
                    ),
                )),
                other => panic!("unexpected day {other:?}"),
            },
        )?
        .collect::<duckdb::Result<_>>()?;
    Ok(days
        .into_iter()
        .filter(|d| match known.get(&d.local_day) {
            None => true,
            Some((false, _, _)) => false,
            Some((true, Some(earliest), Some(latest))) => d.start <= *earliest && *latest < d.end,
            Some((true, _, _)) => true,
        })
        .collect())
}

/// Counts of materialized events, in the columns of the rollup cells.
const COUNTS: &str = "count(*) FILTER (WHERE kind = 1) AS pageviews, count(*) FILTER (WHERE kind = 2) AS custom_events";

/// Totals cells: the materialized events as one cell, and each served day. `grouped` keeps
/// one cell per slot `g` of the events and carries each day's `g` ([`super::baseline`]).
pub fn total_cells(prefix: &str, grouped: bool) -> String {
    let (g, by, day) = slot(grouped);
    format!(
        "SELECT {g}{COUNTS}, count(*) AS eligible_events,
                count(*) FILTER (WHERE daily_id IS NOT NULL OR profile_id IS NOT NULL) AS identified_events,
                count(DISTINCT daily_id) AS visitors
         FROM {prefix}_ev{by}
         UNION ALL
         SELECT {day}a.pageviews, a.custom_events, a.eligible_events, a.identified_events, a.visitors
         FROM day_aggregates a JOIN {prefix}_days d ON a.site = d.site AND a.local_day = d.local_day
         WHERE a.metric_version = {METRIC_VERSION}"
    )
}

/// Cells of field `f{index}`: per value of the materialized events, and per day and value.
pub fn field_cells(prefix: &str, index: usize, field: DimensionField, grouped: bool) -> String {
    let (name, _) = column(field);
    let (g, _, day) = slot(grouped);
    format!(
        "SELECT {g}f{index}, {COUNTS}, count(DISTINCT daily_id) AS visitors FROM {prefix}_ev GROUP BY ALL
         UNION ALL
         SELECT {day}a.value, a.pageviews, a.custom_events, a.visitors
         FROM dimension_aggregates a JOIN {prefix}_days d ON a.site = d.site AND a.local_day = d.local_day
         WHERE a.field = '{name}'"
    )
}

/// The slot column of grouped cells, the grouping of the events cell, and the served days' slot.
fn slot(grouped: bool) -> (&'static str, &'static str, &'static str) {
    if grouped {
        ("g, ", " GROUP BY g", "d.g, ")
    } else {
        ("", "", "")
    }
}

/// Series cells keyed by `bucket`, given the SQL of an instant in epoch ms: hour cells for
/// counts (and hourly visitors), and each visitor-day in the buckets of its first and last
/// event for coarser visitors. [`fits`] keeps the served days exact for both. `grouped` leads
/// each cell with its slot `g`.
pub fn bucket_cells(
    prefix: &str,
    bucket: impl Fn(&str) -> String,
    granularity: Granularity,
    visitors: bool,
    grouped: bool,
) -> String {
    let hourly = granularity == Granularity::Hour;
    let (g, _, day) = slot(grouped);
    let served = |table: &str| {
        format!("{table} a JOIN {prefix}_days d ON a.site = d.site AND a.local_day = d.local_day")
    };
    let mut parts = vec![
        format!(
            "SELECT {g}{} AS bucket, {COUNTS}, count(DISTINCT daily_id) AS visitors FROM {prefix}_ev GROUP BY ALL",
            bucket("t")
        ),
        format!(
            "SELECT {day}{}, a.pageviews, a.custom_events, {} FROM {}",
            bucket("epoch_ms(a.hour)"),
            if hourly { "a.visitors" } else { "0" },
            served("hour_aggregates")
        ),
    ];
    if visitors && !hourly {
        parts.push(format!(
            "SELECT {g}unnest(CASE WHEN f = l THEN [f] ELSE [f, l] END), 0, 0, 1
             FROM (SELECT {day}{} AS f, {} AS l FROM {})",
            bucket("epoch_ms(a.first_time)"),
            bucket("epoch_ms(a.last_time)"),
            served("visitor_days")
        ));
    }
    parts.join(" UNION ALL ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::explore::resolve::Span;
    use crate::identity::SessionPolicy;

    fn days_of(policies: &SitePolicies, span: &Span) -> Vec<NaiveDate> {
        whole_days(policies, (span.start_ms, span.end_ms))
            .iter()
            .map(|d| crate::identity::date_of_local_day(d.local_day))
            .collect()
    }

    fn ms(text: &str) -> i64 {
        chrono::DateTime::parse_from_rfc3339(text)
            .unwrap()
            .timestamp_millis()
    }

    fn date(text: &str) -> NaiveDate {
        text.parse().unwrap()
    }

    #[test]
    fn whole_days_skip_partial_days_and_days_two_epochs_share() {
        let mut policies =
            SitePolicies::new(SessionPolicy::new(chrono_tz::Europe::Vienna, 30).unwrap());
        // Vienna days inside a UTC range: the first and last are partial.
        let utc = Span {
            first: date("2026-09-01"),
            last: date("2026-09-03"),
            start_ms: ms("2026-09-01T00:00:00Z"),
            end_ms: ms("2026-09-04T00:00:00Z"),
        };
        assert_eq!(
            days_of(&policies, &utc),
            [date("2026-09-02"), date("2026-09-03")]
        );
        // Vienna to UTC at 2026-09-02T23:30Z: Vienna's 2026-09-03 started at 22:00Z, so UTC's
        // 2026-09-02 continues a date both epochs have.
        let change = ms("2026-09-02T23:30:00Z");
        policies
            .change(
                SessionPolicy::new(chrono_tz::UTC, 30).unwrap(),
                change,
                change,
            )
            .unwrap();
        assert_eq!(
            shared_dates(&policies),
            HashSet::from([date("2026-09-02"), date("2026-09-03")])
        );
        assert_eq!(days_of(&policies, &utc), []);
    }

    #[test]
    fn series_cells_fit_whole_hour_offsets_but_not_a_repeated_hour() {
        let vienna = chrono_tz::Europe::Vienna;
        let day = |start: &str, end: &str| Day {
            local_day: 0,
            start: ms(start),
            end: ms(end),
        };
        let september = day("2026-09-01T22:00:00Z", "2026-09-02T22:00:00Z");
        assert!(fits(vienna, Granularity::Hour, &september));
        assert!(fits(chrono_tz::UTC, Granularity::Day, &september));
        assert!(!fits(
            chrono_tz::Asia::Kolkata,
            Granularity::Day,
            &september
        ));
        // 2026-10-25 has 02:00 twice in Vienna.
        let fall = day("2026-10-24T22:00:00Z", "2026-10-25T23:00:00Z");
        assert!(!fits(vienna, Granularity::Hour, &fall));
        assert!(fits(vienna, Granularity::Day, &fall));
    }
}
