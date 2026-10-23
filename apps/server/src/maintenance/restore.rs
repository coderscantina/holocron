//! Restore into a new data directory (decision 0014): verify the manifest and every checksum,
//! refuse data newer than this release, copy, check the engine pair against the manifest's
//! positions, then open everything once: credentials must be usable and every record the
//! backup held must reach analytics. Only then is the directory marked initialized.

use std::fs;
use std::io;
use std::path::Path;

use holocron_storage::{Budgets, INBOX_FORMAT, Lane};
use serde_json::{Value, json};

use super::backup::{self, FILES, Manifest};
use super::lock::DataLock;
use super::versions;
use super::{Runtime, internal, storage_failure};
use crate::clock::Clock;
use crate::identity::secrets::SecretKey;
use crate::init::{DONE_MARKER, Exit, Failure};

/// Written before the first file is copied and removed once the restore is validated. A
/// directory with it is not served, and a rerun starts over.
pub const IN_PROGRESS_MARKER: &str = "holocron.restore-in-progress";

pub async fn restore(from: &Path, data_dir: &Path) -> Result<Value, Failure> {
    let manifest = backup::verify(from).await?;
    let release = versions::release();
    if let Some(newer) = manifest.versions.newer_than(release) {
        return Err(Failure::new(
            Exit::Refused,
            "data_newer_than_binary",
            format!("{newer}; restore with the matching or a newer release"),
        ));
    }
    if manifest.inbox_format != INBOX_FORMAT && manifest.positions.inbox_records > 0 {
        return Err(Failure::new(
            Exit::Refused,
            "inbox_format_mismatch",
            format!(
                "the backup holds {} waiting records in inbox format {}, this release reads {INBOX_FORMAT}; restore with the release that took it and let the inbox drain",
                manifest.positions.inbox_records, manifest.inbox_format
            ),
        ));
    }
    prepare(data_dir)?;
    let _lock = DataLock::acquire(data_dir)?;
    let io = |e: io::Error| Failure::storage(data_dir, e);
    fs::write(data_dir.join(IN_PROGRESS_MARKER), b"").map_err(io)?;
    for file in FILES {
        fs::copy(from.join(file), data_dir.join(file)).map_err(io)?;
    }
    let positions = holocron_storage::inspect(data_dir)
        .await
        .map_err(|e| storage_failure(data_dir, e))?;
    if positions != manifest.positions {
        return Err(Failure::new(
            Exit::Refused,
            "restore_mismatch",
            format!(
                "the restored engines stand at {positions:?}, the manifest at {:?}",
                manifest.positions
            ),
        ));
    }
    let current = manifest.versions == release;
    let applied = if current {
        Some(validate(data_dir, &manifest).await?)
    } else {
        None
    };
    fs::write(data_dir.join(DONE_MARKER), b"").map_err(io)?;
    fs::remove_file(data_dir.join(IN_PROGRESS_MARKER)).map_err(io)?;
    Ok(json!({
        "data_dir": data_dir,
        "state": if current { "ready" } else { "upgrade_required" },
        "positions": positions,
        "records_applied": applied,
    }))
}

/// A missing or empty directory, or the leftovers of an interrupted restore.
fn prepare(dir: &Path) -> Result<(), Failure> {
    let io = |e: io::Error| Failure::storage(dir, e);
    if dir.join(IN_PROGRESS_MARKER).exists() {
        for entry in fs::read_dir(dir).map_err(io)? {
            let path = entry.map_err(io)?.path();
            if path.is_dir() {
                fs::remove_dir_all(&path).map_err(io)?;
            } else {
                fs::remove_file(&path).map_err(io)?;
            }
        }
        return Ok(());
    }
    let mut entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return fs::create_dir_all(dir).map_err(io);
        }
        Err(e) => return Err(io(e)),
    };
    if entries.next().is_none() {
        return Ok(());
    }
    let code = if dir.join(DONE_MARKER).exists() {
        "already_initialized"
    } else {
        "directory_not_empty"
    };
    Err(Failure::new(
        Exit::Refused,
        code,
        format!("{}: restore needs a new, empty directory", dir.display()),
    ))
}

/// Opens the restored directory like `serve` does, checks that credentials are usable, and
/// lets transfer apply what the backup's inbox held. Returns the records applied.
async fn validate(dir: &Path, manifest: &Manifest) -> Result<i64, Failure> {
    SecretKey::load_or_create(dir).map_err(|e| Failure::storage(dir, e))?;
    let runtime = Runtime::open(Runtime::config(dir), Budgets::default(), Clock::system()).await?;
    let state = &runtime.state;
    let checked = async {
        let (admins, secrets): (i64, i64) = sqlx::query_as(
            "SELECT (SELECT count(*) FROM users WHERE install_role = 'install_admin'),
                    (SELECT count(*) FROM installation_secret)",
        )
        .fetch_one(&state.db.reader)
        .await?;
        let storage = state.collector.storage();
        storage.drain().await;
        let applied = storage
            .read(Lane::Live, |s| {
                s.connection().query_row(
                    "SELECT coalesce(sum(records), 0)::BIGINT FROM applied_batches",
                    [],
                    |r| r.get::<_, i64>(0),
                )
            })
            .await?
            .value;
        anyhow::Ok((admins, secrets, applied))
    }
    .await;
    runtime.close().await?;
    let (admins, secrets, applied) = checked.map_err(internal)?;

    let invalid =
        |message: &str| Failure::new(Exit::Refused, "restore_invalid", message.to_owned());
    if admins == 0 {
        return Err(invalid("the backup has no install admin who could log in"));
    }
    if secrets != 1 {
        return Err(invalid("the backup has no identity root secret"));
    }
    let expected = manifest.positions.ledger_records + manifest.positions.inbox_records;
    if applied != expected {
        return Err(Failure::new(
            Exit::Refused,
            "restore_mismatch",
            format!(
                "analytics holds {applied} records after transfer, the backup accepted {expected}"
            ),
        ));
    }
    Ok(applied - manifest.positions.ledger_records)
}
