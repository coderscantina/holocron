//! Traffic summary over derived sessions (0003 Formulas): pageviews, events,
//! sessions, visitors or visitor-days with a day series, identified people,
//! bounce, observed span and identity coverage, with the result's freshness.
//!
//! This is the exact reference read for the session-based measures and the
//! correctness fixture. Milestone 06's planner owns grouped and filtered
//! explorations; `day_aggregates` is its identity-day rollup input.

use std::collections::BTreeMap;

use chrono::{DateTime, NaiveDate, Offset, TimeZone};
use chrono_tz::Tz;
use holocron_storage::duckdb::{self, params};
use holocron_storage::{Id, Snapshot};

use crate::freshness::{Freshness, freshness};
use crate::identity::{SessionPolicy, SitePolicies, date_of_local_day};
use crate::jobs::site_policies;

const DAY_MS: i64 = 86_400_000;

/// Resolved half-open bounds `[start_ms, end_ms)`, already clamped to the retention cutoff.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Range {
    pub start_ms: i64,
    pub end_ms: i64,
}

/// "visitors" when the range lies inside one identity day, else "visitor-days"
/// (0003 Visitors versus visitor-days). The number is the same distinct count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisitorLabel {
    Visitors,
    VisitorDays,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Traffic {
    pub pageviews: u64,
    pub events: u64,
    pub events_by_name: BTreeMap<String, u64>,
    pub sessions: u64,
    pub visitor_label: VisitorLabel,
    pub visitors: u64,
    /// Distinct daily pseudonyms per day of the exploration timezone.
    pub visitor_series: BTreeMap<NaiveDate, u64>,
    pub series_sum_differs: bool,
    pub identified_people: u64,
    /// Eligible events linked to a profile, the identified people coverage numerator.
    pub linked_events: u64,
    pub eligible_events: u64,
    /// Eligible events with a daily pseudonym or profile, the identity coverage numerator.
    pub identified_events: u64,
    pub bounces: u64,
    /// Sessions with at least one pageview: the bounce and span denominator.
    pub pageview_sessions: u64,
    pub span_sum_ms: i64,
    pub span_median_ms: Option<i64>,
    pub freshness: Freshness,
}

impl Traffic {
    /// Mean observed span rounded half to even to a whole millisecond.
    pub fn span_mean_ms(&self) -> Option<i64> {
        (self.pageview_sessions > 0)
            .then(|| div_half_even(self.span_sum_ms, self.pageview_sessions as i64))
    }
}

