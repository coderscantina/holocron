//! Identity-day rollups (0008 Rollups): every servable state gives the raw planner's result,
//! byte for byte, whatever mix of rollup days and event-counted instants serves it.

mod common;

use chrono_tz::Europe::Vienna;
use common::{Site, Who, correct, ingest, ms, open};
use holocron_analytics::explore::{Context, explore, explore_raw, servable, validate};
use holocron_analytics::identity::SessionPolicy;
use holocron_analytics::jobs;
use holocron_protocol::exploration::{ExplorationState, Panel};
use holocron_storage::{EventKind, Lane, Record, Storage};
use serde_json::{Value, json};

const SITE: u128 = 0x5a30_3801;
const NOW: &str = "2026-10-28T10:00:00Z";

/// The overview's five panels (`pages/overview/OverviewView.vue`).
fn overview_panels() -> Vec<Panel> {
    ["path", "referrer_domain", "browser", "os", "device"]
        .iter()
        .map(|field| {
            serde_json::from_value(json!({
                "measures": ["visitor_days"], "dimensions": [field],
                "sort": [{ "by": "visitor_days", "dir": "desc" }], "page": { "size": 6, "index": 0 }
            }))
            .unwrap()
        })
        .collect()
}

fn state(start: &str, end: &str, timezone: &str, granularity: &str, extra: Value) -> Value {
    let mut state = json!({
        "v": 1, "site": "s",
        "measures": ["pageviews", "events", "visitor_days", "sessions", "bounce_rate", "span_mean", "span_median"],
        "dimensions": [], "filter": null,
        "range": { "kind": "days", "start": start, "end": end }, "timezone": timezone,
        "granularity": granularity, "compare": "previous_period", "metric_version": 1, "sort": [],
        "page": { "size": 50, "index": 0 }, "visualization": "line"
    });
    for (key, value) in extra.as_object().unwrap() {
        state[key] = value.clone();
    }
    state
}

/// A pageview or custom event with every rollup dimension set; browser and referrer vary per
/// event so session-scoped values differ from the event's own.
fn event(site: &mut Site, kind: EventKind, at: i64, who: Who, n: usize) -> Record {
    let mut record = site.event_at(kind, at, at, who);
    if let Record::Event(e) = &mut record {
        let d = &mut e.dimensions;
        d.path = [Some("/a"), Some("/b"), None, Some("/c")][n % 4].map(Into::into);
        d.referrer_domain =
            [Some("google.com"), None, Some("news.ycombinator.com")][n % 3].map(Into::into);
        d.browser = Some(["firefox", "chrome", "safari"][n % 3].into());
        d.os = [Some("linux"), Some("macos")][n % 2].map(Into::into);
        d.device = Some(["desktop", "mobile", "tablet", "desktop", "mobile"][n % 5].into());
    }
    record
}

/// Traffic on Vienna days 2026-10-20 to 2026-10-27 (02:00 repeats on 10-25): anonymous
/// visitors, a profile whose sessions cross local midnight, custom events and keyless events.
fn traffic(site: &mut Site) -> Vec<Record> {
    let mut out = Vec::new();
    let mut n = 0;
    for day in 20..=27 {
        let midnight = ms(&format!("2026-10-{day}T00:00:00Z"));
        for v in 0..6 {
            let label = format!("v{v}");
            let who = match v {
                0 => Who::Profile("eve"),
                5 => Who::Keyless,
                _ => Who::Anon(&label),
            };
            // Local hours spread over the day, including the hours around local midnight.
            let start = midnight + ((v * 7 + day * 3) % 26) as i64 * 3_600_000 - 2 * 3_600_000;
            for (k, gap_min) in [0, 12, 50].iter().enumerate().take(1 + (v + day) % 3) {
                n += 1;
                let kind = if (n % 4) == 3 {
                    EventKind::CustomEvent
                } else {
                    EventKind::Pageview
                };
                let at = start + (k as i64 * 7 + gap_min) * 60_000;
                out.push(event(site, kind, at, who, n));
            }
        }
    }
    out
}

