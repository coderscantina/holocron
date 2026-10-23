//! Resolves a state's range once per request (0004 Semantics): local dates in the state's
//! timezone become half-open instants, with the previous period of equal local length and the
//! series buckets in wall-clock time. `now` is always the application clock's, passed in with
//! the request.

use chrono::{Datelike, Days, Months, NaiveDate, NaiveDateTime, TimeZone};
use chrono_tz::Tz;
use holocron_protocol::exploration::{Granularity, Range, RangeUnit};
use holocron_protocol::limits::{EXPLORATION_SERIES_POINTS_MAX, TYPICAL_WEEKDAY_WEEKS};

use super::validate::Refusal;

/// How far back a range may reach (0016).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct History {
    /// First local date inside retention. `None` when the site keeps every event.
    pub cutoff: Option<NaiveDate>,
    /// Local date of the site's first stored event, read only for `all`.
    pub first: Option<NaiveDate>,
}

impl History {
    /// The first local date on or after the retention cutoff instant.
    pub fn cutoff_date(tz: Tz, cutoff_ms: i64) -> NaiveDate {
        let date = local(tz, cutoff_ms).date();
        let midnight = local_start(tz, date.and_hms_opt(0, 0, 0).expect("midnight"));
        if midnight >= cutoff_ms {
            date
        } else {
            date + Days::new(1)
        }
    }
}

/// Earliest local date any range may cover: no stored event precedes the Unix epoch, and
/// `days` ranges already start there at the earliest.
const EARLIEST: NaiveDate = NaiveDate::from_ymd_opt(1970, 1, 1).expect("valid date");

/// Inclusive local dates and their instants `[start_ms, end_ms)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub first: NaiveDate,
    pub last: NaiveDate,
    pub start_ms: i64,
    pub end_ms: i64,
}

impl Span {
    fn new(tz: Tz, first: NaiveDate, last: NaiveDate) -> Self {
        Self {
            first,
            last,
            start_ms: local_start(tz, first.and_hms_opt(0, 0, 0).expect("midnight")),
            end_ms: local_start(
                tz,
                (last + Days::new(1))
                    .and_hms_opt(0, 0, 0)
                    .expect("midnight"),
            ),
        }
    }

    pub fn days(&self) -> u64 {
        (self.last - self.first).num_days() as u64 + 1
    }
}

/// The current range and, when compared, the local days of equal count before it.
pub fn resolve(
    range: &Range,
    tz: Tz,
    now_ms: i64,
    history: History,
) -> Result<(Span, Span), Refusal> {
    let today = local(tz, now_ms).date();
    let (first, last) = match range {
        Range::All => {
            let first = history.first.into_iter().chain(history.cutoff).max();
            (first.unwrap_or(today).min(today), today)
        }
        Range::Relative { count: 0, .. } => {
            return Err(Refusal::invalid("range", "count must be at least 1"));
        }
        Range::Relative { unit, count } => {
            let back = u64::from(*count) - 1;
            match unit {
                RangeUnit::Day => (today - Days::new(back), today),
                RangeUnit::Week => {
                    let monday =
                        today - Days::new(u64::from(today.weekday().num_days_from_monday()));
                    (monday - Days::new(7 * back), monday + Days::new(6))
                }
                RangeUnit::Month => {
                    let month = today.with_day(1).expect("first of month");
                    let first = month
                        .checked_sub_months(Months::new(back as u32))
                        .ok_or_else(|| Refusal::invalid("range", "range out of bounds"))?;
                    (first, month + Months::new(1) - Days::new(1))
                }
            }
        }
        Range::Days { start, end } => {
            let parse = |text: &str, field: &str| {
                NaiveDate::parse_from_str(text, "%Y-%m-%d")
                    .map_err(|_| Refusal::invalid(format!("range.{field}"), "dates are YYYY-MM-DD"))
                    .and_then(|date| {
                        // Bounded so date arithmetic around local midnight cannot overflow.
                        if (1970..=9000).contains(&date.year()) {
                            Ok(date)
                        } else {
                            Err(Refusal::invalid(
                                format!("range.{field}"),
                                "the date is out of range",
                            ))
                        }
                    })
            };
            let (first, last) = (parse(start, "start")?, parse(end, "end")?);
            if last < first {
                return Err(Refusal::invalid(
                    "range.end",
                    "the range ends before it starts",
                ));
            }
            (first, last)
        }
    };
    if let Range::Relative { .. } = range {
        let earliest = history.cutoff.unwrap_or(EARLIEST);
        if first < earliest {
            return Err(Refusal {
                limit: Some((today - earliest).num_days() as u32 + 1),
                reason: holocron_protocol::errors::FieldErrorCode::TooMany,
                ..Refusal::invalid(
                    "range",
                    "a relative range reaches back at most to the site's retention",
                )
            });
        }
    }
    let current = Span::new(tz, first, last);
    let previous = Span::new(tz, first - Days::new(current.days()), first - Days::new(1));
    Ok((current, previous))
}

