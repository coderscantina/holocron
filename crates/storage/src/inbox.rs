//! SQLite durable inbox: admission commits, stable batch assignment, retirement.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sqlx::migrate::Migrator;
use sqlx::sqlite::{
    SqliteAutoVacuum, SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions,
    SqliteSynchronous,
};
use sqlx::{QueryBuilder, Row, Sqlite, Transaction};

use crate::fence::{Erasure, Fence};
use crate::record::{ClientEventId, INBOX_FORMAT, Id, Record};
use crate::{Error, Mode, Outcome, failpoint};

/// Client `eventId` deduplication window (0002).
pub(crate) const DEDUP_WINDOW_MS: i64 = 48 * 3600 * 1000;

pub(crate) static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

/// The schema version of this release's inbox: its latest migration.
pub fn inbox_schema() -> i64 {
    latest_migration(&MIGRATOR)
}

pub fn latest_migration(migrator: &Migrator) -> i64 {
    migrator.iter().map(|m| m.version).max().unwrap_or(0)
}

/// The latest migration applied to a SQLite file, `None` for a file without migrations.
pub async fn applied_migration(pool: &SqlitePool) -> sqlx::Result<Option<i64>> {
    let migrated: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE name = '_sqlx_migrations')",
    )
    .fetch_one(pool)
    .await?;
    if !migrated {
        return Ok(None);
    }
    sqlx::query_scalar("SELECT max(version) FROM _sqlx_migrations WHERE success = 1")
        .fetch_one(pool)
        .await
}

/// Where the inbox stands: its sequence counters, its backlog and its assigned batches.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct InboxPositions {
    /// The highest batch ID and inbox sequence ever assigned, retired ones included.
    pub last_batch: i64,
    pub last_seq: i64,
    pub records: i64,
    pub bytes: i64,
    /// `[id, first_seq, last_seq, records]` of every assigned batch not yet retired.
    pub assigned: Vec<[i64; 4]>,
}

pub(crate) async fn positions(pool: &SqlitePool) -> sqlx::Result<InboxPositions> {
    let (last_batch, last_seq, records, bytes): (i64, i64, i64, i64) = sqlx::query_as(
        "SELECT coalesce((SELECT seq FROM sqlite_sequence WHERE name = 'batches'), 0),
                coalesce((SELECT seq FROM sqlite_sequence WHERE name = 'inbox'), 0),
                (SELECT count(*) FROM inbox), (SELECT coalesce(sum(bytes), 0) FROM inbox)",
    )
    .fetch_one(pool)
    .await?;
    let assigned: Vec<(i64, i64, i64, i64)> =
        sqlx::query_as("SELECT id, first_seq, last_seq, records FROM batches ORDER BY id")
            .fetch_all(pool)
            .await?;
    Ok(InboxPositions {
        last_batch,
        last_seq,
        records,
        bytes,
        assigned: assigned
            .into_iter()
            .map(|(a, b, c, d)| [a, b, c, d])
            .collect(),
    })
}

/// The pairing ID, `None` before the pair was created or in a file from before pairing.
pub(crate) async fn pair(pool: &SqlitePool) -> sqlx::Result<Option<String>> {
    let paired: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE name = 'pairing')")
            .fetch_one(pool)
            .await?;
    if !paired {
        return Ok(None);
    }
    sqlx::query_scalar("SELECT pair FROM pairing WHERE id = 1")
        .fetch_optional(pool)
        .await
}

/// Retires assigned batches the ledger already holds with the same membership: what transfer
/// does after an apply, done before an upgrade changes the schema the ledger records.
pub(crate) async fn retire_applied(
    pool: &SqlitePool,
    applied: impl Fn(&[i64; 4]) -> bool,
) -> sqlx::Result<()> {
    for batch in positions(pool).await?.assigned {
        if !applied(&batch) {
            continue;
        }
        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
        for sql in [
            "DELETE FROM inbox WHERE batch_id = ?",
            "DELETE FROM batches WHERE id = ?",
        ] {
            sqlx::query(sql).bind(batch[0]).execute(&mut *tx).await?;
        }
        tx.commit().await?;
    }
    Ok(())
}

/// One admitted request, already encoded by the caller.
pub(crate) struct Encoded {
    pub site: Id,
    /// The client event ID and receive time ms when the client sent one.
    pub client_event_id: Option<(ClientEventId, i64)>,
    pub payload: Vec<u8>,
}

/// A stable batch: its ID and membership never change once committed.
pub(crate) struct Batch {
    pub id: i64,
    pub first_seq: i64,
    pub last_seq: i64,
    pub bytes: u64,
    pub records: Vec<(i64, Record)>,
}

