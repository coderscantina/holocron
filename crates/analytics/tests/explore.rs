//! Exploration planner behaviour beyond the correctness fixture: include and exclude
//! partitions, grouped and repeated conditions, session fields, paging and series.

mod common;

use chrono_tz::Europe::Vienna;
use common::{Site, Who, correct, ingest, ms, open};
use holocron_analytics::explore::{
    Context, Refusal, StateLimits, explore, validate, validate_with,
};
use holocron_protocol::exploration::{Cell, ExplorationState, ExploreResult, Metric, Panel};
use holocron_storage::{EventKind, Lane, Record, Storage};
use serde_json::{Value, json};

const SITE: u128 = 0x5a30_3801;

fn pageview(site: &mut Site, at: &str, who: Who, path: &str, country: &str) -> Record {
    let mut record = site.event(EventKind::Pageview, at, who);
    if let Record::Event(event) = &mut record {
        event.dimensions.path = Some(path.into());
        event.dimensions.country = Some(country.into());
    }
    record
}

fn state(measures: Value, dimensions: Value, filter: Value) -> Value {
    json!({
        "v": 1, "site": "s", "measures": measures, "dimensions": dimensions, "filter": filter,
        "range": { "kind": "days", "start": "2026-09-01", "end": "2026-09-01" }, "timezone": "Europe/Vienna",
        "granularity": "hour", "compare": "none", "metric_version": 1, "sort": [], "page": { "size": 50, "index": 0 },
        "visualization": "table"
    })
}

async fn run(storage: &Storage, state: Value, panels: Vec<Panel>) -> ExploreResult {
    let state: ExplorationState = serde_json::from_value(state).unwrap();
    let query = validate(&state, &panels).unwrap();
    storage
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
        .unwrap()
}

fn count(metric: &Metric) -> i64 {
    match metric {
        Metric::Count { v } => *v,
        other => panic!("{other:?}"),
    }
}

fn cond(id: &str, field: &str, cmp: &str, value: Option<&str>) -> Value {
    let mut c = json!({ "kind": "cond", "id": id, "field": field, "cmp": cmp });
    if let Some(v) = value {
        c["value"] = json!({ "t": "string", "v": v });
    }
    c
}

fn group(op: &str, items: Vec<Value>) -> Value {
    json!({ "kind": "group", "op": op, "items": items })
}

/// Four visitors on 2026-09-01: paths /a (3), /b (2), /c (1) and one pageview without a path.
async fn loaded() -> (tempfile::TempDir, Storage) {
    let (dir, storage) = open().await;
    let mut site = Site::new(SITE, Vienna);
    let mut records = vec![
        pageview(
            &mut site,
            "2026-09-01T08:00:00Z",
            Who::Anon("v1"),
            "/a",
            "AT",
        ),
        pageview(
            &mut site,
            "2026-09-01T08:05:00Z",
            Who::Anon("v1"),
            "/b",
            "DE",
        ),
        pageview(
            &mut site,
            "2026-09-01T09:00:00Z",
            Who::Anon("v2"),
            "/a",
            "DE",
        ),
        pageview(
            &mut site,
            "2026-09-01T09:01:00Z",
            Who::Anon("v2"),
            "/a",
            "DE",
        ),
        pageview(
            &mut site,
            "2026-09-01T12:00:00Z",
            Who::Anon("v3"),
            "/b",
            "AT",
        ),
        pageview(
            &mut site,
            "2026-09-01T12:30:00Z",
            Who::Anon("v4"),
            "/c",
            "AT",
        ),
    ];
    let mut bare = site.event(EventKind::Pageview, "2026-09-01T12:45:00Z", Who::Anon("v4"));
    if let Record::Event(event) = &mut bare {
        event.dimensions.path = None;
    }
    records.push(bare);
    ingest(&storage, &records).await;
    correct(&storage).await;
    (dir, storage)
}