/// The baseline days of a typical-weekday comparison (0015), one list per local day `d` of
/// `current`: the days `d − 7k` for `k = 1..8`, most recent first. A day is kept when it starts
/// at or after the retention `cutoff`, ends after `first_event`, the site's first stored event,
/// and ends by `until`, the start of today: today and later days are never baseline days. A
/// site without a stored event has no baseline days.
pub fn baseline_days(
    tz: Tz,
    current: &Span,
    cutoff: i64,
    first_event: Option<i64>,
    until: i64,
) -> Vec<Vec<Span>> {
    current
        .first
        .iter_days()
        .take(current.days() as usize)
        .map(|day| {
            (1..=TYPICAL_WEEKDAY_WEEKS)
                .map(|k| {
                    let date = day - Days::new(7 * u64::from(k));
                    Span::new(tz, date, date)
                })
                .filter(|span| {
                    span.start_ms >= cutoff
                        && span.end_ms <= until
                        && first_event.is_some_and(|first| span.end_ms > first)
                })
                .collect()
        })
        .collect()
}

/// The distinct local dates of baseline days, ascending, as `YYYY-MM-DD`.
pub fn compare_days(baselines: &[Vec<Span>]) -> Vec<String> {
    let dates: std::collections::BTreeSet<NaiveDate> =
        baselines.iter().flatten().map(|span| span.first).collect();
    dates
        .iter()
        .map(|d| d.format("%Y-%m-%d").to_string())
        .collect()
}

/// The local wall time of an instant.
pub fn local(tz: Tz, ms: i64) -> NaiveDateTime {
    tz.timestamp_millis_opt(ms)
        .single()
        .expect("instant in range")
        .naive_local()
}

/// Where totals of `previous` stop when `current` contains now (0003 Comparison): the same
/// local wall-clock offset into `previous` as now is into `current`, so today until 15:20
/// compares with yesterday until 15:20. `None` when now lies outside `current`.
pub fn same_offset(tz: Tz, current: &Span, previous: &Span, now_ms: i64) -> Option<i64> {
    if !(current.start_ms..current.end_ms).contains(&now_ms) {
        return None;
    }
    let midnight = |date: NaiveDate| date.and_hms_opt(0, 0, 0).expect("midnight");
    let offset = local(tz, now_ms) - midnight(current.first);
    Some(local_start(tz, midnight(previous.first) + offset).min(previous.end_ms))
}

/// The first instant of a local wall time; a time skipped by a DST gap moves to the gap's end.
pub fn local_start(tz: Tz, local: NaiveDateTime) -> i64 {
    let mut probe = local;
    loop {
        if let Some(instant) = tz.from_local_datetime(&probe).earliest() {
            return instant.timestamp_millis();
        }
        probe += chrono::Duration::minutes(15);
    }
}

/// Wall-clock bucket starts of a span: every local hour, day, ISO week or month that touches
/// it. A skipped hour has no bucket; a repeated hour is one bucket.
pub fn buckets(
    tz: Tz,
    span: &Span,
    granularity: Granularity,
) -> Result<Vec<(NaiveDateTime, i64)>, Refusal> {
    let starts = starts_up_to(tz, span, granularity, EXPLORATION_SERIES_POINTS_MAX + 1);
    if starts.len() > EXPLORATION_SERIES_POINTS_MAX {
        return Err(Refusal {
            limit: Some(EXPLORATION_SERIES_POINTS_MAX as u32),
            reason: holocron_protocol::errors::FieldErrorCode::TooMany,
            ..Refusal::invalid(
                "granularity",
                "too many points; choose a coarser granularity",
            )
        });
    }
    Ok(starts)
}

