//! The release gate's report side for the in-process examples `summary_load` and
//! `collect_burst`: the gate fixture's sites in control storage, two report users over the
//! declared explore suite, and the summary step run back to back.

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use holocron_analytics::identity::{DEFAULT_WINDOW_MINUTES, SessionPolicy, SitePolicies};
use holocron_protocol::access::WorkspaceKind;
use holocron_server::AppState;
use holocron_server::db::{Db, new_id, now_ms};
use holocron_server::sites::{self, policy};
use holocron_server::summaries::{self, Schedule};
use holocron_storage::Storage;
use holocron_workload::bench::stats;
use holocron_workload::catalog::Catalog;
use holocron_workload::explore_bench::{Case, measure};
use serde_json::{Value as Json, json};
use tokio::sync::watch;

/// Registers the fixture's sites in control storage under their analytics IDs, so
/// `summaries::round` finds them. Each gets one goal and one daily funnel pinned to a board
/// (the `Q06-30d-A-vienna` steps), so all four rule families read. Does nothing when the
/// first site is already registered.
pub async fn register(db: &Db, catalog: &Catalog, fixtures: &Path) -> anyhow::Result<()> {
    let mut conn = db.writer.acquire().await?;
    let first = catalog.sites[0].id.to_string();
    let known: i64 = sqlx::query_scalar("SELECT count(*) FROM sites WHERE id = ?")
        .bind(&first)
        .fetch_one(&mut *conn)
        .await?;
    if known > 0 {
        return Ok(());
    }
    let case: Json = serde_json::from_str(&std::fs::read_to_string(
        fixtures.join("workload/queries/Q06-30d-A-vienna.json"),
    )?)?;
    let funnel = &case["state"];
    let goal = json!({
        "predicate": funnel["report"]["steps"][2],
        "scope": "session",
        "counting": "once",
    })
    .to_string();
    let now = now_ms();
    let workspace = sites::create_workspace(&mut conn, "Gate", WorkspaceKind::Team).await?;
    let board = new_id();
    sqlx::query(
        "INSERT INTO boards (id, workspace_id, name, created_by, created_at, updated_at)
         VALUES (?, ?, 'Funnels', 'gate', ?, ?)",
    )
    .bind(&board)
    .bind(workspace.as_str())
    .bind(now)
    .bind(now)
    .execute(&mut *conn)
    .await?;
    for (position, site) in catalog.sites.iter().enumerate() {
        let id = site.id.to_string();
        sqlx::query(
            "INSERT INTO sites (id, workspace_id, name, domain, created_at) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(workspace.as_str())
        .bind(format!("Site {}", site.code))
        .bind(site.hostname)
        .bind(now)
        .execute(&mut *conn)
        .await?;
        // The policy `holocron-gate sessions` recorded, so mirroring it changes nothing.
        let policies =
            SitePolicies::new(SessionPolicy::new(site.timezone, DEFAULT_WINDOW_MINUTES)?);
        policy::insert_session_policy(&mut conn, &id, policies.current(), now).await?;
        let goal_id = new_id();
        sqlx::query(
            "INSERT INTO goals (id, site_id, name, name_key, version, created_by, created_at, updated_at)
             VALUES (?, ?, 'Pro signup', 'pro signup', 1, 'gate', ?, ?)",
        )
        .bind(&goal_id)
        .bind(&id)
        .bind(now)
        .bind(now)
        .execute(&mut *conn)
        .await?;
        sqlx::query(
            "INSERT INTO goal_versions (goal_id, version, definition, created_by, created_at)
             VALUES (?, 1, ?, 'gate', ?)",
        )
        .bind(&goal_id)
        .bind(&goal)
        .bind(now)
        .execute(&mut *conn)
        .await?;
        let mut state = funnel.clone();
        state["site"] = json!(id);
        state["timezone"] = json!(site.timezone.name());
        sqlx::query(
            "INSERT INTO board_cards (id, board_id, position, title, site_id, state)
             VALUES (?, ?, ?, 'Pricing to pro signup', ?, ?)",
        )
        .bind(new_id())
        .bind(&board)
        .bind(position as i64)
        .bind(&id)
        .bind(state.to_string())
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

/// Two report users walk `cases` from opposite ends, one request in flight each, with the
/// contract's think time of 500 ms ± 50 ms, for `rounds` passes or until `stop` turns true.
/// Returns one sample per request, as `holocron-gate explore` writes them.
pub async fn users(
    storage: Arc<Storage>,
    cases: Arc<Vec<Case>>,
    rounds: usize,
    stop: watch::Receiver<bool>,
) -> Vec<Json> {
    let samples = Arc::new(Mutex::new(Vec::new()));
    let users: Vec<_> = (0..2)
        .map(|user| {
            let (storage, cases, samples, stop) = (
                storage.clone(),
                cases.clone(),
                samples.clone(),
                stop.clone(),
            );
            tokio::spawn(async move {
                let mut sent = 0u64;
                for round in 0..rounds {
                    for index in 0..cases.len() {
                        if *stop.borrow() {
                            return;
                        }
                        let think = 450 + (sent * 37 + user as u64 * 11) % 101;
                        tokio::time::sleep(Duration::from_millis(think)).await;
                        sent += 1;
                        let case = &cases[if user == 0 {
                            index
                        } else {
                            cases.len() - 1 - index
                        }];
                        let mut sample = measure(&storage, case, false).await;
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
    Arc::try_unwrap(samples).unwrap().into_inner().unwrap()
}

/// Per case the latency percentiles against the target, refusals and errors, plus the
/// execution milliseconds of every read the users made.
pub fn report(cases: &[Case], samples: &[Json]) -> (Json, f64) {
    let mut out = serde_json::Map::new();
    for case in cases {
        let own: Vec<&Json> = samples.iter().filter(|s| s["case"] == case.id).collect();
        let times: Vec<f64> = own.iter().filter_map(|s| s["ms"].as_f64()).collect();
        let latency = stats(times);
        let passes = match (latency["p95"].as_f64(), case.target_ms) {
            (Some(p95), Some(target)) => Some(p95 <= target),
            _ => None,
        };
        out.insert(
            case.id.clone(),
            json!({
                "latency": latency,
                "target_p95_ms": case.target_ms,
                "passes": passes,
                "refused": own.iter().find_map(|s| s.get("refused").cloned()),
                "errors": own.iter().filter(|s| s.get("error").is_some()).count(),
            }),
        );
    }
    let execution = samples
        .iter()
        .filter_map(|s| s["execution_ms"].as_f64())
        .sum();
    (Json::Object(out), execution)
}

/// Runs the summary step in passes until `stop` turns true. A pass starts a fresh schedule,
/// so every site is due, and repeats `summaries::round` until every site is written, 100 ms
/// after a round that yielded to a report or timed out. Passes start `every` apart, or back to
/// back when it is zero. The real step summarizes a site at most every 10 minutes, so back to
/// back is its upper bound on engine use and `every` = 600 s its cadence.
///
/// A site's run time is the gap from its round's start or the previous site's write to its
/// own write. The engine milliseconds add up every round's duration, timed-out reads and the
/// round's own SQLite reads included, so they overstate the step's time on the report lane.
pub async fn summary_step(
    state: AppState,
    mut stop: watch::Receiver<bool>,
    every: Duration,
) -> Json {
    let writes = Arc::new(Mutex::new(Vec::<Instant>::new()));
    let listener = {
        let (mut changed, writes) = (state.summaries.subscribe(), writes.clone());
        tokio::spawn(async move {
            while changed.recv().await.is_ok() {
                writes.lock().unwrap().push(Instant::now());
            }
        })
    };
    let (mut starts, mut passes, mut engine) = (Vec::new(), Vec::new(), Duration::ZERO);
    let (mut rounds, mut written, mut skips, mut timeouts, mut failed) = (0, 0, 0, 0, 0);
    'passes: while !*stop.borrow() {
        let pass = Instant::now();
        let mut schedule = Schedule::default();
        loop {
            let started = Instant::now();
            starts.push(started);
            let round = summaries::round(&state, &mut schedule, &stop)
                .await
                .expect("summary round");
            engine += started.elapsed();
            rounds += 1;
            written += round.written.len();
            skips += round.skipped;
            timeouts += round.timed_out.len();
            failed += round.failed;
            if *stop.borrow() {
                break 'passes;
            }
            if round.skipped == 0 && round.timed_out.is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        passes.push(pass.elapsed().as_secs_f64() * 1000.0);
        tokio::select! {
            _ = tokio::time::sleep_until((pass + every).into()) => {}
            _ = stop.changed() => {}
        }
    }
    listener.abort();
    let writes = writes.lock().unwrap();
    let runs: Vec<f64> = writes
        .iter()
        .map(|&at| {
            let from = starts
                .iter()
                .chain(writes.iter())
                .filter(|&&t| t < at)
                .max()
                .copied()
                .unwrap_or(at);
            (at - from).as_secs_f64() * 1000.0
        })
        .collect();
    json!({
        "complete_passes_ms": stats(passes),
        "site_runs_ms": stats(runs),
        "rounds": rounds,
        "written": written,
        "skips": skips,
        "timeouts": timeouts,
        "failed": failed,
        "engine_ms": engine.as_secs_f64() * 1000.0,
    })
}
