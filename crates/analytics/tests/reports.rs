//! Report definitions in the exploration state (0004 Report definitions): how each kind uses
//! the rest of the state, its limits, and predicate trees checked together with the filter.

mod common;

use common::{ms, open};
use holocron_analytics::explore::{Context, Refusal, explore, validate};
use holocron_protocol::exploration::ExplorationState;
use holocron_storage::Lane;
use serde_json::{Value, json};

fn cond(id: &str, field: &str, value: &str) -> Value {
    json!({ "kind": "cond", "id": id, "field": field, "cmp": "eq", "value": { "t": "string", "v": value } })
}

fn group(items: Vec<Value>) -> Value {
    json!({ "kind": "group", "op": "and", "items": items })
}

/// `n` path conditions with IDs `{prefix}0` and up.
fn conds(prefix: &str, n: usize) -> Vec<Value> {
    (0..n)
        .map(|i| cond(&format!("{prefix}{i}"), "path", "/"))
        .collect()
}

/// A group nested `depth` levels deep around one condition.
fn nested(id: &str, depth: usize) -> Value {
    (1..depth).fold(group(vec![cond(id, "path", "/")]), |inner, _| {
        group(vec![inner])
    })
}

fn with_report(report: Value, visualization: &str) -> Value {
    json!({
        "v": 1, "site": "s", "measures": [], "dimensions": [], "filter": null,
        "range": { "kind": "days", "start": "2026-09-01", "end": "2026-09-07" }, "timezone": "Europe/Vienna",
        "granularity": "day", "compare": "none", "metric_version": 1, "sort": [], "page": { "size": 50, "index": 0 },
        "visualization": visualization, "report": report
    })
}

fn funnel() -> Value {
    with_report(
        json!({ "kind": "funnel", "v": 1, "window_seconds": 3600, "scope": "daily", "steps": [
            group(vec![cond("s1", "path", "/pricing")]),
            group(vec![cond("s2", "event_name", "signup")]),
        ] }),
        "funnel",
    )
}

fn attribution() -> Value {
    let goal = json!({ "kind": "goal", "goal": {
        "predicate": group(vec![cond("g1", "event_name", "signup")]),
        "scope": "session", "counting": "every", "ref": { "id": "goal-1", "version": 2 }
    } });
    with_report(
        json!({ "kind": "attribution", "v": 1, "model": "last_touch", "lookback_days": 30, "scope": "daily",
            "touch_field": "utm_campaign", "conversions": [goal, { "kind": "revenue" }] }),
        "bar",
    )
}

fn retention() -> Value {
    let mut state = with_report(
        json!({ "kind": "retention", "v": 1, "intervals": 3 }),
        "cohort",
    );
    state["granularity"] = json!("week");
    state
}

fn session_list() -> Value {
    let mut state = with_report(
        json!({ "kind": "session_list", "v": 1, "columns": ["session_id", "start", "span"], "order": "start_desc" }),
        "table",
    );
    state["page"] = json!({ "size": 20, "index": 3 });
    state
}

fn journey() -> Value {
    with_report(
        json!({ "kind": "journey", "v": 1, "value": "path", "length": 7, "scope": "session" }),
        "journey",
    )
}

fn goals() -> Value {
    let goal = json!({ "predicate": group(vec![cond("g1", "event_name", "signup")]),
        "scope": "session", "counting": "once", "ref": { "id": "goal-1", "version": 1 } });
    with_report(json!({ "kind": "goals", "v": 1, "goals": [goal] }), "table")
}

fn parse(state: &Value) -> ExplorationState {
    serde_json::from_value(state.clone()).unwrap()
}

fn refusal(state: &Value) -> Refusal {
    validate(&parse(state), &[]).expect_err(&state.to_string())
}

/// Sets `value` at a `/`-separated path of the state.
fn set(mut state: Value, path: &str, value: Value) -> Value {
    *state.pointer_mut(path).unwrap() = value;
    state
}

fn assert_refused(cases: Vec<(Value, &str, &str)>) {
    for (state, code, field) in cases {
        let refusal = refusal(&state);
        assert_eq!(
            (refusal.code.as_str(), refusal.field.as_str()),
            (code, field),
            "{state}"
        );
    }
}

