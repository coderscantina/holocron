//! The control database: one writer connection, a separate read pool, settings applied once.

use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sqlx::SqlitePool;
use sqlx::migrate::Migrator;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};

/// File name of the control database inside the data directory.
pub const CONTROL_DB_FILE: &str = "control.sqlite";

/// Control schema migrations. The latest one is this release's control schema version.
pub static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

const READ_CONNECTIONS: u32 = 4;
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone)]
pub struct Db {
    /// Pool of size one: every command serializes here and starts with `BEGIN IMMEDIATE`.
    pub writer: SqlitePool,
    /// Query-only connections for snapshot reads.
    pub reader: SqlitePool,
}

impl Db {
    /// Opens or creates the control database in `data_dir` and applies pending migrations.
    /// Only `init`, `upgrade` and tests call it; `serve` uses [`Db::connect`].
    pub async fn open(data_dir: &Path) -> Result<Self, sqlx::Error> {
        let db = Self::connect(data_dir).await?;
        MIGRATOR.run(&db.writer).await?;
        Ok(db)
    }

    /// Opens the control database without changing its schema.
    pub async fn connect(data_dir: &Path) -> Result<Self, sqlx::Error> {
        let options = SqliteConnectOptions::new()
            .filename(data_dir.join(CONTROL_DB_FILE))
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            // FULL, not NORMAL: an acknowledged command must survive power loss.
            .synchronous(SqliteSynchronous::Full)
            .foreign_keys(true)
            .busy_timeout(BUSY_TIMEOUT);

        let writer = SqlitePoolOptions::new()
            .max_connections(1)
            // A COMMIT that fails (for example on a full disk) can leave the connection inside a
            // transaction, and every later write would fail until a restart. Replace it instead.
            .before_acquire(|conn, _| {
                Box::pin(async move { Ok(!sqlx::Connection::is_in_transaction(conn)) })
            })
            .connect_with(options.clone())
            .await?;
        let reader = SqlitePoolOptions::new()
            .max_connections(READ_CONNECTIONS)
            .connect_with(options.pragma("query_only", "ON"))
            .await?;

        Ok(Self { writer, reader })
    }

    pub async fn close(&self) {
        self.reader.close().await;
        self.writer.close().await;
    }
}

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default()
}

pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}
