//! Operator commands. `restore` and `upgrade` work on a stopped installation. The others are
//! jobs: while a server runs they are queued for it and the command waits for the result;
//! without one the command takes the data directory lock and runs the queue itself.

use std::path::{Path, PathBuf};
use std::time::Duration;

use clap::Args;
use holocron_storage::Budgets;
use serde_json::Value;
use uuid::Uuid;

use super::jobs::{self, Job};
use super::lock::DataLock;
use super::{Runtime, internal, restore, upgrade, versions};
use crate::clock::Clock;
use crate::db::{self, Db};
use crate::init::{Exit, Failure, check_initialized};
use crate::pseudonyms::{Pseudonyms, root_secret};

const POLL: Duration = Duration::from_millis(200);

#[derive(Debug, Args)]
pub struct DataDir {
    /// Data directory created by `holocron init`.
    #[arg(long, env = "HOLOCRON_DATA_DIR")]
    pub data_dir: PathBuf,
}

#[derive(Debug, Args)]
pub struct BackupArgs {
    #[command(flatten)]
    pub dir: DataDir,
    /// Directory the backup is written into, on other storage than the data directory. Each
    /// backup is a new directory inside it.
    #[arg(long)]
    pub to: PathBuf,
}

#[derive(Debug, Args)]
pub struct RestoreArgs {
    /// A backup directory written by `holocron backup`.
    #[arg(long)]
    pub from: PathBuf,
    /// A new, empty data directory to restore into.
    #[arg(long, env = "HOLOCRON_DATA_DIR")]
    pub data_dir: PathBuf,
}

#[derive(Debug, Args)]
pub struct UpgradeArgs {
    #[command(flatten)]
    pub dir: DataDir,
    /// Where the pre-upgrade backup goes. Needed unless an interrupted upgrade resumes.
    #[arg(long)]
    pub backup_to: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub struct EraseProfileArgs {
    #[command(flatten)]
    pub dir: DataDir,
    /// The site the profile belongs to.
    #[arg(long)]
    pub site: Uuid,
    /// The ID the site passed to `identify`. Only its keyed profile ID is stored in the job.
    #[arg(long)]
    pub distinct_id: String,
}

pub async fn backup(args: BackupArgs) -> Result<Value, Failure> {
    let to = std::path::absolute(&args.to).map_err(|e| Failure::storage(&args.to, e))?;
    run_job(&args.dir.data_dir, Job::Backup { to }).await
}

pub async fn restore(args: RestoreArgs) -> Result<Value, Failure> {
    restore::restore(&args.from, &args.data_dir).await
}

pub async fn upgrade(args: UpgradeArgs) -> Result<Value, Failure> {
    upgrade::upgrade(&args.dir.data_dir, args.backup_to.as_deref()).await
}

pub async fn retention(args: DataDir) -> Result<Value, Failure> {
    run_job(&args.data_dir, Job::Retention).await
}

pub async fn diagnostics(args: DataDir) -> Result<Value, Failure> {
    run_job(&args.data_dir, Job::Diagnostics).await
}

/// Erases a consented visitor's profile history (0001 Revocation: the explicit erasure).
pub async fn erase_profile(args: EraseProfileArgs) -> Result<Value, Failure> {
    let dir = &args.dir.data_dir;
    check_initialized(dir)?;
    let db = Db::connect(dir)
        .await
        .map_err(|e| Failure::storage(dir, e))?;
    let found: sqlx::Result<Option<u128>> = async {
        let site = args.site.hyphenated().to_string();
        let exists: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM sites WHERE id = ?)")
            .bind(&site)
            .fetch_one(&db.reader)
            .await?;
        let root = root_secret(&db, db::now_ms()).await?;
        let profile =
            Pseudonyms::new(db.clone(), root).profile_id(args.site.as_u128(), &args.distinct_id);
        Ok(exists.then_some(profile))
    }
    .await;
    db.close().await;
    let profile = found.map_err(internal)?.ok_or_else(|| {
        Failure::new(
            Exit::Usage,
            "usage",
            format!("site {} does not exist", args.site),
        )
    })?;
    run_job(
        dir,
        Job::EraseProfile {
            site: args.site,
            profile: Uuid::from_u128(profile),
        },
    )
    .await
}

/// Runs `job` on the server that holds the data directory, or here when none does.
async fn run_job(dir: &Path, job: Job) -> Result<Value, Failure> {
    check_initialized(dir)?;
    match DataLock::try_acquire(dir).map_err(|e| Failure::storage(dir, e))? {
        Some(_lock) => {
            versions::check_dir(dir).await?;
            crate::imports::refuse_while_running(dir).await?;
            let runtime =
                Runtime::open(Runtime::config(dir), Budgets::default(), Clock::system()).await?;
            let state = &runtime.state;
            let result =
                match jobs::enqueue(&state.db.writer, &job, state.collector.clock().now_ms()).await
                {
                    Ok(id) => jobs::run_until(state, &id).await,
                    Err(error) => Err(internal(error)),
                };
            runtime.close().await?;
            result
        }
        None => {
            // An import holds the lock and never runs jobs, so waiting for one would hang.
            crate::imports::refuse_while_running(dir).await?;
            let db = Db::connect(dir)
                .await
                .map_err(|e| Failure::storage(dir, e))?;
            let result: sqlx::Result<Result<Value, Failure>> = async {
                let id = jobs::enqueue(&db.writer, &job, db::now_ms()).await?;
                loop {
                    if let Some(result) = jobs::finished(&db.reader, &id).await? {
                        return Ok(result);
                    }
                    tokio::time::sleep(POLL).await;
                }
            }
            .await;
            db.close().await;
            result.map_err(internal)?
        }
    }
}