#[tokio::test(flavor = "multi_thread")]
async fn include_and_exclude_partition_the_total_and_repeated_conditions_combine() {
    let (_dir, storage) = loaded().await;
    let total = run(
        &storage,
        state(json!(["pageviews"]), json!([]), Value::Null),
        vec![],
    )
    .await;
    let include = run(
        &storage,
        state(
            json!(["pageviews"]),
            json!([]),
            group("and", vec![cond("f1", "path", "eq", Some("/a"))]),
        ),
        vec![],
    )
    .await;
    let exclude = run(
        &storage,
        state(
            json!(["pageviews"]),
            json!([]),
            group("and", vec![cond("f1", "path", "neq", Some("/a"))]),
        ),
        vec![],
    )
    .await;
    assert_eq!(count(&total.main.totals[0]), 7);
    assert_eq!(
        count(&include.main.totals[0]) + count(&exclude.main.totals[0]),
        7
    );
    // The pageview without a path is in neither `/a` cell nor lost: not set is its own cell.
    let not_set = run(
        &storage,
        state(
            json!(["pageviews"]),
            json!([]),
            group("and", vec![cond("f1", "path", "is_not_set", None)]),
        ),
        vec![],
    )
    .await;
    assert_eq!(count(&not_set.main.totals[0]), 1);

    // Repeated conditions on one field inside an OR group, ANDed with another field.
    let filter = group(
        "and",
        vec![
            group(
                "or",
                vec![
                    cond("f1", "path", "eq", Some("/a")),
                    cond("f2", "path", "eq", Some("/b")),
                ],
            ),
            cond("f3", "path", "neq", Some("/b")),
        ],
    );
    let combined = run(
        &storage,
        state(json!(["pageviews"]), json!([]), filter),
        vec![],
    )
    .await;
    assert_eq!(count(&combined.main.totals[0]), 3);
}

#[tokio::test(flavor = "multi_thread")]
async fn matches_and_not_matches_partition_the_total() {
    let (_dir, storage) = loaded().await;
    let total = |cmp: &str, glob: &str| {
        let filter = group("and", vec![cond("f1", "path", cmp, Some(glob))]);
        run(
            &storage,
            state(json!(["pageviews"]), json!([]), filter),
            vec![],
        )
    };
    let matched = count(&total("matches", "/a**").await.main.totals[0]);
    let rest = count(&total("not_matches", "/a**").await.main.totals[0]);
    // The pageview without a path is in the complement.
    assert_eq!((matched, rest), (3, 4));
}

#[test]
fn matches_needs_a_glob_field_and_lists_take_the_caller_limit() {
    let refused = |filter: Value, limits: StateLimits| {
        let state: ExplorationState =
            serde_json::from_value(state(json!(["pageviews"]), json!([]), filter)).unwrap();
        validate_with(&state, &[], limits).map_err(|r| (r.code.as_str().to_owned(), r.field))
    };
    let one =
        |field: &str, glob: &str| group("and", vec![cond("f1", field, "matches", Some(glob))]);
    let invalid = Err(("invalid_arguments".to_owned(), "filter.f1".to_owned()));
    assert!(refused(one("title", "*Pricing*"), StateLimits::EXPLORATION).is_ok());
    let property = json!({ "kind": "group", "op": "and", "items": [{ "kind": "cond", "id": "f1",
        "field": { "scope": "session", "key": "plan" }, "cmp": "not_matches", "value": { "t": "string", "v": "pro-*" } }] });
    assert!(refused(property, StateLimits::EXPLORATION).is_ok());
    assert_eq!(
        refused(one("event_name", "sign*"), StateLimits::EXPLORATION).map(drop),
        invalid
    );
    assert_eq!(
        refused(one("path", &"*a".repeat(9)), StateLimits::EXPLORATION).map(drop),
        invalid
    );

    let list = |n: usize| {
        let items: Vec<Value> = (0..n)
            .map(|i| json!({ "t": "string", "v": format!("/{i}") }))
            .collect();
        json!({ "kind": "group", "op": "and", "items": [{ "kind": "cond", "id": "f1", "field": "path",
            "cmp": "not_in", "value": { "t": "list", "v": items } }] })
    };
    assert!(refused(list(50), StateLimits::EXPLORATION).is_ok());
    assert!(refused(list(51), StateLimits::EXPLORATION).is_err());
    assert!(refused(list(1_000), StateLimits::REPORT_QUERY).is_ok());
    assert!(refused(list(1_001), StateLimits::REPORT_QUERY).is_err());
}

