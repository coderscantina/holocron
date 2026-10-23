//! Import state in the control database (`0008_imports.sql`): the run, step checkpoints and the
//! inventory of what was not imported. [`state`] is what other commands read to tell whether an
//! import is in progress.

use std::path::Path;

use serde::Serialize;
use serde_json::Value;
use sqlx::{SqliteConnection, SqlitePool};

use crate::db::{Db, now_ms};
use crate::init::{Exit, Failure};

/// Whether this data directory has an import, and how far it got.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ImportState {
    /// No import was ever started here.
    None,
    /// Started and not finished: running now, or interrupted and resumable with the same
    /// source and options, in the setup wizard or with `holocron import run`.
    Running { source: String, started_at: i64 },
    Complete {
        source: String,
        started_at: i64,
        finished_at: i64,
    },
}

/// Refuses with `import_in_progress` while an import into `dir` is unfinished. Erasing or
/// expiring data then would remove rows that resuming the import writes again. `serve` starts
/// in setup mode instead (decision 0021).
pub async fn refuse_while_running(dir: &Path) -> Result<(), Failure> {
    let db = Db::connect(dir)
        .await
        .map_err(|e| Failure::storage(dir, e))?;
    let found = state(&db).await;
    db.close().await;
    match found.map_err(|e| Failure::storage(dir, e))? {
        ImportState::Running { .. } => Err(Failure::new(
            Exit::Refused,
            "import_in_progress",
            "an import into this data directory has not finished; resume it in the setup wizard of `holocron serve`, or rerun `holocron import run`",
        )),
        ImportState::None | ImportState::Complete { .. } => Ok(()),
    }
}

/// The import state of the data directory `db` belongs to.
pub async fn state(db: &Db) -> Result<ImportState, sqlx::Error> {
    let row: Option<(String, String, i64, Option<i64>)> = sqlx::query_as(
        "SELECT source, state, started_at, finished_at FROM import_runs WHERE id = 1",
    )
    .fetch_optional(&db.reader)
    .await?;
    Ok(match row {
        None => ImportState::None,
        Some((source, state, started_at, finished_at)) => match (state.as_str(), finished_at) {
            ("complete", Some(finished_at)) => ImportState::Complete {
                source,
                started_at,
                finished_at,
            },
            _ => ImportState::Running { source, started_at },
        },
    })
}

/// The stored run: what it was started with and whether it finished.
pub struct Run {
    pub source: String,
    pub fingerprint: Value,
    pub options: Value,
    pub clock_ms: i64,
    pub manifest: Option<Value>,
}

pub async fn run(db: &Db) -> Result<Option<Run>, sqlx::Error> {
    let row: Option<(String, String, String, i64, Option<String>)> = sqlx::query_as(
        "SELECT source, fingerprint, options, clock_ms, manifest FROM import_runs WHERE id = 1",
    )
    .fetch_optional(&db.reader)
    .await?;
    let json = |text: &str| serde_json::from_str(text).map_err(|e| sqlx::Error::Decode(e.into()));
    row.map(|(source, fingerprint, options, clock_ms, manifest)| {
        Ok(Run {
            source,
            fingerprint: json(&fingerprint)?,
            options: json(&options)?,
            clock_ms,
            manifest: manifest.as_deref().map(json).transpose()?,
        })
    })
    .transpose()
}

pub async fn start(
    db: &Db,
    source: &str,
    fingerprint: &Value,
    options: &Value,
    clock_ms: i64,
) -> Result<(), sqlx::Error> {
    let now = now_ms();
    sqlx::query(
        "INSERT INTO import_runs (id, source, fingerprint, options, clock_ms, state, started_at, updated_at)
         VALUES (1, ?, ?, ?, ?, 'running', ?, ?)",
    )
    .bind(source)
    .bind(fingerprint.to_string())
    .bind(options.to_string())
    .bind(clock_ms)
    .bind(now)
    .bind(now)
    .execute(&db.writer)
    .await?;
    Ok(())
}

pub async fn complete(db: &Db, manifest: &Value) -> Result<(), sqlx::Error> {
    let now = now_ms();
    sqlx::query(
        "UPDATE import_runs SET state = 'complete', manifest = ?, finished_at = ?, updated_at = ? WHERE id = 1",
    )
    .bind(manifest.to_string())
    .bind(now)
    .bind(now)
    .execute(&db.writer)
    .await?;
    Ok(())
}

/// One step's checkpoint.
#[derive(Debug, Clone, Default)]
pub struct Step {
    pub cursor: Option<String>,
    pub chunks: i64,
    pub done: bool,
}

pub async fn step(db: &Db, name: &str) -> Result<Step, sqlx::Error> {
    let row: Option<(Option<String>, i64, bool)> =
        sqlx::query_as("SELECT cursor, chunks, done FROM import_steps WHERE step = ?")
            .bind(name)
            .fetch_optional(&db.reader)
            .await?;
    Ok(
        row.map_or_else(Step::default, |(cursor, chunks, done)| Step {
            cursor,
            chunks,
            done,
        }),
    )
}

/// What one committed chunk adds to a step.
#[derive(Debug, Clone, Copy, Default)]
pub struct Progress {
    pub rows: i64,
    pub imported: i64,
    pub skipped: i64,
}

