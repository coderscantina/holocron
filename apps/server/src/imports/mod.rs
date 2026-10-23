//! Historical import from Umami (milestone 11, decision 0013): `holocron import inspect`, `run`
//! and `status`.
//!
//! Inspection reads a source read only and accepts exactly Umami 3.4.0 on PostgreSQL or 2.20.2
//! on MySQL ([`signature`]). A run imports into an initialized data directory that has no sites
//! yet, in steps that each commit with a checkpoint ([`state`]): accounts and configuration in
//! one transaction ([`control`]), then events, identify data, heatmaps and session links in
//! chunks through the inbox ([`events`]), replays into the replay store ([`replays`]),
//! correction of sessions and aggregates, and the reconciliation manifest ([`reconcile`]).
//! Running it again after an interruption resumes from the checkpoints and yields the same
//! destination. The source is never written: the session reads only, and credentials that
//! could write get a warning ([`warnings`]).
//!
//! The setup wizard runs the same import inside `serve` through [`import`], with a [`Reporter`]
//! for its progress stream (decision 0021).

mod control;
mod definitions;
mod events;
mod progress;
mod reconcile;
mod replays;
mod signature;
mod source;
mod state;

use std::path::{Path, PathBuf};

use chrono_tz::Tz;
use holocron_analytics::{Limits, run_until_clean};
use holocron_protocol::retention::Retention;
use holocron_protocol::setup::{
    SetupImportProgress, SetupImportState, SetupImportWarningCode, SetupStepState,
};
use holocron_storage::{Budgets, Storage};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::watch;

pub use progress::Reporter;
pub use signature::{Engine, Layout};
pub use state::{ImportState, refuse_while_running, state};
pub(crate) use state::{forget_session_links, forget_site, session_links};

use crate::db::Db;
use crate::identity::secrets::SecretKey;
use crate::init::{Exit, Failure};
use crate::recordings::store::Store;
use signature::Verdict;
use source::{Arg, Source};

#[derive(clap::Subcommand)]
pub enum Command {
    /// Reads an Umami database without changing it and reports whether it can be imported, what
    /// an import would bring and what it would skip.
    Inspect {
        /// `postgres://` or `mysql://` URL of a read-only database user. Off a private network
        /// the URL must ask for TLS (`sslmode=verify-full` or `ssl-mode=VERIFY_IDENTITY`).
        #[arg(long, env = "HOLOCRON_IMPORT_SOURCE", hide_env_values = true)]
        source: String,
        /// Timezone the retention preview counts days in.
        #[arg(long, default_value = "UTC")]
        timezone: String,
    },
    /// Imports an Umami database into an initialized data directory without sites, or resumes
    /// an interrupted import of it.
    Run(RunArgs),
    /// Prints the import state and manifest of a data directory.
    Status {
        #[arg(long, env = "HOLOCRON_DATA_DIR")]
        data_dir: PathBuf,
    },
}

#[derive(clap::Args, Clone)]
pub struct RunArgs {
    #[arg(long, env = "HOLOCRON_DATA_DIR")]
    pub data_dir: PathBuf,
    /// `postgres://` or `mysql://` URL of a read-only database user. Off a private network the
    /// URL must ask for TLS (`sslmode=verify-full` or `ssl-mode=VERIFY_IDENTITY`).
    #[arg(long, env = "HOLOCRON_IMPORT_SOURCE", hide_env_values = true)]
    pub source: String,
    #[command(flatten)]
    pub options: Options,
    /// File holding Umami's `TWO_FACTOR_ENCRYPTION_KEY` (64 hex characters). Without it every
    /// account with 2FA must enroll again at its first login.
    #[arg(long)]
    pub two_factor_key_file: Option<PathBuf>,
    /// Free space the data volume keeps, in MiB, as `serve` does. Below it the import stops
    /// and can be resumed once there is room (decision 0008).
    #[arg(long, env = "HOLOCRON_DISK_RESERVE_MIB", default_value_t = 4096)]
    pub disk_reserve_mib: u64,
}