/// [`buckets`] without the series point limit, for spans that passed it or are one day long.
pub fn bucket_starts(tz: Tz, span: &Span, granularity: Granularity) -> Vec<(NaiveDateTime, i64)> {
    starts_up_to(tz, span, granularity, usize::MAX)
}

fn starts_up_to(
    tz: Tz,
    span: &Span,
    granularity: Granularity,
    max: usize,
) -> Vec<(NaiveDateTime, i64)> {
    let midnight = |date: NaiveDate| date.and_hms_opt(0, 0, 0).expect("midnight");
    let end = midnight(span.last + Days::new(1));
    let mut local = match granularity {
        Granularity::Hour | Granularity::Day => midnight(span.first),
        Granularity::Week => {
            midnight(span.first - Days::new(u64::from(span.first.weekday().num_days_from_monday())))
        }
        Granularity::Month => midnight(span.first.with_day(1).expect("first of month")),
    };
    let mut out = Vec::new();
    while local < end && out.len() < max {
        if tz.from_local_datetime(&local).earliest().is_some() || granularity != Granularity::Hour {
            out.push((local, local_start(tz, local)));
        }
        local = step(granularity, local);
    }
    out
}

/// The next bucket start after `local`.
pub fn step(granularity: Granularity, local: NaiveDateTime) -> NaiveDateTime {
    match granularity {
        Granularity::Hour => local + chrono::Duration::hours(1),
        Granularity::Day => local + Days::new(1),
        Granularity::Week => local + Days::new(7),
        Granularity::Month => local + Months::new(1),
    }
}

