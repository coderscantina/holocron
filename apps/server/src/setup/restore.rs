//! The restore path of setup mode (decision 0021): the backups on offer, the manifest of any
//! host path, and the restore as a job inside `serve` with its progress stream. It runs
//! `holocron restore` and, for a backup from an older release, `holocron upgrade` right after,
//! reports both as phases, then ends setup like the fresh path. The setup token authorizes
//! every step: no admin exists until the backup brings its own accounts.

use std::fs;
use std::path::{Path, PathBuf};

use axum::Json;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::Response;
use holocron_protocol::errors::ErrorCode;
use holocron_protocol::setup::{
    SetupBackup, SetupBackups, SetupImportError, SetupRestoreArgs, SetupRestoreInspectArgs,
    SetupRestoreInspection, SetupRestorePhase, SetupRestoreProgress, SetupRestoreState,
    SetupTokenArgs,
};

use super::{Phase, Shared};
use crate::error::ApiError;
use crate::init::Failure;
use crate::maintenance::backup::{self, Manifest};
use crate::maintenance::{restore, upgrade, versions};

/// Header of the progress stream, which a browser can't send a body on.
const TOKEN_HEADER: &str = "x-holocron-setup-token";

/// The backups in the backup directory, newest first, and the restore in progress. A missing
/// directory is an empty list: the compose files mount it, other installs may not.
pub(super) async fn backups(
    State(setup): State<Shared>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<SetupBackups>, ApiError> {
    let args: SetupTokenArgs = setup.admit(&headers, &body)?;
    setup.verify(&args.token)?;
    let directory = setup.config.backup_dir.clone();
    let listed = tokio::task::spawn_blocking({
        let directory = directory.clone();
        move || list(&directory)
    })
    .await
    .map_err(ApiError::internal)?;
    Ok(Json(SetupBackups {
        directory: directory.display().to_string(),
        backups: listed.map_err(|e| ApiError::internal(e.message))?,
        restore: setup.restore.borrow().clone(),
    }))
}

/// The manifest of the backup at a host path, or why it can't be restored here. The files are
/// checked when the restore starts.
pub(super) async fn inspect(
    State(setup): State<Shared>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<SetupRestoreInspection>, ApiError> {
    let args: SetupRestoreInspectArgs = setup.admit(&headers, &body)?;
    setup.verify(&args.token)?;
    let path = PathBuf::from(args.path.trim());
    let described = tokio::task::spawn_blocking(move || describe(&path))
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(match described {
        Ok(backup) => SetupRestoreInspection {
            backup: Some(backup),
            refusal: None,
        },
        Err(failure) => SetupRestoreInspection {
            backup: None,
            refusal: Some(error(&failure)),
        },
    }))
}

/// Starts the restore as a job of this process. While one runs, asking again answers its
/// progress, so a reloaded wizard attaches.
pub(super) async fn start(
    State(setup): State<Shared>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<SetupRestoreProgress>, ApiError> {
    let args: SetupRestoreArgs = setup.admit(&headers, &body)?;
    setup.verify(&args.token)?;
    let mut phase = setup.phase.lock().await;
    match *phase {
        Phase::Restoring => return Ok(Json(setup.restore.borrow().clone())),
        Phase::Claim => {}
        _ => return Err(ApiError::new(ErrorCode::SetupRequired)),
    }
    let path = PathBuf::from(args.path.trim());
    let manifest = match preflight(&path) {
        Ok(manifest) => manifest,
        Err(failure) => {
            let frame = SetupRestoreProgress {
                state: SetupRestoreState::Failed,
                error: Some(error(&failure)),
                ..SetupRestoreProgress::default()
            };
            setup.restore.send_replace(frame.clone());
            return Ok(Json(frame));
        }
    };
    *phase = Phase::Restoring;
    let frame = SetupRestoreProgress {
        state: SetupRestoreState::Running,
        phase: Some(SetupRestorePhase::Restoring),
        upgrade: manifest.versions.older_than(versions::release()),
        error: None,
    };
    setup.restore.send_replace(frame.clone());
    drop(phase);
    tracing::info!(from = %path.display(), upgrade = frame.upgrade, "setup: restore started");
    tokio::spawn(job(setup, path, frame.upgrade));
    Ok(Json(frame))
}

/// The progress stream: the current frame, then one per change, until setup mode stops.
pub(super) async fn progress(
    State(setup): State<Shared>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let token = headers
        .get(TOKEN_HEADER)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    setup.verify(token)?;
    Ok(super::frames_stream(&setup, setup.restore.subscribe()))
}

/// Restores, upgrades when the backup is older, and hands over to `serve`. A failure leaves
/// setup mode where it was, with the error in the last frame.
async fn job(setup: Shared, from: PathBuf, upgrading: bool) {
    let dir = setup.config.data_dir.clone();
    let result = async {
        let restored = restore::restore(&from, &dir).await?;
        if upgrading || restored["state"] == "upgrade_required" {
            setup.restore.send_modify(|frame| {
                frame.phase = Some(SetupRestorePhase::Upgrading);
                frame.upgrade = true;
            });
            upgrade::upgrade_restored(&dir, &from).await?;
        }
        Ok::<_, Failure>(())
    }
    .await;

    let mut phase = setup.phase.lock().await;
    match result {
        Ok(()) => {
            *phase = Phase::Done;
            setup
                .claimed
                .store(true, std::sync::atomic::Ordering::SeqCst);
            setup.restore.send_modify(|frame| {
                frame.state = SetupRestoreState::Complete;
                frame.phase = None;
            });
            tracing::info!("setup: restore finished");
            // Graceful shutdown lets open streams and requests finish before `serve` takes over.
            let _ = setup.done.send(true);
        }
        Err(failure) => {
            tracing::warn!(code = %failure.code, message = %failure.message, "setup: restore stopped");
            *phase = Phase::Claim;
            setup.restore.send_replace(SetupRestoreProgress {
                state: SetupRestoreState::Failed,
                error: Some(error(&failure)),
                ..SetupRestoreProgress::default()
            });
        }
    }
}

/// The checks that need no hashing: an absolute path, a readable manifest, a release that is not
/// older than the backup. The restore verifies every file next.
fn preflight(path: &Path) -> Result<Manifest, Failure> {
    if !path.is_absolute() {
        return Err(Failure::new(
            crate::init::Exit::Refused,
            "backup_invalid",
            "give the absolute path of the backup directory on the server",
        ));
    }
    let manifest = backup::read_manifest(path)?;
    if let Some(newer) = manifest.versions.newer_than(versions::release()) {
        return Err(Failure::new(
            crate::init::Exit::Refused,
            "data_newer_than_binary",
            newer,
        ));
    }
    Ok(manifest)
}

fn describe(path: &Path) -> Result<SetupBackup, Failure> {
    preflight(path).map(|manifest| entry(path, &manifest))
}

fn entry(path: &Path, manifest: &Manifest) -> SetupBackup {
    SetupBackup {
        path: path.display().to_string(),
        created_at_ms: manifest.created_at_ms,
        bytes: manifest.files.iter().map(|file| file.bytes).sum(),
        release: manifest.binary_version.clone(),
        upgrade: manifest.versions.older_than(versions::release()),
    }
}

/// The `holocron-backup-*` directories of `dir` with a readable manifest, newest first. Newer
/// backups are listed too: the wizard shows why they can't be restored.
fn list(dir: &Path) -> Result<Vec<SetupBackup>, Failure> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(Failure::storage(dir, e)),
    };
    let mut backups: Vec<SetupBackup> = entries
        .filter_map(Result::ok)
        .filter(|item| {
            item.file_name()
                .to_string_lossy()
                .starts_with(backup::PREFIX)
                && item.path().is_dir()
        })
        .filter_map(|item| {
            let manifest = backup::read_manifest(&item.path()).ok()?;
            Some(entry(&item.path(), &manifest))
        })
        .collect();
    backups.sort_by_key(|b| std::cmp::Reverse(b.created_at_ms));
    Ok(backups)
}