#[tokio::test(flavor = "multi_thread")]
async fn session_fields_come_from_the_session_and_sessions_split_into_every_cell() {
    let (_dir, storage) = loaded().await;
    // v1's session starts in AT, so its DE pageview counts in AT.
    let by_country = run(
        &storage,
        state(
            json!(["pageviews", "sessions"]),
            json!(["country"]),
            Value::Null,
        ),
        vec![],
    )
    .await;
    let cells: Vec<(Cell, i64, i64)> = by_country
        .main
        .rows
        .iter()
        .map(|r| (r.key[0].clone(), count(&r.values[0]), count(&r.values[1])))
        .collect();
    assert_eq!(
        cells,
        vec![
            (Cell::String("AT".into()), 5, 3),
            (Cell::String("DE".into()), 2, 1)
        ]
    );

    // v1 visited /a and /b, so its one session is in both cells; the total counts it once.
    let by_path = run(
        &storage,
        state(json!(["sessions"]), json!(["path"]), Value::Null),
        vec![],
    )
    .await;
    let sessions: Vec<(Cell, i64)> = by_path
        .main
        .rows
        .iter()
        .map(|r| (r.key[0].clone(), count(&r.values[0])))
        .collect();
    assert_eq!(
        sessions,
        vec![
            (Cell::String("/a".into()), 2),
            (Cell::String("/b".into()), 2),
            (Cell::String("/c".into()), 1),
            (Cell::NotSet, 1)
        ]
    );
    assert_eq!(count(&by_path.main.totals[0]), 4);
}

/// v1 enters on `/de/start` and moves on to `/en/x`; v2 has only a custom event on `/de/promo`;
/// v3 fires a custom event on `/x` before its first pageview `/y`.
async fn entered() -> (tempfile::TempDir, Storage) {
    let (dir, storage) = open().await;
    let mut site = Site::new(SITE, Vienna);
    let mut custom = |site: &mut Site, at: &str, who: &'static str, path: &str| {
        let mut record = site.event(EventKind::CustomEvent, at, Who::Anon(who));
        if let Record::Event(event) = &mut record {
            event.dimensions.path = Some(path.into());
        }
        record
    };
    let records = vec![
        pageview(
            &mut site,
            "2026-09-01T08:00:00Z",
            Who::Anon("v1"),
            "/de/start",
            "AT",
        ),
        pageview(
            &mut site,
            "2026-09-01T08:05:00Z",
            Who::Anon("v1"),
            "/en/x",
            "AT",
        ),
        custom(&mut site, "2026-09-01T09:00:00Z", "v2", "/de/promo"),
        custom(&mut site, "2026-09-01T10:00:00Z", "v3", "/x"),
        pageview(
            &mut site,
            "2026-09-01T10:05:00Z",
            Who::Anon("v3"),
            "/y",
            "AT",
        ),
    ];
    ingest(&storage, &records).await;
    correct(&storage).await;
    (dir, storage)
}

