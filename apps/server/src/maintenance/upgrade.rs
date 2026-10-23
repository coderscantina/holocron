//! `holocron upgrade` (0006 Schema versions): a verified pre-upgrade backup first, then the
//! SQLite files, then DuckDB. SQLite records each migration as it commits; DuckDB records its
//! version last. The marker file names the backup from the first step until the last one, so a
//! failed or killed upgrade is an explicit state: `serve` refuses it, and a rerun resumes from
//! the last completed migration without a second backup.

use std::fs;
use std::path::{Path, PathBuf};

use holocron_storage::Budgets;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::backup::{self, Source};
use super::lock::DataLock;
use super::versions::{self, OLDEST, UPGRADE_MARKER, Versions};
use crate::db::Db;
use crate::init::{Exit, Failure, check_initialized};
use crate::recordings::store::Store;

#[derive(Debug, Serialize, Deserialize)]
struct Marker {
    backup: PathBuf,
    from: Versions,
}

pub async fn upgrade(data_dir: &Path, backup_to: Option<&Path>) -> Result<Value, Failure> {
    check_initialized(data_dir)?;
    let _lock = DataLock::acquire(data_dir)?;
    let release = versions::release();
    let marker_path = data_dir.join(UPGRADE_MARKER);
    let marker = match fs::read_to_string(&marker_path) {
        Ok(text) => {
            let marker: Marker = serde_json::from_str(&text).map_err(|e| {
                Failure::new(
                    Exit::Refused,
                    "upgrade_interrupted",
                    format!("{}: {e}", marker_path.display()),
                )
            })?;
            // Resuming relies on the backup the first run took.
            backup::verify(&marker.backup).await?;
            marker
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let stored = versions::stored(data_dir).await?;
            if let Some(newer) = stored.newer_than(release) {
                return Err(Failure::new(
                    Exit::Refused,
                    "data_newer_than_binary",
                    format!("{newer}; install the matching or a newer release"),
                ));
            }
            if !stored.older_than(release) {
                return Ok(json!({ "state": "current", "versions": stored }));
            }
            if stored.older_than(OLDEST) {
                return Err(Failure::new(
                    Exit::Refused,
                    "upgrade_unsupported",
                    format!(
                        "the data predates the oldest schemas this release upgrades ({OLDEST:?}); upgrade with an intermediate release first"
                    ),
                ));
            }
            let to = backup_to.ok_or_else(|| {
                Failure::new(
                    Exit::Usage,
                    "usage",
                    "--backup-to is required: the upgrade takes a backup first",
                )
            })?;
            let (backup, _) = backup::take(Source::Stopped(data_dir), to).await?;
            let marker = Marker {
                backup,
                from: stored,
            };
            fs::write(
                &marker_path,
                serde_json::to_string(&marker).expect("markers serialize"),
            )
            .map_err(|e| Failure::storage(data_dir, e))?;
            marker
        }
        Err(e) => return Err(Failure::storage(data_dir, e)),
    };

    let failed = |step: &str, error: &dyn std::fmt::Display| {
        Failure::new(
            Exit::Internal,
            "upgrade_failed",
            format!(
                "{step} failed: {error}. The data directory is half upgraded and is not served. \
                 Fix the cause and run `holocron upgrade --data-dir {dir}` again to resume, or \
                 restore the pre-upgrade backup into a new directory: \
                 holocron restore --from {backup} --data-dir <new directory>",
                dir = data_dir.display(),
                backup = marker.backup.display(),
            ),
        )
    };
    Db::open(data_dir)
        .await
        .map_err(|e| failed("control.sqlite migration", &e))?
        .close()
        .await;
    Store::open(data_dir)
        .await
        .map_err(|e| failed("recordings.sqlite migration", &e))?
        .close()
        .await;
    holocron_storage::upgrade(data_dir, &Budgets::default())
        .await
        .map_err(|e| failed("inbox and analytics migration", &e))?;
    let upgraded = versions::stored(data_dir).await?;
    if upgraded != release {
        return Err(failed(
            "version check",
            &format!("the data is at {upgraded:?}, the release at {release:?}"),
        ));
    }
    fs::remove_file(&marker_path).map_err(|e| Failure::storage(data_dir, e))?;
    Ok(json!({
        "state": "upgraded",
        "from": marker.from,
        "to": release,
        "backup": marker.backup,
    }))
}