/// Settings a run is started with. A resume must use the same ones.
#[derive(clap::Args, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Options {
    /// IANA timezone of every imported site. Umami keeps no site timezone.
    #[arg(long, default_value = "UTC")]
    pub timezone: String,
    /// Domain for logins of Umami usernames that are not email addresses: `<username>@<domain>`.
    #[arg(long, default_value = "umami.invalid")]
    pub login_domain: String,
    /// Source rows per chunk, the unit of memory use and of checkpoints.
    #[arg(long, default_value_t = 5000, value_parser = clap::value_parser!(i64).range(1..=100_000))]
    pub chunk_rows: i64,
    /// Event retention of every imported site, link and pixel: 1y, 2y, 3y, 5y, 10y or forever
    /// (0016). Events before it are not imported. Runs started before this option kept 1y.
    #[arg(long, default_value = "forever", value_parser = parse_retention)]
    #[serde(default)]
    pub retention: Retention,
}

fn parse_retention(code: &str) -> Result<Retention, String> {
    Retention::from_code(code).ok_or_else(|| {
        let codes: Vec<&str> = Retention::ALL.iter().map(|r| r.code()).collect();
        format!("expected one of {}", codes.join(", "))
    })
}

/// What every step reads and writes through.
pub(crate) struct Ctx {
    source: Source,
    layout: Layout,
    data_dir: PathBuf,
    db: Db,
    storage: Storage,
    store: Store,
    secrets: SecretKey,
    options: Options,
    timezone: Tz,
    clock_ms: i64,
    daily_key: [u8; 32],
    two_factor_key: Option<[u8; 32]>,
    progress: Reporter,
}

impl Ctx {
    /// The daily pseudonym of an imported Umami session on one local day. Keyed by the
    /// installation root secret, so it is stable across resumes and does not reveal the source
    /// session; it never meets a live pseudonym, which comes from day keys (0001).
    fn daily_id(&self, site: u128, session: u128, local_day: i32) -> u128 {
        let mut input = [0u8; 36];
        input[..16].copy_from_slice(&site.to_be_bytes());
        input[16..32].copy_from_slice(&session.to_be_bytes());
        input[32..].copy_from_slice(&local_day.to_be_bytes());
        let hash = blake3::keyed_hash(&self.daily_key, &input);
        u128::from_be_bytes(hash.as_bytes()[..16].try_into().expect("16 of 32 bytes"))
    }
}

/// A UUID for an object Holocron needs and Umami has no ID for, derived from the source ID it
/// belongs to, so every run creates the same one.
pub fn derived_id(kind: &str, source: &str) -> String {
    let hash = blake3::hash(format!("holocron import {kind} {source}").as_bytes());
    let bytes: [u8; 16] = hash.as_bytes()[..16].try_into().expect("16 of 32 bytes");
    uuid::Builder::from_random_bytes(bytes)
        .into_uuid()
        .hyphenated()
        .to_string()
}

pub(crate) fn internal(error: impl std::fmt::Display) -> Failure {
    Failure::new(Exit::Internal, "internal", error.to_string())
}

pub(crate) fn storage(error: impl std::fmt::Display) -> Failure {
    Failure::new(
        Exit::StorageUnavailable,
        "storage_unavailable",
        error.to_string(),
    )
}

pub(crate) fn source_error(error: impl std::fmt::Display) -> Failure {
    source::unavailable(error)
}

fn refused(code: &'static str, message: impl Into<String>) -> Failure {
    Failure::new(Exit::Refused, code, message)
}

fn usage(message: impl Into<String>) -> Failure {
    Failure::new(Exit::Usage, "usage", message)
}

/// Runs one `holocron import` command, printing its report on stdout.
pub async fn cli(command: Command) -> Result<(), Failure> {
    match command {
        Command::Inspect { source, timezone } => {
            let tz = parse_timezone(&timezone)?;
            let (report, outcome) = inspect(&source, tz, crate::db::now_ms()).await?;
            println!(
                "{}",
                serde_json::to_string_pretty(&report).map_err(internal)?
            );
            outcome
        }
        Command::Run(args) => {
            let budgets = Budgets {
                disk_reserve_bytes: args.disk_reserve_mib * 1024 * 1024,
                ..Budgets::default()
            };
            let manifest = run(args, budgets, crate::db::now_ms()).await?;
            println!(
                "{}",
                serde_json::to_string_pretty(&manifest).map_err(internal)?
            );
            Ok(())
        }
        Command::Status { data_dir } => {
            let report = status(&data_dir).await?;
            println!(
                "{}",
                serde_json::to_string_pretty(&report).map_err(internal)?
            );
            Ok(())
        }
    }
}

