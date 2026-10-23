//! The retention report (0003 Retention and cohorts, 0004 Report definitions).
//!
//! Consented persistent profiles only: a profile enters the cohort of the local day, or the
//! Monday-based local week, of its first eligible event since the retention cutoff. Activity
//! is any eligible event in a calendar interval counted from the cohort period. Everything is
//! read from immutable events, so the result needs no derived session data unless the cohort
//! predicate names a session field.

use std::collections::HashMap;

use chrono::{Datelike, Days, NaiveDate};
use holocron_protocol::exploration::{
    CohortRow, Coverage, Granularity, Metric, Reason, RetentionCell, RetentionReport,
    RetentionResult,
};
use holocron_storage::duckdb::{self, params};

use super::{Input, Outcome};
use crate::explore::resolve::local_start;
use crate::explore::run::{self, Plan, ratio4};
use crate::traffic::local_ms_sql;

const DAY_MS: i64 = 86_400_000;

pub fn run(input: &Input, report: &RetentionReport) -> Outcome<RetentionResult> {
    let tz = input.query.timezone;
    let conn = input.snapshot.connection();
    let site = input.ctx.site;
    let week = input.query.state.granularity == Granularity::Week;
    let len: u64 = if week { 7 } else { 1 };
    let intervals = u64::from(report.intervals);

    // Cohort periods the range touches, by their first local date.
    let first = if week {
        monday(input.span.first)
    } else {
        input.span.first
    };
    let cohorts: Vec<NaiveDate> = std::iter::successors(Some(first), |d| {
        Some(*d + Days::new(len)).filter(|next| *next <= input.span.last)
    })
    .collect();
    let at = |date: NaiveDate| local_start(tz, date.and_hms_opt(0, 0, 0).expect("midnight"));
    let last = *cohorts.last().expect("a range has a cohort period");
    let history = input.ctx.cutoff_ms(input.policies);
    let entry = (
        at(first).max(history),
        at(last + Days::new(len)).max(history),
    );
    let activity_end = at(last + Days::new(len * intervals)).max(entry.1);

    // Local day number of an instant column, and its cohort period's first day.
    let local_day = |column: &str| {
        format!(
            "(({}) // {DAY_MS})",
            local_ms_sql(tz, entry.0, activity_end.max(entry.0 + 1), column)
        )
    };
    let period = |day: String| {
        if week {
            format!("({day} - (({day} + 3) % 7))")
        } else {
            day
        }
    };

    // The cohort predicate keeps profiles with a matching event in their cohort period.
    let filtered = match input.query.filter.as_ref() {
        None => false,
        Some(filter) => {
            let fields = input.query.fields();
            let plan = Plan {
                site,
                timezone: tz,
                filter: Some(filter),
                read: vec![false; fields.len()],
                fields,
                sessions: false,
                rollup: false,
                hostnames: input.ctx.hostnames,
                performance: false,
                revenue: false,
                revenue_only: false,
            };
            run::materialize(conn, &plan, "rt", entry)?;
            true
        }
    };
    let matched = if filtered {
        format!(
            "SEMI JOIN (SELECT DISTINCT profile_id, {} AS cohort FROM rt_ev WHERE profile_id IS NOT NULL) x
             ON x.profile_id = f.profile_id AND x.cohort = {}",
            period(local_day("t")),
            period(local_day("f.first"))
        )
    } else {
        String::new()
    };
    conn.execute(
        &format!(
            "CREATE OR REPLACE TEMP TABLE rt_members AS
             WITH f AS (
                 SELECT profile_id, min(epoch_ms(event_time)) AS first FROM events
                 WHERE site = ? AND kind IN (1, 2) AND profile_id IS NOT NULL
                   AND event_time >= epoch_ms(?) AND event_time < epoch_ms(?)
                 GROUP BY profile_id
             )
             SELECT f.profile_id, {} AS cohort FROM f {matched} WHERE f.first >= ?",
            period(local_day("f.first"))
        ),
        params![site, history, entry.1, entry.0],
    )?;

    let sizes: HashMap<i64, i64> = rows(
        conn,
        "SELECT cohort, 0, count(*) FROM rt_members GROUP BY cohort",
        params![],
    )?
    .into_iter()
    .map(|(cohort, _, n)| (cohort, n))
    .collect();
    let event_day = local_day("epoch_ms(e.event_time)");
    let active: HashMap<(i64, i64), i64> = rows(
        conn,
        &format!(
            "SELECT cohort, k, count(*) FROM (
                 SELECT DISTINCT m.cohort, ({event_day} - m.cohort) // {len} AS k, m.profile_id
                 FROM events e JOIN rt_members m ON e.profile_id = m.profile_id
                 WHERE e.site = ? AND e.kind IN (1, 2)
                   AND e.event_time >= epoch_ms(?) AND e.event_time < epoch_ms(?))
             WHERE k >= 0 AND k < {intervals} GROUP BY cohort, k"
        ),
        params![site, entry.0, activity_end],
    )?
    .into_iter()
    .map(|(cohort, k, n)| ((cohort, k), n))
    .collect();
    // Consented eligible events out of all eligible events of each cohort period.
    let coverage: HashMap<i64, (i64, i64)> = rows(
        conn,
        &format!(
            "SELECT {} AS cohort, count(e.profile_id), count(*) FROM events e
             WHERE e.site = ? AND e.kind IN (1, 2)
               AND e.event_time >= epoch_ms(?) AND e.event_time < epoch_ms(?)
             GROUP BY ALL",
            period(event_day.clone())
        ),
        params![site, entry.0, entry.1],
    )?
    .into_iter()
    .map(|(cohort, consented, all)| (cohort, (consented, all)))
    .collect();
    conn.execute_batch("DROP TABLE rt_members")?;
    if filtered {
        run::drop_tables(conn, "rt")?;
    }

    let now = input.ctx.now_ms;
    let mut pooled = vec![(0_i64, 0_i64); usize::from(report.intervals)];
    let mut total = Coverage {
        with_identity: 0,
        eligible_events: 0,
    };
    let rows = cohorts
        .iter()
        .map(|date| {
            let day = date.signed_duration_since(epoch()).num_days();
            let size = sizes.get(&day).copied().unwrap_or(0);
            let (consented, eligible) = coverage.get(&day).copied().unwrap_or((0, 0));
            total.with_identity += consented;
            total.eligible_events += eligible;
            let cells = (0..intervals)
                .map(|k| {
                    let start = at(*date + Days::new(k * len));
                    let end = at(*date + Days::new((k + 1) * len));
                    let active = active.get(&(day, k as i64)).copied().unwrap_or(0);
                    let complete = end <= now;
                    if complete {
                        pooled[k as usize].0 += active;
                        pooled[k as usize].1 += size;
                    }
                    RetentionCell {
                        start,
                        end,
                        active,
                        rate: rate(active, size),
                        complete,
                    }
                })
                .collect();
            CohortRow {
                start: at(*date),
                local: date.format("%Y-%m-%d").to_string(),
                size,
                cells,
                coverage: Coverage {
                    with_identity: consented,
                    eligible_events: eligible,
                },
            }
        })
        .collect();
    Ok(Ok(RetentionResult {
        cohorts: rows,
        averages: pooled
            .into_iter()
            .map(|(active, size)| rate(active, size))
            .collect(),
        coverage: total,
        watermark: now,
        history_start: input.ctx.retention.months().map(|_| history),
    }))
}

/// `active / size`, unavailable without a denominator (0003 Comparison and zero baselines).
fn rate(active: i64, size: i64) -> Metric {
    match size {
        0 => Metric::Unavailable {
            reason: Reason::NoDenominator,
        },
        _ => Metric::Ratio {
            v: ratio4(i128::from(active), i128::from(size)),
            numerator: active,
            denominator: size,
        },
    }
}

fn monday(date: NaiveDate) -> NaiveDate {
    date - Days::new(u64::from(date.weekday().num_days_from_monday()))
}

fn epoch() -> NaiveDate {
    NaiveDate::from_ymd_opt(1970, 1, 1).expect("epoch")
}

/// Rows of three integers.
fn rows(
    conn: &duckdb::Connection,
    sql: &str,
    params: impl duckdb::Params,
) -> duckdb::Result<Vec<(i64, i64, i64)>> {
    let mut statement = conn.prepare(sql)?;
    let rows = statement.query_map(params, |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
    rows.collect()
}
