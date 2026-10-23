//! Report latency through the milestone 06 planner. Two report users loop over every declared
//! `analytics.explore` case, each running the case's own exploration state through `validate`
//! and `explore` on the report lane, until every case has `--observations` samples. No
//! ingestion runs alongside; `bench` covers collection under load.
//!
//! Per case it records the full read time (queue wait plus execution), whether the planner
//! refused the state, and whether its resolved range equals the case's declared one. Q01
//! cases carry the overview's five panels, as the overview sends them. A case the rollups
//! serve is also run once through the raw planner, untimed, and both results are compared.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use holocron_analytics::explore::{
    Context, explore, explore_raw, record_plans, servable, validate,
};
use holocron_protocol::exploration::{ExplorationState, Panel};
use holocron_storage::{Lane, Storage};
use serde_json::{Value as Json, json};

use crate::bench::{stats, write_lines};
use crate::gate;

/// The instant relative ranges resolve against, the same one `bench` uses.
pub const MEASUREMENT_CLOCK_MS: i64 = 1_790_812_800_000;
const USERS: usize = 2;

#[derive(clap::Args)]
pub struct Args {
    #[arg(long)]
    pub data: PathBuf,
    #[arg(long)]
    pub out: PathBuf,
    #[arg(long, default_value_t = 20)]
    pub observations: usize,
    /// Only cases whose ID starts with this prefix.
    #[arg(long, default_value = "")]
    pub only: String,
    /// Only cases whose ID contains this text, such as `-30d-`.
    #[arg(long, default_value = "")]
    pub contains: String,
    /// Replace the comparison of every state without a report, such as `typical_weekday`.
    #[arg(long)]
    pub compare: Option<String>,
    /// Write each case's statement plans to `<out>/plans/<case>.txt`, from one untimed run.
    #[arg(long)]
    pub plans: bool,
    /// Time the raw planner instead of rollups, for a before and after on the same data.
    #[arg(long)]
    pub raw: bool,
    /// Also abandon each case after 25 ms on every report worker and record how long the next
    /// read waits for a worker, in `<out>/cancel.json`.
    #[arg(long)]
    pub cancel: bool,
}

/// A declared `analytics.explore` case, ready to [`measure`].
pub struct Case {
    pub id: String,
    state: ExplorationState,
    panels: Vec<Panel>,
    site: u128,
    pub target_ms: Option<f64>,
    resolved: (i64, i64),
}

fn ms(value: &Json) -> i64 {
    chrono::DateTime::parse_from_rfc3339(value.as_str().expect("instant"))
        .expect("RFC 3339 instant")
        .timestamp_millis()
}

/// The panels `apps/web/src/pages/overview/OverviewView.vue` sends with its state.
fn overview_panels() -> Vec<Panel> {
    [
        ("path", 10),
        ("referrer_domain", 10),
        ("browser", 6),
        ("os", 6),
        ("device", 6),
    ]
    .into_iter()
    .map(|(field, size)| {
        serde_json::from_value(json!({
            "measures": ["visitor_days"], "dimensions": [field],
            "sort": [{ "by": "visitor_days", "dir": "desc" }], "page": { "size": size, "index": 0 }
        }))
        .expect("overview panel")
    })
    .collect()
}

/// The declared cases whose ID starts with `only` and contains `contains`, with `compare`
/// replacing the comparison of every state without a report.
pub fn cases(fixtures: &Path, only: &str, contains: &str, compare: Option<&str>) -> Vec<Case> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(fixtures.join("workload/queries"))
        .expect("read cases")
        .map(|entry| entry.expect("case entry").path())
        .collect();
    paths.sort();
    paths
        .iter()
        .filter_map(|path| {
            let file: Json = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
            let id = file["id"].as_str()?.to_owned();
            // Report cases (Q06 to Q11) name the proposed per-report operations; they run as
            // `analytics.explore` with the state's `report`.
            // Revenue and performance (Q10, Q11) are plain states with their own measures.
            let explores = file["operation"] == "analytics.explore"
                || file["state"]["report"].is_object()
                || matches!(file["family"].as_str(), Some("Q10" | "Q11"));
            if !explores || !id.starts_with(only) || !id.contains(contains) {
                return None;
            }
            let mut state = file["state"].clone();
            if let Some(compare) = compare
                && !state["report"].is_object()
            {
                state["compare"] = json!(compare);
            }
            let state: ExplorationState = serde_json::from_value(state).ok()?;
            let site = uuid::Uuid::parse_str(state.site.as_str()).ok()?.as_u128();
            let panels = if file["family"] == "Q01" {
                overview_panels()
            } else {
                Vec::new()
            };
            Some(Case {
                id,
                panels,
                site,
                target_ms: file["latency"]["target_p95_ms"].as_f64(),
                resolved: (ms(&file["resolved"]["start"]), ms(&file["resolved"]["end"])),
                state,
            })
        })
        .collect()
}