#[test]
fn a_state_of_every_kind_validates_without_measures_series_or_panels() {
    for state in [
        funnel(),
        journey(),
        attribution(),
        retention(),
        session_list(),
        goals(),
    ] {
        let query = validate(&parse(&state), &[]).unwrap();
        assert!(query.report.is_some());
        assert!(query.main.measures.is_empty() && !query.series);
    }
    let query = validate(&parse(&session_list()), &[]).unwrap();
    assert_eq!((query.main.offset, query.main.size), (60, 20));
}

#[test]
fn a_plain_state_without_report_serializes_byte_identically() {
    let text = r#"{"v":1,"site":"0d2c9a6e-5a3b-4f1e-9b1c-7e2f1a4d8c01","measures":["events","visitor_days"],"dimensions":["country","referrer_domain"],"filter":{"kind":"group","op":"and","items":[{"kind":"cond","id":"f1","field":"device","cmp":"eq","value":{"t":"string","v":"mobile"}},{"kind":"cond","id":"f3","field":{"scope":"event","key":"plan"},"cmp":"eq","value":{"t":"string","v":"pro"}}]},"range":{"kind":"relative","unit":"day","count":30},"timezone":"Europe/Vienna","granularity":"day","compare":"previous_period","metric_version":1,"sort":[{"by":"events","dir":"desc"}],"page":{"size":50,"index":0},"visualization":"table"}"#;
    let state: ExplorationState = serde_json::from_str(text).unwrap();
    assert_eq!(state.report, None);
    assert_eq!(serde_json::to_string(&state).unwrap(), text);
}

#[test]
fn the_rest_of_the_state_must_match_the_kind() {
    let unsupported = "report_field_unsupported";
    assert_refused(vec![
        (
            set(funnel(), "/measures", json!(["pageviews"])),
            unsupported,
            "measures",
        ),
        (
            set(journey(), "/dimensions", json!(["path"])),
            unsupported,
            "dimensions",
        ),
        (
            set(attribution(), "/visualization", json!("line")),
            unsupported,
            "visualization",
        ),
        (
            set(funnel(), "/visualization", json!("journey")),
            unsupported,
            "visualization",
        ),
        (
            set(funnel(), "/granularity", json!("week")),
            unsupported,
            "granularity",
        ),
        (
            set(retention(), "/granularity", json!("month")),
            unsupported,
            "granularity",
        ),
        (
            set(attribution(), "/compare", json!("previous_period")),
            unsupported,
            "compare",
        ),
        (
            set(
                session_list(),
                "/sort",
                json!([{ "by": "pageviews", "dir": "desc" }]),
            ),
            unsupported,
            "sort",
        ),
        (
            set(journey(), "/page", json!({ "size": 50, "index": 1 })),
            unsupported,
            "page",
        ),
        (
            set(
                session_list(),
                "/page",
                json!({ "size": 100, "index": 100 }),
            ),
            "page_out_of_range",
            "page",
        ),
    ]);
    let panel = serde_json::from_value(
        json!({ "measures": ["pageviews"], "dimensions": [], "sort": [], "page": { "size": 50, "index": 0 } }),
    )
    .unwrap();
    let refusal = validate(&parse(&funnel()), &[panel]).unwrap_err();
    assert_eq!(
        (refusal.code.as_str(), refusal.field.as_str()),
        (unsupported, "panels")
    );
}

#[test]
fn only_the_goals_report_compares_with_a_typical_weekday() {
    validate(
        &parse(&set(goals(), "/compare", json!("typical_weekday"))),
        &[],
    )
    .unwrap();
    assert_refused(
        [
            funnel(),
            journey(),
            attribution(),
            retention(),
            session_list(),
        ]
        .into_iter()
        .map(|state| {
            (
                set(state, "/compare", json!("typical_weekday")),
                "compare_unsupported",
                "compare",
            )
        })
        .collect(),
    );
}

#[test]
fn report_visualizations_need_their_report() {
    let plain = set(funnel(), "/report", Value::Null);
    let plain = set(plain, "/measures", json!(["pageviews"]));
    assert_refused(
        ["funnel", "journey", "cohort"]
            .into_iter()
            .map(|v| {
                (
                    set(plain.clone(), "/visualization", json!(v)),
                    "report_missing",
                    "visualization",
                )
            })
            .collect(),
    );
}

#[test]
fn unknown_kinds_and_versions_are_refused() {
    assert_refused(vec![
        (
            set(funnel(), "/report", json!({ "kind": "sankey", "v": 1 })),
            "report_version_unsupported",
            "report.kind",
        ),
        (
            set(retention(), "/report/v", json!(2)),
            "report_version_unsupported",
            "report.v",
        ),
    ]);
}