/// The DuckDB `date_trunc` part for a granularity.
pub fn trunc(granularity: Granularity) -> &'static str {
    match granularity {
        Granularity::Hour => "hour",
        Granularity::Day => "day",
        Granularity::Week => "week",
        Granularity::Month => "month",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(text: &str) -> i64 {
        chrono::DateTime::parse_from_rfc3339(text)
            .unwrap()
            .timestamp_millis()
    }

    #[test]
    fn last_30_days_resolve_in_the_state_timezone_with_the_previous_period() {
        let range = Range::Relative {
            unit: RangeUnit::Day,
            count: 30,
        };
        let (current, previous) = resolve(
            &range,
            chrono_tz::Europe::Vienna,
            ms("2026-10-03T10:00:00Z"),
            History::default(),
        )
        .unwrap();
        assert_eq!(
            (current.start_ms, current.end_ms),
            (ms("2026-09-03T22:00:00Z"), ms("2026-10-03T22:00:00Z"))
        );
        assert_eq!(
            (previous.start_ms, previous.end_ms),
            (ms("2026-08-04T22:00:00Z"), ms("2026-09-03T22:00:00Z"))
        );
    }

    #[test]
    fn hourly_buckets_follow_the_wall_clock_across_dst() {
        let tz = chrono_tz::Europe::Vienna;
        let day = |d: &str| NaiveDate::parse_from_str(d, "%Y-%m-%d").unwrap();
        let spring = Span::new(tz, day("2026-03-29"), day("2026-03-29"));
        let fall = Span::new(tz, day("2025-10-26"), day("2025-10-26"));
        assert_eq!(buckets(tz, &spring, Granularity::Hour).unwrap().len(), 23);
        assert_eq!(buckets(tz, &fall, Granularity::Hour).unwrap().len(), 24);
        let weeks = buckets(
            tz,
            &Span::new(tz, day("2026-09-03"), day("2026-10-03")),
            Granularity::Week,
        )
        .unwrap();
        assert_eq!(weeks[0].0.date(), day("2026-08-31"));
        assert_eq!(weeks.len(), 5);
    }

    fn day(text: &str) -> NaiveDate {
        NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn typical_weekday_baselines_are_clipped_at_retention_and_the_first_event() {
        let tz = chrono_tz::Europe::Vienna;
        let tuesday = Span::new(tz, day("2026-10-13"), day("2026-10-13"));
        let dates =
            |cutoff, first| compare_days(&baseline_days(tz, &tuesday, cutoff, first, i64::MAX));
        let all = dates(0, Some(0));
        assert_eq!(
            all,
            [
                "2026-08-18",
                "2026-08-25",
                "2026-09-01",
                "2026-09-08",
                "2026-09-15",
                "2026-09-22",
                "2026-09-29",
                "2026-10-06"
            ]
        );
        // A day starting exactly at the cutoff is kept whole; the ones before it are gone.
        let cutoff = ms("2026-08-31T22:00:00Z");
        assert_eq!(dates(cutoff, Some(0)), all[2..]);
        assert_eq!(dates(cutoff + 1, Some(0)), all[3..]);
        // The day of the first stored event counts; the days before it do not.
        assert_eq!(dates(0, Some(ms("2026-09-20T12:00:00Z"))), all[5..]);
        assert!(dates(0, None).is_empty());
        // A day that has not ended by the start of today is no baseline day.
        let until = |until| compare_days(&baseline_days(tz, &tuesday, 0, Some(0), until));
        assert_eq!(until(ms("2026-10-06T22:00:00Z")), all);
        assert_eq!(until(ms("2026-10-05T22:00:00Z")), all[..7]);

        // Each day of a range gets its own weekday; the echo lists every date once.
        let two = Span::new(tz, day("2026-10-13"), day("2026-10-14"));
        let baselines = baseline_days(tz, &two, ms("2026-09-29T22:00:00Z"), Some(0), i64::MAX);
        let firsts = |i: usize| baselines[i].iter().map(|s| s.first).collect::<Vec<_>>();
        assert_eq!(firsts(0), [day("2026-10-06")]);
        assert_eq!(firsts(1), [day("2026-10-07"), day("2026-09-30")]);
        assert_eq!(
            compare_days(&baselines),
            ["2026-09-30", "2026-10-06", "2026-10-07"]
        );
    }

    #[test]
    fn typical_weekday_baseline_days_keep_their_local_length_across_dst() {
        let tz = chrono_tz::Europe::Vienna;
        let span = |d| Span::new(tz, day(d), day(d));
        // The Sunday after spring forward reads the 23-hour 2026-03-29.
        let spring = &baseline_days(tz, &span("2026-04-05"), 0, Some(0), i64::MAX)[0][0];
        assert_eq!(
            (spring.first, spring.start_ms, spring.end_ms),
            (
                day("2026-03-29"),
                ms("2026-03-28T23:00:00Z"),
                ms("2026-03-29T22:00:00Z")
            )
        );
        // The Sunday after fall back reads the 25-hour 2025-10-26, then a CEST Sunday.
        let fall = &baseline_days(tz, &span("2025-11-02"), 0, Some(0), i64::MAX)[0];
        assert_eq!(
            (fall[0].start_ms, fall[0].end_ms),
            (ms("2025-10-25T22:00:00Z"), ms("2025-10-26T23:00:00Z"))
        );
        assert_eq!(
            (fall[1].first, fall[1].end_ms - fall[1].start_ms),
            (day("2025-10-19"), 24 * 3_600_000)
        );
    }

    #[test]
    fn relative_ranges_reach_back_to_the_retention_and_series_over_744_points_are_refused() {
        let tz = chrono_tz::UTC;
        let day = |d: &str| NaiveDate::parse_from_str(d, "%Y-%m-%d").unwrap();
        let now = ms("2026-10-03T10:00:00Z");
        let one_year = History {
            cutoff: Some(day("2025-10-03")),
            first: Some(day("2025-08-14")),
        };
        let weeks = |count| Range::Relative {
            unit: RangeUnit::Week,
            count,
        };
        assert_eq!(
            resolve(&weeks(53), tz, now, one_year).unwrap_err().field,
            "range"
        );
        assert!(resolve(&weeks(52), tz, now, one_year).is_ok());
        let three_years = Range::Relative {
            unit: RangeUnit::Month,
            count: 36,
        };
        assert!(resolve(&three_years, tz, now, History::default()).is_ok());
        // `all` starts at the later of the cutoff and the first stored event.
        let (all, _) = resolve(&Range::All, tz, now, one_year).unwrap();
        assert_eq!(
            (all.first, all.last),
            (day("2025-10-03"), day("2026-10-03"))
        );
        let forever = History {
            cutoff: None,
            ..one_year
        };
        let (all, _) = resolve(&Range::All, tz, now, forever).unwrap();
        assert_eq!(all.first, day("2025-08-14"));
        let (month, _) = resolve(
            &Range::Days {
                start: "2026-01-01".into(),
                end: "2026-02-01".into(),
            },
            tz,
            0,
            History::default(),
        )
        .unwrap();
        assert_eq!(
            buckets(tz, &month, Granularity::Hour).unwrap_err().field,
            "granularity"
        );
    }
}
