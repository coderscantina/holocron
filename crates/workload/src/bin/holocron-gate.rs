//! Storage gate commands. See docs/evidence/m02-storage-gate.md for the run order.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use clap::{Parser, Subcommand};
use holocron_storage::{Record, Refusal, Storage};
use holocron_workload::cases::Case;
use holocron_workload::catalog::Catalog;
use holocron_workload::fixture::{Fixture, GROUPS_PER_MONTH, MONTHS};
use holocron_workload::load::{Identities, records};
use holocron_workload::oracle::Oracle;
use holocron_workload::{associated, bench, correction, explore_bench, gate, probes};
use serde_json::{Value as Json, json};

#[derive(Parser)]
struct Cli {
    /// Repository `fixtures/` directory.
    #[arg(long, default_value = "fixtures")]
    fixtures: PathBuf,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Stream the fixture through the oracle; write the inventory, sample, golden documents and case expectations.
    Oracle {
        #[arg(long, default_value_t = 6)]
        threads: usize,
        /// Smaller values cover each month's first days only and write to `--out` instead of the fixtures.
        #[arg(long, default_value_t = GROUPS_PER_MONTH)]
        groups_per_month: u64,
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Load the retained fixture through admission, the inbox and transfer.
    Load {
        #[arg(long)]
        data: PathBuf,
        /// Groups per month to load (1,000,000 is the full fixture).
        #[arg(long, default_value_t = GROUPS_PER_MONTH)]
        groups_per_month: u64,
        #[arg(long, default_value_t = 4)]
        generators: usize,
    },
    /// Load retained associated records: 9M heatmap interactions and 250,000 replay fragments.
    LoadAssociated {
        #[arg(long)]
        data: PathBuf,
        #[arg(long, default_value_t = 4)]
        generators: usize,
        /// Skip the 16.4 GB replay set (heatmap only).
        #[arg(long)]
        no_replay: bool,
    },
    /// Layout experiment: rewrite events and properties ordered by (site, event_time).
    Recluster {
        #[arg(long)]
        data: PathBuf,
    },
    /// Derive sessions for every dirty identity day (milestone 05 correction).
    Sessions {
        #[arg(long)]
        data: PathBuf,
    },
    /// Cluster every closed month by site as `serve` does in the background (0014 Compaction).
    Compact {
        #[arg(long)]
        data: PathBuf,
        /// The clock compaction runs at (RFC 3339); months that ended more than a day before it
        /// are closed. Defaults to the wall clock.
        #[arg(long)]
        now: Option<chrono::DateTime<chrono::Utc>>,
    },
    /// Run every early-gate case once through the engine probes and compare with the golden hashes.
    Verify {
        #[arg(long)]
        data: PathBuf,
        /// Directory written by a limited `oracle --out`; defaults to the hashes in the case files.
        #[arg(long)]
        golden: Option<PathBuf>,
        /// Only cases whose ID starts with this prefix.
        #[arg(long, default_value = "")]
        only: String,
    },
    /// Replay template set: gzip transport size and compression ratio over 100 captures.
    ReplayTemplates,
    /// Storage layout: file sizes, row counts, per-column compression and bytes per record.
    Layout {
        #[arg(long)]
        data: PathBuf,
    },
    /// Latency, burst, visibility and resource measurements. Writes JSON samples to `--out`.
    Bench(bench::Args),
    /// Correction probe: backfill steps, late records under a follower, freshness timings, session oracle.
    Correct(correction::Args),
    /// Report latency of every declared exploration case through the milestone 06 planner.
    Explore(explore_bench::Args),
}

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() {
    let cli = Cli::parse();
    match cli.command {
        Command::Oracle {
            threads,
            groups_per_month,
            out,
        } => oracle(&cli.fixtures, threads, groups_per_month, out),
        Command::Load {
            data,
            groups_per_month,
            generators,
        } => {
            load(
                &cli.fixtures,
                &data,
                generators,
                Source::Primary { groups_per_month },
            )
            .await
        }
        Command::LoadAssociated {
            data,
            generators,
            no_replay,
        } => {
            load(&cli.fixtures, &data, generators, Source::Heatmap).await;
            if !no_replay {
                load(&cli.fixtures, &data, generators, Source::Replay).await;
            }
        }
        Command::Sessions { data } => {
            let storage = Storage::open(&data, gate::budgets())
                .await
                .expect("open storage");
            let started = Instant::now();
            let catalog = Catalog::load(&cli.fixtures.join("workload/manifest.json"));
            let sessions = gate::derive_sessions(&storage, catalog)
                .await
                .expect("derive sessions");
            eprintln!("sessions: {sessions} derived in {:.1?}", started.elapsed());
            storage.close().await.expect("close");
        }
        Command::Compact { data, now } => {
            let storage = Storage::open(&data, gate::budgets())
                .await
                .expect("open storage");
            let now_ms = now.unwrap_or_else(chrono::Utc::now).timestamp_millis();
            let started = Instant::now();
            let moved = gate::compact(&storage, now_ms).await.expect("compact");
            eprintln!("compact: {moved} events moved in {:.1?}", started.elapsed());
            storage.close().await.expect("close");
        }
        Command::Recluster { data } => {
            let storage = Storage::open(&data, gate::budgets())
                .await
                .expect("open storage");
            let partitions =
                (0..holocron_storage::PROPERTY_PARTS).map(|p| format!("event_properties_{p}"));
            for table in ["events".to_owned(), "session_properties".to_owned()]
                .into_iter()
                .chain(partitions)
            {
                let started = Instant::now();
                gate::recluster(&storage, table.clone())
                    .await
                    .expect("recluster");
                eprintln!("recluster: {table} in {:.1?}", started.elapsed());
            }
            storage.close().await.expect("close");
        }
        Command::Verify { data, golden, only } => {
            let storage = Storage::open(&data, gate::budgets())
                .await
                .expect("open storage");
            let cases: Vec<Case> = Case::early_gate(&cli.fixtures)
                .into_iter()
                .filter(|c| c.id.starts_with(only.as_str()))
                .collect();
            let mut failures = 0;
            for case in &cases {
                let expected = match &golden {
                    Some(dir) => {
                        let doc: Json = serde_json::from_str(
                            &std::fs::read_to_string(dir.join(format!("golden/{}.json", case.id)))
                                .unwrap(),
                        )
                        .unwrap();
                        Some(holocron_workload::result::sha256_hex(
                            &holocron_workload::result::canonical(&doc),
                        ))
                    }
                    None => case.file["expected"]["golden_result_hash"]
                        .as_str()
                        .map(str::to_owned),
                };
                let run = probes::run(&storage, case.clone(), holocron_storage::Lane::Report)
                    .await
                    .expect("probe");
                let ok = expected.as_deref() == Some(run.hash.as_str());
                failures += !ok as usize;
                println!(
                    "{:<28} {} rows={} queue={:>6.1}ms exec={:>8.1}ms",
                    case.id,
                    if ok { "match" } else { "MISMATCH" },
                    run.row_count,
                    run.queue_wait.as_secs_f64() * 1e3,
                    run.execution.as_secs_f64() * 1e3
                );
                if !ok {
                    let path = data.join(format!("mismatch-{}.json", case.id));
                    std::fs::write(&path, serde_json::to_string_pretty(&run.doc).unwrap()).unwrap();
                }
            }
            storage.close().await.expect("close");
            println!(
                "{} of {} cases match their golden hash",
                cases.len() - failures,
                cases.len()
            );
            if failures > 0 {
                std::process::exit(1);
            }
        }
        Command::Bench(args) => bench::run(&cli.fixtures, args).await,
        Command::Correct(args) => correction::run(&cli.fixtures, args).await,
        Command::Explore(args) => explore_bench::run(&cli.fixtures, args).await,
        Command::ReplayTemplates => {
            use std::io::Write;
            let (mut raw, mut gzip) = (0usize, 0usize);
            for capture in 0..100u64 {
                let stream = associated::capture_stream(capture, 5 * associated::FRAGMENT_BYTES);
                for fragment in stream.chunks(associated::FRAGMENT_BYTES) {
                    let mut encoder =
                        flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
                    encoder.write_all(fragment).unwrap();
                    raw += fragment.len();
                    gzip += encoder.finish().unwrap().len();
                }
            }
            println!(
                "{}",
                json!({ "fragments": 500, "raw_bytes": raw, "gzip_bytes": gzip, "gzip_ratio": raw as f64 / gzip as f64,
                        "gzip_bytes_per_fragment": gzip / 500, "templates": 4 })
            );
        }
        Command::Layout { data } => {
            let storage = Storage::open(&data, gate::budgets())
                .await
                .expect("open storage");
            let layout = gate::layout(&storage, &data).await;
            println!("{}", serde_json::to_string_pretty(&layout).unwrap());
            storage.close().await.expect("close");
        }
    }
}

fn oracle(fixtures: &Path, threads: usize, groups_per_month: u64, out: Option<PathBuf>) {
    let cat = Catalog::load(&fixtures.join("workload/manifest.json"));
    let fx = Fixture::new(cat);
    let cases = Case::early_gate(fixtures);
    let started = Instant::now();
    let mut sample = Vec::new();
    let (goldens, inventory) = Oracle::new(&fx, &cases).run(threads, groups_per_month, &mut sample);
    eprintln!("oracle: {:.1?}", started.elapsed());

    let full = groups_per_month == GROUPS_PER_MONTH;
    if full {
        gate::check_inventory(cat, &inventory);
    }
    let base = out.unwrap_or_else(|| fixtures.join("workload"));
    assert!(
        full || base != fixtures.join("workload"),
        "a limited run must not overwrite the fixture results"
    );
    let generated = base.join("generated");
    let golden_dir = base.join("golden");
    std::fs::create_dir_all(&generated).unwrap();
    std::fs::create_dir_all(&golden_dir).unwrap();
    std::fs::write(
        generated.join("inventory.json"),
        serde_json::to_string_pretty(&inventory).unwrap() + "\n",
    )
    .unwrap();
    sample.sort_by_key(|g| g["group"].as_u64());
    let lines: String = sample
        .iter()
        .map(|g| serde_json::to_string(g).unwrap() + "\n")
        .collect();
    std::fs::write(generated.join("sample-month-11.jsonl"), lines).unwrap();

    for (case, golden) in cases.iter().zip(goldens) {
        let class = gate::selectivity_class(golden.matched_records, golden.population);
        let declared = case.selectivity.as_deref();
        let fits =
            declared.is_none_or(|d| gate::fits_class(d, golden.matched_records, golden.population));
        println!(
            "{:<28} rows={:<6} matched={:<9} population={:<9} share={:.5} declared={:<9} observed={:<9} {} {}",
            case.id,
            golden.row_count,
            golden.matched_records,
            golden.population,
            golden.matched_records as f64 / golden.population.max(1) as f64,
            declared.unwrap_or("-"),
            class,
            if fits { "ok" } else { "OUTSIDE CLASS" },
            &golden.hash[..12]
        );
        std::fs::write(
            golden_dir.join(format!("{}.json", case.id)),
            serde_json::to_string_pretty(&golden.doc).unwrap() + "\n",
        )
        .unwrap();
        if !full {
            continue;
        }
        let path = fixtures.join(format!("workload/queries/{}.json", case.id));
        let mut file: Json =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let expected = &mut file["expected"];
        expected["row_count"] = json!(golden.row_count);
        expected["matched_records"] = json!(golden.matched_records);
        expected["golden_result_hash"] = json!(golden.hash);
        expected["golden_document"] = json!(format!("fixtures/workload/golden/{}.json", case.id));
        expected["status"] =
            json!("fixed by milestone 02 first generation; a changed hash is a failure");
        std::fs::write(&path, serde_json::to_string_pretty(&file).unwrap() + "\n").unwrap();
    }
}

/// What a load run generates, split into chunks that generator threads take in order.
#[derive(Clone, Copy)]
enum Source {
    Primary { groups_per_month: u64 },
    Heatmap,
    Replay,
}

impl Source {
    const PRIMARY_CHUNK: u64 = 2_000;
    const HEATMAP_CHUNK: u64 = 20_000;
    const REPLAY_CHUNK: u64 = 2_000;