#[test]
fn funnels_group_only_from_version_two() {
    let v1 = funnel();
    validate(&parse(&v1), &[]).unwrap();
    assert_eq!(serde_json::to_value(parse(&v1)).unwrap(), v1);
    let mut grouped = set(funnel(), "/report/v", json!(2));
    validate(&parse(&grouped), &[]).unwrap();
    grouped["report"]["group_by"] = json!({ "scope": "event", "key": "plan" });
    validate(&parse(&grouped), &[]).unwrap();
    let mut on_v1 = funnel();
    on_v1["report"]["group_by"] = json!("country");
    let mut perf = grouped.clone();
    perf["report"]["group_by"] = json!("perf_metric");
    assert_refused(vec![
        (on_v1, "report_field_unsupported", "report.group_by"),
        (perf, "invalid_arguments", "report.group_by"),
        (
            set(funnel(), "/report/v", json!(3)),
            "report_version_unsupported",
            "report.v",
        ),
    ]);
}

#[test]
fn predicate_ids_are_unique_across_the_filter_and_every_report_tree() {
    let filter = group(vec![cond("s2", "country", "AT")]);
    let funnel_clash = set(funnel(), "/filter", filter);
    let goal_clash = set(
        attribution(),
        "/report/conversions/1",
        json!({ "kind": "goal", "goal": { "predicate": group(vec![cond("g1", "path", "/")]), "scope": "person", "counting": "once" } }),
    );
    assert_refused(vec![
        (funnel_clash, "duplicate_predicate_id", "report.steps.1.s2"),
        (
            goal_clash,
            "duplicate_predicate_id",
            "report.conversions.1.goal.predicate.g1",
        ),
    ]);
}

#[test]
fn the_condition_limit_counts_every_tree_together() {
    let state = set(funnel(), "/filter", group(conds("f", 20)));
    let state = set(state, "/report/steps/0", group(conds("a", 6)));
    let at_limit = set(state.clone(), "/report/steps/1", group(conds("b", 6)));
    validate(&parse(&at_limit), &[]).unwrap();
    let over = set(state, "/report/steps/1", group(conds("b", 7)));
    assert_refused(vec![(over, "too_many_conditions", "report.steps.1")]);
}

#[test]
fn nesting_depth_applies_to_each_tree_on_its_own() {
    let state = set(funnel(), "/filter", nested("f1", 3));
    validate(
        &parse(&set(state.clone(), "/report/steps/0", nested("a", 3))),
        &[],
    )
    .unwrap();
    assert_refused(vec![(
        set(state, "/report/steps/0", nested("a", 4)),
        "filter_too_deep",
        "report.steps.0",
    )]);
}

#[test]
fn report_predicates_are_typed_like_the_filter() {
    let weekday = json!({ "kind": "cond", "id": "w", "field": "weekday", "cmp": "contains", "value": { "t": "integer", "v": 1 } });
    let key = json!({ "kind": "cond", "id": "k", "field": { "scope": "event", "key": "" }, "cmp": "is_set" });
    assert_refused(vec![
        (
            set(funnel(), "/report/steps/1", group(vec![weekday])),
            "invalid_arguments",
            "report.steps.1.w",
        ),
        (
            set(
                attribution(),
                "/report/conversions/0/goal/predicate",
                group(vec![key]),
            ),
            "invalid_arguments",
            "report.conversions.0.goal.predicate.k",
        ),
    ]);
}