pub fn traffic(
    snapshot: &Snapshot,
    site: Id,
    range: Range,
    timezone: Tz,
) -> duckdb::Result<Traffic> {
    let conn = snapshot.connection();
    let Range { start_ms, end_ms } = range;
    let bounds = params![site, start_ms, end_ms];
    let in_range = "site = ? AND event_time >= epoch_ms(?) AND event_time < epoch_ms(?)";

    let (pageviews, events, eligible_events, identified_events, linked_events, visitors, identified_people) =
        conn.query_row(
            &format!(
                "SELECT count(*) FILTER (WHERE kind = 1), count(*) FILTER (WHERE kind = 2),
                        count(*) FILTER (WHERE kind IN (1, 2)),
                        count(*) FILTER (WHERE kind IN (1, 2) AND (daily_id IS NOT NULL OR profile_id IS NOT NULL)),
                        count(*) FILTER (WHERE kind IN (1, 2) AND profile_id IS NOT NULL),
                        count(DISTINCT daily_id) FILTER (WHERE kind IN (1, 2)),
                        count(DISTINCT profile_id) FILTER (WHERE kind IN (1, 2))
                 FROM events WHERE {in_range}"
            ),
            bounds,
            |r| {
                Ok((
                    r.get::<_, i64>(0)? as u64,
                    r.get::<_, i64>(1)? as u64,
                    r.get::<_, i64>(2)? as u64,
                    r.get::<_, i64>(3)? as u64,
                    r.get::<_, i64>(4)? as u64,
                    r.get::<_, i64>(5)? as u64,
                    r.get::<_, i64>(6)? as u64,
                ))
            },
        )?;

    let mut statement = conn.prepare(&format!(
        "SELECT event_name, count(*) FROM events WHERE {in_range} AND kind = 2 AND event_name IS NOT NULL GROUP BY 1"
    ))?;
    let events_by_name = statement
        .query_map(bounds, |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as u64))
        })?
        .collect::<duckdb::Result<_>>()?;

    let session_range = "site = ? AND start_time >= epoch_ms(?) AND start_time < epoch_ms(?)";
    let (sessions, pageview_sessions, bounces, span_sum_ms) = conn.query_row(
        &format!(
            "SELECT count(*), count(*) FILTER (WHERE pageviews > 0),
                    count(*) FILTER (WHERE pageviews = 1 AND custom_events = 0),
                    coalesce(sum(epoch_ms(last_time) - epoch_ms(start_time)) FILTER (WHERE pageviews > 0), 0)
             FROM sessions WHERE {session_range}"
        ),
        bounds,
        |r| {
            Ok((
                r.get::<_, i64>(0)? as u64,
                r.get::<_, i64>(1)? as u64,
                r.get::<_, i64>(2)? as u64,
                r.get::<_, i64>(3)?,
            ))
        },
    )?;
    // Nearest rank: the ceil(n / 2)-th smallest span (0003 Percentiles).
    let span_median_ms = conn
        .query_row(
            &format!(
                "SELECT span FROM (
                    SELECT epoch_ms(last_time) - epoch_ms(start_time) AS span,
                           row_number() OVER (ORDER BY epoch_ms(last_time) - epoch_ms(start_time)) AS rank,
                           count(*) OVER () AS n
                    FROM sessions WHERE {session_range} AND pageviews > 0)
                 WHERE rank = (n + 1) // 2"
            ),
            bounds,
            |r| r.get::<_, i64>(0),
        )
        .map(Some)
        .or_else(|e| match e {
            duckdb::Error::QueryReturnedNoRows => Ok(None),
            e => Err(e),
        })?;

    let local = local_ms_sql(timezone, start_ms, end_ms, "epoch_ms(event_time)");
    let mut statement = conn.prepare(&format!(
        "SELECT ({local}) // {DAY_MS} AS day, count(DISTINCT daily_id)
         FROM events WHERE {in_range} AND kind IN (1, 2) AND daily_id IS NOT NULL GROUP BY 1"
    ))?;
    let visitor_series: BTreeMap<NaiveDate, u64> = statement
        .query_map(bounds, |r| {
            Ok((
                date_of_local_day(r.get::<_, i64>(0)? as i32),
                r.get::<_, i64>(1)? as u64,
            ))
        })?
        .collect::<duckdb::Result<_>>()?;
    let series_sum_differs = visitor_series.values().sum::<u64>() > visitors;

    let policies = site_policies(conn, site)?.unwrap_or_else(|| {
        SitePolicies::new(SessionPolicy::new(chrono_tz::UTC, 30).expect("default policy"))
    });
    let visitor_label = if policies.identity_day(start_ms) == policies.identity_day(end_ms - 1) {
        VisitorLabel::Visitors
    } else {
        VisitorLabel::VisitorDays
    };

    Ok(Traffic {
        pageviews,
        events,
        events_by_name,
        sessions,
        visitor_label,
        visitors,
        visitor_series,
        series_sum_differs,
        identified_people,
        linked_events,
        eligible_events,
        identified_events,
        bounces,
        pageview_sessions,
        span_sum_ms,
        span_median_ms,
        freshness: freshness(snapshot, site, start_ms, end_ms)?,
    })
}

/// SQL for `column` (UTC epoch ms) shifted to the wall clock of `tz`. Offsets
/// are piecewise constant, so this is a CASE over the transitions in range.
pub(crate) fn local_ms_sql(tz: Tz, start_ms: i64, end_ms: i64, column: &str) -> String {
    let offset = |ms: i64| {
        let utc = DateTime::from_timestamp_millis(ms)
            .expect("instant in range")
            .naive_utc();
        i64::from(tz.offset_from_utc_datetime(&utc).fix().local_minus_utc()) * 1000
    };
    let mut segments = vec![(start_ms, offset(start_ms))];
    let mut t = start_ms;
    while t < end_ms {
        let next = (t + 3_600_000).min(end_ms);
        let current = segments.last().expect("one segment").1;
        if offset(next) != current {
            // First millisecond of the new offset within (t, next].
            let (mut lo, mut hi) = (t, next);
            while hi - lo > 1 {
                let mid = lo + (hi - lo) / 2;
                if offset(mid) == current {
                    lo = mid
                } else {
                    hi = mid
                }
            }
            segments.push((hi, offset(hi)));
        }
        t = next;
    }
    if let [(_, only)] = segments[..] {
        return format!("{column} + {only}");
    }
    let mut sql = format!("{column} + CASE");
    for pair in segments.windows(2) {
        sql += &format!(" WHEN {column} < {} THEN {}", pair[1].0, pair[0].1);
    }
    sql += &format!(" ELSE {} END", segments.last().expect("one segment").1);
    sql
}

fn div_half_even(numerator: i64, denominator: i64) -> i64 {
    let (q, r) = (
        numerator.div_euclid(denominator),
        numerator.rem_euclid(denominator),
    );
    match (2 * r).cmp(&denominator) {
        std::cmp::Ordering::Less => q,
        std::cmp::Ordering::Greater => q + 1,
        std::cmp::Ordering::Equal => q + (q & 1),
    }
}

#[cfg(test)]
mod tests {
    use super::div_half_even;

    #[test]
    fn mean_rounds_half_to_even() {
        assert_eq!(div_half_even(13_304_998, 60), 221_750);
        assert_eq!(div_half_even(1_815_999, 4), 454_000);
        assert_eq!(div_half_even(5, 2), 2);
        assert_eq!(div_half_even(7, 2), 4);
    }
}
