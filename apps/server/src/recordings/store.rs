//! The replay store, `recordings.sqlite`: compressed replay parts by capture and part index,
//! one summary row per capture, and the byte count the recording cap is checked against.
//!
//! Every write is one durable transaction on a single writer, so the cap check, duplicate
//! check, exclusion and the part itself commit together. Reads use a separate pool.

use std::path::{Path, PathBuf};
use std::time::Duration;

use sqlx::sqlite::{
    SqliteAutoVacuum, SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions,
    SqliteSynchronous,
};
use sqlx::{FromRow, QueryBuilder, Sqlite};

use super::policy;
use super::replay::Summary;

pub const RECORDINGS_DB_FILE: &str = "recordings.sqlite";

/// Replay store migrations. The latest one is this release's recordings schema version.
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations-recordings");

/// Blobs of tens of kilobytes take fewer overflow pages at 16 KiB.
const PAGE_SIZE: u32 = 16 * 1024;

pub fn id_bytes(id: u128) -> Vec<u8> {
    id.to_be_bytes().to_vec()
}

pub fn id_of(bytes: &[u8]) -> u128 {
    u128::from_be_bytes(bytes.try_into().unwrap_or([0; 16]))
}

#[derive(Clone)]
pub struct Store {
    writer: SqlitePool,
    reader: SqlitePool,
    path: PathBuf,
}

/// Where a part came from, read once per capture from its first part's request.
#[derive(Debug, Default, Clone)]
pub struct Client {
    pub browser: Option<String>,
    pub os: Option<String>,
    pub device: Option<String>,
    pub country: Option<String>,
}

/// One recorder request ready to store.
pub struct Part<'a> {
    pub site: u128,
    pub capture: u128,
    pub visitor: Option<u128>,
    pub chunk: i64,
    pub received_at: i64,
    pub summary: &'a Summary,
    pub raw_bytes: i64,
    pub payload: Vec<u8>,
    pub fingerprint: [u8; 16],
    pub client: &'a Client,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stored {
    Accepted,
    /// The same part was stored before.
    Duplicate,
    /// The capture reached an excluded page; nothing of it is kept.
    Excluded,
    /// Over the recording cap. The capture is marked truncated.
    OverCap,
}

#[derive(Debug, Clone, FromRow)]
pub struct Capture {
    pub site: Vec<u8>,
    pub capture: Vec<u8>,
    pub visitor: Option<Vec<u8>>,
    pub received_at: i64,
    pub started_at: i64,
    pub ended_at: i64,
    pub events: i64,
    pub bytes: i64,
    pub entry_path: Option<String>,
    pub pages: i64,
    pub browser: Option<String>,
    pub os: Option<String>,
    pub device: Option<String>,
    pub country: Option<String>,
    pub truncated: bool,
    pub saved_at: Option<i64>,
}

macro_rules! capture_columns {
    () => {
        "site, capture, visitor, received_at, started_at, ended_at, events, bytes, entry_path, pages,
         browser, os, device, country, truncated, saved_at"
    };
}

/// A stored, not excluded capture by site and capture ID.
const CAPTURE_BY_ID: &str = concat!(
    "SELECT ",
    capture_columns!(),
    " FROM captures WHERE site = ? AND capture = ? AND excluded = 0 AND chunks > 0"
);

/// What a replay list asks for. `cutoff` hides unsaved captures past retention whether or not
/// removal has run (0006).
#[derive(Debug, Default)]
pub struct Filter {
    pub start: i64,
    pub end: i64,
    pub cutoff: i64,
    pub path: Option<String>,
    pub device: Option<String>,
    pub min_duration_ms: Option<i64>,
    pub saved: bool,
    /// Visitor and receive window of one analytical session.
    pub session: Option<(u128, i64, i64)>,
    /// `(received_at, capture)` of the last capture on the previous page.
    pub before: Option<(i64, u128)>,
}

