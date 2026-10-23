//! Query cases from `fixtures/workload/queries/`, parsed into the subset of
//! ExplorationState v1 (0004) the early gate uses. Anything outside that
//! subset is refused by name rather than guessed.

use std::path::Path;

use chrono_tz::Tz;
use serde_json::Value as Json;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    And,
    Or,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cmp {
    Eq,
    Neq,
    Contains,
    NotContains,
    StartsWith,
    IsSet,
    IsNotSet,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Field {
    /// A catalog dimension such as `path` or `device`.
    Dimension(String),
    /// An event-scope property key.
    EventProperty(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Group {
        op: Op,
        items: Vec<Node>,
    },
    Cond {
        field: Field,
        cmp: Cmp,
        value: Option<String>,
    },
}

#[derive(Debug, Clone)]
pub struct Funnel {
    pub steps: Vec<Node>,
    pub window_ms: i64,
}

/// Half-open `[start, end)` in UTC milliseconds, already clipped to the retention start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Range {
    pub start: i64,
    pub end: i64,
}

impl Range {
    pub fn contains(&self, t: i64) -> bool {
        self.start <= t && t < self.end
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Granularity {
    Hour,
    Day,
}

#[derive(Debug, Clone)]
pub struct Case {
    pub id: String,
    pub family: String,
    pub site: String,
    pub site_id: uuid::Uuid,
    pub timezone: Tz,
    pub current: Range,
    pub previous: Option<Range>,
    /// The `resolved` object echoed into result documents.
    pub resolved: Json,
    pub measures: Vec<String>,
    pub dimensions: Vec<String>,
    pub filter: Option<Node>,
    pub granularity: Granularity,
    /// Measure to sort rows by, descending; ties by dimension values ascending.
    pub sort_by: Option<String>,
    pub page_size: usize,
    pub page_index: usize,
    pub funnel: Option<Funnel>,
    pub selectivity: Option<String>,
    pub file: Json,
}

fn ms(j: &Json) -> i64 {
    chrono::DateTime::parse_from_rfc3339(j.as_str().expect("instant"))
        .expect("rfc3339")
        .timestamp_millis()
}

fn node(j: &Json) -> Node {
    match j["kind"].as_str().expect("node kind") {
        "group" => Node::Group {
            op: match j["op"].as_str().unwrap() {
                "and" => Op::And,
                "or" => Op::Or,
                other => panic!("unknown op {other}"),
            },
            items: j["items"].as_array().unwrap().iter().map(node).collect(),
        },
        "cond" => {
            let field = match &j["field"] {
                Json::String(name) => Field::Dimension(name.clone()),
                Json::Object(p) if p["scope"] == "event" => {
                    Field::EventProperty(p["key"].as_str().unwrap().to_owned())
                }
                other => panic!("field {other} is outside the early-gate subset"),
            };
            let cmp = match j["cmp"].as_str().unwrap() {
                "eq" => Cmp::Eq,
                "neq" => Cmp::Neq,
                "contains" => Cmp::Contains,
                "not_contains" => Cmp::NotContains,
                "starts_with" => Cmp::StartsWith,
                "is_set" => Cmp::IsSet,
                "is_not_set" => Cmp::IsNotSet,
                other => panic!("operator {other} is outside the early-gate subset"),
            };
            let value = j.get("value").map(|v| {
                assert_eq!(
                    v["t"], "string",
                    "only string values are in the early-gate subset"
                );
                v["v"].as_str().unwrap().to_owned()
            });
            Node::Cond { field, cmp, value }
        }
        other => panic!("unknown node kind {other}"),
    }
}

impl Case {
    pub fn load(path: &Path) -> Case {
        let file: Json = serde_json::from_str(&std::fs::read_to_string(path).expect("read case"))
            .expect("parse case");
        let state = &file["state"];
        let resolved = &file["resolved"];
        let clip = |start: &str, retention: &str, end: &str| {
            let start = ms(&resolved[start]);
            let start = resolved.get(retention).map_or(start, |r| start.max(ms(r)));
            Range {
                start,
                end: ms(&resolved[end]),
            }
        };
        let current = clip("start", "retention_start", "end");
        let previous = (state["compare"] == "previous_period")
            .then(|| clip("compare_start", "compare_retention_start", "compare_end"));
        let funnel = state
            .get("report")
            .filter(|r| r["kind"] == "funnel")
            .map(|r| {
                assert_eq!(
                    r["scope"], "daily",
                    "only daily funnels are in the early-gate subset"
                );
                Funnel {
                    steps: r["steps"].as_array().unwrap().iter().map(node).collect(),
                    window_ms: r["window_seconds"].as_i64().unwrap() * 1000,
                }
            });
        Case {
            id: file["id"].as_str().unwrap().to_owned(),
            family: file["family"].as_str().unwrap().to_owned(),
            site: file["site"].as_str().unwrap().to_owned(),
            site_id: state["site"].as_str().unwrap().parse().unwrap(),
            timezone: state["timezone"].as_str().unwrap().parse().unwrap(),
            current,
            previous,
            resolved: resolved.clone(),
            measures: state["measures"]
                .as_array()
                .unwrap()
                .iter()
                .map(|m| m.as_str().expect("named measure").to_owned())
                .collect(),
            dimensions: state["dimensions"]
                .as_array()
                .unwrap()
                .iter()
                .map(|d| d.as_str().expect("named dimension").to_owned())
                .collect(),
            filter: (!state["filter"].is_null()).then(|| node(&state["filter"])),
            granularity: match state["granularity"].as_str().unwrap() {
                "hour" => Granularity::Hour,
                "day" => Granularity::Day,
                other => panic!("granularity {other} is outside the early-gate subset"),
            },
            sort_by: state["sort"].as_array().unwrap().first().map(|s| {
                assert_eq!(s["dir"], "desc");
                s["by"].as_str().unwrap().to_owned()
            }),
            page_size: state["page"]["size"].as_u64().unwrap() as usize,
            page_index: state["page"]["index"].as_u64().unwrap() as usize,
            funnel,
            selectivity: file["selectivity"].as_str().map(str::to_owned),
            file,
        }
    }

    /// The early-gate subset from `streams.json`, in file order.
    pub fn early_gate(fixtures: &Path) -> Vec<Case> {
        let streams: Json = serde_json::from_str(
            &std::fs::read_to_string(fixtures.join("workload/streams.json")).unwrap(),
        )
        .unwrap();
        streams["early_gate"]["subset"]
            .as_array()
            .unwrap()
            .iter()
            .map(|id| {
                Case::load(
                    &fixtures.join(format!("workload/queries/{}.json", id.as_str().unwrap())),
                )
            })
            .collect()
    }

    /// Series and tables over sessions are only defined for unfiltered, ungrouped cases here.
    pub fn has_series(&self) -> bool {
        self.dimensions.is_empty() && self.funnel.is_none()
    }
}