fn parse_timezone(name: &str) -> Result<Tz, Failure> {
    name.parse()
        .map_err(|_| usage(format!("unknown timezone {name}")))
}

/// Connects read only and checks the layout. A refusal carries the inspection's code.
async fn accepted(source: &mut Source) -> Result<Layout, Failure> {
    match verdict(source).await? {
        Verdict::Accept(accepted) => Ok(accepted.layout),
        Verdict::Refuse(refusal) => Err(refused(refusal.code, refusal.message)),
    }
}

async fn verdict(source: &mut Source) -> Result<Verdict, Failure> {
    if !source.has_migration_table().await? {
        return Ok(Verdict::Refuse(signature::Refusal {
            code: "not_umami",
            details: json!({}),
            message: "This database has no _prisma_migrations table, so it is not an Umami database or its schema was never migrated. Point --source at the database Umami uses. Holocron did not change the source.".into(),
        }));
    }
    Ok(signature::check(&source.observe().await?))
}

/// What an import warns about and proceeds with, each logged and as `{code, message}`: a
/// connection that may be unencrypted, and credentials that could change the source. Also
/// returns the write grants, empty for a read-only user.
async fn warnings(source: &mut Source) -> Result<(Vec<String>, Vec<Value>), Failure> {
    let grants = source.write_grants().await?;
    let mut warnings = Vec::new();
    if let Some(message) = source.warning.clone() {
        warnings.push((SetupImportWarningCode::SourceUnencrypted, message));
    }
    if !grants.is_empty() {
        warnings.push((
            SetupImportWarningCode::CredentialsWritable,
            writable(source.engine, &grants),
        ));
    }
    let warnings = warnings
        .into_iter()
        .map(|(code, message)| {
            tracing::warn!(?code, "{message}");
            json!({ "code": code, "message": message })
        })
        .collect();
    Ok((grants, warnings))
}

/// The warning for credentials that could write, with the SQL for a read-only user.
fn writable(engine: Engine, grants: &[String]) -> String {
    let guidance = match engine {
        Engine::Postgresql => {
            "CREATE ROLE holocron_import LOGIN PASSWORD '...'; GRANT CONNECT ON DATABASE umami TO holocron_import; GRANT USAGE ON SCHEMA public TO holocron_import; GRANT SELECT ON ALL TABLES IN SCHEMA public TO holocron_import;"
        }
        Engine::Mysql => {
            "CREATE USER 'holocron_import'@'%' IDENTIFIED BY '...'; GRANT SELECT ON umami.* TO 'holocron_import'@'%';"
        }
    };
    format!(
        "These credentials can change the source ({}). Holocron imports anyway and only reads: it makes its session read only before the first query. A read-only user also guards against mistakes outside Holocron. If you can, create one, for example `{guidance}`, and pass its URL.",
        grants.join(", ")
    )
}