#[tokio::test(flavor = "multi_thread")]
async fn entry_path_is_the_first_pageview_else_the_first_event() {
    let (_dir, storage) = entered().await;
    let result = run(
        &storage,
        state(json!(["sessions"]), json!(["entry_path"]), Value::Null),
        vec![],
    )
    .await;
    let cells: Vec<(Cell, i64)> = result
        .main
        .rows
        .iter()
        .map(|r| (r.key[0].clone(), count(&r.values[0])))
        .collect();
    assert_eq!(
        cells,
        vec![
            (Cell::String("/de/promo".into()), 1),
            (Cell::String("/de/start".into()), 1),
            (Cell::String("/y".into()), 1)
        ]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn entry_path_takes_glob_filters_and_covers_the_whole_session() {
    let (_dir, storage) = entered().await;
    let filter = group(
        "and",
        vec![cond("f1", "entry_path", "matches", Some("/de/**"))],
    );
    let result = run(
        &storage,
        state(json!(["pageviews", "sessions"]), json!([]), filter),
        vec![],
    )
    .await;
    // v1's second pageview is on `/en/x` but its session entered on `/de/start`.
    assert_eq!(
        (count(&result.main.totals[0]), count(&result.main.totals[1])),
        (2, 2)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn rows_page_after_sorting_with_ties_broken_by_dimension_values() {
    let (_dir, storage) = loaded().await;
    let mut paged = state(json!(["pageviews"]), json!(["path"]), Value::Null);
    paged["sort"] = json!([{ "by": "pageviews", "dir": "desc" }]);
    paged["page"] = json!({ "size": 2, "index": 1 });
    let result = run(&storage, paged.clone(), vec![]).await;
    let keys: Vec<Cell> = result.main.rows.iter().map(|r| r.key[0].clone()).collect();
    // /a 3, /b 2, then /c and not set tie at 1: values before not set.
    assert_eq!(keys, vec![Cell::String("/c".into()), Cell::NotSet]);
    assert_eq!(
        (result.main.row_count, count(&result.main.totals[0])),
        (4, 7)
    );

    paged["page"] = json!({ "size": 2, "index": 5 });
    let beyond = run(&storage, paged, vec![]).await;
    assert_eq!((beyond.main.rows.len(), beyond.main.row_count), (0, 4));
}

#[tokio::test(flavor = "multi_thread")]
async fn series_and_panels_describe_the_same_rows_as_the_totals() {
    let (_dir, storage) = loaded().await;
    let mut line = state(
        json!(["pageviews", "visitor_days"]),
        json!([]),
        group("and", vec![cond("f1", "country", "eq", Some("AT"))]),
    );
    line["visualization"] = json!("line");
    let panel = Panel {
        measures: serde_json::from_value(json!(["pageviews"])).unwrap(),
        dimensions: serde_json::from_value(json!(["path"])).unwrap(),
        sort: Vec::new(),
        page: holocron_analytics::explore::DEFAULT_PAGE,
    };
    let result = run(&storage, line, vec![panel]).await;
    let series = result.main.series.as_ref().unwrap();
    assert_eq!(series.points.len(), 24);
    assert_eq!(series.points[10].local, "2026-09-01T10:00");
    let summed: i64 = series.points.iter().map(|p| count(&p.values[0])).sum();
    assert_eq!(summed, count(&result.main.totals[0]));
    let panel_sum: i64 = result.panels[0]
        .rows
        .iter()
        .map(|r| count(&r.values[0]))
        .sum();
    assert_eq!(panel_sum, count(&result.main.totals[0]));
    assert!(!series.series_sum_differs);
}

#[tokio::test(flavor = "multi_thread")]
async fn typical_weekday_refuses_measures_without_a_baseline_and_ranges_over_31_days() {
    let typical = |measures: Value, filter: Value, start: &str| {
        let mut state = state(measures, json!([]), filter);
        state["compare"] = json!("typical_weekday");
        state["range"]["start"] = json!(start);
        serde_json::from_value::<ExplorationState>(state).unwrap()
    };
    let refused = |refusal: Refusal| (refusal.code.as_str(), refusal.field);
    let unsupported = ("compare_unsupported", "compare".to_owned());
    let perf = group("and", vec![cond("p1", "perf_metric", "eq", Some("LCP"))]);
    let property = json!({ "stat": "sum", "field": { "scope": "event", "key": "plan" } });
    for (measures, filter) in [
        (json!(["p75"]), perf),
        (json!(["pageviews", "identified_people"]), Value::Null),
        (json!(["pageviews", property]), Value::Null),
    ] {
        let state = typical(measures, filter, "2026-09-01");
        assert_eq!(
            refused(validate(&state, &[]).unwrap_err()),
            unsupported,
            "{state:?}"
        );
    }
    let counts = json!([
        "pageviews",
        "events",
        "visitors",
        "visitor_days",
        "sessions",
        "revenue_count",
        "bounce_rate",
        "span_mean"
    ]);
    let accepted = typical(counts, Value::Null, "2026-09-01");
    validate(&accepted, &[]).unwrap();
    let money = json!(["revenue_net", "revenue_gross", "revenue_refunds"]);
    validate(&typical(money, Value::Null, "2026-09-01"), &[]).unwrap();
    let people: Panel = serde_json::from_value(
        json!({ "measures": ["identified_people"], "dimensions": [], "sort": [], "page": { "size": 50, "index": 0 } }),
    )
    .unwrap();
    assert_eq!(
        refused(validate(&accepted, &[people]).unwrap_err()),
        unsupported
    );

    // The range limit needs the resolved range; a state within it has its baseline.
    let (_dir, storage) = loaded().await;
    let outcome = |start: &str| {
        let query = validate(&typical(json!(["span_median"]), Value::Null, start), &[]).unwrap();
        storage.read(Lane::Report, move |s| {
            let context = Context {
                site: SITE,
                now_ms: ms("2026-09-02T10:00:00Z"),
                hostnames: &[],
                retention: holocron_protocol::retention::Retention::OneYear,
            };
            explore(s, &query, &context)
        })
    };
    let over = outcome("2026-08-01").await.unwrap().value.unwrap_err();
    assert_eq!(refused(over), unsupported);
    let within = outcome("2026-08-02").await.unwrap().value.unwrap();
    assert!(within.main.compare.is_some());
}