async fn loaded() -> (tempfile::TempDir, Storage, Site) {
    let (dir, storage) = open().await;
    let mut site = Site::new(SITE, Vienna);
    jobs::record_policies(&storage, SITE, site.policies.clone())
        .await
        .unwrap();
    ingest(&storage, &traffic(&mut site)).await;
    correct(&storage).await;
    (dir, storage, site)
}

/// Runs a state through rollups and through events at `now`, asserts both serialize to the
/// same bytes, and returns how many rollup days served the current range's totals.
async fn same(storage: &Storage, state: Value, panels: Vec<Panel>, now: &str) -> i64 {
    let state: ExplorationState = serde_json::from_value(state).unwrap();
    let query = validate(&state, &panels).unwrap();
    assert!(servable(&query), "{state:?}");
    let now = ms(now);
    let run = |raw: bool| {
        let query = query.clone();
        async move {
            let result = storage
                .read(Lane::Report, move |s| {
                    let ctx = Context {
                        site: SITE,
                        now_ms: now,
                        hostnames: &[],
                        retention: holocron_protocol::retention::Retention::OneYear,
                    };
                    if raw {
                        explore_raw(s, &query, &ctx)
                    } else {
                        explore(s, &query, &ctx)
                    }
                })
                .await
                .unwrap()
                .value
                .unwrap();
            serde_json::to_value(&result).unwrap()
        }
    };
    let (rollup, raw) = (run(false).await, run(true).await);
    assert_eq!(
        serde_json::to_string(&rollup).unwrap(),
        serde_json::to_string(&raw).unwrap()
    );

    // Each served day adds 1,000 to the pageviews total once its aggregate is bumped.
    bump(storage, 1_000).await;
    let bumped = run(false).await;
    bump(storage, -1_000).await;
    let pageviews = |v: &Value| v["main"]["totals"][0]["v"].as_i64().unwrap();
    (pageviews(&bumped) - pageviews(&raw)) / 1_000
}