/// A failure in the wizard's words. The code stays the one 0014 names.
fn error(failure: &Failure) -> SetupImportError {
    let detail = &failure.message;
    let message = match &*failure.code {
        "data_newer_than_binary" => format!(
            "This backup comes from a newer Holocron than the one running here ({detail}). Run the newer release, then restore again."
        ),
        "backup_corrupt" => format!(
            "The backup does not match its manifest, so it is damaged or incomplete: {detail}. Restore another backup."
        ),
        "backup_invalid" => format!(
            "This is not a Holocron backup folder: {detail}. Pick a folder named holocron-backup-… that holocron backup wrote."
        ),
        "backup_unsupported" => {
            format!("This backup is in a format this release can't read: {detail}.")
        }
        "storage_unavailable" => format!(
            "Holocron couldn't read the backup or write the data directory: {detail}. Check that the server can read the folder and that the data volume has room."
        ),
        "inbox_format_mismatch" => format!(
            "The backup holds events that are still waiting, in a format this release can't read: {detail}."
        ),
        "restore_mismatch" | "restore_invalid" => format!(
            "The restored data did not pass its checks: {detail}. Nothing is served from it; restore again or pick another backup."
        ),
        "directory_not_empty" | "already_initialized" => format!(
            "The data directory is not empty: {detail}. Restore needs a new, empty directory."
        ),
        _ => detail.clone(),
    };
    SetupImportError {
        code: failure.code.to_string(),
        message,
    }
}