/// The inspection report and whether the source can be imported.
pub async fn inspect(
    url: &str,
    timezone: Tz,
    now_ms: i64,
) -> Result<(Value, Result<(), Failure>), Failure> {
    let mut source = Source::connect(url).await?;
    let engine = source.engine;
    let (grants, warnings) = warnings(&mut source).await?;
    let accepted = match verdict(&mut source).await? {
        Verdict::Accept(accepted) => accepted,
        Verdict::Refuse(refusal) => {
            source.close().await;
            let report = json!({
                "outcome": "refuse", "engine": engine.name(), "code": refusal.code,
                "details": refusal.details, "message": refusal.message, "warnings": warnings,
            });
            return Ok((report, Err(refused(refusal.code, refusal.message))));
        }
    };
    let layout = accepted.layout;
    source.begin_snapshot().await?;
    let fingerprint = fingerprint(&mut source, layout).await?;
    // What each `--retention` choice leaves behind, at today's cutoffs (0016).
    let policies = holocron_analytics::identity::SitePolicies::new(
        holocron_analytics::identity::SessionPolicy::new(timezone, 30).map_err(internal)?,
    );
    let cutoffs: Vec<(Retention, i64)> = Retention::ALL
        .into_iter()
        .filter_map(|r| Some((r, policies.retention_cutoff_ms(now_ms, r)?)))
        .collect();
    let e = source.engine;
    let outside_sql: Vec<String> = cutoffs
        .iter()
        .map(|_| {
            e.int(&format!(
                "sum(CASE WHEN created_at < {} THEN 1 ELSE 0 END)",
                e.ms_param()
            ))
        })
        .collect();
    let args: Vec<Arg> = cutoffs.iter().map(|(_, c)| Arg::Int(*c)).collect();
    let events = source
        .rows(
            &format!(
                "SELECT count(*), {}, {}, {} FROM website_event",
                e.ms("min(created_at)"),
                e.ms("max(created_at)"),
                outside_sql.join(", ")
            ),
            &args,
        )
        .await?;
    let row = events.first().ok_or_else(|| internal("no aggregate row"))?;
    use sqlx::Row;
    let total: i64 = row.try_get(0).map_err(source_error)?;
    let mut outside = serde_json::Map::new();
    let mut retention_cutoffs = serde_json::Map::new();
    for (i, (retention, cutoff)) in cutoffs.iter().enumerate() {
        let count: Option<i64> = row.try_get(3 + i).map_err(source_error)?;
        outside.insert(retention.code().into(), count.unwrap_or(0).into());
        retention_cutoffs.insert(retention.code().into(), (*cutoff).into());
    }
    outside.insert(Retention::Forever.code().into(), 0.into());
    let size = source
        .scalar(
            match e {
                Engine::Postgresql => "SELECT pg_database_size(current_database())",
                Engine::Mysql => {
                    "SELECT CAST(sum(data_length + index_length) AS SIGNED) FROM information_schema.tables WHERE table_schema = DATABASE()"
                }
            },
            &[],
        )
        .await?;
    source.close().await;
    let mut report = json!({
        "outcome": "accept",
        "source": layout.source_name(),
        "migrations": signature::migration_count(layout),
        "credentials": if grants.is_empty() { json!("read_only") } else { json!({ "writable": grants }) },
        "preview": {
            "clock": now_ms,
            "timezone": timezone.name(),
            "events": {
                "total": total,
                "outside_retention": outside,
                "retention_cutoffs": retention_cutoffs,
                "first": row.try_get::<Option<i64>, _>(1).map_err(source_error)?,
                "last": row.try_get::<Option<i64>, _>(2).map_err(source_error)?,
            },
            "source_bytes": size,
            "rows": fingerprint["rows"],
        },
        "warnings": warnings,
    });
    if let Some(tolerated) = accepted.report() {
        report["tolerated"] = tolerated;
    }
    Ok((report, Ok(())))
}

/// Row counts of every table and the newest event, so a resume can tell the source changed.
async fn fingerprint(source: &mut Source, layout: Layout) -> Result<Value, Failure> {
    let mut rows = serde_json::Map::new();
    for table in signature::tables(layout) {
        let sql = format!("SELECT count(*) FROM {}", source.engine.quote(table));
        rows.insert(table.to_owned(), source.scalar(&sql, &[]).await?.into());
    }
    let newest = source
        .scalar(
            &format!(
                "SELECT {} FROM website_event",
                source.engine.ms("max(created_at)")
            ),
            &[],
        )
        .await?;
    Ok(json!({ "rows": rows, "newest_event": newest }))
}

/// Imports or resumes into `args.data_dir` and returns the reconciliation manifest. `now_ms`
/// becomes the import clock at the first start; a resume keeps the first one.
pub async fn run(args: RunArgs, budgets: Budgets, now_ms: i64) -> Result<Value, Failure> {
    crate::init::check_initialized(&args.data_dir)?;
    let _lock = crate::maintenance::lock::DataLock::acquire(&args.data_dir)?;
    let two_factor_key = args
        .two_factor_key_file
        .as_deref()
        .map(read_two_factor_key)
        .transpose()?;
    let job = Job {
        data_dir: args.data_dir,
        source: args.source,
        options: args.options,
        two_factor_key,
    };
    import(job, budgets, now_ms, Reporter::default()).await
}

