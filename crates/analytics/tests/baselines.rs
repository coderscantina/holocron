//! Typical-weekday baselines (0015) and previous-period series points (0003) on hand-computed
//! data. Every site in Europe/Vienna gets traffic from one rule, so each expectation below is
//! derived from the rule and a calendar.
//!
//! **Traffic rule.** A day with weight `w` has, in every hour that starts on it (UTC hours, so a
//! 23-hour spring-forward day has 23 and a 25-hour fall-back day 25, its local hour 2 twice):
//! `w` bouncing visitors with one pageview at `:05`, and one engaged visitor with pageviews at
//! `:10` and `:30`, one session spanning 20 minutes. So an hour has `w + 2` pageviews, `w + 1`
//! visitors and sessions, `w` bounces, and a whole 24-hour day `24 (w + 2)` pageviews. Events
//! at or after the clock are not sent. The goals site adds a `signup` custom event at `:06` in
//! each bouncing visitor's session, so an hour has `w` signups out of `w + 1` sessions.
//!
//! Medians are nearest-rank: of 8 values the 4th smallest, of 4 the 2nd, of 5 the 3rd.
//!
//! **Projections** (0015 Pace projection). Basis day `k` is the range shifted back `k` weeks.
//! With `u_k` its count today up to now, `a_k` what its days add after now, `o` the observed
//! value and `t` today's so far: `p_k = o + s_k × a_k`, with `s_k = t / u_k`, or 1 when
//! `u_k < 20`. The band is nearest-rank too: of 8 the 2nd, 4th and 6th smallest; of 7 the 2nd,
//! 4th and 6th. Values are rounded to 4 places.

mod common;

use chrono::{Days, NaiveDate, TimeZone};
use chrono_tz::Europe::Vienna;
use common::{Site, Who, correct, ingest, ms, open};
use holocron_analytics::explore::{Context, explore, validate};
use holocron_analytics::jobs;
use holocron_protocol::exploration::{
    Comparison, ExplorationState, ExploreResult, GoalResult, Metric, Panel, Projection, Reason,
    ReportResult,
};
use holocron_storage::{EventKind, Lane, Record, Storage};
use serde_json::{Value, json};

/// Tuesday 2026-10-13, 15:20 in Vienna (CEST).
const NOW: &str = "2026-10-13T13:20:00Z";
/// A clock after both DST fixtures and within 12 months of them.
const SPRING_NOW: &str = "2026-04-20T10:00:00Z";

const HOUR: i64 = 3_600_000;
const MINUTE: i64 = 60_000;

fn date(text: &str) -> NaiveDate {
    text.parse().unwrap()
}

fn local_start(date: NaiveDate) -> i64 {
    Vienna
        .from_local_datetime(&date.and_hms_opt(0, 0, 0).unwrap())
        .earliest()
        .unwrap()
        .timestamp_millis()
}

/// One day of the traffic rule with weight `w`, without events at or after `now`.
fn day(site: &mut Site, day: &str, w: usize, now: i64) -> Vec<Record> {
    let start = local_start(date(day));
    let end = local_start(date(day) + Days::new(1));
    let mut out = Vec::new();
    let mut hour = start;
    while hour < end {
        let mut push = |at: i64, label: String| {
            if at < now {
                out.push(site.event_at(EventKind::Pageview, at, at, Who::Anon(&label)));
            }
        };
        for i in 0..w {
            push(hour + 5 * MINUTE, format!("b{hour}-{i}"));
        }
        push(hour + 10 * MINUTE, format!("e{hour}"));
        push(hour + 30 * MINUTE, format!("e{hour}"));
        hour += HOUR;
    }
    out
}

