//! Funnel groups and step durations (0022 Funnel v2 and glob matching) on a hand-built day.

mod common;

use chrono_tz::Europe::Vienna;
use common::{Site, Who, correct, ingest, ms, open};
use holocron_analytics::explore::{Context, explore, validate};
use holocron_protocol::exploration::{
    Cell, ExplorationState, FunnelResult, Metric, Reason, ReportResult, StepDuration,
};
use holocron_storage::{EventKind, Lane, Record, Storage};
use serde_json::{Value, json};

const SITE: u128 = 0x5a30_3802;

fn pageview(site: &mut Site, at: &str, who: &str, path: &str, country: Option<&str>) -> Record {
    let mut record = site.event(EventKind::Pageview, at, Who::Anon(who));
    if let Record::Event(event) = &mut record {
        event.dimensions.path = Some(path.into());
        event.dimensions.country = country.map(Into::into);
    }
    record
}

fn cond(id: &str, field: &str, cmp: &str, value: &str) -> Value {
    json!({ "kind": "cond", "id": id, "field": field, "cmp": cmp, "value": { "t": "string", "v": value } })
}

fn step(c: Value) -> Value {
    json!({ "kind": "group", "op": "and", "items": [c] })
}

fn funnel(report: Value) -> ExplorationState {
    serde_json::from_value(json!({
        "v": 1, "site": "s", "measures": [], "dimensions": [], "filter": null,
        "range": { "kind": "days", "start": "2026-09-01", "end": "2026-09-01" }, "timezone": "Europe/Vienna",
        "granularity": "day", "compare": "none", "metric_version": 1, "sort": [], "page": { "size": 50, "index": 0 },
        "visualization": "funnel", "report": report
    }))
    .unwrap()
}

async fn run(storage: &Storage, state: ExplorationState) -> FunnelResult {
    let query = validate(&state, &[]).unwrap();
    let result = storage
        .read(Lane::Report, move |s| {
            let context = Context {
                site: SITE,
                now_ms: ms("2026-09-02T10:00:00Z"),
                hostnames: &[],
                retention: holocron_protocol::retention::Retention::OneYear,
            };
            explore(s, &query, &context)
        })
        .await
        .unwrap()
        .value
        .unwrap();
    match result.report {
        Some(ReportResult::Funnel(funnel)) => funnel,
        other => panic!("{other:?}"),
    }
}

fn reached(steps: &[holocron_protocol::exploration::FunnelStep]) -> Vec<i64> {
    steps.iter().map(|s| s.reached).collect()
}

fn millis(v: i64) -> Metric {
    Metric::Millis { v }
}

fn duration(from: u8, to: u8, quantiles: [i64; 3], buckets: [i64; 5]) -> StepDuration {
    StepDuration {
        from_step: from,
        to_step: to,
        p25: millis(quantiles[0]),
        median: millis(quantiles[1]),
        p75: millis(quantiles[2]),
        buckets,
    }
}

/// `/a`, then `/b`, then a signup, by five visitors in three countries (one without).
async fn steps_day() -> (tempfile::TempDir, Storage) {
    let (dir, storage) = open().await;
    let mut site = Site::new(SITE, Vienna);
    let mut records = vec![
        pageview(&mut site, "2026-09-01T08:00:00Z", "v1", "/a", Some("AT")),
        pageview(&mut site, "2026-09-01T08:00:30Z", "v1", "/b", Some("AT")),
        pageview(&mut site, "2026-09-01T09:00:00Z", "v2", "/a", Some("DE")),
        pageview(&mut site, "2026-09-01T10:30:00Z", "v2", "/b", Some("DE")),
        pageview(&mut site, "2026-09-01T09:10:00Z", "v3", "/a", Some("DE")),
        pageview(&mut site, "2026-09-01T09:20:00Z", "v3", "/b", Some("DE")),
        pageview(&mut site, "2026-09-01T11:00:00Z", "v4", "/a", Some("AT")),
        pageview(&mut site, "2026-09-01T12:00:00Z", "v5", "/a", None),
        pageview(&mut site, "2026-09-01T12:00:01Z", "v5", "/b", None),
    ];
    records.push(site.event(
        EventKind::CustomEvent,
        "2026-09-01T08:10:00Z",
        Who::Anon("v1"),
    ));
    records.push(site.event(
        EventKind::CustomEvent,
        "2026-09-01T12:30:00Z",
        Who::Anon("v2"),
    ));
    ingest(&storage, &records).await;
    correct(&storage).await;
    (dir, storage)
}

fn three_steps(v: u8, group_by: Option<&str>) -> Value {
    let mut report = json!({ "kind": "funnel", "v": v, "window_seconds": 86_400, "scope": "daily", "steps": [
        step(cond("s1", "path", "eq", "/a")),
        step(cond("s2", "path", "eq", "/b")),
        step(cond("s3", "event_name", "eq", "signup")),
    ] });
    if let Some(field) = group_by {
        report["group_by"] = json!(field);
    }
    report
}