/// One import into an initialized directory whose lock the caller holds. The source URL and
/// the 2FA key stay in memory (decision 0021).
pub struct Job {
    pub data_dir: PathBuf,
    pub source: String,
    pub options: Options,
    pub two_factor_key: Option<[u8; 32]>,
}

/// [`run`] without the lock, for setup mode, which holds it while the wizard runs. Reports each
/// checkpoint to `progress`.
pub async fn import(
    job: Job,
    budgets: Budgets,
    now_ms: i64,
    progress: Reporter,
) -> Result<Value, Failure> {
    let timezone = parse_timezone(&job.options.timezone)?;
    let dir = &job.data_dir;
    let db = Db::open(dir).await.map_err(storage)?;
    if let Some(run) = state::run(&db).await.map_err(internal)?
        && let Some(manifest) = run.manifest
    {
        db.close().await;
        return Ok(manifest);
    }

    let mut source = Source::connect(&job.source).await?;
    let layout = accepted(&mut source).await?;
    warnings(&mut source).await?;
    source.begin_snapshot().await?;
    let fingerprint = fingerprint(&mut source, layout).await?;
    let options = serde_json::to_value(&job.options).map_err(internal)?;
    let clock_ms = match state::run(&db).await.map_err(internal)? {
        Some(run) => {
            if run.source != layout.source_name() || run.fingerprint != fingerprint {
                return Err(refused(
                    "source_changed",
                    format!(
                        "The source differs from the one this import started with ({} then, {} now, or other row counts). Resume only against the same stopped snapshot. To import a newer snapshot, start over with a new data directory.",
                        run.source,
                        layout.source_name()
                    ),
                ));
            }
            if serde_json::from_value::<Options>(run.options.clone())
                .ok()
                .as_ref()
                != Some(&job.options)
            {
                return Err(usage(format!(
                    "This import was started with {}. Resume it with the same --timezone, --login-domain, --chunk-rows and --retention.",
                    run.options
                )));
            }
            run.clock_ms
        }
        None => {
            let sites: i64 = sqlx::query_scalar("SELECT count(*) FROM sites")
                .fetch_one(&db.reader)
                .await
                .map_err(internal)?;
            let fresh = Storage::open(dir, budgets.clone()).await.map_err(storage)?;
            let applied = fresh.status();
            fresh.close().await.map_err(storage)?;
            if sites > 0 || applied.watermark.batch > 0 || applied.inbox_records > 0 {
                return Err(refused(
                    "destination_not_new",
                    format!(
                        "{} already has sites or analytics. Import into a new data directory made with `holocron init`.",
                        dir.display()
                    ),
                ));
            }
            state::start(&db, &layout.source_name(), &fingerprint, &options, now_ms)
                .await
                .map_err(internal)?;
            now_ms
        }
    };

    let root = crate::pseudonyms::root_secret(&db, clock_ms)
        .await
        .map_err(internal)?;
    progress.start(
        layout,
        &fingerprint,
        &state::steps(&db).await.map_err(internal)?,
    );
    let mut ctx = Ctx {
        source,
        layout,
        data_dir: dir.clone(),
        storage: Storage::open(dir, budgets).await.map_err(storage)?,
        store: Store::open(dir).await.map_err(storage)?,
        secrets: SecretKey::load_or_create(dir).map_err(storage)?,
        db,
        options: job.options,
        timezone,
        clock_ms,
        daily_key: blake3::derive_key("holocron 2026-10-05 imported daily pseudonym", &root),
        two_factor_key: job.two_factor_key,
        progress,
    };
    let result = steps(&mut ctx).await;
    let Ctx {
        source,
        db,
        storage: store,
        store: replays,
        ..
    } = ctx;
    source.close().await;
    replays.close().await;
    store.close().await.map_err(storage)?;
    db.close().await;
    result
}