async fn bump(storage: &Storage, by: i64) {
    storage
        .write(move |tx| tx.execute("UPDATE day_aggregates SET pageviews = pageviews + ?", [by]))
        .await
        .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn rollups_match_events_in_the_site_timezone() {
    let (_dir, storage, _) = loaded().await;
    // Hourly across the repeated hour of 10-25: that day alone is counted from events.
    let hourly = state(
        "2026-10-24",
        "2026-10-26",
        "Europe/Vienna",
        "hour",
        json!({}),
    );
    assert_eq!(same(&storage, hourly, overview_panels(), NOW).await, 2);
    // The overview: day series, previous period and the five panels.
    let daily = state(
        "2026-10-21",
        "2026-10-27",
        "Europe/Vienna",
        "day",
        json!({}),
    );
    assert_eq!(same(&storage, daily, overview_panels(), NOW).await, 7);
    // Weeks of 10-19 and 10-26; 10-19 and 10-28 have no events and add nothing.
    let weekly = state(
        "2026-10-19",
        "2026-10-28",
        "Europe/Vienna",
        "week",
        json!({}),
    );
    assert_eq!(same(&storage, weekly, vec![], NOW).await, 8);
    // Pages, sorted by visitor-days with ties by path, second page, compared.
    let pages = state(
        "2026-10-20",
        "2026-10-27",
        "Europe/Vienna",
        "day",
        json!({
            "measures": ["pageviews", "visitor_days", "events"], "dimensions": ["path"],
            "sort": [{ "by": "visitor_days", "dir": "desc" }], "page": { "size": 2, "index": 1 },
            "visualization": "table"
        }),
    );
    assert_eq!(same(&storage, pages, vec![], NOW).await, 8);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_range_containing_now_compares_up_to_the_same_offset_from_rollups_and_events() {
    let (_dir, storage, _) = loaded().await;
    // NOW is 11:00 in Vienna on 10-28: the previous period is 10-25 to 10-26 until 11:00 on
    // 10-26, so its last day is read from events and the earlier ones from rollups.
    let days = state(
        "2026-10-27",
        "2026-10-28",
        "Europe/Vienna",
        "day",
        json!({}),
    );
    assert_eq!(same(&storage, days, overview_panels(), NOW).await, 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn another_timezone_counts_boundary_days_from_events_and_visitor_days_in_each_bucket() {
    let (_dir, storage, mut site) = loaded().await;
    // One visitor-day on both sides of UTC midnight: it counts on both UTC days.
    let night = [
        event(
            &mut site,
            EventKind::Pageview,
            ms("2026-10-22T23:40:00Z"),
            Who::Anon("owl"),
            1,
        ),
        event(
            &mut site,
            EventKind::Pageview,
            ms("2026-10-23T00:20:00Z"),
            Who::Anon("owl"),
            2,
        ),
    ];
    ingest(&storage, &night).await;
    correct(&storage).await;
    // UTC days: the first and last Vienna days are partial and counted from events.
    for granularity in ["day", "hour"] {
        let utc = state("2026-10-21", "2026-10-23", "UTC", granularity, json!({}));
        assert_eq!(same(&storage, utc, overview_panels(), NOW).await, 2);
    }
    // Half-hour offsets split UTC-hour cells across Kolkata days: the series reads events.
    let kolkata = state("2026-10-21", "2026-10-26", "Asia/Kolkata", "day", json!({}));
    assert_eq!(same(&storage, kolkata, overview_panels(), NOW).await, 0);
    let kolkata_table = state(
        "2026-10-21",
        "2026-10-26",
        "Asia/Kolkata",
        "day",
        json!({ "visualization": "table" }),
    );
    // Without a series, Vienna's 10-21 to 10-25 lie whole inside the Kolkata range.
    assert_eq!(
        same(&storage, kolkata_table, overview_panels(), NOW).await,
        5
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn dirty_days_are_counted_from_events_until_corrected() {
    let (_dir, storage, mut site) = loaded().await;
    let late = [
        event(
            &mut site,
            EventKind::Pageview,
            ms("2026-10-22T09:00:00Z"),
            Who::Anon("v1"),
            3,
        ),
        event(
            &mut site,
            EventKind::Pageview,
            ms("2026-10-22T09:05:00Z"),
            Who::Anon("late"),
            4,
        ),
    ];
    ingest(&storage, &late).await;
    let range = || {
        state(
            "2026-10-21",
            "2026-10-23",
            "Europe/Vienna",
            "hour",
            json!({}),
        )
    };
    assert_eq!(same(&storage, range(), overview_panels(), NOW).await, 2);
    correct(&storage).await;
    assert_eq!(same(&storage, range(), overview_panels(), NOW).await, 3);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_day_with_events_but_no_rollups_sends_the_request_to_events() {
    let (_dir, storage, _) = loaded().await;
    storage
        .write(|tx| {
            tx.execute(
                "DELETE FROM day_aggregates WHERE local_day = DATE '2026-10-22'",
                [],
            )
        })
        .await
        .unwrap();
    // The day looks empty, so the count of the range's events differs and nothing is served.
    let range = state(
        "2026-10-21",
        "2026-10-23",
        "Europe/Vienna",
        "day",
        json!({}),
    );
    assert_eq!(same(&storage, range, overview_panels(), NOW).await, 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_profile_session_crossing_midnight_keeps_the_next_days_session_fields() {
    let (_dir, storage) = open().await;
    let mut site = Site::new(SITE, Vienna);
    jobs::record_policies(&storage, SITE, site.policies.clone())
        .await
        .unwrap();
    // A late first event on 10-23 moves the start of zed's only session, which crosses into
    // 10-24, so 10-24's browser and referrer cells change while only 10-23 is dirty and no
    // session starts on 10-24.
    let night = [
        event(
            &mut site,
            EventKind::Pageview,
            ms("2026-10-23T21:50:00Z"),
            Who::Profile("zed"),
            0,
        ),
        event(
            &mut site,
            EventKind::Pageview,
            ms("2026-10-23T22:10:00Z"),
            Who::Profile("zed"),
            1,
        ),
    ];
    ingest(&storage, &night).await;
    correct(&storage).await;
    let earlier = [event(
        &mut site,
        EventKind::Pageview,
        ms("2026-10-23T21:30:00Z"),
        Who::Profile("zed"),
        2,
    )];
    ingest(&storage, &earlier).await;
    correct(&storage).await;
    let next_day = state(
        "2026-10-24",
        "2026-10-24",
        "Europe/Vienna",
        "day",
        json!({}),
    );
    assert_eq!(same(&storage, next_day, overview_panels(), NOW).await, 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_timezone_change_day_is_counted_from_events() {
    let (_dir, storage, mut site) = loaded().await;
    // Vienna to UTC at 2026-10-26T23:30Z: both epochs have the dates 10-26 and 10-27. Vienna's
    // 10-25 and UTC's 10-28 are served.
    let change = ms("2026-10-26T23:30:00Z");
    site.policies
        .change(
            SessionPolicy::new(chrono_tz::UTC, 30).unwrap(),
            change,
            change,
        )
        .unwrap();
    jobs::record_policies(&storage, SITE, site.policies.clone())
        .await
        .unwrap();
    let after = [
        event(
            &mut site,
            EventKind::Pageview,
            ms("2026-10-27T08:00:00Z"),
            Who::Anon("v2"),
            1,
        ),
        event(
            &mut site,
            EventKind::Pageview,
            ms("2026-10-28T08:00:00Z"),
            Who::Anon("v2"),
            2,
        ),
    ];
    ingest(&storage, &after).await;
    correct(&storage).await;
    let across = state("2026-10-24", "2026-10-28", "UTC", "day", json!({}));
    assert_eq!(
        same(&storage, across, overview_panels(), "2026-10-29T10:00:00Z").await,
        2
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn local_days_stamped_under_another_timezone_are_counted_from_events() {
    // Collection stamped Vienna days, but analytics has no policy copy and assumes UTC.
    let (_dir, storage) = open().await;
    let mut site = Site::new(SITE, Vienna);
    ingest(&storage, &traffic(&mut site)).await;
    correct(&storage).await;
    let utc = state("2026-10-21", "2026-10-26", "UTC", "day", json!({}));
    assert_eq!(same(&storage, utc, overview_panels(), NOW).await, 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_retention_cutoff_drops_rollup_days_before_it() {
    let (_dir, storage, _) = loaded().await;
    // On 2027-10-23 the cutoff is local 2026-10-23: three of the range's days are retained.
    let range = state(
        "2026-10-20",
        "2026-10-25",
        "Europe/Vienna",
        "day",
        json!({}),
    );
    assert_eq!(
        same(&storage, range, overview_panels(), "2027-10-23T10:00:00Z").await,
        3
    );
}

#[test]
fn states_outside_the_rollups_use_the_raw_planner() {
    let query = |extra: Value, panels: Vec<Panel>| {
        let state: ExplorationState =
            serde_json::from_value(state("2026-10-20", "2026-10-27", "UTC", "day", extra)).unwrap();
        validate(&state, &panels).unwrap()
    };
    assert!(servable(&query(json!({}), overview_panels())));
    let filter = json!({ "filter": { "kind": "group", "op": "and", "items": [
        { "kind": "cond", "id": "f1", "field": "path", "cmp": "eq", "value": { "t": "string", "v": "/a" } }
    ] } });
    let refused = [
        filter,
        json!({ "measures": ["identified_people"] }),
        json!({ "measures": ["sessions"], "dimensions": ["path"], "visualization": "table" }),
        json!({ "measures": ["pageviews"], "dimensions": ["path", "browser"], "visualization": "table" }),
        json!({ "measures": ["pageviews"], "dimensions": ["country"], "visualization": "table" }),
        json!({ "measures": ["pageviews"], "dimensions": [{ "scope": "event", "key": "plan" }], "visualization": "table" }),
        json!({ "measures": [{ "field": { "scope": "event", "key": "amount" }, "stat": "sum" }] }),
    ];
    for extra in refused {
        assert!(!servable(&query(extra.clone(), vec![])), "{extra}");
    }
    // One panel outside the rollups sends the whole request to the raw planner.
    let mut panels = overview_panels();
    panels[4].dimensions = vec![serde_json::from_value(json!("country")).unwrap()];
    assert!(!servable(&query(json!({}), panels)));
}