pub async fn run(fixtures: &Path, args: Args) {
    std::fs::create_dir_all(&args.out).expect("create out");
    let cases = Arc::new(cases(
        fixtures,
        &args.only,
        &args.contains,
        args.compare.as_deref(),
    ));
    let storage = Arc::new(
        Storage::open(&args.data, gate::budgets())
            .await
            .expect("open storage"),
    );
    let raw = args.raw;
    let mut compared = BTreeMap::new();
    for case in cases.iter() {
        compared.insert(case.id.clone(), compare(&storage, case, &args.out).await);
    }
    if args.plans {
        let dir = args.out.join("plans");
        std::fs::create_dir_all(&dir).expect("create plans dir");
        for case in cases.iter() {
            if let Some(plans) = plans(&storage, case, raw).await {
                std::fs::write(
                    dir.join(format!("{}.txt", case.id)),
                    plans.join("\n\n----\n\n"),
                )
                .expect("write plans");
            }
        }
    }
    if args.cancel {
        let mut waits = BTreeMap::new();
        for case in cases.iter() {
            waits.insert(case.id.clone(), cancel(&storage, case).await);
        }
        std::fs::write(
            args.out.join("cancel.json"),
            serde_json::to_string_pretty(&waits).expect("cancel"),
        )
        .expect("write cancel");
    }
    let samples = Arc::new(Mutex::new(Vec::<Json>::new()));
    let started = Instant::now();

    let users: Vec<_> = (0..USERS)
        .map(|user| {
            let (cases, storage, samples) = (cases.clone(), storage.clone(), samples.clone());
            let rounds = args.observations.div_ceil(USERS);
            tokio::spawn(async move {
                for round in 0..rounds {
                    // The users walk the list from opposite ends, so they overlap on different cases.
                    for index in 0..cases.len() {
                        let case = &cases[if user == 0 {
                            index
                        } else {
                            cases.len() - 1 - index
                        }];
                        let mut sample = measure(&storage, case, raw).await;
                        sample.insert("user".into(), json!(user));
                        sample.insert("round".into(), json!(round));
                        sample.insert("case".into(), json!(case.id));
                        samples.lock().unwrap().push(Json::Object(sample));
                    }
                }
            })
        })
        .collect();
    for user in users {
        user.await.expect("report user");
    }
    if let Ok(storage) = Arc::try_unwrap(storage) {
        storage.close().await.expect("close storage");
    }

    let samples = samples.lock().unwrap().clone();
    write_lines(&args.out.join("explore.jsonl"), &samples);
    let mut summary = BTreeMap::new();
    for case in cases.iter() {
        let own: Vec<&Json> = samples.iter().filter(|s| s["case"] == case.id).collect();
        let times: Vec<f64> = own.iter().filter_map(|s| s["ms"].as_f64()).collect();
        let p95 = stats(times.clone())["p95"].as_f64();
        summary.insert(
            case.id.clone(),
            json!({
                "n": times.len(),
                "latency": stats(times),
                "target_p95_ms": case.target_ms,
                "passes": match (p95, case.target_ms) { (Some(p), Some(t)) => Some(p <= t), _ => None },
                "refused": own.iter().find_map(|s| s.get("refused").cloned()),
                "resolved_matches": own.iter().all(|s| s["resolved_matches"] != false),
                "panels": case.panels.len(),
                "rollup": compared[&case.id].0,
                "matches_raw": compared[&case.id].1,
            }),
        );
    }
    let document = json!({
        "users": USERS,
        "planner": if raw { "raw" } else { "rollup" },
        "seconds": started.elapsed().as_secs_f64(),
        "cases": summary,
    });
    std::fs::write(
        args.out.join("explore-summary.json"),
        serde_json::to_string_pretty(&document).expect("summary"),
    )
    .expect("write summary");
    eprintln!(
        "explore: {} cases, {} samples in {:.0?}",
        cases.len(),
        samples.len(),
        started.elapsed()
    );
}

