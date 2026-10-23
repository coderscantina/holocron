//! Typical-weekday baselines (0015). Each local day `d` of the range compares with its baseline
//! days `d − 7k` ([`resolve::baseline_days`]). [`Windows`] plans what they read. [`Baseline`]
//! materializes the baseline days once, from the rollups when the request reads them, and
//! aggregates every window with one grouped totals, rows or bucket query, the same queries as
//! the range itself, in the same snapshot.
//!
//! Counts take the nearest-rank median over a day's baseline days, per series bucket, and add
//! up the days of a coarser bucket or of the totals. Rates and spans pool every baseline day
//! into one population. A total, row or bucket with a day of fewer than
//! [`TYPICAL_WEEKDAY_DAYS_MIN`] baseline days has no baseline (`insufficient_history`).
//!
//! When the range contains now, totals and rows read today's baseline days only up to now's
//! local time and skip the days after today. Series buckets always read whole baseline days.
//!
//! A range containing now also projects each count to its end (0015 Pace projection, [`pace`]).
//! Basis day `k` is the range shifted back `k` weeks: today's baseline day `k` up to now gives
//! the scale, and what the baseline days `k` of today and the later range days add after now
//! is scaled onto the observed value. Whole minus up to now is what a day adds after now, also
//! for distinct counts, so a projection at scale 1 ends at the basis day's own total.

use std::collections::{BTreeMap, HashMap};

use chrono::{NaiveDate, Timelike};
use chrono_tz::Tz;
use holocron_protocol::exploration::{
    Cell, Field, Granularity, Measure, Metric, NamedMeasure, ProjectedPoint, Projection, Reason,
    Series,
};
use holocron_protocol::limits::{PROJECTION_SCALING_FLOOR, TYPICAL_WEEKDAY_DAYS_MIN};
use holocron_storage::duckdb::types::Value;
use holocron_storage::duckdb::{self, Connection};

use super::resolve::{self, Span};
use super::rollup;
use super::run::{self, Aggs, Compared, Page, Plan};
use super::sql::columns;
use super::validate::CutSpec;
use crate::identity::SitePolicies;
use crate::traffic::local_ms_sql;

/// One window a typical-weekday comparison reads.
#[derive(Debug, Clone, Copy)]
pub struct Window {
    /// The baseline day's date and the instants read.
    pub span: Span,
    /// A whole baseline day; else one up to now's local time.
    pub whole: bool,
}

/// One local day of the range and the windows it reads, by index, most recent baseline day first.
#[derive(Debug)]
pub struct Day {
    pub date: NaiveDate,
    /// Its whole baseline days.
    pub whole: Vec<usize>,
    /// What its totals and rows read: the whole baseline days, today's up to now's local time,
    /// nothing after today.
    pub totals: Option<Vec<usize>>,
}

/// What a typical-weekday comparison reads, planned without touching the store: [`Baseline`]
/// materializes the windows for explorations, the goals report counts them itself.
#[derive(Debug)]
pub struct Windows {
    pub windows: Vec<Window>,
    pub days: Vec<Day>,
    /// Today's index in `days` when the range contains now.
    pub today: Option<usize>,
}

impl Windows {
    /// Plans the windows of `current` from its baseline days, one list per local day as
    /// [`resolve::baseline_days`] returns them. A whole baseline day shared by two range days is
    /// one window.
    pub fn plan(tz: Tz, current: &Span, baselines: &[Vec<Span>], now_ms: i64) -> Self {
        let now = resolve::local(tz, now_ms);
        let contains_now = (current.start_ms..current.end_ms).contains(&now_ms);
        let mut windows = Vec::new();
        let mut whole_days: BTreeMap<NaiveDate, usize> = BTreeMap::new();
        let mut days = Vec::new();
        for (date, spans) in current.first.iter_days().zip(baselines) {
            let whole = spans
                .iter()
                .map(|span| {
                    *whole_days.entry(span.first).or_insert_with(|| {
                        windows.push(Window {
                            span: *span,
                            whole: true,
                        });
                        windows.len() - 1
                    })
                })
                .collect::<Vec<_>>();
            let totals = if !contains_now || date < now.date() {
                Some(whole.clone())
            } else if date == now.date() {
                let until = spans.iter().map(|span| {
                    let end = resolve::local_start(tz, span.first.and_time(now.time()))
                        .clamp(span.start_ms, span.end_ms);
                    windows.push(Window {
                        span: Span {
                            end_ms: end,
                            ..*span
                        },
                        whole: false,
                    });
                    windows.len() - 1
                });
                Some(until.collect())
            } else {
                None
            };
            days.push(Day {
                date,
                whole,
                totals,
            });
        }
        let today = contains_now
            .then(|| days.iter().position(|d| d.date == now.date()))
            .flatten();
        Self {
            windows,
            days,
            today,
        }
    }