async fn steps(ctx: &mut Ctx) -> Result<Value, Failure> {
    crate::recordings::heatmaps::create(&ctx.storage)
        .await
        .map_err(storage)?;
    control::import(ctx).await?;
    let sites: Vec<String> = sqlx::query_scalar("SELECT id FROM sites")
        .fetch_all(&ctx.db.reader)
        .await
        .map_err(internal)?;
    let mut conn = ctx.db.reader.acquire().await.map_err(internal)?;
    for site in &sites {
        crate::sites::policy::mirror(&mut conn, &ctx.storage, site)
            .await
            .map_err(internal)?;
    }
    drop(conn);
    events::import(ctx).await?;
    replays::import(ctx).await?;
    ctx.progress.finishing();
    ctx.storage.drain().await;
    imported_history(ctx).await?;
    run_until_clean(&ctx.storage, Limits::default())
        .await
        .map_err(storage)?;
    let manifest = reconcile::manifest(ctx).await?;
    state::complete(&ctx.db, &manifest)
        .await
        .map_err(internal)?;
    Ok(manifest)
}

/// Marks the span of imported events per site, which results carry as a limitation.
async fn imported_history(ctx: &Ctx) -> Result<(), Failure> {
    let source = ctx.layout.source_name();
    ctx.storage
        .write(move |tx| {
            tx.execute("DELETE FROM imported_history", [])?;
            tx.execute(
                "INSERT INTO imported_history
                 SELECT site, ?, min(event_time), max(event_time) FROM events GROUP BY site",
                [source],
            )?;
            Ok(())
        })
        .await
        .map_err(storage)
}

fn read_two_factor_key(path: &Path) -> Result<[u8; 32], Failure> {
    let text =
        std::fs::read_to_string(path).map_err(|e| usage(format!("{}: {e}", path.display())))?;
    parse_two_factor_key(&text)
        .ok_or_else(|| usage(format!("{} must hold 64 hex characters", path.display())))
}

/// Umami's `TWO_FACTOR_ENCRYPTION_KEY`: 64 hex characters.
pub fn parse_two_factor_key(text: &str) -> Option<[u8; 32]> {
    data_encoding::HEXLOWER_PERMISSIVE
        .decode(text.trim().as_bytes())
        .ok()
        .and_then(|bytes| bytes.try_into().ok())
}

/// The stored run of `db`'s directory: source, options and, once complete, the manifest.
pub(crate) async fn stored_run(db: &Db) -> sqlx::Result<Option<state::Run>> {
    state::run(db).await
}

/// The stored run as a progress frame, for a wizard that attaches before a job of this process
/// reported anything.
pub(crate) async fn stored_progress(db: &Db) -> sqlx::Result<SetupImportProgress> {
    let Some(run) = state::run(db).await? else {
        return Ok(SetupImportProgress::default());
    };
    let complete = run.manifest.is_some();
    let (sender, receiver) = watch::channel(SetupImportProgress {
        state: match complete {
            true => SetupImportState::Complete,
            false => SetupImportState::Interrupted,
        },
        ..SetupImportProgress::default()
    });
    let layout = [Layout::Umami3Postgresql, Layout::Umami2Mysql]
        .into_iter()
        .find(|layout| layout.source_name() == run.source);
    if let Some(layout) = layout {
        Reporter::new(sender.clone()).start(layout, &run.fingerprint, &state::steps(db).await?);
    }
    sender.send_modify(|progress| {
        for step in &mut progress.steps {
            if complete {
                step.state = SetupStepState::Done;
            } else if step.state == SetupStepState::Running {
                step.state = SetupStepState::Pending;
            }
        }
    });
    Ok(receiver.borrow().clone())
}

/// The import state and, once complete, the manifest.
pub async fn status(data_dir: &Path) -> Result<Value, Failure> {
    crate::init::check_initialized(data_dir)?;
    let db = Db::open(data_dir).await.map_err(storage)?;
    let report = async {
        let run = state::run(&db).await?;
        Ok::<_, sqlx::Error>(json!({
            "import": state(&db).await?,
            "clock": run.as_ref().map(|r| r.clock_ms),
            "options": run.as_ref().map(|r| r.options.clone()),
            "steps": state::steps(&db).await?,
            "manifest": run.and_then(|r| r.manifest),
        }))
    }
    .await
    .map_err(internal);
    db.close().await;
    report
}