/// One exploration of a case: elapsed milliseconds with their queue wait and execution, or the
/// refusal code.
pub async fn measure(storage: &Storage, case: &Case, raw: bool) -> serde_json::Map<String, Json> {
    let query = match validate(&case.state, &case.panels) {
        Ok(query) => query,
        Err(refusal) => return refused(&refusal),
    };
    let site = case.site;
    let started = Instant::now();
    let read = storage
        .read(Lane::Report, move |snapshot| {
            let context = Context {
                site,
                now_ms: MEASUREMENT_CLOCK_MS,
                hostnames: &[],
                retention: holocron_protocol::retention::Retention::OneYear,
            };
            if raw {
                explore_raw(snapshot, &query, &context)
            } else {
                explore(snapshot, &query, &context)
            }
        })
        .await;
    let elapsed = started.elapsed().as_secs_f64() * 1000.0;
    let mut sample = serde_json::Map::new();
    match read {
        Ok(read) => match read.value {
            Ok(result) => {
                sample.insert("ms".into(), json!(elapsed));
                sample.insert(
                    "queue_ms".into(),
                    json!(read.queue_wait.as_secs_f64() * 1000.0),
                );
                sample.insert(
                    "execution_ms".into(),
                    json!(read.execution.as_secs_f64() * 1000.0),
                );
                let resolved = (result.resolved.start, result.resolved.end);
                sample.insert("resolved_matches".into(), json!(resolved == case.resolved));
            }
            Err(refusal) => return refused(&refusal),
        },
        Err(error) => {
            sample.insert("error".into(), json!(format!("{error:?}")));
        }
    }
    sample
}

/// Starts the case on every report worker, stops waiting after 25 ms as a replaced exploration
/// does, and returns how long the next read then waits for a worker, in milliseconds.
async fn cancel(storage: &Arc<Storage>, case: &Case) -> Json {
    let Ok(query) = validate(&case.state, &case.panels) else {
        return Json::Null;
    };
    let abandoned: Vec<_> = (0..gate::budgets().report_workers)
        .map(|_| {
            let (storage, query, site) = (storage.clone(), query.clone(), case.site);
            tokio::spawn(async move {
                let read = storage.read(Lane::Report, move |snapshot| {
                    let context = Context {
                        site,
                        now_ms: MEASUREMENT_CLOCK_MS,
                        hostnames: &[],
                        retention: holocron_protocol::retention::Retention::OneYear,
                    };
                    explore(snapshot, &query, &context)
                });
                tokio::time::timeout(std::time::Duration::from_millis(25), read)
                    .await
                    .is_err()
            })
        })
        .collect();
    let mut timed_out = 0;
    for read in abandoned {
        timed_out += usize::from(read.await.expect("abandoned read"));
    }
    let next = storage
        .read(Lane::Report, |s| {
            s.connection()
                .query_row("SELECT 1", [], |r| r.get::<_, i64>(0))
        })
        .await
        .expect("next read");
    json!({
        "abandoned": timed_out,
        "next_wait_ms": next.queue_wait.as_secs_f64() * 1000.0,
    })
}

/// Whether the rollups serve a case and, when they do, whether their result serializes to the
/// same bytes as the raw planner's. Differing results are written to `<out>/mismatch/`.
async fn compare(storage: &Storage, case: &Case, out: &Path) -> (bool, Option<bool>) {
    let Ok(query) = validate(&case.state, &case.panels) else {
        return (false, None);
    };
    if !servable(&query) {
        return (false, None);
    }
    let site = case.site;
    let read = storage
        .read(Lane::Report, move |snapshot| {
            let context = Context {
                site,
                now_ms: MEASUREMENT_CLOCK_MS,
                hostnames: &[],
                retention: holocron_protocol::retention::Retention::OneYear,
            };
            let text = |result: Result<_, _>| match result {
                Ok(result) => serde_json::to_string(&result).expect("result"),
                Err(refusal) => format!("{refusal:?}"),
            };
            Ok((
                text(explore(snapshot, &query, &context)?),
                text(explore_raw(snapshot, &query, &context)?),
            ))
        })
        .await
        .expect("compare");
    let (rollup, raw) = read.value;
    if rollup != raw {
        let dir = out.join("mismatch");
        std::fs::create_dir_all(&dir).expect("create mismatch dir");
        for (name, text) in [("rollup", &rollup), ("raw", &raw)] {
            std::fs::write(dir.join(format!("{}.{name}.json", case.id)), text).expect("write");
        }
    }
    (true, Some(rollup == raw))
}

/// The plans of one exploration of a case, or `None` when the planner refuses it.
async fn plans(storage: &Storage, case: &Case, raw: bool) -> Option<Vec<String>> {
    let query = validate(&case.state, &case.panels).ok()?;
    let site = case.site;
    let read = storage
        .read(Lane::Report, move |snapshot| {
            let context = Context {
                site,
                now_ms: MEASUREMENT_CLOCK_MS,
                hostnames: &[],
                retention: holocron_protocol::retention::Retention::OneYear,
            };
            let (result, plans) = record_plans(|| {
                if raw {
                    explore_raw(snapshot, &query, &context)
                } else {
                    explore(snapshot, &query, &context)
                }
            });
            result.map(|_| plans)
        })
        .await
        .ok()?;
    Some(read.value)
}

fn refused(refusal: &impl std::fmt::Debug) -> serde_json::Map<String, Json> {
    let mut sample = serde_json::Map::new();
    sample.insert("refused".into(), json!(format!("{refusal:?}")));
    sample
}