    /// The window lists of the days the totals and rows read.
    pub fn counted(&self) -> impl Iterator<Item = &Vec<usize>> {
        self.days.iter().filter_map(|d| d.totals.as_ref())
    }

    /// The typical value of a count over the counted days: the sum of each day's nearest-rank
    /// median, `None` when a day has fewer than the minimum of baseline days.
    pub fn median_sum(&self, value: impl Fn(usize) -> i64) -> Option<i64> {
        self.counted()
            .map(|parts| {
                (parts.len() >= TYPICAL_WEEKDAY_DAYS_MIN as usize)
                    .then(|| median(parts.iter().map(|&p| value(p)).collect()))
            })
            .sum()
    }

    /// The range days from today on, when the range contains now.
    pub fn remaining(&self) -> Vec<NaiveDate> {
        self.today
            .map(|t| self.days[t..].iter().map(|d| d.date).collect())
            .unwrap_or_default()
    }

    /// The basis days of a projection over `dates` (today or later range days) as week back
    /// `k`: today's window `k` up to now, and the whole baseline day `k` of each date. A week
    /// missing any of them is no basis day.
    fn basis(&self, dates: &[NaiveDate]) -> Vec<(usize, Vec<usize>)> {
        let Some(today) = self.today.map(|t| &self.days[t]) else {
            return Vec::new();
        };
        let first = self.days[0].date;
        let week = |day: &Day, part: usize| (day.date - self.windows[part].span.first).num_days();
        today
            .totals
            .iter()
            .flatten()
            .filter_map(|&until| {
                let k = week(today, until);
                let whole = dates
                    .iter()
                    .map(|date| {
                        let day = &self.days[(*date - first).num_days() as usize];
                        day.whole.iter().copied().find(|&p| week(day, p) == k)
                    })
                    .collect::<Option<Vec<_>>>()?;
                Some((until, whole))
            })
            .collect()
    }

    /// 0015 Pace projection of one count over `dates`: `observed` is what it measured so far
    /// (the range's total or one bucket's value) and `today` what today measured so far. Per
    /// basis day, `until` is a window's count up to now, which sets the scale, and `within` a
    /// window's count inside what is projected.
    pub fn pace(
        &self,
        dates: &[NaiveDate],
        (observed, today): (i64, i64),
        until: impl Fn(usize) -> i64,
        within: impl Fn(usize) -> i64,
    ) -> Projection {
        let has_today = self
            .today
            .is_some_and(|t| dates.contains(&self.days[t].date));
        let projections = self
            .basis(dates)
            .into_iter()
            .map(|(up_to_now, whole)| {
                let before = until(up_to_now);
                let after = whole.iter().map(|&p| within(p)).sum::<i64>()
                    - if has_today { within(up_to_now) } else { 0 };
                let scale = if before < PROJECTION_SCALING_FLOOR {
                    1.0
                } else {
                    today as f64 / before as f64
                };
                observed as f64 + scale * after as f64
            })
            .collect();
        estimate(projections)
    }
}

/// Each point's baseline, and the projected buckets when the range contains now.
type SeriesBaseline = (Vec<Vec<Metric>>, Option<Vec<ProjectedPoint>>);

/// A part's aggregates inside the series bucket of an index.
type Within = Box<dyn Fn(usize, usize) -> Aggs>;

/// The materialized baseline days, as one table set.
const READ: &str = "zb";

/// Today's baseline days up to now's local time, from events, when [`READ`] is from rollups.
const UNTIL: &str = "zu";

/// [`READ`] with each row in the slot `g` of its window: a whole baseline day by its local
/// day number, and again a row of today's baseline days before now's local time by `-1 - day`.
const SLOTS: &str = "zs";