#[test]
fn kind_limits_are_refused_as_invalid_reports() {
    let invalid = "report_invalid";
    let step = || group(vec![cond("x", "path", "/")]);
    let nine: Vec<Value> = (0..9)
        .map(|i| group(vec![cond(&format!("x{i}"), "path", "/")]))
        .collect();
    let five = json!(vec![json!({ "kind": "revenue" }); 5]);
    let day_retention = set(retention(), "/granularity", json!("day"));
    assert_refused(vec![
        (
            set(funnel(), "/report/steps", json!([step()])),
            invalid,
            "report.steps",
        ),
        (
            set(funnel(), "/report/steps", json!(nine)),
            invalid,
            "report.steps",
        ),
        (
            set(funnel(), "/report/window_seconds", json!(59)),
            invalid,
            "report.window_seconds",
        ),
        (
            set(funnel(), "/report/window_seconds", json!(2_592_001)),
            invalid,
            "report.window_seconds",
        ),
        (
            set(journey(), "/report/length", json!(8)),
            invalid,
            "report.length",
        ),
        (
            set(attribution(), "/report/lookback_days", json!(7)),
            invalid,
            "report.lookback_days",
        ),
        (
            set(attribution(), "/report/conversions", five),
            invalid,
            "report.conversions",
        ),
        (
            set(
                attribution(),
                "/report/conversions/0/goal/ref/version",
                json!(0),
            ),
            invalid,
            "report.conversions.0.goal.ref",
        ),
        (
            set(day_retention.clone(), "/report/intervals", json!(33)),
            invalid,
            "report.intervals",
        ),
        (
            set(retention(), "/report/intervals", json!(54)),
            invalid,
            "report.intervals",
        ),
        (
            set(session_list(), "/report/columns", json!(["start", "start"])),
            invalid,
            "report.columns",
        ),
        (
            set(goals(), "/report/goals", json!([])),
            invalid,
            "report.goals",
        ),
        (
            set(goals(), "/report/goals/0/ref/version", json!(0)),
            invalid,
            "report.goals.0.ref",
        ),
        (
            set(goals(), "/visualization", json!("funnel")),
            "report_field_unsupported",
            "visualization",
        ),
    ]);
    validate(
        &parse(&set(day_retention, "/report/intervals", json!(32))),
        &[],
    )
    .unwrap();
    validate(
        &parse(&set(retention(), "/report/intervals", json!(53))),
        &[],
    )
    .unwrap();
}

#[test]
fn percentiles_need_a_perf_metric_condition_and_measure_nothing_else() {
    let plain = set(
        set(funnel(), "/report", Value::Null),
        "/visualization",
        json!("table"),
    );
    let p95 = set(plain.clone(), "/measures", json!(["p95"]));
    let missing = refusal(&p95);
    assert_eq!(missing.field, "measures");
    assert!(
        missing.message.contains("perf_metric"),
        "{}",
        missing.message
    );

    let metric = group(vec![cond("m", "perf_metric", "lcp")]);
    let with_metric = set(p95, "/filter", metric.clone());
    validate(&parse(&with_metric), &[]).unwrap();
    let mixed = set(with_metric, "/measures", json!(["p95", "pageviews"]));
    let by_metric = set(
        set(plain.clone(), "/dimensions", json!(["perf_metric"])),
        "/measures",
        json!(["pageviews"]),
    );
    let in_report = set(retention(), "/filter", metric);
    let amount_sort = set(
        set(plain, "/measures", json!(["revenue_net"])),
        "/sort",
        json!([{ "by": "revenue_net", "dir": "desc" }]),
    );
    validate(
        &parse(&set(
            amount_sort.clone(),
            "/dimensions",
            json!(["currency"]),
        )),
        &[],
    )
    .unwrap();
    assert_refused(vec![
        (mixed, "invalid_arguments", "measures"),
        (by_metric, "invalid_arguments", "measures"),
        (in_report, "invalid_arguments", "filter.m"),
        (amount_sort, "invalid_arguments", "sort"),
    ]);
}

#[tokio::test(flavor = "multi_thread")]
async fn refusals_that_need_the_range_or_the_catalog_name_the_report_field() {
    let (_dir, storage) = open().await;
    let long = set(retention(), "/granularity", json!("day"));
    let long = set(long, "/report/intervals", json!(32));
    let long = set(
        long,
        "/range",
        json!({ "kind": "days", "start": "2026-01-01", "end": "2026-04-30" }),
    );
    let unknown = json!({ "kind": "cond", "id": "p", "field": { "scope": "event", "key": "plan" }, "cmp": "is_set" });
    let unknown = set(
        attribution(),
        "/report/conversions/0/goal/predicate",
        group(vec![unknown]),
    );
    for (state, code, field) in [
        (long, "report_too_large", "report.intervals"),
        (
            unknown,
            "invalid_arguments",
            "report.conversions.0.goal.predicate.p",
        ),
    ] {
        let query = validate(&parse(&state), &[]).unwrap();
        let refusal = storage
            .read(Lane::Report, move |s| {
                let context = Context {
                    site: 1,
                    now_ms: ms("2026-09-02T10:00:00Z"),
                    hostnames: &[],
                    retention: holocron_protocol::retention::Retention::OneYear,
                };
                explore(s, &query, &context)
            })
            .await
            .unwrap()
            .value
            .unwrap_err();
        assert_eq!(
            (refusal.code.as_str(), refusal.field.as_str()),
            (code, field)
        );
    }
}
