//! The summary step beside two report users (decision 0015 Where it runs), over a data
//! directory `holocron-gate load` and `sessions` filled. Registers the fixture's sites in the
//! directory's control database, then runs the declared explore suite with two users while
//! the summary step runs back to back, and writes per-case latency, the step's runs, skips and
//! timeouts, and its share of report-lane engine time to `<out>/summary-load.json`.
//!
//! ```text
//! cargo run --release -p holocron-server --example summary_load -- \
//!   --data GATE --out OUT --observations 20 --contains=-30d- --compare typical_weekday
//! ```
//!
//! `--no-summaries` runs the users alone, for the comparison. Users resolve relative ranges at
//! `holocron-gate explore`'s measurement clock; the summary step reads at `--now`.

mod gate;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use clap::Parser;
use holocron_server::AppState;
use holocron_server::clock::Clock;
use holocron_server::config::{CollectConfig, Config};
use holocron_server::db::Db;
use holocron_storage::Storage;
use holocron_workload::catalog::Catalog;
use holocron_workload::explore_bench::cases;
use serde_json::json;

#[derive(Parser)]
struct Args {
    #[arg(long)]
    data: PathBuf,
    #[arg(long)]
    out: PathBuf,
    /// Repository `fixtures/` directory.
    #[arg(long, default_value = "fixtures")]
    fixtures: PathBuf,
    #[arg(long, default_value_t = 20)]
    observations: usize,
    /// Only cases whose ID contains this text.
    #[arg(long, default_value = "-30d-")]
    contains: String,
    /// Replace the comparison of every state without a report, such as `typical_weekday`.
    #[arg(long)]
    compare: Option<String>,
    /// Run the report users without the summary step.
    #[arg(long)]
    no_summaries: bool,
    /// The summary step's application clock, RFC 3339. The default is a Wednesday afternoon in
    /// Vienna on the fixture's last day.
    #[arg(long, default_value = "2026-09-30T13:20:00Z")]
    now: String,
    /// Seconds between the starts of two summary passes over every site; 0 runs them back to
    /// back, 600 is the step's 10-minute cadence.
    #[arg(long, default_value_t = 0)]
    every_s: u64,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    std::fs::create_dir_all(&args.out)?;
    let now = chrono::DateTime::parse_from_rfc3339(&args.now)?.timestamp_millis();
    let catalog = Catalog::load(&args.fixtures.join("workload/manifest.json"));
    let db = Db::open(&args.data).await?;
    gate::register(&db, catalog, &args.fixtures).await?;
    let budgets = holocron_workload::gate::budgets();
    let storage = Arc::new(Storage::open(&args.data, budgets.clone()).await?);
    let config = Config {
        data_dir: args.data.clone(),
        bind: "127.0.0.1:0".parse()?,
        web_dir: None,
        public_origin: None,
        instance_name: "summary-load".into(),
        setup_token: None,
        collect: CollectConfig::default(),
        disk_reserve_bytes: budgets.disk_reserve_bytes,
    };
    let state = AppState::new(db, storage.clone(), config, Clock::frozen(now)).await?;
    let cases = Arc::new(cases(
        &args.fixtures,
        "",
        &args.contains,
        args.compare.as_deref(),
    ));

    let (stop, stopped) = tokio::sync::watch::channel(false);
    let step = (!args.no_summaries).then(|| {
        tokio::spawn(gate::summary_step(
            state.clone(),
            stopped.clone(),
            std::time::Duration::from_secs(args.every_s),
        ))
    });
    let started = Instant::now();
    let samples = gate::users(
        storage,
        cases.clone(),
        args.observations.div_ceil(2),
        stopped,
    )
    .await;
    let seconds = started.elapsed().as_secs_f64();
    // Without the summary step nobody listens any more.
    let _ = stop.send(true);
    let step = match step {
        Some(step) => Some(step.await?),
        None => None,
    };

    let (cases_report, users_ms) = gate::report(&cases, &samples);
    let share = step.as_ref().map(|step| {
        let step_ms = step["engine_ms"].as_f64().unwrap_or(0.0);
        step_ms / (step_ms + users_ms)
    });
    let failing: Vec<&String> = cases_report
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(_, case)| case["passes"] == false)
        .map(|(id, _)| id)
        .collect();
    let document = json!({
        "users": 2,
        "compare": args.compare,
        "summaries": step,
        "summary_now": args.now,
        "summary_every_s": args.every_s,
        "seconds": seconds,
        "users_engine_ms": users_ms,
        "summary_engine_share": share,
        "failing": failing,
        "cases": cases_report,
    });
    holocron_workload::bench::write_lines(&args.out.join("summary-load.jsonl"), &samples);
    std::fs::write(
        args.out.join("summary-load.json"),
        serde_json::to_string_pretty(&document)?,
    )?;
    eprintln!(
        "summary_load: {} cases, {} samples in {seconds:.0} s, {} failing, engine share {share:?}",
        cases.len(),
        samples.len(),
        failing.len()
    );
    Ok(())
}