const DAY_MS: i64 = 86_400_000;

/// Today's part of the range so far.
#[derive(Debug)]
struct Part {
    prefix: String,
    /// Read from rollups; else from events.
    rollup: bool,
}

/// The materialized windows of one request: every baseline day read once, the windows told
/// apart by slot ([`SLOTS`]), so each aggregate is one grouped read.
pub struct Baseline {
    current: Span,
    now_ms: i64,
    windows: Windows,
    /// The baseline days were read from rollups; else from events.
    rollup: bool,
    /// From the first baseline day's start to the last one's end.
    hull: Span,
    /// Today's part of the range so far, for a range containing now that starts before today.
    /// A range that is today scales with its own totals.
    today: Option<Part>,
}

/// The slot of a window in [`SLOTS`].
fn slot(window: &Window) -> i64 {
    let day = (window.span.first - NaiveDate::default()).num_days();
    if window.whole { day } else { -1 - day }
}

impl Baseline {
    /// Materializes the windows of `current`, one baseline list per local day as
    /// [`resolve::baseline_days`] returns them. The whole baseline days form runs of
    /// consecutive days, one run for a range of a week or more, and each run is read once,
    /// from the rollups when the request reads them.
    pub fn materialize(
        conn: &Connection,
        plan: &Plan,
        policies: &SitePolicies,
        current: &Span,
        baselines: &[Vec<Span>],
        (series, now_ms): (Option<Granularity>, i64),
    ) -> duckdb::Result<Self> {
        let tz = plan.timezone;
        let windows = Windows::plan(tz, current, baselines, now_ms);
        let mut whole: Vec<&Span> = windows
            .windows
            .iter()
            .filter(|w| w.whole)
            .map(|w| &w.span)
            .collect();
        whole.sort_by_key(|span| span.first);
        let mut runs: Vec<(i64, i64)> = Vec::new();
        for span in &whole {
            match runs.last_mut() {
                Some(run) if run.1 == span.start_ms => run.1 = span.end_ms,
                _ => runs.push((span.start_ms, span.end_ms)),
            }
        }
        // Without baseline days an empty run still creates the tables.
        if runs.is_empty() {
            runs.push((current.start_ms, current.start_ms));
        }
        let hull = Span {
            first: whole.first().map_or(current.first, |s| s.first),
            last: whole.last().map_or(current.first, |s| s.last),
            start_ms: runs[0].0,
            end_ms: runs[runs.len() - 1].1,
        };
        let until: Vec<(i64, i64)> = windows
            .windows
            .iter()
            .filter(|w| !w.whole)
            .map(|w| (w.span.start_ms, w.span.end_ms))
            .collect();
        let served: HashMap<NaiveDate, &Span> =
            whole.iter().map(|span| (span.first, *span)).collect();
        // A served day lies inside one baseline day and carries its date, so its slot is that
        // day's.
        let keep = |date: NaiveDate, start: i64, end: i64| {
            served
                .get(&date)
                .is_some_and(|span| span.start_ms <= start && end <= span.end_ms)
        };
        // Hourly buckets need served days whose rollup cells fit them.
        let fit = series.filter(|g| *g == Granularity::Hour);
        let raw = Plan {
            rollup: false,
            ..plan.clone()
        };
        // Materializes `spans` as one table set under `name`; with rollups, whether exact.
        let read = |name: &str, spans: &[(i64, i64)], rollup: bool, fit: Option<Granularity>| {
            let prefixes: Vec<String> = match spans.len() {
                1 => vec![name.to_owned()],
                n => (0..n).map(|i| format!("{name}{i}_")).collect(),
            };
            for (prefix, span) in prefixes.iter().zip(spans) {
                if !rollup {
                    run::materialize(conn, &raw, prefix, *span)?;
                } else if !rollup::materialize_days(conn, plan, policies, prefix, *span, fit, keep)?
                {
                    for prefix in &prefixes {
                        run::drop_tables(conn, prefix)?;
                    }
                    return Ok(false);
                }
            }
            if prefixes.len() > 1 {
                let mut tables = vec!["ev"];
                tables.extend(plan.sessions.then_some("se"));
                tables.extend(rollup.then_some("days"));
                for table in tables {
                    let union: Vec<String> = prefixes
                        .iter()
                        .map(|p| format!("SELECT * FROM {p}_{table}"))
                        .collect();
                    conn.execute_batch(&format!(
                        "CREATE OR REPLACE TEMP TABLE {name}_{table} AS {}",
                        union.join(" UNION ALL ")
                    ))?;
                }
                for prefix in &prefixes {
                    run::drop_tables(conn, prefix)?;
                }
            }
            Ok::<_, duckdb::Error>(true)
        };
        // Rollups serve whole days only, so the parts up to now are read from events on their
        // own; events already hold them.
        let rollup = plan.rollup
            && read(READ, &runs, true, fit)?
            && (until.is_empty() || read(UNTIL, &until, true, None)?);
        if !rollup {
            read(READ, &runs, false, fit)?;
        }

        let slotted = |table: &str, column: &str| {
            let local = local_ms_sql(tz, hull.start_ms, hull.end_ms, column);
            let day = format!("(({local}) // {DAY_MS})::BIGINT");
            let mut sql = format!(
                "CREATE OR REPLACE TEMP VIEW {SLOTS}_{table} AS SELECT *, {day} AS g FROM {READ}_{table}"
            );
            if !until.is_empty() {
                let source = if rollup {
                    format!("{UNTIL}_{table}")
                } else {
                    let within: Vec<String> = until
                        .iter()
                        .map(|(a, b)| format!("({column} >= {a} AND {column} < {b})"))
                        .collect();
                    format!(
                        "(SELECT * FROM {READ}_{table} WHERE {})",
                        within.join(" OR ")
                    )
                };
                sql += &format!(" UNION ALL SELECT *, -1 - {day} AS g FROM {source}");
            }
            sql
        };
        let mut views = vec![slotted("ev", "t")];
        if plan.sessions {
            views.push(slotted("se", "st"));
        }
        if rollup {
            views.push(format!(
                "CREATE OR REPLACE TEMP VIEW {SLOTS}_days AS
                 SELECT site, local_day, (local_day - DATE '1970-01-01')::BIGINT AS g FROM {READ}_days"
            ));
        }
        conn.execute_batch(&views.join(";\n"))?;

        let today = match windows.today {
            Some(t) if t > 0 => {
                let midnight = windows.days[t].date.and_hms_opt(0, 0, 0).expect("midnight");
                let bounds = (resolve::local_start(tz, midnight), current.end_ms);
                let prefix = "zt_".to_owned();
                let rollup = plan.rollup
                    && rollup::materialize(conn, plan, policies, &prefix, bounds, None)?;
                if !rollup {
                    run::materialize(conn, &raw, &prefix, bounds)?;
                }
                Some(Part { prefix, rollup })
            }
            _ => None,
        };
        Ok(Self {
            current: *current,
            now_ms,
            windows,
            rollup,
            hull,
            today,
        })
    }