/// Loads sites, each a list of `(local date, weight)`, with Vienna policies.
async fn load(sites: &[(u128, Vec<(String, usize)>)], now: &str) -> (tempfile::TempDir, Storage) {
    let (dir, storage) = open().await;
    let mut records = Vec::new();
    for (id, days) in sites {
        let mut site = Site::new(*id, Vienna);
        jobs::record_policies(&storage, *id, site.policies.clone())
            .await
            .unwrap();
        for (d, w) in days {
            records.extend(day(&mut site, d, *w, ms(now)));
        }
    }
    ingest(&storage, &records).await;
    correct(&storage).await;
    (dir, storage)
}

/// Every day from `first` to `last` with weight 1, except the given ones.
fn days(first: &str, last: &str, weights: &[(&str, usize)]) -> Vec<(String, usize)> {
    date(first)
        .iter_days()
        .take_while(|d| *d <= date(last))
        .map(|d| {
            let text = d.format("%Y-%m-%d").to_string();
            let w = weights
                .iter()
                .find(|(t, _)| *t == text)
                .map_or(1, |(_, w)| *w);
            (text, w)
        })
        .collect()
}

const MEASURES: [&str; 6] = [
    "pageviews",
    "visitor_days",
    "sessions",
    "bounce_rate",
    "span_mean",
    "span_median",
];

fn state(range: (&str, &str), granularity: &str, compare: &str, filter: Value) -> ExplorationState {
    serde_json::from_value(json!({
        "v": 1, "site": "s", "measures": MEASURES, "dimensions": [], "filter": filter,
        "range": { "kind": "days", "start": range.0, "end": range.1 }, "timezone": "Europe/Vienna",
        "granularity": granularity, "compare": compare, "metric_version": 1, "sort": [],
        "page": { "size": 50, "index": 0 }, "visualization": "line"
    }))
    .unwrap()
}

fn path_panel() -> Panel {
    serde_json::from_value(json!({
        "measures": ["visitor_days"], "dimensions": ["path"],
        "sort": [{ "by": "visitor_days", "dir": "desc" }], "page": { "size": 5, "index": 0 }
    }))
    .unwrap()
}

/// A filter every event of the traffic rule matches: they all have the path `/`.
fn everything() -> Value {
    json!({ "kind": "group", "op": "and", "items": [
        { "kind": "cond", "id": "c1", "field": "path", "cmp": "eq", "value": { "t": "string", "v": "/" } }
    ] })
}

async fn run(
    storage: &Storage,
    site: u128,
    state: ExplorationState,
    panels: Vec<Panel>,
    now: &str,
) -> ExploreResult {
    let query = validate(&state, &panels).unwrap();
    let now = ms(now);
    storage
        .read(Lane::Report, move |s| {
            let ctx = Context {
                site,
                now_ms: now,
                hostnames: &[],
                retention: holocron_protocol::retention::Retention::OneYear,
            };
            explore(s, &query, &ctx)
        })
        .await
        .unwrap()
        .value
        .unwrap()
}

fn previous(comparison: &Comparison) -> i64 {
    match comparison.previous {
        Metric::Count { v } | Metric::Millis { v } => v,
        ref other => panic!("{other:?}"),
    }
}

fn totals(result: &ExploreResult) -> &[Comparison] {
    result.main.compare.as_deref().unwrap()
}

fn estimate(value: f64, low: f64, high: f64, basis_days: u32) -> Option<Projection> {
    Some(Projection::Estimate {
        value,
        low,
        high,
        basis_days,
    })
}

const UNPROJECTED: Option<Projection> = Some(Projection::Unavailable {
    reason: Reason::InsufficientHistory,
});

/// The previous value of the first measure at each series point.
fn points(result: &ExploreResult) -> Vec<(String, i64)> {
    let series = result.main.series.as_ref().unwrap();
    series
        .points
        .iter()
        .map(|p| {
            (
                p.local[11..].to_owned(),
                previous(&p.compare.as_ref().unwrap()[0]),
            )
        })
        .collect()
}

const A: u128 = 0xa;