pub enum Saved {
    Saved(Box<Capture>),
    NotFound,
    OverBudget,
}

/// Where a part sits in recording order: its index, then arrival.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Position {
    pub chunk: i64,
    pub id: i64,
}

impl Position {
    pub const START: Position = Position {
        chunk: i64::MIN,
        id: i64::MIN,
    };

    /// `<chunk>.<id>`, opaque to the browser.
    pub fn cursor(self) -> String {
        format!("{}.{}", self.chunk, self.id)
    }

    pub fn parse(cursor: &str) -> Option<Self> {
        let (chunk, id) = cursor.split_once('.')?;
        Some(Self {
            chunk: chunk.parse().ok()?,
            id: id.parse().ok()?,
        })
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Usage {
    pub bytes: i64,
    pub saved_bytes: i64,
    pub captures: i64,
    pub file_bytes: i64,
}

impl Store {
    pub async fn open(data_dir: &Path) -> Result<Self, sqlx::Error> {
        let path = data_dir.join(RECORDINGS_DB_FILE);
        let options = SqliteConnectOptions::new()
            .filename(&path)
            .create_if_missing(true)
            .page_size(PAGE_SIZE)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Full)
            .busy_timeout(Duration::from_secs(5));
        #[cfg(target_os = "macos")]
        let options = options.pragma("fullfsync", "ON");
        // Incremental vacuum lets retention give removed replays back to the file system.
        let writer = SqlitePoolOptions::new()
            .max_connections(1)
            // A COMMIT that fails (for example on a full disk) can leave the connection inside a
            // transaction, and every later write would fail until a restart. Replace it instead.
            .before_acquire(|conn, _| {
                Box::pin(async move { Ok(!sqlx::Connection::is_in_transaction(conn)) })
            })
            .connect_with(options.clone().auto_vacuum(SqliteAutoVacuum::Incremental))
            .await?;
        MIGRATOR.run(&writer).await?;
        let reader = SqlitePoolOptions::new()
            .max_connections(4)
            .connect_with(options.read_only(true))
            .await?;
        Ok(Self {
            writer,
            reader,
            path,
        })
    }

    pub async fn close(&self) {
        self.reader.close().await;
        self.writer.close().await;
    }

    /// Stores one part unless the capture is excluded, the part is a duplicate, or it would
    /// take stored bytes over `cap`.
    pub async fn put(
        &self,
        part: Part<'_>,
        excluded_paths: &[String],
        cap: u64,
    ) -> Result<Stored, sqlx::Error> {
        let (site, capture) = (id_bytes(part.site), id_bytes(part.capture));
        let mut tx = self.writer.begin_with("BEGIN IMMEDIATE").await?;
        let state: Option<(bool, i64)> =
            sqlx::query_as("SELECT excluded, bytes FROM captures WHERE site = ? AND capture = ?")
                .bind(&site)
                .bind(&capture)
                .fetch_optional(&mut *tx)
                .await?;
        if state.is_some_and(|(excluded, _)| excluded) {
            return Ok(Stored::Excluded);
        }
        if part
            .summary
            .pages
            .iter()
            .any(|path| policy::excluded(excluded_paths, path))
        {
            // Keep a row that refuses later parts; drop everything stored so far.
            let freed = state.map_or(0, |(_, bytes)| bytes);
            sqlx::query("DELETE FROM chunks WHERE site = ? AND capture = ?")
                .bind(&site)
                .bind(&capture)
                .execute(&mut *tx)
                .await?;
            sqlx::query("DELETE FROM capture_pages WHERE site = ? AND capture = ?")
                .bind(&site)
                .bind(&capture)
                .execute(&mut *tx)
                .await?;
            sqlx::query(
                "INSERT INTO captures (site, capture, received_at, last_received_at, started_at, ended_at,
                     first_chunk, chunks, events, bytes, pages, excluded)
                 VALUES (?, ?, ?, ?, 0, 0, ?, 0, 0, 0, 0, 1)
                 ON CONFLICT (site, capture) DO UPDATE SET excluded = 1, chunks = 0, events = 0, bytes = 0,
                     pages = 0, entry_path = NULL, saved_at = NULL",
            )
            .bind(&site)
            .bind(&capture)
            .bind(part.received_at)
            .bind(part.received_at)
            .bind(part.chunk)
            .execute(&mut *tx)
            .await?;
            sqlx::query("UPDATE usage SET bytes = bytes - ?")
                .bind(freed)
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
            return Ok(Stored::Excluded);
        }

        let earlier: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM chunks WHERE site = ? AND capture = ? AND chunk = ? AND fingerprint = ?",
        )
        .bind(&site)
        .bind(&capture)
        .bind(part.chunk)
        .bind(&part.fingerprint[..])
        .fetch_optional(&mut *tx)
        .await?;
        if earlier.is_some() {
            return Ok(Stored::Duplicate);
        }