#[tokio::test(flavor = "multi_thread")]
async fn durations_are_timed_from_the_previous_step_and_from_the_first() {
    let (_dir, storage) = steps_day().await;
    let result = run(&storage, funnel(three_steps(1, None))).await;
    assert_eq!(reached(&result.steps), [5, 4, 2]);
    assert!(result.groups.is_empty() && !result.groups_truncated);
    // To /b: 1 s, 30 s, 10 min and 1.5 h. To the signup: 9.5 min and 2 h from /b,
    // 10 min and 3.5 h from /a.
    assert_eq!(
        result.durations,
        [
            duration(0, 1, [1_000, 30_000, 600_000], [3, 1, 0, 0, 0]),
            duration(1, 2, [570_000, 570_000, 7_200_000], [1, 1, 0, 0, 0]),
            duration(0, 2, [600_000, 600_000, 12_600_000], [1, 1, 0, 0, 0]),
        ]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn groups_split_the_funnel_by_the_entering_event_and_add_up_to_it() {
    let (_dir, storage) = steps_day().await;
    let overall = run(&storage, funnel(three_steps(1, None))).await;
    let grouped = run(&storage, funnel(three_steps(2, Some("country")))).await;
    assert_eq!(grouped.steps, overall.steps);
    assert_eq!(grouped.durations, overall.durations);
    assert_eq!(grouped.leaks, overall.leaks);
    assert!(!grouped.groups_truncated);

    let groups: Vec<(Cell, Vec<i64>)> = grouped
        .groups
        .iter()
        .map(|g| (g.value.clone(), reached(&g.steps)))
        .collect();
    // AT and DE tie at 2 entering and order by value; not set comes last.
    assert_eq!(
        groups,
        [
            (Cell::String("AT".into()), vec![2, 1, 1]),
            (Cell::String("DE".into()), vec![2, 2, 1]),
            (Cell::NotSet, vec![1, 1, 0]),
        ]
    );
    for step in 0..3 {
        let sum: i64 = grouped.groups.iter().map(|g| g.steps[step].reached).sum();
        assert_eq!(sum, overall.steps[step].reached);
    }
    let unset = &grouped.groups[2];
    assert_eq!(
        unset.durations[0],
        duration(0, 1, [1_000, 1_000, 1_000], [1, 0, 0, 0, 0])
    );
    let nobody = Metric::Unavailable {
        reason: Reason::NoValues,
    };
    assert_eq!(
        unset.durations[1],
        StepDuration {
            from_step: 1,
            to_step: 2,
            p25: nobody.clone(),
            median: nobody.clone(),
            p75: nobody,
            buckets: [0; 5],
        }
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn groups_can_be_the_session_entry_path() {
    let (_dir, storage) = steps_day().await;
    let grouped = run(&storage, funnel(three_steps(2, Some("entry_path")))).await;
    // Every session entered on `/a`, the first step.
    let groups: Vec<(Cell, Vec<i64>)> = grouped
        .groups
        .iter()
        .map(|g| (g.value.clone(), reached(&g.steps)))
        .collect();
    assert_eq!(groups, [(Cell::String("/a".into()), vec![5, 4, 2])]);
}

#[tokio::test(flavor = "multi_thread")]
async fn at_most_fifty_groups_are_returned() {
    let (_dir, storage) = open().await;
    let mut site = Site::new(SITE, Vienna);
    let mut records: Vec<Record> = (0..51)
        .map(|i| {
            pageview(
                &mut site,
                "2026-09-01T08:00:00Z",
                &format!("v{i}"),
                &format!("/p/{i}"),
                None,
            )
        })
        .collect();
    records.push(pageview(
        &mut site,
        "2026-09-01T09:00:00Z",
        "extra",
        "/p/0",
        None,
    ));
    ingest(&storage, &records).await;
    correct(&storage).await;

    let report = json!({ "kind": "funnel", "v": 2, "window_seconds": 3600, "scope": "daily", "group_by": "path", "steps": [
        step(cond("s1", "path", "matches", "/p/*")),
        step(cond("s2", "event_name", "eq", "signup")),
    ] });
    let result = run(&storage, funnel(report)).await;
    assert_eq!(result.steps[0].reached, 52);
    assert!(result.groups_truncated);
    assert_eq!(result.groups.len(), 50);
    assert_eq!(
        (&result.groups[0].value, result.groups[0].steps[0].reached),
        (&Cell::String("/p/0".into()), 2)
    );
    // The rest tie at 1 and order by value, so `/p/9` sorts last and is left out.
    assert_eq!(result.groups[1].value, Cell::String("/p/1".into()));
    assert_eq!(result.groups[49].value, Cell::String("/p/8".into()));
}