    fn chunks(self) -> u64 {
        match self {
            Source::Primary { groups_per_month } => {
                MONTHS * groups_per_month.div_ceil(Self::PRIMARY_CHUNK)
            }
            Source::Heatmap => 9_000_000 / Self::HEATMAP_CHUNK,
            Source::Replay => (associated::replay_candidates().end
                - associated::replay_candidates().start)
                .div_ceil(Self::REPLAY_CHUNK),
        }
    }

    fn produce(self, fx: &Fixture, ids: &Identities, chunk: u64) -> Vec<Record> {
        match self {
            Source::Primary { groups_per_month } => {
                let per_month = groups_per_month.div_ceil(Self::PRIMARY_CHUNK);
                let (month, first) = (chunk / per_month, (chunk % per_month) * Self::PRIMARY_CHUNK);
                let last = (first + Self::PRIMARY_CHUNK).min(groups_per_month);
                (first..last)
                    .flat_map(|v| {
                        records(
                            fx.cat,
                            ids,
                            &fx.group(month * GROUPS_PER_MONTH + fx.day.index(v)),
                        )
                    })
                    .collect()
            }
            Source::Heatmap => associated::retained_heatmap(
                fx,
                chunk * Self::HEATMAP_CHUNK..(chunk + 1) * Self::HEATMAP_CHUNK,
            )
            .collect(),
            Source::Replay => {
                let candidates = associated::replay_candidates();
                let first = candidates.start + chunk * Self::REPLAY_CHUNK;
                associated::retained_replay(
                    fx,
                    first..(first + Self::REPLAY_CHUNK).min(candidates.end),
                )
                .flatten()
                .collect()
            }
        }
    }
}

async fn load(fixtures: &Path, data: &Path, generators: usize, source: Source) {
    std::fs::create_dir_all(data).unwrap();
    let cat = Catalog::load(&fixtures.join("workload/manifest.json"));
    let storage = Arc::new(
        Storage::open(data, gate::load_budgets())
            .await
            .expect("open storage"),
    );
    let (tx, mut rx) = tokio::sync::mpsc::channel::<Vec<Record>>(generators * 2);
    let next = Arc::new(AtomicU64::new(0));
    let started = Instant::now();
    let chunks = source.chunks();
    let mut handles = Vec::new();
    for _ in 0..generators {
        let (tx, next) = (tx.clone(), next.clone());
        handles.push(std::thread::spawn(move || {
            let fx = Fixture::new(cat);
            let ids = Identities::new(cat);
            loop {
                let chunk = next.fetch_add(1, Ordering::SeqCst);
                if chunk >= chunks {
                    return;
                }
                if tx.blocking_send(source.produce(&fx, &ids, chunk)).is_err() {
                    return;
                }
            }
        }));
    }
    drop(tx);

    let admitted = Arc::new(AtomicU64::new(0));
    let refusals = Arc::new(AtomicU64::new(0));
    let mut inflight = tokio::task::JoinSet::new();
    let mut last_report = Instant::now();
    while let Some(batch) = rx.recv().await {
        while inflight.len() >= 4 {
            inflight.join_next().await;
        }
        let (task_storage, task_admitted, task_refusals) =
            (storage.clone(), admitted.clone(), refusals.clone());
        inflight.spawn(async move {
            let (storage, admitted, refusals) = (task_storage, task_admitted, task_refusals);
            let n = batch.len() as u64;
            loop {
                match storage.admit(&batch).await {
                    Ok(_) => break,
                    Err(Refusal::Busy) => {
                        refusals.fetch_add(1, Ordering::Relaxed);
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                    Err(Refusal::Unavailable) => panic!("storage unavailable during load"),
                }
            }
            admitted.fetch_add(n, Ordering::Relaxed);
        });
        if last_report.elapsed() > Duration::from_secs(30) {
            last_report = Instant::now();
            let status = storage.status();
            eprintln!(
                "load: {:>7.0}s admitted={} inbox={} ({} MiB) applied_seq={} busy_retries={}",
                started.elapsed().as_secs_f64(),
                admitted.load(Ordering::Relaxed),
                status.inbox_records,
                status.inbox_bytes >> 20,
                status.watermark.sequence,
                refusals.load(Ordering::Relaxed)
            );
        }
    }
    while inflight.join_next().await.is_some() {}
    for h in handles {
        h.join().unwrap();
    }
    let admitted_at = started.elapsed();
    storage.drain().await;
    let drained_at = started.elapsed();
    let storage = Arc::into_inner(storage).expect("sole owner");
    storage.close().await.expect("close");
    println!(
        "{}",
        json!({
            "chunks": chunks,
            "records": admitted.load(Ordering::Relaxed),
            "admission_seconds": admitted_at.as_secs_f64(),
            "drained_seconds": drained_at.as_secs_f64(),
            "closed_seconds": started.elapsed().as_secs_f64(),
            "busy_retries": refusals.load(Ordering::Relaxed),
        })
    );
}