#[derive(Default)]
pub(crate) struct Backlog {
    pub records: AtomicU64,
    pub bytes: AtomicU64,
}

#[derive(Clone)]
pub(crate) struct Inbox {
    writer: SqlitePool,
    reader: SqlitePool,
    pub backlog: Arc<Backlog>,
}

impl Inbox {
    pub async fn open(path: &Path, mode: Mode) -> Result<Self, crate::Error> {
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Full)
            .foreign_keys(true)
            .busy_timeout(Duration::from_secs(5));
        // macOS fsync does not flush the drive cache; FULL needs F_FULLFSYNC there.
        #[cfg(target_os = "macos")]
        let options = options.pragma("fullfsync", "ON");

        // Incremental vacuum lets a drained inbox give its peak size back (0008 Budgets).
        let writer = SqlitePoolOptions::new()
            .max_connections(1)
            // A COMMIT that fails (for example on a full disk) can leave the connection inside a
            // transaction, and every later write would fail until a restart. Replace it instead.
            .before_acquire(|conn, _| {
                Box::pin(async move { Ok(!sqlx::Connection::is_in_transaction(conn)) })
            })
            .connect_with(options.clone().auto_vacuum(SqliteAutoVacuum::Incremental))
            .await?;
        let latest = inbox_schema();
        match applied_migration(&writer).await? {
            Some(version) if version > latest => {
                return Err(Error::DataNewer(format!(
                    "inbox.sqlite has schema {version}, this release {latest}"
                )));
            }
            Some(version) if version < latest && mode == Mode::Serve => {
                return Err(Error::UpgradeRequired(format!(
                    "inbox.sqlite has schema {version}, this release {latest}"
                )));
            }
            Some(version) if version == latest => {}
            _ => MIGRATOR.run(&writer).await?,
        }
        let auto_vacuum: i64 = sqlx::query_scalar("PRAGMA auto_vacuum")
            .fetch_one(&writer)
            .await?;
        if auto_vacuum != 2 {
            // Files created before incremental vacuum need one full VACUUM to switch.
            sqlx::query("VACUUM").execute(&writer).await?;
        }
        let reader = SqlitePoolOptions::new()
            .max_connections(2)
            .connect_with(options.read_only(true))
            .await?;