    pub fn drop_tables(&self, conn: &Connection) -> duckdb::Result<()> {
        conn.execute_batch(&format!(
            "DROP VIEW IF EXISTS {SLOTS}_ev; DROP VIEW IF EXISTS {SLOTS}_se; DROP VIEW IF EXISTS {SLOTS}_days;"
        ))?;
        run::drop_tables(conn, READ)?;
        run::drop_tables(conn, UNTIL)?;
        if let Some(part) = &self.today {
            run::drop_tables(conn, &part.prefix)?;
        }
        Ok(())
    }

    fn plan<'a>(&self, plan: &Plan<'a>, rollup: bool) -> Plan<'a> {
        Plan {
            rollup,
            ..plan.clone()
        }
    }

    /// The index of each window by its slot.
    fn by_slot<T: Default>(&self, mut found: HashMap<i64, T>) -> HashMap<usize, T> {
        self.windows
            .windows
            .iter()
            .enumerate()
            .map(|(p, w)| (p, found.remove(&slot(w)).unwrap_or_default()))
            .collect()
    }

    /// Every window's totals of a cut.
    fn totals(
        &self,
        conn: &Connection,
        plan: &Plan,
        spec: &CutSpec,
    ) -> duckdb::Result<HashMap<usize, Aggs>> {
        let found = run::totals_by(conn, &self.plan(plan, self.rollup), SLOTS, spec, true)?;
        Ok(self.by_slot(found))
    }

    /// Every window's aggregates of the page rows with these raw keys.
    fn rows(
        &self,
        conn: &Connection,
        plan: &Plan,
        spec: &CutSpec,
        keys: &[Vec<Value>],
    ) -> duckdb::Result<HashMap<usize, HashMap<Vec<Cell>, Aggs>>> {
        let plan = self.plan(plan, self.rollup);
        let page = run::rows_by(conn, &plan, SLOTS, spec, Some(keys), true)?;
        let mut found: HashMap<i64, HashMap<Vec<Cell>, Aggs>> = HashMap::new();
        for (cells, raw, aggs) in page.rows {
            let Value::BigInt(g) = raw[0] else {
                panic!("unexpected slot {:?}", raw[0])
            };
            found.entry(g).or_default().insert(cells, aggs);
        }
        Ok(self.by_slot(found))
    }

    /// Today's measured aggregates so far: `observed`, the range's own, when the range is today.
    fn today(
        &self,
        conn: &Connection,
        plan: &Plan,
        spec: &CutSpec,
        observed: &Aggs,
    ) -> duckdb::Result<Aggs> {
        match &self.today {
            Some(part) => run::totals(conn, &self.plan(plan, part.rollup), &part.prefix, spec),
            None => Ok(observed.clone()),
        }
    }

    /// The baselines of a cut's totals and page rows, and where its totals end when the range
    /// contains now. `observed` holds the cut's own totals.
    pub fn cut(
        &self,
        conn: &Connection,
        plan: &Plan,
        spec: &CutSpec,
        page: &Page,
        observed: &Aggs,
    ) -> duckdb::Result<Compared> {
        let counted: Vec<&Vec<usize>> = self.windows.counted().collect();
        let remaining = self.windows.remaining();
        let totals = self.totals(conn, plan, spec)?;
        let keys: Vec<Vec<Value>> = page.rows.iter().map(|(_, raw, _)| raw.clone()).collect();
        let rows = if keys.is_empty() {
            HashMap::new()
        } else {
            self.rows(conn, plan, spec, &keys)?
        };
        let population = [counted.iter().copied().flatten().copied().collect()];
        let medians = |keys: &[String]| -> duckdb::Result<Vec<(usize, Vec<Value>, i64)>> {
            if !spans_median(&spec.measures) {
                return Ok(Vec::new());
            }
            self.pooled_medians(conn, &population, keys, keys)
        };
        let median_of = |medians: &[(usize, Vec<Value>, i64)], key: &[Value]| {
            medians
                .iter()
                .find(|(_, k, _)| k.as_slice() == key)
                .map(|(_, _, m)| *m)
        };

        let per_day = |aggs: &dyn Fn(usize) -> Aggs| -> Vec<Vec<Aggs>> {
            counted
                .iter()
                .map(|parts| parts.iter().map(|&p| aggs(p)).collect())
                .collect()
        };
        let total_median = median_of(&medians(&[])?, &[]);
        let totals_values = value(
            &spec.measures,
            &plan.fields,
            &per_day(&|p| totals[&p].clone()),
            total_median,
        );

        let key_columns: Vec<String> = spec
            .dimensions
            .iter()
            .flat_map(|d| {
                let i = plan.fields.iter().position(|f| f == d).expect("field");
                columns(i, d)
            })
            .collect();
        let row_medians = if keys.is_empty() {
            Vec::new()
        } else {
            medians(&key_columns)?
        };
        let empty = Aggs::default();
        let rows_values = page
            .rows
            .iter()
            .map(|(cells, raw, _)| {
                let median = median_of(&row_medians, raw);
                let aggs = |p: usize| rows[&p].get(cells).unwrap_or(&empty).clone();
                value(&spec.measures, &plan.fields, &per_day(&aggs), median)
            })
            .collect();

        let projection = match self.windows.today {
            Some(_) => {
                let today = self.today(conn, plan, spec, observed)?;
                let fields = &plan.fields;
                let projections = spec.measures.iter().map(|m| {
                    (!pooled(m)).then(|| {
                        let of = |p: usize| count(m, &totals[&p], fields);
                        let measured = (count(m, observed, fields), count(m, &today, fields));
                        self.windows.pace(&remaining, measured, of, of)
                    })
                });
                Some(projections.collect())
            }
            None => None,
        };
        Ok(Compared {
            totals: totals_values,
            rows: rows_values,
            projection,
        })
    }

    fn day(&self, date: NaiveDate) -> &Day {
        &self.windows.days[(date - self.current.first).num_days() as usize]
    }

    /// Per window, the aggregates of each local wall-clock hour. A skipped hour has none and
    /// reads as empty; a repeated hour holds both of its instants (0004).
    fn hours(
        &self,
        conn: &Connection,
        plan: &Plan,
        measures: &[Measure],
    ) -> duckdb::Result<HashMap<usize, HashMap<u32, Aggs>>> {
        let plan = self.plan(plan, self.rollup);
        let found = run::buckets_by(
            conn,
            &plan,
            SLOTS,
            &self.hull,
            Granularity::Hour,
            measures,
            true,
        )?;
        let mut by_slot: HashMap<i64, HashMap<u32, Aggs>> = HashMap::new();
        for ((g, local), aggs) in found {
            by_slot.entry(g).or_default().insert(local.hour(), aggs);
        }
        Ok(self.by_slot(by_slot))
    }

    /// The baseline of each point of the main cut's series, in point order, and when the range
    /// contains now the projection of each bucket that ends after now. `observed` holds the
    /// cut's own totals.
    pub fn series(
        &self,
        conn: &Connection,
        plan: &Plan,
        spec: &CutSpec,
        series: &Series,
        observed: &Aggs,
    ) -> duckdb::Result<SeriesBaseline> {
        let tz = plan.timezone;
        let measures = &spec.measures;
        let fields = &plan.fields;
        let starts = resolve::bucket_starts(tz, &self.current, series.granularity);
        let hourly = series.granularity == Granularity::Hour;
        let totals = if !hourly || self.windows.today.is_some() {
            self.totals(conn, plan, spec)?
        } else {
            HashMap::new()
        };
        let empty = Aggs::default();

        let (points, within): (Vec<Vec<Metric>>, Within) = if hourly {
            let hours = self.hours(conn, plan, measures)?;
            let mut medians: HashMap<(NaiveDate, u32), i64> = HashMap::new();
            if spans_median(measures) {
                let local = local_ms_sql(tz, self.hull.start_ms, self.hull.end_ms, "st");
                let sets: Vec<Vec<usize>> =
                    self.windows.days.iter().map(|d| d.whole.clone()).collect();
                let hour = [format!("hour(epoch_ms({local}))")];
                for (d, key, median) in self.pooled_medians(conn, &sets, &hour, &[])? {
                    if let [Value::BigInt(hour)] = key.as_slice() {
                        medians.insert((self.windows.days[d].date, *hour as u32), median);
                    }
                }
            }
            let points = starts
                .iter()
                .map(|(local, _)| {
                    let (date, hour) = (local.date(), local.hour());
                    let aggs = self
                        .day(date)
                        .whole
                        .iter()
                        .map(|p| hours[p].get(&hour).unwrap_or(&empty).clone())
                        .collect();
                    let median = medians.get(&(date, hour)).copied();
                    value(measures, fields, &[aggs], median)
                })
                .collect();
            let hour_of: Vec<u32> = starts.iter().map(|(local, _)| local.hour()).collect();
            let within = move |p: usize, bucket: usize| {
                hours[&p].get(&hour_of[bucket]).cloned().unwrap_or_default()
            };
            (points, Box::new(within))
        } else {
            // Day, week and month buckets add up the days of the range inside them.
            let buckets: Vec<Vec<&Day>> = starts
                .iter()
                .map(|(local, _)| {
                    self.bucket_days(series.granularity, *local)
                        .into_iter()
                        .map(|d| self.day(d))
                        .collect()
                })
                .collect();
            let medians = if spans_median(measures) {
                let sets: Vec<Vec<usize>> = buckets
                    .iter()
                    .map(|days| days.iter().flat_map(|d| d.whole.iter().copied()).collect())
                    .collect();
                self.pooled_medians(conn, &sets, &[], &[])?
            } else {
                Vec::new()
            };
            let points = buckets
                .iter()
                .enumerate()
                .map(|(i, days)| {
                    let median = medians.iter().find(|(b, _, _)| *b == i).map(|m| m.2);
                    let aggs: Vec<Vec<Aggs>> = days
                        .iter()
                        .map(|d| d.whole.iter().map(|p| totals[p].clone()).collect())
                        .collect();
                    value(measures, fields, &aggs, median)
                })
                .collect();
            let whole = totals.clone();
            (points, Box::new(move |p: usize, _| whole[&p].clone()))
        };

        let Some(t) = self.windows.today else {
            return Ok((points, None));
        };
        let today = self.today(conn, plan, spec, observed)?;
        let until = &totals;
        let today_date = self.windows.days[t].date;
        let mut projected = Vec::new();
        for (i, ((local, _), point)) in starts.iter().zip(&series.points).enumerate() {
            let end = starts.get(i + 1).map_or(self.current.end_ms, |(_, s)| *s);
            if end <= self.now_ms {
                continue;
            }
            let dates: Vec<NaiveDate> = match series.granularity {
                Granularity::Hour => vec![local.date()],
                granularity => self
                    .bucket_days(granularity, *local)
                    .into_iter()
                    .filter(|d| *d >= today_date)
                    .collect(),
            };
            let values = measures
                .iter()
                .zip(&point.values)
                .map(|(m, measured)| {
                    let Metric::Count { v } = measured else {
                        return None;
                    };
                    if pooled(m) {
                        return None;
                    }
                    let up_to_now = |p: usize| count(m, &until[&p], fields);
                    let inside = |p: usize| count(m, &within(p, i), fields);
                    let today = count(m, &today, fields);
                    Some(self.windows.pace(&dates, (*v, today), up_to_now, inside))
                })
                .collect();
            projected.push(ProjectedPoint {
                start: point.start,
                local: point.local.clone(),
                values,
            });
        }
        Ok((points, Some(projected)))
    }

    /// The range days inside the day, week or month bucket starting at `local`.
    fn bucket_days(
        &self,
        granularity: Granularity,
        local: chrono::NaiveDateTime,
    ) -> Vec<NaiveDate> {
        let next = resolve::step(granularity, local).date();
        self.current
            .first
            .iter_days()
            .take_while(|d| *d <= self.current.last)
            .filter(|d| *d >= local.date() && *d < next)
            .collect()
    }

    /// Nearest-rank median span of the sessions with a pageview, per set of windows (by
    /// position) and group: each set pools its windows as one population, a window listed
    /// twice counts twice (0015). `groups` are expressions over the `_se` columns, `keys` the
    /// key columns they read. Sets and groups without sessions are absent.
    fn pooled_medians(
        &self,
        conn: &Connection,
        sets: &[Vec<usize>],
        groups: &[String],
        keys: &[String],
    ) -> duckdb::Result<Vec<(usize, Vec<Value>, i64)>> {
        let members: Vec<String> = sets
            .iter()
            .enumerate()
            .flat_map(|(s, parts)| {
                parts
                    .iter()
                    .map(move |&p| format!("({}, {s})", slot(&self.windows.windows[p])))
            })
            .collect();
        if members.is_empty() {
            return Ok(Vec::new());
        }
        let distinct: String = keys.iter().map(|k| format!("{k}, ")).collect();
        let named: String = groups
            .iter()
            .enumerate()
            .map(|(i, g)| format!("{g} AS g{i}, "))
            .collect();
        let sql = format!(
            "SELECT w.s::BIGINT, {named}quantile_disc(x.span, 0.5)
             FROM (SELECT DISTINCT g, {distinct}session_id, pv, span, st FROM {SLOTS}_se WHERE pv > 0) x
             JOIN (VALUES {}) w(g, s) ON w.g = x.g
             GROUP BY ALL",
            members.join(", ")
        );
        let mut statement = conn.prepare(&sql)?;
        let mut result = statement.query([])?;
        let mut out = Vec::new();
        let width = groups.len();
        while let Some(row) = result.next()? {
            let set: i64 = row.get(0)?;
            let key = (0..width)
                .map(|i| row.get::<_, Value>(i + 1))
                .collect::<duckdb::Result<_>>()?;
            if let Some(median) = row.get::<_, Option<i64>>(width + 1)? {
                out.push((set as usize, key, median));
            }
        }
        Ok(out)
    }
}