        let size = part.payload.len() as i64;
        let used: i64 = sqlx::query_scalar("SELECT bytes FROM usage")
            .fetch_one(&mut *tx)
            .await?;
        if (used + size) as u64 > cap {
            sqlx::query("UPDATE captures SET truncated = 1 WHERE site = ? AND capture = ?")
                .bind(&site)
                .bind(&capture)
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
            return Ok(Stored::OverCap);
        }

        sqlx::query(
            "INSERT INTO chunks (site, capture, chunk, received_at, events, raw_bytes, fingerprint, payload)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&site)
        .bind(&capture)
        .bind(part.chunk)
        .bind(part.received_at)
        .bind(part.summary.events)
        .bind(part.raw_bytes)
        .bind(&part.fingerprint[..])
        .bind(&part.payload)
        .execute(&mut *tx)
        .await?;
        // Without event times the part's index, in browser seconds, stands in, as upstream does.
        let fallback = part.chunk.saturating_mul(1000);
        sqlx::query(
            "INSERT INTO captures (site, capture, visitor, received_at, last_received_at, started_at, ended_at,
                 first_chunk, chunks, events, bytes, entry_path, pages, browser, os, device, country)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, 1, ?, ?, ?, 0, ?, ?, ?, ?)
             ON CONFLICT (site, capture) DO UPDATE SET
                 visitor = coalesce(visitor, excluded.visitor),
                 received_at = min(received_at, excluded.received_at),
                 last_received_at = max(last_received_at, excluded.last_received_at),
                 started_at = min(started_at, excluded.started_at),
                 ended_at = max(ended_at, excluded.ended_at),
                 entry_path = CASE WHEN excluded.first_chunk < first_chunk
                     THEN coalesce(excluded.entry_path, entry_path) ELSE coalesce(entry_path, excluded.entry_path) END,
                 first_chunk = min(first_chunk, excluded.first_chunk),
                 chunks = chunks + 1,
                 events = events + excluded.events,
                 bytes = bytes + excluded.bytes,
                 browser = coalesce(browser, excluded.browser),
                 os = coalesce(os, excluded.os),
                 device = coalesce(device, excluded.device),
                 country = coalesce(country, excluded.country)",
        )
        .bind(&site)
        .bind(&capture)
        .bind(part.visitor.map(id_bytes))
        .bind(part.received_at)
        .bind(part.received_at)
        .bind(part.summary.first_ms.unwrap_or(fallback))
        .bind(part.summary.last_ms.unwrap_or(fallback))
        .bind(part.chunk)
        .bind(part.summary.events)
        .bind(size)
        .bind(part.summary.pages.first())
        .bind(&part.client.browser)
        .bind(&part.client.os)
        .bind(&part.client.device)
        .bind(&part.client.country)
        .execute(&mut *tx)
        .await?;
        for path in &part.summary.pages {
            sqlx::query(
                "INSERT OR IGNORE INTO capture_pages (site, capture, path) VALUES (?, ?, ?)",
            )
            .bind(&site)
            .bind(&capture)
            .bind(path)
            .execute(&mut *tx)
            .await?;
        }
        sqlx::query(
            "UPDATE captures SET pages = (SELECT count(*) FROM capture_pages p
                 WHERE p.site = captures.site AND p.capture = captures.capture)
             WHERE site = ? AND capture = ?",
        )
        .bind(&site)
        .bind(&capture)
        .execute(&mut *tx)
        .await?;
        sqlx::query("UPDATE usage SET bytes = bytes + ?")
            .bind(size)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(Stored::Accepted)
    }

    /// One listed capture: stored parts, not excluded.
    pub async fn capture(&self, site: u128, capture: u128) -> Result<Option<Capture>, sqlx::Error> {
        sqlx::query_as(CAPTURE_BY_ID)
            .bind(id_bytes(site))
            .bind(id_bytes(capture))
            .fetch_optional(&self.reader)
            .await
    }

    /// Captures of `site` matching `filter`, newest first.
    pub async fn list(
        &self,
        site: u128,
        filter: &Filter,
        limit: u32,
    ) -> Result<Vec<Capture>, sqlx::Error> {
        let mut query = QueryBuilder::<Sqlite>::new(concat!(
            "SELECT ",
            capture_columns!(),
            " FROM captures c WHERE c.site = "
        ));
        query
            .push_bind(id_bytes(site))
            .push(" AND c.excluded = 0 AND c.chunks > 0 AND c.received_at >= ")
            .push_bind(filter.start)
            .push(" AND c.received_at < ")
            .push_bind(filter.end)
            .push(" AND (c.saved_at IS NOT NULL OR c.received_at >= ")
            .push_bind(filter.cutoff)
            .push(")");
        if let Some(device) = &filter.device {
            query.push(" AND c.device = ").push_bind(device.clone());
        }
        if let Some(min) = filter.min_duration_ms {
            query
                .push(" AND c.ended_at - c.started_at >= ")
                .push_bind(min);
        }
        if filter.saved {
            query.push(" AND c.saved_at IS NOT NULL");
        }
        if let Some((visitor, from, to)) = filter.session {
            query
                .push(" AND c.visitor = ")
                .push_bind(id_bytes(visitor))
                .push(" AND c.received_at >= ")
                .push_bind(from)
                .push(" AND c.received_at < ")
                .push_bind(to);
        }
        if let Some(path) = &filter.path {
            query
                .push(" AND EXISTS (SELECT 1 FROM capture_pages p WHERE p.site = c.site AND p.capture = c.capture AND instr(p.path, ")
                .push_bind(path.clone())
                .push(") > 0)");
        }
        if let Some((received_at, capture)) = filter.before {
            query
                .push(" AND (c.received_at, c.capture) < (")
                .push_bind(received_at)
                .push(", ")
                .push_bind(id_bytes(capture))
                .push(")");
        }
        query
            .push(" ORDER BY c.received_at DESC, c.capture DESC LIMIT ")
            .push_bind(i64::from(limit));
        query.build_query_as().fetch_all(&self.reader).await
    }

    /// Up to `limit` stored parts after `after`, in recording order: position, size, payload.
    pub async fn parts(
        &self,
        site: u128,
        capture: u128,
        after: Position,
        limit: i64,
    ) -> Result<Vec<(Position, i64, Vec<u8>)>, sqlx::Error> {
        let rows: Vec<(i64, i64, i64, Vec<u8>)> = sqlx::query_as(
            "SELECT chunk, id, raw_bytes, payload FROM chunks
             WHERE site = ? AND capture = ? AND (chunk, id) > (?, ?) ORDER BY chunk, id LIMIT ?",
        )
        .bind(id_bytes(site))
        .bind(id_bytes(capture))
        .bind(after.chunk)
        .bind(after.id)
        .bind(limit)
        .fetch_all(&self.reader)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(chunk, id, raw_bytes, payload)| (Position { chunk, id }, raw_bytes, payload))
            .collect())
    }

    /// Marks a capture saved unless saved replays would exceed `saved_cap` bytes.
    pub async fn save(
        &self,
        site: u128,
        capture: u128,
        now: i64,
        saved_cap: u64,
    ) -> Result<Saved, sqlx::Error> {
        let mut tx = self.writer.begin_with("BEGIN IMMEDIATE").await?;
        let found: Option<Capture> = sqlx::query_as(CAPTURE_BY_ID)
            .bind(id_bytes(site))
            .bind(id_bytes(capture))
            .fetch_optional(&mut *tx)
            .await?;
        let Some(mut row) = found else {
            return Ok(Saved::NotFound);
        };
        if row.saved_at.is_some() {
            return Ok(Saved::Saved(Box::new(row)));
        }
        let saved: i64 = sqlx::query_scalar(
            "SELECT coalesce(sum(bytes), 0) FROM captures WHERE saved_at IS NOT NULL",
        )
        .fetch_one(&mut *tx)
        .await?;
        if (saved + row.bytes) as u64 > saved_cap {
            return Ok(Saved::OverBudget);
        }
        sqlx::query("UPDATE captures SET saved_at = ? WHERE site = ? AND capture = ?")
            .bind(now)
            .bind(id_bytes(site))
            .bind(id_bytes(capture))
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        row.saved_at = Some(now);
        Ok(Saved::Saved(Box::new(row)))
    }

    pub async fn unsave(&self, site: u128, capture: u128) -> Result<Option<Capture>, sqlx::Error> {
        sqlx::query("UPDATE captures SET saved_at = NULL WHERE site = ? AND capture = ?")
            .bind(id_bytes(site))
            .bind(id_bytes(capture))
            .execute(&self.writer)
            .await?;
        // Read through the writer: the change is committed there first.
        sqlx::query_as(CAPTURE_BY_ID)
            .bind(id_bytes(site))
            .bind(id_bytes(capture))
            .fetch_optional(&self.writer)
            .await
    }

    pub async fn usage(&self) -> Result<Usage, sqlx::Error> {
        let (bytes, saved_bytes, captures): (i64, i64, i64) = sqlx::query_as(
            "SELECT (SELECT bytes FROM usage),
                    (SELECT coalesce(sum(bytes), 0) FROM captures WHERE saved_at IS NOT NULL),
                    (SELECT count(*) FROM captures WHERE excluded = 0 AND chunks > 0)",
        )
        .fetch_one(&self.reader)
        .await?;
        let size = |path: &Path| std::fs::metadata(path).map_or(0, |m| m.len() as i64);
        let wal = self.path.with_extension("sqlite-wal");
        Ok(Usage {
            bytes,
            saved_bytes,
            captures,
            file_bytes: size(&self.path) + size(&wal),
        })
    }

    /// Sites that have captures received before `before`.
    pub async fn sites_before(&self, before: i64) -> Result<Vec<u128>, sqlx::Error> {
        let rows: Vec<Vec<u8>> =
            sqlx::query_scalar("SELECT DISTINCT site FROM captures WHERE received_at < ?")
                .bind(before)
                .fetch_all(&self.reader)
                .await?;
        Ok(rows.iter().map(|site| id_of(site)).collect())
    }

    /// Removes up to `limit` whole captures of `site` received before `cutoff`, saved ones only
    /// when `saved` is set. Returns how many went.
    pub async fn expire(
        &self,
        site: u128,
        cutoff: i64,
        saved: bool,
        limit: i64,
    ) -> Result<u64, sqlx::Error> {
        let site = id_bytes(site);
        let mut tx = self.writer.begin_with("BEGIN IMMEDIATE").await?;
        let doomed: Vec<(Vec<u8>, i64)> = sqlx::query_as(
            "SELECT capture, bytes FROM captures
             WHERE site = ? AND received_at < ? AND (saved_at IS NULL OR ?) LIMIT ?",
        )
        .bind(&site)
        .bind(cutoff)
        .bind(saved)
        .bind(limit)
        .fetch_all(&mut *tx)
        .await?;
        delete_captures(&mut tx, &site, &doomed).await?;
        tx.commit().await?;
        Ok(doomed.len() as u64)
    }

    /// Captures of `site` recorded under any of `visitors`, the daily pseudonyms of a profile
    /// being erased.
    pub async fn captures_of(
        &self,
        site: u128,
        visitors: &[u128],
    ) -> Result<Vec<u128>, sqlx::Error> {
        let mut captures = Vec::new();
        for visitor in visitors {
            let rows: Vec<Vec<u8>> =
                sqlx::query_scalar("SELECT capture FROM captures WHERE site = ? AND visitor = ?")
                    .bind(id_bytes(site))
                    .bind(id_bytes(*visitor))
                    .fetch_all(&self.reader)
                    .await?;
            captures.extend(rows.iter().map(|capture| id_of(capture)));
        }
        Ok(captures)
    }

    /// Removes the given captures of `site`, saved ones too. Returns how many went.
    pub async fn remove(&self, site: u128, captures: &[u128]) -> Result<u64, sqlx::Error> {
        let site = id_bytes(site);
        let mut tx = self.writer.begin_with("BEGIN IMMEDIATE").await?;
        let mut doomed = Vec::with_capacity(captures.len());
        for capture in captures {
            let capture = id_bytes(*capture);
            let bytes: Option<i64> =
                sqlx::query_scalar("SELECT bytes FROM captures WHERE site = ? AND capture = ?")
                    .bind(&site)
                    .bind(&capture)
                    .fetch_optional(&mut *tx)
                    .await?;
            if let Some(bytes) = bytes {
                doomed.push((capture, bytes));
            }
        }
        delete_captures(&mut tx, &site, &doomed).await?;
        tx.commit().await?;
        Ok(doomed.len() as u64)
    }

    /// Writes a consistent copy of the replay store to `path` while recording continues.
    pub async fn vacuum_into(&self, path: &Path) -> Result<(), sqlx::Error> {
        let target = path.to_str().ok_or_else(|| {
            sqlx::Error::Configuration(format!("{} is not UTF-8", path.display()).into())
        })?;
        sqlx::query("VACUUM INTO ?")
            .bind(target)
            .execute(&self.reader)
            .await?;
        Ok(())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns up to `pages` free pages to the file system. True when free pages remain.
    pub async fn vacuum(&self, pages: u32) -> Result<bool, sqlx::Error> {
        // A number formatted into a pragma, which takes no bind parameters.
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "PRAGMA incremental_vacuum({pages})"
        )))
        .execute(&self.writer)
        .await?;
        let free: i64 = sqlx::query_scalar("PRAGMA freelist_count")
            .fetch_one(&self.writer)
            .await?;
        Ok(free > 0)
    }
}

/// Deletes whole captures with their parts and pages, and gives their bytes back to the cap.
async fn delete_captures(
    tx: &mut sqlx::Transaction<'_, Sqlite>,
    site: &[u8],
    doomed: &[(Vec<u8>, i64)],
) -> Result<(), sqlx::Error> {
    let mut freed = 0;
    for (capture, bytes) in doomed {
        for sql in [
            "DELETE FROM chunks WHERE site = ? AND capture = ?",
            "DELETE FROM capture_pages WHERE site = ? AND capture = ?",
            "DELETE FROM captures WHERE site = ? AND capture = ?",
        ] {
            sqlx::query(sql)
                .bind(site)
                .bind(capture)
                .execute(&mut **tx)
                .await?;
        }
        freed += bytes;
    }
    sqlx::query("UPDATE usage SET bytes = bytes - ?")
        .bind(freed)
        .execute(&mut **tx)
        .await?;
    Ok(())
}