/// Records a committed chunk: the new cursor and chunk count, and what it read.
pub async fn checkpoint(
    conn: &mut SqliteConnection,
    name: &str,
    cursor: &str,
    chunks: i64,
    progress: Progress,
    done: bool,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO import_steps (step, cursor, chunks, rows, imported, skipped, done, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT (step) DO UPDATE SET cursor = excluded.cursor, chunks = excluded.chunks,
             rows = rows + excluded.rows, imported = imported + excluded.imported,
             skipped = skipped + excluded.skipped, done = excluded.done, updated_at = excluded.updated_at",
    )
    .bind(name)
    .bind(cursor)
    .bind(chunks)
    .bind(progress.rows)
    .bind(progress.imported)
    .bind(progress.skipped)
    .bind(done)
    .bind(now_ms())
    .execute(conn)
    .await?;
    Ok(())
}

/// Every step's counters, for status and the manifest.
pub async fn steps(db: &Db) -> Result<Value, sqlx::Error> {
    let rows: Vec<(String, i64, i64, i64, i64, bool)> = sqlx::query_as(
        "SELECT step, chunks, rows, imported, skipped, done FROM import_steps ORDER BY step",
    )
    .fetch_all(&db.reader)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(step, chunks, rows, imported, skipped, done)| {
            (
                step,
                serde_json::json!({ "chunks": chunks, "rows": rows, "imported": imported,
                                    "skipped": skipped, "done": done }),
            )
        })
        .collect::<serde_json::Map<_, _>>()
        .into())
}

/// A source object kept with the reason it was not imported, or imported with a loss.
#[derive(Debug, Clone)]
pub struct Entry {
    pub table: &'static str,
    pub key: String,
    pub reason: String,
    pub representation: Value,
}

/// Column values that are credentials never leave the source, not even into the inventory.
const SECRET_COLUMNS: [&str; 5] = ["password", "secret", "key_hash", "code_hash", "otp"];

impl Entry {
    /// `reason` is a code, optionally followed by `:` and a detail, like the mapping errors of
    /// `definitions`. The representation is `{ "row": <source row>, "detail": ... }`.
    pub fn new(table: &'static str, key: impl Into<String>, reason: &str, mut row: Value) -> Self {
        if let Some(object) = row.as_object_mut() {
            for column in SECRET_COLUMNS {
                if let Some(value) = object.get_mut(column)
                    && !value.is_null()
                {
                    *value = Value::from("<redacted>");
                }
            }
        }
        let (reason, detail) = match reason.split_once(':') {
            Some((code, detail)) => (code, Some(detail)),
            None => (reason, None),
        };
        let mut representation = serde_json::json!({ "row": row });
        if let Some(detail) = detail {
            representation["detail"] = Value::from(detail);
        }
        Self {
            table,
            key: key.into(),
            reason: reason.to_owned(),
            representation,
        }
    }

    /// An entry for a definition imported without the settings in `lost`.
    pub fn partial(
        table: &'static str,
        key: impl Into<String>,
        row: Value,
        lost: &[String],
    ) -> Self {
        Self::new(
            table,
            key,
            &format!("partially_mapped:{}", lost.join(", ")),
            row,
        )
    }
}

pub async fn record(conn: &mut SqliteConnection, entries: &[Entry]) -> Result<(), sqlx::Error> {
    for entry in entries {
        sqlx::query(
            "INSERT OR IGNORE INTO import_inventory (source_table, source_key, reason, representation)
             VALUES (?, ?, ?, ?)",
        )
        .bind(entry.table)
        .bind(&entry.key)
        .bind(&entry.reason)
        .bind(entry.representation.to_string())
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

/// Removes the inventory rows of `site` (decision 0014 erasure): all of them when the site is
/// deleted, only its history (identity links and replays) when it is reset. A row belongs to a
/// site when its source row names it as `website_id`, or as the entity of a website share;
/// imported sites keep their Umami website ID.
pub async fn forget_site(pool: &SqlitePool, site: &str, history_only: bool) -> sqlx::Result<u64> {
    Ok(sqlx::query(
        "DELETE FROM import_inventory
         WHERE (lower(json_extract(representation, '$.row.website_id')) = ?1
                OR (source_table = 'share'
                    AND lower(json_extract(representation, '$.row.entity_id')) = ?1))
           AND (NOT ?2 OR source_table IN ('session_link', 'session_replay', 'session_replay_saved'))",
    )
    .bind(site)
    .bind(history_only)
    .execute(pool)
    .await?
    .rows_affected())
}

/// Source keys and distinct IDs of the inventoried `session_link` rows of `site`.
pub async fn session_links(pool: &SqlitePool, site: &str) -> sqlx::Result<Vec<(String, String)>> {
    sqlx::query_as(
        "SELECT source_key, json_extract(representation, '$.row.distinct_id') FROM import_inventory
         WHERE source_table = 'session_link'
           AND lower(json_extract(representation, '$.row.website_id')) = ?",
    )
    .bind(site)
    .fetch_all(pool)
    .await
}

/// Removes the `session_link` rows with these source keys.
pub async fn forget_session_links(pool: &SqlitePool, keys: &[String]) -> sqlx::Result<()> {
    for key in keys {
        sqlx::query(
            "DELETE FROM import_inventory WHERE source_table = 'session_link' AND source_key = ?",
        )
        .bind(key)
        .execute(pool)
        .await?;
    }
    Ok(())
}

/// Inventory counts by table and reason.
pub async fn inventory_counts(db: &Db) -> Result<Value, sqlx::Error> {
    let rows: Vec<(String, String, i64)> = sqlx::query_as(
        "SELECT source_table, reason, count(*) FROM import_inventory
         GROUP BY source_table, reason ORDER BY source_table, reason",
    )
    .fetch_all(&db.reader)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(table, reason, count)| serde_json::json!({ "table": table, "reason": reason, "count": count }))
        .collect::<Vec<_>>()
        .into())
}