/// Rates and spans pool their baseline days; every other baselined measure is a count.
fn pooled(measure: &Measure) -> bool {
    matches!(
        measure,
        Measure::Named(
            NamedMeasure::BounceRate | NamedMeasure::SpanMean | NamedMeasure::SpanMedian
        )
    )
}

fn spans_median(measures: &[Measure]) -> bool {
    measures.contains(&Measure::Named(NamedMeasure::SpanMedian))
}

/// One baseline per measure from the aggregates of each range day's baseline days: counts add
/// up the per-day medians, rates and spans pool every baseline day, with `span_median` the
/// pooled median span. A day with fewer than the minimum of baseline days leaves no baseline.
fn value(
    measures: &[Measure],
    fields: &[Field],
    days: &[Vec<Aggs>],
    span_median: Option<i64>,
) -> Vec<Metric> {
    if days
        .iter()
        .any(|d| d.len() < TYPICAL_WEEKDAY_DAYS_MIN as usize)
    {
        let unavailable = Metric::Unavailable {
            reason: Reason::InsufficientHistory,
        };
        return vec![unavailable; measures.len()];
    }
    let mut pool = Aggs {
        span_median,
        ..Aggs::default()
    };
    for aggs in days.iter().flatten() {
        pool.pageview_sessions += aggs.pageview_sessions;
        pool.bounces += aggs.bounces;
        pool.span_sum += aggs.span_sum;
    }
    measures
        .iter()
        .map(|measure| {
            if pooled(measure) {
                return run::metric(measure, &pool, fields);
            }
            let v = days
                .iter()
                .map(|d| median(d.iter().map(|aggs| count(measure, aggs, fields)).collect()))
                .sum();
            Metric::Count { v }
        })
        .collect()
}