/// Site A: hourly traffic from Tuesday 2026-08-04, weight 1 except the Tuesdays below. The
/// eight Tuesdays before 10-13 weigh 1, 2, 3, 4, 50 (the spike), 5, 6, 7; 10-13 weighs 5.
fn site_a() -> Vec<(String, usize)> {
    days(
        "2026-08-04",
        "2026-10-13",
        &[
            ("2026-08-25", 2),
            ("2026-09-01", 3),
            ("2026-09-08", 4),
            ("2026-09-15", 50),
            ("2026-09-22", 5),
            ("2026-09-29", 6),
            ("2026-10-06", 7),
            ("2026-10-13", 5),
        ],
    )
}

#[tokio::test(flavor = "multi_thread")]
async fn today_at_15_20_compares_with_the_median_tuesday_until_15_20() {
    let (_dir, storage) = load(&[(A, site_a())], NOW).await;
    let today = ("2026-10-13", "2026-10-13");
    let result = run(
        &storage,
        A,
        state(today, "hour", "typical_weekday", Value::Null),
        vec![path_panel()],
        NOW,
    )
    .await;
    assert_eq!(
        result.resolved.compare_days.as_deref().unwrap(),
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
    assert_eq!(result.resolved.compare_start, None);

    // Until 15:20 a baseline day has hours 0 to 14 whole, and from hour 15 the bouncers at
    // 15:05 and the engaged first pageview at 15:10: 15 (w + 2) + w + 1 = 16w + 31 pageviews,
    // 16 (w + 1) visitors and sessions. Weights sorted 1 2 3 4 5 6 7 50: the median takes w = 4
    // and ignores the spike (a mean would give 16 × 9.75 + 31 = 187).
    let t = totals(&result);
    assert_eq!(previous(&t[0]), 95);
    // Today, w = 5: 16 × 5 + 31 = 111, so 16 more, 16 / 95 = 0.1684.
    assert_eq!(
        (t[0].change, t[0].relative_change),
        (Some(16.0), Some(0.1684))
    );
    assert_eq!(previous(&t[1]), 80);
    assert_eq!(t[1].relative_change, Some(0.2));
    assert_eq!(previous(&t[2]), 80);
    // Rates and spans pool the eight days: bounces 16w, sessions with a pageview 16 (w + 1),
    // so 16 × 78 / (16 × 86) = 1248 / 1376; every engaged session spans 20 min, 128 of them.
    assert_eq!(
        t[3].previous,
        Metric::Ratio {
            v: 0.907,
            numerator: 1248,
            denominator: 1376
        }
    );
    // 128 × 1,200,000 / 1376 = 111,627.9 ms; the pooled median of 1248 zero spans and 128
    // spans of 20 min is 0.
    assert_eq!(previous(&t[4]), 111_628);
    assert_eq!(previous(&t[5]), 0);
    // One path: its row compares with the same visitors baseline as the total.
    let row = &result.panels[0].rows[0];
    assert_eq!(previous(&row.compare.as_ref().unwrap()[0]), 80);

    // The series has a baseline for all 24 hours from whole baseline days: w + 2 = 6 pageviews
    // at the median w = 4, also after now.
    let hours = points(&result);
    assert_eq!(hours.len(), 24);
    assert!(hours.iter().all(|(_, v)| *v == 6), "{hours:?}");
    let series = result.main.series.as_ref().unwrap();
    let late = &series.points[20];
    assert_eq!(
        (
            &late.local[..],
            &late.values[0],
            late.compare.as_ref().unwrap()[0].change
        ),
        ("2026-10-13T20:00", &Metric::Count { v: 0 }, Some(-6.0))
    );
    assert_eq!(
        late.compare.as_ref().unwrap()[3].previous,
        Metric::Ratio {
            v: 0.907,
            numerator: 78,
            denominator: 86
        }
    );

    // Projection. Pageviews: u_k = 16w + 31, a_k = 24 (w + 2) − u_k = 8w + 17, o = t = 111.
    // p_k = 111 + 111 (8w + 17) / (16w + 31), sorted: w = 50 gives 166.7004, 7 167.6643,
    // 6 167.8110, 5 168, 4 168.2526, 3 168.6076, 2 169.1429, 1 170.0426. The spike barely
    // moves it: the scale corrects each day's level, so only the shape of its day counts.
    let projection = result.main.projection.as_ref().unwrap();
    assert_eq!(projection[0], estimate(168.0, 167.6643, 168.6076, 8));
    // Visitor-days and sessions: u_k = 16 (w + 1), a_k = 8 (w + 1), o = t = 96: every p_k is
    // 96 + 48 = 144. Rates and spans are no counts and have none.
    assert_eq!(projection[1], estimate(144.0, 144.0, 144.0, 8));
    assert_eq!(projection[2], estimate(144.0, 144.0, 144.0, 8));
    assert_eq!(projection[3..], [None, None, None]);
    // The path panel projects its own totals: visitor-days as above.
    assert_eq!(
        result.panels[0].projection.as_deref().unwrap(),
        [estimate(144.0, 144.0, 144.0, 8)]
    );
    // The series projects 15:00 to 23:00, apart from its points. Hour 15 measured w + 1 = 6
    // until now and each basis day adds its 15:30 pageview: 6 + 111 / (16w + 31), sorted
    // 6.1336 6.7762 6.8740 7 7.1684 7.4051 7.7619 8.3617. Hour 16 is all after now:
    // 111 (w + 2) / (16w + 31), sorted 6.9458 6.9860 6.9921 7 7.0105 7.0253 7.0476 7.0851.
    let projected = series.projection.as_ref().unwrap();
    let hours: Vec<&str> = projected.iter().map(|p| &p.local[11..]).collect();
    assert_eq!(
        hours,
        [
            "15:00", "16:00", "17:00", "18:00", "19:00", "20:00", "21:00", "22:00", "23:00"
        ]
    );
    assert_eq!(projected[0].values[0], estimate(7.0, 6.7762, 7.4051, 8));
    assert_eq!(projected[1].values[0], estimate(7.0, 6.986, 7.0253, 8));
    assert_eq!(projected[0].values[3], None);
    // Measured values never include the estimate.
    assert_eq!(series.points[15].values[0], Metric::Count { v: 6 });
    assert_eq!(series.points[16].values[0], Metric::Count { v: 0 });

    // A filter matching every event scans events instead of rollups: the same baselines.
    let filtered = run(
        &storage,
        A,
        state(today, "hour", "typical_weekday", everything()),
        vec![path_panel()],
        NOW,
    )
    .await;
    let compares = |r: &ExploreResult| {
        json!([
            r.main.compare,
            r.main.projection,
            r.main.series.as_ref().unwrap().projection,
            r.main
                .series
                .as_ref()
                .unwrap()
                .points
                .iter()
                .map(|p| &p.compare)
                .collect::<Vec<_>>(),
            r.panels[0].rows[0].compare
        ])
    };
    assert_eq!(compares(&filtered), compares(&result));
    // Totals, rows, series and projection share the result's one snapshot and freshness.
    assert_eq!(result.freshness.metric_version, 1);
    assert_eq!(
        filtered.freshness.watermark_batch,
        result.freshness.watermark_batch
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn ranges_after_today_and_previous_period_points() {
    let (_dir, storage) = load(&[(A, site_a())], NOW).await;
    // Tuesday to Tuesday, by day. Days after today count in no total, and 10-13 is no baseline
    // day of 10-20: today is never one.
    for filter in [Value::Null, everything()] {
        let week = run(
            &storage,
            A,
            state(
                ("2026-10-13", "2026-10-20"),
                "day",
                "typical_weekday",
                filter,
            ),
            vec![],
            NOW,
        )
        .await;
        let compare_days = week.resolved.compare_days.as_deref().unwrap();
        assert!(compare_days.contains(&"2026-10-12".to_owned()));
        assert!(!compare_days.contains(&"2026-10-13".to_owned()));
        // The totals read only today until 15:20, as above.
        assert_eq!(previous(&totals(&week)[0]), 95);
        // Whole days: Tuesday 10-13 at w = 4, 24 × 6 = 144; Wednesday to Monday w = 1, 72;
        // Tuesday 10-20 reads 7 Tuesdays weighing 2 3 4 5 6 7 50, median w = 5: 24 × 7 = 168.
        // Counting 10-13 as an empty day would give w = 4, 144.
        let days: Vec<i64> = points(&week).into_iter().map(|(_, v)| v).collect();
        assert_eq!(days, [144, 72, 72, 72, 72, 72, 72, 168]);

        // Projection of the week: basis day k reads 10-13 − 7k up to now and the whole days
        // 10-13 − 7k to 10-20 − 7k. Week 1 lacks 10-20 − 7 = today, so 7 basis days. With w
        // today's k-th weight and v that of 10-20 − 7k: a_k = 8w + 17 + 6 × 72 + 24 (v + 2).
        // k = 2..8: (w, v) = (6, 7) (5, 6) (50, 5) (4, 50) (3, 4) (2, 3) (1, 2), so
        // p_k = 111 + 111 a_k / (16w + 31): 734.1732, 792, 246.8448, 2131.2, 977.9241,
        // 1141.7143, 1417.0213. Sorted, the 2nd, 4th and 6th. Every week is scaled by
        // today's pace, so a spike Tuesday in a basis week swings it.
        assert_eq!(
            week.main.projection.as_ref().unwrap()[0],
            estimate(977.9241, 734.1732, 1417.0213, 7)
        );
    }

    // Previous period: totals stop at 15:20 on Monday 10-12 (w = 1): 16 + 31 = 47; the series
    // compares every hour with the whole Monday: w + 2 = 3.
    let previous_period = run(
        &storage,
        A,
        state(
            ("2026-10-13", "2026-10-13"),
            "hour",
            "previous_period",
            Value::Null,
        ),
        vec![],
        NOW,
    )
    .await;
    assert_eq!(previous(&totals(&previous_period)[0]), 47);
    let hours = points(&previous_period);
    assert_eq!(hours.len(), 24);
    assert!(hours.iter().all(|(_, v)| *v == 3), "{hours:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn young_sites_and_empty_baselines() {
    const YOUNG: u128 = 0xb;
    const MONDAYS: u128 = 0xc;
    let young = ["2026-09-22", "2026-09-29", "2026-10-06", "2026-10-13"]
        .map(|d| (d.to_owned(), 1))
        .to_vec();
    let mondays = date("2026-08-03")
        .iter_days()
        .step_by(7)
        .take(11)
        .map(|d| (d.format("%Y-%m-%d").to_string(), 1))
        .chain([("2026-10-13".to_owned(), 1)])
        .collect();
    let (_dir, storage) = load(&[(YOUNG, young), (MONDAYS, mondays)], NOW).await;
    let today = ("2026-10-13", "2026-10-13");

    // First stored event on 09-22: three baseline Tuesdays, fewer than four.
    let result = run(
        &storage,
        YOUNG,
        state(today, "hour", "typical_weekday", Value::Null),
        vec![],
        NOW,
    )
    .await;
    assert_eq!(result.resolved.compare_days.as_ref().unwrap().len(), 3);
    let insufficient = Metric::Unavailable {
        reason: Reason::InsufficientHistory,
    };
    assert!(totals(&result).iter().all(|c| c.previous == insufficient
        && c.reason == Some(Reason::InsufficientHistory)
        && c.change.is_none()
        && c.relative_change.is_none()));
    let series = result.main.series.as_ref().unwrap();
    assert!(
        series
            .points
            .iter()
            .all(|p| p.compare.as_ref().unwrap()[0].previous == insufficient)
    );
    // Three basis days project nothing, in totals and buckets alike.
    assert_eq!(
        result.main.projection.as_ref().unwrap()[..3],
        [UNPROJECTED; 3]
    );
    let projected = series.projection.as_ref().unwrap();
    assert!(projected.iter().all(|p| p.values[0] == UNPROJECTED));

    // Traffic on Mondays only: eight empty Tuesdays. Today until 15:20 has 47 pageviews.
    let mut empty = state(today, "hour", "typical_weekday", Value::Null);
    empty.measures = serde_json::from_value(json!(["pageviews", "events", "bounce_rate"])).unwrap();
    let result = run(&storage, MONDAYS, empty, vec![], NOW).await;
    let t = totals(&result);
    assert_eq!(result.resolved.compare_days.as_ref().unwrap().len(), 8);
    // 0003: a zero baseline has no relative change, whether or not today has traffic.
    assert_eq!(
        (
            previous(&t[0]),
            t[0].change,
            t[0].relative_change,
            t[0].reason
        ),
        (0, Some(47.0), None, Some(Reason::ZeroBaseline))
    );
    assert_eq!(
        (
            previous(&t[1]),
            t[1].change,
            t[1].relative_change,
            t[1].reason
        ),
        (0, Some(0.0), None, Some(Reason::ZeroBaseline))
    );
    assert_eq!(
        t[2].previous,
        Metric::Unavailable {
            reason: Reason::NoDenominator
        }
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn retention_clips_the_baseline() {
    const RETAINED: u128 = 0xd;
    // Tuesdays of 2025. At the clock the cutoff is the start of 2025-10-13 in Vienna, so the
    // heavy 09-30 and 10-07 are gone and 11-11 reads 10-14 to 11-04, weighing 1 2 3 4.
    let tuesdays = [
        ("2025-09-30", 20),
        ("2025-10-07", 20),
        ("2025-10-14", 1),
        ("2025-10-21", 2),
        ("2025-10-28", 3),
        ("2025-11-04", 4),
        ("2025-11-11", 5),
    ]
    .map(|(d, w)| (d.to_owned(), w))
    .to_vec();
    let (_dir, storage) = load(&[(RETAINED, tuesdays)], NOW).await;
    let result = run(
        &storage,
        RETAINED,
        state(
            ("2025-11-11", "2025-11-11"),
            "day",
            "typical_weekday",
            Value::Null,
        ),
        vec![],
        NOW,
    )
    .await;
    assert_eq!(
        result.resolved.compare_days.as_deref().unwrap(),
        ["2025-10-14", "2025-10-21", "2025-10-28", "2025-11-04"]
    );
    // Day totals 72 96 120 144: the median is 96. With the clipped days it would be 120.
    assert_eq!(previous(&totals(&result)[0]), 96);
    assert_eq!(points(&result), [("00:00".to_owned(), 96)]);
    // A range without now projects nothing.
    assert_eq!(result.main.projection, None);
    assert_eq!(result.main.series.as_ref().unwrap().projection, None);
}

#[tokio::test(flavor = "multi_thread")]
async fn dst_hours_match_by_wall_clock() {
    const SPRING: u128 = 0xe;
    const FALL: u128 = 0xf;
    let sundays = |list: [(&str, usize); 6]| list.map(|(d, w)| (d.to_owned(), w)).to_vec();
    // 2026-03-29 has 23 hours, no 02:00; 2025-10-26 has 25, 02:00 twice.
    let spring = sundays([
        ("2026-03-01", 1),
        ("2026-03-08", 2),
        ("2026-03-15", 4),
        ("2026-03-22", 5),
        ("2026-03-29", 3),
        ("2026-04-05", 6),
    ]);
    let fall = sundays([
        ("2025-09-28", 1),
        ("2025-10-05", 2),
        ("2025-10-12", 4),
        ("2025-10-19", 5),
        ("2025-10-26", 3),
        ("2025-11-02", 6),
    ]);
    let (_dir, storage) = load(&[(SPRING, spring), (FALL, fall)], SPRING_NOW).await;
    let hourly = |site, day: &'static str| {
        let storage = &storage;
        async move {
            let mut state = state((day, day), "hour", "typical_weekday", Value::Null);
            state.measures = serde_json::from_value(json!(["pageviews"])).unwrap();
            let result = run(storage, site, state, vec![], SPRING_NOW).await;
            (previous(&totals(&result)[0]), points(&result))
        }
    };
    let hour =
        |points: &[(String, i64)], at: &str| points.iter().find(|(h, _)| h == at).map(|(_, v)| *v);

    // The spring-forward day: no hour 2 on either side. Its four baseline Sundays have
    // 3 4 6 7 pageviews an hour (median 4) and days of 72 96 144 168 (median 96).
    let (total, points) = hourly(SPRING, "2026-03-29").await;
    assert_eq!(
        (total, points.len(), hour(&points, "02:00")),
        (96, 23, None)
    );
    assert!(points.iter().all(|(_, v)| *v == 4));
    // The Sunday after reads the spring-forward day too, whose hour 2 is empty: hour 2 has
    // 0 3 4 6 7 (median 4) against 3 4 5 6 7 (median 5) elsewhere. Days: 23 × 5 = 115 and
    // 72 96 144 168: median 115.
    let (total, points) = hourly(SPRING, "2026-04-05").await;
    assert_eq!(
        (total, hour(&points, "02:00"), hour(&points, "03:00")),
        (115, Some(4), Some(5))
    );

    // The fall-back day: one hour 2 bucket holding both instants on the current side, a single
    // hour on its baseline days (median 4 of 3 4 6 7); days 72 96 144 168: median 96.
    let (total, points) = hourly(FALL, "2025-10-26").await;
    assert_eq!(
        (total, points.len(), hour(&points, "02:00")),
        (96, 24, Some(4))
    );
    // The Sunday after: the fall-back day's hour 2 holds 2 × 5 = 10, so hour 2 has 3 4 6 7 10
    // (median 6) against 3 4 5 6 7 (median 5). Days: 25 × 5 = 125 and 72 96 144 168: 125.
    let (total, points) = hourly(FALL, "2025-11-02").await;
    assert_eq!(
        (total, hour(&points, "02:00"), hour(&points, "03:00")),
        (125, Some(6), Some(5))
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_quiet_morning_does_not_scale_its_basis_day() {
    // Tuesday 10-13 at 03:20, before any later event exists.
    const EARLY: &str = "2026-10-13T01:20:00Z";
    let (_dir, storage) = load(&[(A, site_a())], EARLY).await;
    let today = ("2026-10-13", "2026-10-13");
    let result = run(
        &storage,
        A,
        state(today, "hour", "typical_weekday", Value::Null),
        vec![],
        EARLY,
    )
    .await;
    let projection = result.main.projection.as_ref().unwrap();
    // Pageviews until 03:20: hours 0 to 2 and hour 3's first two minutes of traffic, 4w + 7,
    // so 11, 15 and 19 for w = 1, 2, 3: under 20, scale 1. a_k = 24 (w + 2) − u_k = 20w + 41,
    // o = t = 27. w = 1 2 3: 27 + a_k = 88 108 128; w = 4 5 6 7 50: 27 + 27 a_k / u_k =
    // 169.0435 168 167.2258 166.6286 162.7826. Scaling all eight would give 168 (176.7273,
    // 172.8 and 170.5263 for w = 1 2 3).
    assert_eq!(projection[0], estimate(162.7826, 108.0, 167.2258, 8));
    // Visitor-days: u_k = 4 (w + 1), so w = 4 sits exactly on the floor and scales. Unscaled
    // 24 + 20 (w + 1) = 64 84 104 for w = 1 2 3; every scaled one is 24 + 120 = 144.
    assert_eq!(projection[1], estimate(144.0, 84.0, 144.0, 8));
}

const G: u128 = 0x10;

/// Site G: site A's traffic and a signup in every bouncing session.
async fn load_goals(now: &str) -> (tempfile::TempDir, Storage) {
    let (dir, storage) = open().await;
    let mut site = Site::new(G, Vienna);
    jobs::record_policies(&storage, G, site.policies.clone())
        .await
        .unwrap();
    let mut records = Vec::new();
    for (d, w) in site_a() {
        records.extend(day(&mut site, &d, w, ms(now)));
        let start = local_start(date(&d));
        let end = local_start(date(&d) + Days::new(1));
        let mut hour = start;
        while hour < end {
            for i in 0..w {
                let at = hour + 6 * MINUTE;
                if at < ms(now) {
                    let label = format!("b{hour}-{i}");
                    records.push(site.event_at(EventKind::CustomEvent, at, at, Who::Anon(&label)));
                }
            }
            hour += HOUR;
        }
    }
    ingest(&storage, &records).await;
    correct(&storage).await;
    (dir, storage)
}

fn goals_state(day: &str) -> ExplorationState {
    let goal = json!({ "scope": "session", "counting": "once", "predicate": { "kind": "group", "op": "and", "items": [
        { "kind": "cond", "id": "g1", "field": "event_name", "cmp": "eq", "value": { "t": "string", "v": "signup" } }
    ] } });
    serde_json::from_value(json!({
        "v": 1, "site": "s", "measures": [], "dimensions": [], "filter": null,
        "range": { "kind": "days", "start": day, "end": day }, "timezone": "Europe/Vienna",
        "granularity": "day", "compare": "typical_weekday", "metric_version": 1, "sort": [],
        "page": { "size": 50, "index": 0 }, "visualization": "table",
        "report": { "kind": "goals", "v": 1, "goals": [goal] }
    }))
    .unwrap()
}

fn goal(result: &ExploreResult) -> &GoalResult {
    match &result.report {
        Some(ReportResult::Goals(goals)) => &goals.goals[0],
        other => panic!("{other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn goals_compare_with_the_median_tuesday_and_project_completions() {
    let (_dir, storage) = load_goals(NOW).await;
    let result = run(&storage, G, goals_state("2026-10-13"), vec![], NOW).await;
    assert_eq!(result.resolved.compare_days.as_ref().unwrap().len(), 8);
    let today = goal(&result);
    // Until 15:20 a day has 16w signups out of 16 (w + 1) sessions: today w = 5, 80 of 96.
    assert_eq!((today.completions, today.denominator), (80, 96));
    let compare = today.compare.as_ref().unwrap();
    // The median Tuesday has w = 4: 64 completions, 16 more today, 0.25.
    assert_eq!(compare.completions.previous, Metric::Count { v: 64 });
    assert_eq!(
        (
            compare.completions.change,
            compare.completions.relative_change
        ),
        (Some(16.0), Some(0.25))
    );
    // The rate pools the eight days: 16 × 78 signups out of 16 × 86 sessions.
    assert_eq!(
        compare.rate.previous,
        Metric::Ratio {
            v: 0.907,
            numerator: 1248,
            denominator: 1376
        }
    );
    // u_k = 16w, a_k = 24w − 16w = 8w, o = t = 80. w = 1 is under the floor: 80 + 8 = 88;
    // every other basis day: 80 + 80 × 8w / 16w = 120.
    assert_eq!(today.projection, estimate(120.0, 120.0, 120.0, 8));

    // A past Tuesday: whole days, no projection. 10-06 has 24 × 7 = 168 signups; its baseline
    // Tuesdays weigh 6 5 50 4 3 2 1 1, median w = 3: 72.
    let past = run(&storage, G, goals_state("2026-10-06"), vec![], NOW).await;
    let past = goal(&past);
    assert_eq!(past.completions, 168);
    assert_eq!(
        past.compare.as_ref().unwrap().completions.previous,
        Metric::Count { v: 72 }
    );
    assert_eq!(past.projection, None);
}
