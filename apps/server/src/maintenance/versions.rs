//! Schema versions (decisions 0006 and 0014): what this release runs on, what a data
//! directory holds, and whether `serve` may start on it. `serve` never migrates; only
//! `holocron upgrade` changes a schema.

use std::path::Path;

use holocron_storage::{ANALYTICS_SCHEMA, applied_migration, inbox_schema, latest_migration};
use serde::{Deserialize, Serialize};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

use crate::db::{CONTROL_DB_FILE, MIGRATOR};
use crate::init::{Exit, Failure};
use crate::recordings::store::{self, RECORDINGS_DB_FILE};

/// Written before the first migration of an upgrade and removed after the last. While it
/// exists the directory is half upgraded: `serve` refuses with `upgrade_interrupted`.
pub const UPGRADE_MARKER: &str = "holocron.upgrade-in-progress";

/// One schema version per engine file. SQLite files count their migrations; DuckDB records
/// its version in `schema_info`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Versions {
    pub control: i64,
    pub inbox: i64,
    pub recordings: i64,
    pub analytics: i64,
}

impl Versions {
    fn pairs(self, other: Versions) -> [(&'static str, i64, i64); 4] {
        [
            ("control.sqlite", self.control, other.control),
            ("inbox.sqlite", self.inbox, other.inbox),
            ("recordings.sqlite", self.recordings, other.recordings),
            ("analytics.duckdb", self.analytics, other.analytics),
        ]
    }

    /// Whether any file of `self` is newer than `release`.
    pub fn newer_than(self, release: Versions) -> Option<String> {
        self.pairs(release)
            .into_iter()
            .find(|(_, data, binary)| data > binary)
            .map(|(file, data, binary)| {
                format!("{file} has schema {data}, this release runs {binary}")
            })
    }

    pub fn older_than(self, other: Versions) -> bool {
        self.pairs(other)
            .into_iter()
            .any(|(_, data, binary)| data < binary)
    }
}

/// The versions this binary runs on.
pub fn release() -> Versions {
    Versions {
        control: latest_migration(&MIGRATOR),
        inbox: inbox_schema(),
        recordings: latest_migration(&store::MIGRATOR),
        analytics: ANALYTICS_SCHEMA,
    }
}

/// The oldest data this release upgrades: every schema since the first release.
pub const OLDEST: Versions = Versions {
    control: 1,
    inbox: 1,
    recordings: 1,
    analytics: 1,
};

/// Versions as stored. A file that does not exist yet counts as current: `serve` creates it.
pub async fn stored(dir: &Path) -> Result<Versions, Failure> {
    let release = release();
    let storage = holocron_storage::stored_schemas(dir)
        .await
        .map_err(|e| Failure::storage(dir, e))?;
    Ok(Versions {
        control: sqlite_version(&dir.join(CONTROL_DB_FILE))
            .await?
            .unwrap_or(release.control),
        inbox: storage.inbox.unwrap_or(release.inbox),
        recordings: sqlite_version(&dir.join(RECORDINGS_DB_FILE))
            .await?
            .unwrap_or(release.recordings),
        analytics: storage.analytics.unwrap_or(release.analytics),
    })
}

async fn sqlite_version(path: &Path) -> Result<Option<i64>, Failure> {
    if !path.exists() {
        return Ok(None);
    }
    let failure = |e: sqlx::Error| Failure::storage(path, e);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(SqliteConnectOptions::new().filename(path).read_only(true))
        .await
        .map_err(failure)?;
    let version = applied_migration(&pool).await.map_err(failure)?;
    pool.close().await;
    Ok(version)
}

/// Whether `serve` may start on data at `stored`: the outcomes of 0006 Schema versions, all
/// exit code 3.
pub fn check(stored: Versions, release: Versions, upgrading: bool) -> Result<(), Failure> {
    if let Some(newer) = stored.newer_than(release) {
        return Err(Failure::new(
            Exit::Refused,
            "data_newer_than_binary",
            format!("{newer}; install the matching or a newer release"),
        ));
    }
    if upgrading {
        return Err(Failure::new(
            Exit::Refused,
            "upgrade_interrupted",
            "an upgrade stopped before it finished; run `holocron upgrade` again to resume it",
        ));
    }
    if stored.older_than(release) {
        return Err(Failure::new(
            Exit::Refused,
            "upgrade_required",
            "the data is older than this release; run `holocron upgrade` (it takes a backup first)",
        ));
    }
    Ok(())
}

/// [`check`] on a data directory.
pub async fn check_dir(dir: &Path) -> Result<(), Failure> {
    check(
        stored(dir).await?,
        release(),
        dir.join(UPGRADE_MARKER).exists(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const RELEASE: Versions = Versions {
        control: 9,
        inbox: 4,
        recordings: 1,
        analytics: 4,
    };

    fn code(stored: Versions, upgrading: bool) -> Option<String> {
        check(stored, RELEASE, upgrading).err().map(|f| {
            assert_eq!(f.exit, Exit::Refused);
            f.code.into_owned()
        })
    }

    #[test]
    fn serve_starts_only_on_current_data() {
        assert_eq!(code(RELEASE, false), None);
        let older = Versions {
            control: 8,
            analytics: 3,
            ..RELEASE
        };
        assert_eq!(code(older, false).as_deref(), Some("upgrade_required"));
        let newer = Versions {
            analytics: 5,
            ..RELEASE
        };
        assert_eq!(
            code(newer, false).as_deref(),
            Some("data_newer_than_binary")
        );
        // SQLite migrated, DuckDB not, or the other way round.
        let half = Versions {
            analytics: 3,
            ..RELEASE
        };
        assert_eq!(code(half, true).as_deref(), Some("upgrade_interrupted"));
        assert_eq!(code(RELEASE, true).as_deref(), Some("upgrade_interrupted"));
    }
}