fn count(measure: &Measure, aggs: &Aggs, fields: &[Field]) -> i64 {
    match run::metric(measure, aggs, fields) {
        Metric::Count { v } => v,
        other => unreachable!("validation baselines counts and pooled measures only: {other:?}"),
    }
}

/// Nearest-rank median: the lower middle value of an even count, as 0003 ranks percentiles.
fn median(mut values: Vec<i64>) -> i64 {
    values.sort_unstable();
    values[values.len().div_ceil(2) - 1]
}

/// The median of the basis days' projections with their 25th and 75th percentiles as the band,
/// nearest-rank like [`median`], rounded to 4 places. Fewer than the minimum of basis days
/// project nothing.
fn estimate(mut projections: Vec<f64>) -> Projection {
    let n = projections.len();
    if n < TYPICAL_WEEKDAY_DAYS_MIN as usize {
        return Projection::Unavailable {
            reason: Reason::InsufficientHistory,
        };
    }
    projections.sort_by(f64::total_cmp);
    let rank = |percent: usize| (projections[(percent * n).div_ceil(100) - 1] * 1e4).round() / 1e4;
    Projection::Estimate {
        value: rank(50),
        low: rank(25),
        high: rank(75),
        basis_days: n as u32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_median_of_an_even_count_is_the_lower_middle_value() {
        assert_eq!(median(vec![3, 100, 1, 2]), 2);
        assert_eq!(median(vec![5, 1, 9]), 5);
        assert_eq!(median(vec![7, 7, 7, 7, 7, 7, 7, 900]), 7);
    }

    #[test]
    fn a_projection_band_ranks_like_the_median() {
        // Of 8 the 2nd, 4th and 6th smallest; of 5 the 2nd, 3rd and 4th.
        let eight = estimate((1..=8).rev().map(f64::from).collect());
        assert_eq!(
            eight,
            Projection::Estimate {
                value: 4.0,
                low: 2.0,
                high: 6.0,
                basis_days: 8
            }
        );
        let five = estimate(vec![10.0, 50.0, 20.0, 40.0, 30.0]);
        assert_eq!(
            five,
            Projection::Estimate {
                value: 30.0,
                low: 20.0,
                high: 40.0,
                basis_days: 5
            }
        );
        assert_eq!(
            estimate(vec![1.0, 2.0, 3.0]),
            Projection::Unavailable {
                reason: Reason::InsufficientHistory
            }
        );
    }
}
