//! Operations and recovery (milestone 12, decision 0014): coordinated shutdown, backup and
//! restore, schema upgrade, retention, erasure, compaction, health and scrubbed diagnostics.
//!
//! Work that touches several engines runs as a durable job (`maintenance_jobs` in the control
//! database), one at a time, beside request handling. Dashboard commands and operator
//! commands queue jobs; a running server picks them up, and without a server the operator
//! command composes the services itself and runs the queue. Every command ends with the exit
//! codes of 0006: 0 success, 1 internal, 2 usage, 3 refused by data-directory state, 4 storage
//! unavailable.

pub mod backup;
pub mod commands;
pub mod compaction;
pub mod diagnostics;
mod erasure;
pub mod health;
pub mod jobs;
pub mod lock;
pub mod restore;
mod retention;
pub mod samples;
mod status;
pub mod upgrade;
pub mod versions;

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use holocron_storage::{Budgets, Storage};
use tokio::sync::Notify;

use crate::clock::Clock;
use crate::config::{CollectConfig, Config};
use crate::db::Db;
use crate::init::{Exit, Failure};
use crate::state::AppState;

/// Maintenance state shared with request handling.
#[derive(Default)]
pub struct Maintenance {
    /// Wakes the job runner after a command queued a job.
    pub wake: Notify,
    stopping: AtomicBool,
    /// The last compaction pass since start. Compaction runs beside the job queue, so it has no
    /// row in `maintenance_jobs`.
    compaction: Mutex<Option<CompactionPass>>,
}

/// When a compaction pass ran, Unix milliseconds, and whether it failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompactionPass {
    pub started_at: i64,
    pub finished_at: i64,
    pub failed: bool,
}

impl Maintenance {
    /// Marks the server as shutting down: readiness answers unavailable from now on.
    pub fn stop(&self) {
        self.stopping.store(true, Ordering::SeqCst);
    }

    pub fn stopping(&self) -> bool {
        self.stopping.load(Ordering::SeqCst)
    }

    pub fn compacted(&self, pass: CompactionPass) {
        *self.compaction.lock().expect("compaction pass lock") = Some(pass);
    }

    pub fn last_compaction(&self) -> Option<CompactionPass> {
        *self.compaction.lock().expect("compaction pass lock")
    }
}

/// The services of one data directory, composed the same way for `serve` and for operator
/// commands that run without a server.
pub struct Runtime {
    pub state: AppState,
    storage: Arc<Storage>,
    db: Db,
}

impl Runtime {
    /// Opens every engine without changing a schema. Data of another schema, or an inbox
    /// and analytics file that do not belong together, are refused with exit 3.
    pub async fn open(config: Config, budgets: Budgets, clock: Clock) -> Result<Self, Failure> {
        let dir = config.data_dir.clone();
        let db = Db::connect(&dir)
            .await
            .map_err(|e| Failure::storage(&dir, e))?;
        let storage = Arc::new(
            Storage::open(&dir, budgets)
                .await
                .map_err(|e| storage_failure(&dir, e))?,
        );
        let state = AppState::new(db.clone(), storage.clone(), config, clock)
            .await
            .map_err(|e| Failure::storage(&dir, e))?;
        Ok(Self { state, storage, db })
    }

    /// Settings for an operator command: the defaults of `serve`.
    pub fn config(data_dir: &Path) -> Config {
        Config {
            data_dir: data_dir.to_path_buf(),
            bind: ([127, 0, 0, 1], 0).into(),
            web_dir: None,
            public_origin: None,
            instance_name: "Holocron".into(),
            setup_token: None,
            backup_dir: "/backups".into(),
            collect: CollectConfig::default(),
            disk_reserve_bytes: Budgets::default().disk_reserve_bytes,
        }
    }

    /// Shuts the engines down in order (see [`Storage::shutdown`]).
    pub async fn close(self) -> Result<(), Failure> {
        let dir = self.state.config.data_dir.clone();
        self.state.maintenance.stop();
        let result = self.storage.shutdown().await;
        self.state.recordings.store.close().await;
        self.db.close().await;
        result.map_err(|e| storage_failure(&dir, e))
    }
}

/// Exit outcome of a storage error.
pub fn storage_failure(dir: &Path, error: holocron_storage::Error) -> Failure {
    use holocron_storage::Error;
    match error {
        Error::UpgradeRequired(message) => Failure::new(
            Exit::Refused,
            "upgrade_required",
            format!("{message}; run `holocron upgrade` (it takes a backup first)"),
        ),
        Error::DataNewer(message) => Failure::new(
            Exit::Refused,
            "data_newer_than_binary",
            format!("{message}; install the matching or a newer release"),
        ),
        Error::Mismatch(message) => Failure::new(
            Exit::Refused,
            "engines_mismatched",
            format!(
                "{message}. The engine files were copied or restored separately; restore a whole backup with `holocron restore` instead"
            ),
        ),
        error => Failure::storage(dir, error),
    }
}

fn internal(error: impl std::fmt::Display) -> Failure {
    Failure::new(Exit::Internal, "internal", error.to_string())
}
