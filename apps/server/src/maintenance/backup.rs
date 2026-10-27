//! Coordinated backup (decision 0014): one directory holding every engine file, the secret
//! key, and one manifest of schema versions, transfer positions and checksums.
//!
//! On a running server the analytics file and the inbox are copied while the analytical
//! writer waits, so the pair matches; admission and reads go on, and the inbox absorbs new
//! events. The control database and the replay store are copied right after; a job they hold
//! that the analytics copy predates runs again after a restore. The copy is checked as a pair,
//! hashed, verified, and only then renamed into place.

use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::{TimeZone, Utc};
use holocron_storage::{ANALYTICS_FILE, INBOX_FILE, INBOX_FORMAT, Positions};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

use super::versions::{self, Versions};
use super::{internal, storage_failure};
use crate::db::CONTROL_DB_FILE;
use crate::identity::secrets::SECRETS_KEY_FILE;
use crate::init::{Exit, Failure};
use crate::recordings::store::RECORDINGS_DB_FILE;
use crate::state::AppState;

pub const MANIFEST_FILE: &str = "manifest.json";
const FORMAT: i64 = 1;
pub const PREFIX: &str = "holocron-backup-";
const PARTIAL: &str = ".partial";

/// Files of a backup, named as in the data directory.
pub const FILES: [&str; 5] = [
    CONTROL_DB_FILE,
    INBOX_FILE,
    RECORDINGS_DB_FILE,
    ANALYTICS_FILE,
    SECRETS_KEY_FILE,
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub format: i64,
    pub created_at_ms: i64,
    pub binary_version: String,
    pub versions: Versions,
    pub inbox_format: i64,
    pub positions: Positions,
    /// How long transfer and analytical writes waited; zero for a stopped installation.
    pub pause_ms: u64,
    pub files: Vec<FileEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEntry {
    pub name: String,
    pub bytes: u64,
    pub blake3: String,
}

pub enum Source<'a> {
    /// A serving installation, paused while its analytics and inbox are copied.
    Running(&'a AppState),
    /// The data directory of a stopped installation, at any schema (the pre-upgrade backup).
    Stopped(&'a Path),
}

/// Writes a verified backup into a new directory inside `to` and returns its path.
pub async fn take(source: Source<'_>, to: &Path) -> Result<(PathBuf, Manifest), Failure> {
    let io = |e: io::Error| Failure::storage(to, e);
    fs::create_dir_all(to).map_err(io)?;
    remove_partials(to).map_err(io)?;
    let now = match source {
        Source::Running(state) => state.collector.clock().now_ms(),
        Source::Stopped(_) => crate::db::now_ms(),
    };
    let stamp = Utc
        .timestamp_millis_opt(now)
        .single()
        .expect("clock in range")
        .format("%Y%m%dT%H%M%SZ");
    let name = format!("{PREFIX}{stamp}-{}", &crate::db::new_id()[..8]);
    let partial = to.join(format!(".{name}{PARTIAL}"));
    fs::create_dir(&partial).map_err(io)?;
    match write(source, &partial, now).await {
        Ok(manifest) => {
            let done = to.join(&name);
            fs::rename(&partial, &done).map_err(io)?;
            Ok((done, manifest))
        }
        Err(failure) => {
            let _ = fs::remove_dir_all(&partial);
            Err(failure)
        }
    }
}

async fn write(source: Source<'_>, dir: &Path, now: i64) -> Result<Manifest, Failure> {
    let (data_dir, versions, pause) = match source {
        Source::Running(state) => {
            let data_dir = state.config.data_dir.clone();
            let pause = state
                .collector
                .storage()
                .snapshot(dir)
                .await
                .map_err(|e| storage_failure(&data_dir, e))?;
            vacuum_into(&state.db.writer, &dir.join(CONTROL_DB_FILE)).await?;
            state
                .recordings
                .store
                .vacuum_into(&dir.join(RECORDINGS_DB_FILE))
                .await
                .map_err(internal)?;
            (data_dir, versions::release(), pause)
        }
        Source::Stopped(data_dir) => {
            if let Some(missing) = FILES.iter().find(|f| !data_dir.join(f).exists()) {
                return Err(Failure::new(
                    Exit::Refused,
                    "nothing_to_back_up",
                    format!(
                        "{} has no {missing}; start the server once first",
                        data_dir.display()
                    ),
                ));
            }
            let versions = versions::stored(data_dir).await?;
            holocron_storage::snapshot_stopped(data_dir, dir)
                .await
                .map_err(|e| storage_failure(data_dir, e))?;
            for file in [CONTROL_DB_FILE, RECORDINGS_DB_FILE] {
                let pool = SqlitePoolOptions::new()
                    .max_connections(1)
                    .connect_with(
                        SqliteConnectOptions::new()
                            .filename(data_dir.join(file))
                            .read_only(true),
                    )
                    .await
                    .map_err(internal)?;
                vacuum_into(&pool, &dir.join(file)).await?;
                pool.close().await;
            }
            (data_dir.to_path_buf(), versions, Duration::ZERO)
        }
    };
    fs::copy(data_dir.join(SECRETS_KEY_FILE), dir.join(SECRETS_KEY_FILE))
        .map_err(|e| Failure::storage(&data_dir, e))?;
    // A backup holds the secret key and every account: readable by the owner only, whatever
    // the permissions of the files it was copied from.
    let private = || -> io::Result<()> {
        for entry in fs::read_dir(dir)? {
            fs::set_permissions(entry?.path(), fs::Permissions::from_mode(0o600))?;
        }
        Ok(())
    };
    private().map_err(|e| Failure::storage(dir, e))?;
    // Checks the copies as a pair and folds a copied DuckDB WAL into its file.
    let positions = holocron_storage::inspect(dir)
        .await
        .map_err(|e| storage_failure(dir, e))?;
    let manifest = Manifest {
        format: FORMAT,
        created_at_ms: now,
        binary_version: env!("CARGO_PKG_VERSION").to_owned(),
        versions,
        inbox_format: INBOX_FORMAT,
        positions,
        pause_ms: pause.as_millis() as u64,
        files: hash_files(dir).await?,
    };
    let text = serde_json::to_string_pretty(&manifest).expect("manifests serialize");
    fs::write(dir.join(MANIFEST_FILE), text).map_err(|e| Failure::storage(dir, e))?;
    verify(dir).await?;
    Ok(manifest)
}

async fn vacuum_into(pool: &sqlx::SqlitePool, path: &Path) -> Result<(), Failure> {
    let target = path
        .to_str()
        .ok_or_else(|| internal(format!("{} is not UTF-8", path.display())))?;
    sqlx::query("VACUUM INTO ?")
        .bind(target)
        .execute(pool)
        .await
        .map_err(|e| Failure::storage(path, e))?;
    Ok(())
}

/// The manifest of a backup directory, without checking its files.
pub fn read_manifest(dir: &Path) -> Result<Manifest, Failure> {
    let refused = |code: &'static str, message: String| Failure::new(Exit::Refused, code, message);
    let text = fs::read_to_string(dir.join(MANIFEST_FILE)).map_err(|e| {
        refused(
            "backup_invalid",
            format!("{}: no readable {MANIFEST_FILE}: {e}", dir.display()),
        )
    })?;
    let manifest: Manifest = serde_json::from_str(&text).map_err(|e| {
        refused(
            "backup_invalid",
            format!("{}: {MANIFEST_FILE}: {e}", dir.display()),
        )
    })?;
    if manifest.format != FORMAT {
        return Err(refused(
            "backup_unsupported",
            format!("manifest format {} is not {FORMAT}", manifest.format),
        ));
    }
    Ok(manifest)
}

/// Reads a backup's manifest and checks every file against it.
pub async fn verify(dir: &Path) -> Result<Manifest, Failure> {
    let refused = |code: &'static str, message: String| Failure::new(Exit::Refused, code, message);
    let manifest = read_manifest(dir)?;
    let actual = hash_files(dir).await?;
    for name in FILES {
        let listed = manifest.files.iter().find(|f| f.name == name);
        let found = actual.iter().find(|f| f.name == name);
        if listed.is_none() || listed != found {
            return Err(refused(
                "backup_corrupt",
                format!(
                    "{}: {name} does not match the manifest; this backup cannot be restored",
                    dir.display()
                ),
            ));
        }
    }
    Ok(manifest)
}

/// Size and BLAKE3 hash of every backup file present in `dir`.
async fn hash_files(dir: &Path) -> Result<Vec<FileEntry>, Failure> {
    let dir = dir.to_path_buf();
    tokio::task::spawn_blocking(move || {
        FILES
            .iter()
            .filter(|name| dir.join(name).exists())
            .map(|name| {
                let path = dir.join(name);
                let mut file = fs::File::open(&path)?;
                let mut hasher = blake3::Hasher::new();
                let bytes = io::copy(&mut file, &mut hasher)?;
                Ok(FileEntry {
                    name: (*name).to_owned(),
                    bytes,
                    blake3: hasher.finalize().to_hex().to_string(),
                })
            })
            .collect::<io::Result<Vec<_>>>()
            .map_err(|e| Failure::storage(&dir, e))
    })
    .await
    .expect("hash task")
}

/// Removes what interrupted backups left in `to`. Returns how many there were.
fn remove_partials(to: &Path) -> io::Result<usize> {
    let mut removed = 0;
    for entry in fs::read_dir(to)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with(&format!(".{PREFIX}")) && name.ends_with(PARTIAL) {
            fs::remove_dir_all(entry.path())?;
            removed += 1;
        }
    }
    if removed > 0 {
        tracing::info!(removed, "removed interrupted backups");
    }
    Ok(removed)
}

/// What an operator sees after a backup.
pub fn summary(path: &Path, manifest: &Manifest) -> Value {
    json!({
        "path": path,
        "bytes": manifest.files.iter().map(|f| f.bytes).sum::<u64>(),
        "pause_ms": manifest.pause_ms,
        "versions": manifest.versions,
        "positions": manifest.positions,
    })
}