        let row = sqlx::query("SELECT count(*), coalesce(sum(bytes), 0) FROM inbox")
            .fetch_one(&writer)
            .await?;
        let backlog = Backlog::default();
        backlog
            .records
            .store(row.get::<i64, _>(0) as u64, Ordering::Relaxed);
        backlog
            .bytes
            .store(row.get::<i64, _>(1) as u64, Ordering::Relaxed);
        Ok(Self {
            writer,
            reader,
            backlog: Arc::new(backlog),
        })
    }

    /// Commits a group of requests in one durable transaction. Returns per-request outcomes.
    pub async fn commit(&self, requests: &[&[Encoded]]) -> sqlx::Result<Vec<Vec<Outcome>>> {
        // SQLite allows 32,766 bound variables per statement; four per row.
        const ROWS_PER_INSERT: usize = 8_000;
        let now = now_ms();
        let mut tx = self.writer.begin_with("BEGIN IMMEDIATE").await?;
        let mut outcomes = Vec::with_capacity(requests.len());
        let (mut records, mut bytes) = (0u64, 0u64);
        for request in requests {
            let mut out = vec![Outcome::Duplicate { conflict: false }; request.len()];
            let mut accepted = Vec::with_capacity(request.len());
            for (i, record) in request.iter().enumerate() {
                match &record.client_event_id {
                    Some((event_id, received)) => {
                        match claim_event_id(&mut tx, record.site, event_id, *received).await? {
                            None => accepted.push(i),
                            Some(conflict) => out[i] = Outcome::Duplicate { conflict },
                        }
                    }
                    None => accepted.push(i),
                }
            }
            for chunk in accepted.chunks(ROWS_PER_INSERT) {
                let mut insert = QueryBuilder::<Sqlite>::new(
                    "INSERT INTO inbox (received_at, format, bytes, payload) ",
                );
                insert.push_values(chunk, |mut row, i| {
                    let payload = &request[*i].payload;
                    row.push_bind(now)
                        .push_bind(INBOX_FORMAT)
                        .push_bind(payload.len() as i64)
                        .push_bind(payload.as_slice());
                });
                // One statement inserts its rows in VALUES order with consecutive sequences.
                let last = insert.build().execute(&mut *tx).await?.last_insert_rowid();
                for (offset, i) in chunk.iter().enumerate() {
                    out[*i] = Outcome::Accepted {
                        sequence: last - (chunk.len() - 1 - offset) as i64,
                    };
                    records += 1;
                    bytes += request[*i].payload.len() as u64;
                }
            }
            outcomes.push(out);
        }
        failpoint::hit("admission.before_commit");
        tx.commit().await?;
        self.backlog.records.fetch_add(records, Ordering::Relaxed);
        self.backlog.bytes.fetch_add(bytes, Ordering::Relaxed);
        Ok(outcomes)
    }

    /// Commits one import chunk under `key` with its ledger row, or nothing when the key is
    /// already in the ledger. Returns whether the chunk was new and its recorded size.
    pub async fn commit_import(&self, key: &str, records: &[Encoded]) -> sqlx::Result<(bool, i64)> {
        let mut tx = self.writer.begin_with("BEGIN IMMEDIATE").await?;
        let existing: Option<i64> =
            sqlx::query_scalar("SELECT records FROM import_chunks WHERE key = ?")
                .bind(key)
                .fetch_optional(&mut *tx)
                .await?;
        if let Some(count) = existing {
            tx.commit().await?;
            return Ok((false, count));
        }
        let now = now_ms();
        let mut range: Option<(i64, i64)> = None;
        let mut bytes = 0u64;
        for record in records {
            let seq = sqlx::query(
                "INSERT INTO inbox (received_at, format, bytes, payload) VALUES (?, ?, ?, ?)",
            )
            .bind(now)
            .bind(INBOX_FORMAT)
            .bind(record.payload.len() as i64)
            .bind(record.payload.as_slice())
            .execute(&mut *tx)
            .await?
            .last_insert_rowid();
            range = Some(range.map_or((seq, seq), |(first, _)| (first, seq)));
            bytes += record.payload.len() as u64;
        }
        sqlx::query(
            "INSERT INTO import_chunks (key, first_seq, last_seq, records, admitted_at) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(key)
        .bind(range.map(|(first, _)| first))
        .bind(range.map(|(_, last)| last))
        .bind(records.len() as i64)
        .bind(now)
        .execute(&mut *tx)
        .await?;
        failpoint::hit("admission.before_commit");
        tx.commit().await?;
        self.backlog
            .records
            .fetch_add(records.len() as u64, Ordering::Relaxed);
        self.backlog.bytes.fetch_add(bytes, Ordering::Relaxed);
        Ok((true, records.len() as i64))
    }

    /// Returns the oldest assigned batch, or assigns a new one from pending records.
    pub async fn next_batch(
        &self,
        max_records: usize,
        max_bytes: u64,
    ) -> Result<Option<Batch>, crate::Error> {
        let mut tx = self.writer.begin_with("BEGIN IMMEDIATE").await?;
        let assigned =
            sqlx::query("SELECT id, first_seq, last_seq, bytes FROM batches ORDER BY id LIMIT 1")
                .fetch_optional(&mut *tx)
                .await?;
        let (id, first_seq, last_seq, bytes) = match assigned {
            Some(row) => {
                tx.commit().await?;
                (
                    row.get(0),
                    row.get(1),
                    row.get(2),
                    row.get::<i64, _>(3) as u64,
                )
            }
            None => {
                let pending = sqlx::query(
                    "SELECT seq, bytes FROM inbox WHERE batch_id IS NULL ORDER BY seq LIMIT ?",
                )
                .bind(max_records as i64)
                .fetch_all(&mut *tx)
                .await?;
                if pending.is_empty() {
                    tx.commit().await?;
                    return Ok(None);
                }
                failpoint::hit("transfer.before_assign");
                let (mut last_seq, mut bytes, mut count) = (0i64, 0u64, 0i64);
                for row in &pending {
                    let size = row.get::<i64, _>(1) as u64;
                    if count > 0 && bytes + size > max_bytes {
                        break;
                    }
                    last_seq = row.get(0);
                    bytes += size;
                    count += 1;
                }
                let first_seq: i64 = pending[0].get(0);
                let id: i64 = sqlx::query_scalar(
                    "INSERT INTO batches (first_seq, last_seq, records, bytes, assigned_at) VALUES (?, ?, ?, ?, ?) RETURNING id",
                )
                .bind(first_seq)
                .bind(last_seq)
                .bind(count)
                .bind(bytes as i64)
                .bind(now_ms())
                .fetch_one(&mut *tx)
                .await?;
                sqlx::query(
                    "UPDATE inbox SET batch_id = ? WHERE batch_id IS NULL AND seq BETWEEN ? AND ?",
                )
                .bind(id)
                .bind(first_seq)
                .bind(last_seq)
                .execute(&mut *tx)
                .await?;
                tx.commit().await?;
                failpoint::hit("transfer.after_assign");
                (id, first_seq, last_seq, bytes)
            }
        };

        let rows =
            sqlx::query("SELECT seq, format, payload FROM inbox WHERE batch_id = ? ORDER BY seq")
                .bind(id)
                .fetch_all(&self.reader)
                .await?;
        let mut records = Vec::with_capacity(rows.len());
        for row in rows {
            let seq: i64 = row.get(0);
            let format: i64 = row.get(1);
            let record = Record::decode(format, row.get::<&[u8], _>(2))
                .map_err(|e| crate::Error::Incompatible(format!("inbox record {seq}: {e}")))?;
            records.push((seq, record));
        }
        Ok(Some(Batch {
            id,
            first_seq,
            last_seq,
            bytes,
            records,
        }))
    }

    /// Deletes a batch whose ledger entry is committed in DuckDB.
    pub async fn retire(&self, batch: &Batch) -> sqlx::Result<()> {
        let mut tx = self.writer.begin_with("BEGIN IMMEDIATE").await?;
        sqlx::query("DELETE FROM inbox WHERE batch_id = ?")
            .bind(batch.id)
            .execute(&mut *tx)
            .await?;
        failpoint::hit("transfer.during_retire");
        sqlx::query("DELETE FROM batches WHERE id = ?")
            .bind(batch.id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        let retired = batch.records.len() as u64;
        if self.backlog.records.load(Ordering::SeqCst) == retired {
            // Drained: return free pages to the filesystem before the backlog reads empty, so
            // a drain that returns has given the space back. Under load freed pages are reused.
            sqlx::query("PRAGMA incremental_vacuum")
                .execute(&self.writer)
                .await?;
        }
        self.backlog.records.fetch_sub(retired, Ordering::Relaxed);
        self.backlog.bytes.fetch_sub(batch.bytes, Ordering::Relaxed);
        Ok(())
    }

    /// Receive time of the oldest record not yet retired, in Unix milliseconds.
    /// Receive time of the oldest waiting record after inbox sequence `after`.
    pub async fn oldest_received_ms(&self, after: i64) -> sqlx::Result<Option<i64>> {
        sqlx::query_scalar("SELECT received_at FROM inbox WHERE seq > ? ORDER BY seq LIMIT 1")
            .bind(after)
            .fetch_optional(&self.reader)
            .await
    }

    pub fn writer(&self) -> &SqlitePool {
        &self.writer
    }

    pub async fn positions(&self) -> sqlx::Result<InboxPositions> {
        positions(&self.writer).await
    }

    pub async fn pair(&self) -> sqlx::Result<Option<String>> {
        pair(&self.writer).await
    }

    pub async fn set_pair(&self, pair: &str) -> sqlx::Result<()> {
        sqlx::query("INSERT INTO pairing (id, pair) VALUES (1, ?)")
            .bind(pair)
            .execute(&self.writer)
            .await?;
        Ok(())
    }

    /// The last sequence admission handed out.
    pub async fn last_sequence(&self) -> sqlx::Result<i64> {
        sqlx::query_scalar(LAST_SEQUENCE)
            .fetch_one(&self.writer)
            .await
    }

    /// Commits a fence for `erasure` covering records admitted up to `through`, or up to now,
    /// or returns the fence `key` committed before. Serialized with admission on the single
    /// writer.
    pub async fn fence(
        &self,
        key: &str,
        erasure: Erasure,
        through: Option<i64>,
        now_ms: i64,
    ) -> sqlx::Result<Fence> {
        let mut tx = self.writer.begin_with("BEGIN IMMEDIATE").await?;
        if let Some(fence) = sqlx::query_as::<_, FenceRow>(FENCE_SELECT_BY_KEY)
            .bind(key)
            .fetch_optional(&mut *tx)
            .await?
        {
            tx.commit().await?;
            return Ok(fence.into());
        }
        let through = match through {
            Some(through) => through,
            None => {
                sqlx::query_scalar(LAST_SEQUENCE)
                    .fetch_one(&mut *tx)
                    .await?
            }
        };
        let (profile, forever) = match erasure {
            Erasure::Site(_) => (None, true),
            Erasure::SiteHistory(_) => (None, false),
            Erasure::Profile { profile, .. } => (Some(profile.to_be_bytes().to_vec()), false),
        };
        sqlx::query(
            "INSERT INTO fences (key, site, profile, forever, through_seq, created_at) VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(key)
        .bind(&erasure.site().to_be_bytes()[..])
        .bind(profile)
        .bind(forever)
        .bind(through)
        .bind(now_ms)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(Fence {
            erasure,
            through_seq: through,
            created_at_ms: now_ms,
        })
    }

    pub async fn fences(&self) -> sqlx::Result<Vec<Fence>> {
        let rows: Vec<FenceRow> = sqlx::query_as(FENCE_SELECT).fetch_all(&self.writer).await?;
        Ok(rows.into_iter().map(Fence::from).collect())
    }

    /// Writes a consistent copy of the inbox to `path` while admission continues.
    pub async fn vacuum_into(&self, path: &Path) -> sqlx::Result<()> {
        vacuum_into(&self.reader, path).await
    }

    /// Forgets client event IDs first claimed before `before_ms`: past the 48-hour window they
    /// no longer make a copy a duplicate.
    pub async fn prune_event_ids(&self, before_ms: i64) -> sqlx::Result<u64> {
        Ok(
            sqlx::query("DELETE FROM client_event_ids WHERE first_received_at < ?")
                .bind(before_ms)
                .execute(&self.writer)
                .await?
                .rows_affected(),
        )
    }

    pub async fn close(&self) {
        self.reader.close().await;
        self.writer.close().await;
    }
}

/// `VACUUM INTO` writes one compact, consistent file from a read transaction.
pub(crate) async fn vacuum_into(pool: &SqlitePool, path: &Path) -> sqlx::Result<()> {
    let target = path.to_str().ok_or_else(|| {
        sqlx::Error::Configuration(format!("{} is not UTF-8", path.display()).into())
    })?;
    sqlx::query("VACUUM INTO ?")
        .bind(target)
        .execute(pool)
        .await?;
    Ok(())
}

const LAST_SEQUENCE: &str =
    "SELECT coalesce((SELECT seq FROM sqlite_sequence WHERE name = 'inbox'), 0)";
const FENCE_SELECT: &str =
    "SELECT site, profile, forever, through_seq, created_at FROM fences ORDER BY id";
const FENCE_SELECT_BY_KEY: &str =
    "SELECT site, profile, forever, through_seq, created_at FROM fences WHERE key = ?";

#[derive(sqlx::FromRow)]
struct FenceRow {
    site: Vec<u8>,
    profile: Option<Vec<u8>>,
    forever: bool,
    through_seq: i64,
    created_at: i64,
}

fn id_of(bytes: &[u8]) -> Id {
    u128::from_be_bytes(bytes.try_into().expect("16-byte IDs"))
}

impl From<FenceRow> for Fence {
    fn from(row: FenceRow) -> Self {
        let site = id_of(&row.site);
        let erasure = match (row.forever, row.profile) {
            (true, _) => Erasure::Site(site),
            (false, Some(profile)) => Erasure::Profile {
                site,
                profile: id_of(&profile),
            },
            (false, None) => Erasure::SiteHistory(site),
        };
        Fence {
            erasure,
            through_seq: row.through_seq,
            created_at_ms: row.created_at,
        }
    }
}

/// First accepted event wins within the window. Returns `None` when this event claims the ID,
/// else `Some(conflict)` for a duplicate, where `conflict` means the first payload differed.
async fn claim_event_id(
    tx: &mut Transaction<'_, Sqlite>,
    site: Id,
    event_id: &ClientEventId,
    received: i64,
) -> sqlx::Result<Option<bool>> {
    let site = site.to_be_bytes();
    let fingerprint = event_id.fingerprint.to_be_bytes();
    let claimed = sqlx::query(
        "INSERT INTO client_event_ids (site, event_id, first_received_at, fingerprint) VALUES (?, ?, ?, ?)
         ON CONFLICT (site, event_id) DO UPDATE
         SET first_received_at = excluded.first_received_at, fingerprint = excluded.fingerprint
         WHERE first_received_at <= excluded.first_received_at - ?",
    )
    .bind(&site[..])
    .bind(&event_id.id)
    .bind(received)
    .bind(&fingerprint[..])
    .bind(DEDUP_WINDOW_MS)
    .execute(&mut **tx)
    .await?;
    if claimed.rows_affected() == 1 {
        return Ok(None);
    }
    let first: Option<Vec<u8>> = sqlx::query_scalar(
        "SELECT fingerprint FROM client_event_ids WHERE site = ? AND event_id = ?",
    )
    .bind(&site[..])
    .bind(&event_id.id)
    .fetch_one(&mut **tx)
    .await?;
    Ok(Some(first.as_deref() != Some(&fingerprint[..])))
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after 1970")
        .as_millis() as i64
}
