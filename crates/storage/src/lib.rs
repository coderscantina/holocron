//! Durable collection storage: SQLite inbox, stable batch transfer into DuckDB,
//! analytical watermark, and bounded reads.
//!
//! Callers see four operations: [`Storage::admit`] returns per-record outcomes
//! only after a durable SQLite commit; a background transfer applies stable
//! batches to DuckDB exactly once through an applied-batch ledger and retires
//! them from the inbox; [`Storage::watermark`] follows the highest contiguous
//! applied position; [`Storage::read`] runs bounded analytical reads on a
//! consistent snapshot. Ledger, batch membership and retirement stay inside.
//!
//! Operations (decision 0014): [`Storage::fence`] and [`Storage::erase`] remove data so no
//! writer brings it back, [`Storage::snapshot`] copies the pair under a pause of the
//! analytical writer, [`Storage::shutdown`] stops in order, [`Storage::compact`] clusters closed
//! months by site, and [`upgrade`] is the only path that changes the schema of existing files.

mod analytics;
mod compaction;
mod failpoint;
mod fence;
mod inbox;
mod pairing;
mod reader;
mod record;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tokio::sync::{Notify, mpsc, oneshot, watch};
use tokio::task::JoinHandle;

pub use analytics::{
    ANALYTICS_SCHEMA, Erased, PROPERTY_PARTS, event_property_table, property_type,
};
pub use compaction::Compacted;
pub use duckdb;
pub use failpoint::BOUNDARIES as FAILPOINTS;
use fence::Fences;
pub use fence::{Erasure, Fence};
pub use inbox::{applied_migration, inbox_schema, latest_migration};
pub use pairing::Positions;
pub use reader::{Read, ReadError, Snapshot};
pub use record::*;

use inbox::{Encoded, Inbox};
use reader::Workers;

/// Files of the pair inside the data directory.
pub const INBOX_FILE: &str = "inbox.sqlite";
pub const ANALYTICS_FILE: &str = "analytics.duckdb";
pub const ANALYTICS_WAL_FILE: &str = "analytics.duckdb.wal";
pub const SPILL_DIR: &str = "spill";

/// Installation-wide resource budgets. Fixed before a run and recorded with its results.
#[derive(Debug, Clone)]
pub struct Budgets {
    /// Encoded bytes allowed in the durable inbox backlog. Admission is refused above it.
    pub inbox_bytes: u64,
    /// Encoded bytes waiting for the next group commit.
    pub admission_queue_bytes: u64,
    /// Free bytes the data volume keeps for controlled shutdown and diagnostics.
    pub disk_reserve_bytes: u64,
    /// Largest stable batch, in records and encoded bytes.
    pub batch_records: usize,
    pub batch_bytes: u64,
    /// Wait after a commit wakeup before assigning, so batches amortize DuckDB commits.
    pub transfer_delay: Duration,
    /// DuckDB worker threads shared by the writer and every reader.
    pub engine_threads: u32,
    /// DuckDB buffer manager limit; spill goes to `<data>/spill` beyond it.
    pub engine_memory_bytes: u64,
    pub spill_bytes: u64,
    /// Concurrent report reads and how many may wait for a worker.
    pub report_workers: usize,
    pub report_queue: usize,
    /// Cancellation limit for one read, queue wait included (0004: 30 s).
    pub query_timeout: Duration,
}

impl Default for Budgets {
    /// The reference-host budgets recorded in decision 0008.
    fn default() -> Self {
        const MIB: u64 = 1024 * 1024;
        Self {
            inbox_bytes: 1024 * MIB,
            admission_queue_bytes: 64 * MIB,
            disk_reserve_bytes: 4 * 1024 * MIB,
            batch_records: 20_000,
            batch_bytes: 32 * MIB,
            transfer_delay: Duration::from_millis(250),
            engine_threads: 2,
            engine_memory_bytes: 1024 * MIB,
            spill_bytes: 40 * 1024 * MIB,
            report_workers: 2,
            report_queue: 8,
            query_timeout: Duration::from_secs(30),
        }
    }
}

/// Highest contiguous accepted position applied to analytics: the last applied
/// batch and the last inbox sequence it contains. Batches apply in ID order.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Watermark {
    pub batch: i64,
    pub sequence: i64,
}

/// Per-record admission outcome, returned only after the durable commit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Accepted {
        sequence: i64,
    },
    /// Same client `eventId` within 48 hours; not stored (0002). `conflict` is set when the
    /// first accepted payload differed from this one.
    Duplicate {
        conflict: bool,
    },
}

/// Whole-request refusal. Nothing from the request is stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Refusal {
    /// Inbox or admission queue budget is full (HTTP 429 with Retry-After).
    #[error("admission budget full")]
    Busy,
    /// Durable commit failed, disk headroom is below the reserve, or storage is shutting
    /// down (HTTP 503).
    #[error("storage unavailable")]
    Unavailable,
}

#[derive(Debug, Clone, Copy)]
pub enum Lane {
    /// Dashboard reports, bounded by `report_workers` and `report_queue`.
    Report,
    /// Short live reads (realtime, status, visibility probes) that must not queue behind reports.
    Live,
}

#[derive(Debug, Clone, Copy)]
pub struct Status {
    pub inbox_records: u64,
    pub inbox_bytes: u64,
    pub watermark: Watermark,
    /// Failed transfer attempts since start.
    pub transfer_failures: u64,
    /// Since when every transfer attempt failed, Unix milliseconds; `None` while it works.
    pub transfer_failing_since_ms: Option<i64>,
}

/// Bytes the engines take on the data volume (performance validation: WAL, inbox, spill).
#[derive(Debug, Clone, Copy, Default, serde::Serialize)]
pub struct Disk {
    pub analytics_bytes: u64,
    pub analytics_wal_bytes: u64,
    pub inbox_bytes: u64,
    pub inbox_wal_bytes: u64,
    pub spill_bytes: u64,
    pub free_bytes: u64,
    pub reserve_bytes: u64,
}

/// Schema versions of the pair as stored; `None` for a file that does not exist yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoredSchemas {
    pub inbox: Option<i64>,
    pub analytics: Option<i64>,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("inbox: {0}")]
    Sqlite(#[from] sqlx::Error),
    #[error("inbox migration: {0}")]
    Migrate(#[from] sqlx::migrate::MigrateError),
    #[error("analytical engine: {0}")]
    Engine(#[from] duckdb::Error),
    #[error("filesystem: {0}")]
    Io(#[from] std::io::Error),
    #[error("refusing incompatible replay: {0}")]
    Incompatible(String),
    /// An older schema: only `holocron upgrade` changes it.
    #[error("upgrade required: {0}")]
    UpgradeRequired(String),
    #[error("data newer than this release: {0}")]
    DataNewer(String),
    /// The inbox and the analytics file do not belong together.
    #[error("inbox and analytics do not match: {0}")]
    Mismatch(String),
}

/// How engine files are opened: serving refuses an older schema, upgrading migrates it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    Serve,
    Upgrade,
}

struct Request {
    records: Vec<Encoded>,
    bytes: u64,
    reply: oneshot::Sender<Result<Vec<Outcome>, Refusal>>,
}

/// Transfer attempts, for health: a failing transfer is not the same as a slow one.
#[derive(Default)]
struct TransferHealth {
    failures: AtomicU64,
    /// Zero while transfer works.
    failing_since_ms: AtomicI64,
}

pub struct Storage {
    budgets: Budgets,
    data_dir: PathBuf,
    inbox: Inbox,
    queued: Arc<AtomicU64>,
    admissions: Mutex<Option<mpsc::UnboundedSender<Request>>>,
    writer: Arc<Mutex<duckdb::Connection>>,
    reports: Workers,
    live: Workers,
    watermark: watch::Receiver<Watermark>,
    wake: Arc<Notify>,
    fences: Arc<RwLock<Fences>>,
    transfer: Arc<TransferHealth>,
    stop: watch::Sender<bool>,
    tasks: Mutex<Vec<JoinHandle<()>>>,
}

impl Storage {
    /// Opens or creates `<data_dir>/inbox.sqlite` and `<data_dir>/analytics.duckdb`,
    /// checks that they are one pair, publishes the committed watermark, and resumes any
    /// assigned batch. Files of an older schema are refused with [`Error::UpgradeRequired`].
    pub async fn open(data_dir: &Path, budgets: Budgets) -> Result<Self, Error> {
        let spill = data_dir.join(SPILL_DIR);
        std::fs::create_dir_all(&spill)?;
        let inbox = Inbox::open(&data_dir.join(INBOX_FILE), Mode::Serve).await?;
        let writer = Arc::new(Mutex::new(analytics::open(
            &data_dir.join(ANALYTICS_FILE),
            &spill,
            &budgets,
            Mode::Serve,
        )?));
        pair_up(&inbox, &writer).await?;
        let mut fences = Fences::default();
        for fence in inbox.fences().await? {
            fences.insert(fence);
        }
        let fences = Arc::new(RwLock::new(fences));
        let (reports, live, applied) = {
            let conn = writer.lock().expect("analytical writer");
            let reports = Workers::start(
                "report",
                &conn,
                budgets.report_workers,
                budgets.report_queue,
                budgets.query_timeout,
            )?;
            let live = Workers::start(
                "live",
                &conn,
                1,
                budgets.report_queue,
                budgets.query_timeout,
            )?;
            (reports, live, analytics::watermark(&conn)?)
        };
        // Restart reconciliation: the ledger is the source of truth for visibility.
        let (watermark_tx, watermark) = watch::channel(applied);

        let (admissions, requests) = mpsc::unbounded_channel();
        let queued = Arc::new(AtomicU64::new(0));
        let wake = Arc::new(Notify::new());
        let transfer = Arc::new(TransferHealth::default());
        let (stop, stopped) = watch::channel(false);
        let tasks = vec![
            tokio::spawn(admit_loop(
                inbox.clone(),
                requests,
                queued.clone(),
                wake.clone(),
                data_dir.to_path_buf(),
                budgets.disk_reserve_bytes,
            )),
            tokio::spawn(transfer_loop(
                Transfer {
                    inbox: inbox.clone(),
                    writer: writer.clone(),
                    fences: fences.clone(),
                    health: transfer.clone(),
                    watermark: watermark_tx,
                    budgets: budgets.clone(),
                },
                wake.clone(),
                stopped,
            )),
        ];
        Ok(Self {
            budgets,
            data_dir: data_dir.to_path_buf(),
            inbox,
            queued,
            admissions: Mutex::new(Some(admissions)),
            writer,
            reports,
            live,
            watermark,
            wake,
            fences,
            transfer,
            stop,
            tasks: Mutex::new(tasks),
        })
    }

    /// Admits one request's records in one durable commit, possibly grouped with
    /// other requests. Outcomes return only after SQLite reports the commit.
    pub async fn admit(&self, records: &[Record]) -> Result<Vec<Outcome>, Refusal> {
        if records.is_empty() {
            return Ok(Vec::new());
        }
        let records: Vec<Encoded> = records
            .iter()
            .map(|record| Encoded {
                site: record.site(),
                client_event_id: match record {
                    Record::Event(e) => e.client_event_id.clone().map(|id| (id, e.receive_time_ms)),
                    _ => None,
                },
                payload: record.encode(),
            })
            .collect();
        let bytes: u64 = records.iter().map(|r| r.payload.len() as u64).sum();
        let queued = self.queued.fetch_add(bytes, Ordering::SeqCst) + bytes;
        let backlog = self.inbox.backlog.bytes.load(Ordering::SeqCst);
        if queued > self.budgets.admission_queue_bytes
            || backlog + queued > self.budgets.inbox_bytes
        {
            self.queued.fetch_sub(bytes, Ordering::SeqCst);
            return Err(Refusal::Busy);
        }
        let (reply, outcome) = oneshot::channel();
        let sender = self.admissions.lock().expect("admissions").clone();
        let sent = sender.is_some_and(|sender| {
            sender
                .send(Request {
                    records,
                    bytes,
                    reply,
                })
                .is_ok()
        });
        if !sent {
            self.queued.fetch_sub(bytes, Ordering::SeqCst);
            return Err(Refusal::Unavailable);
        }
        outcome.await.unwrap_or(Err(Refusal::Unavailable))
    }

    /// Admits one chunk of imported records under `key` (milestone 11). The key's ledger row
    /// commits with the records, so a chunk retried after a crash is not admitted again: the
    /// answer is then `false`. A key that comes back with another record count is refused as
    /// incompatible. Waits for transfer to drain the inbox when the chunk would exceed its
    /// budget, so imports yield to the same budget as collection.
    pub async fn admit_import(&self, key: &str, records: &[Record]) -> Result<bool, Error> {
        let records: Vec<Encoded> = records
            .iter()
            .map(|record| Encoded {
                site: record.site(),
                client_event_id: None,
                payload: record.encode(),
            })
            .collect();
        let bytes: u64 = records.iter().map(|r| r.payload.len() as u64).sum();
        if self.inbox.backlog.bytes.load(Ordering::SeqCst) + bytes > self.budgets.inbox_bytes {
            self.drain().await;
        }
        if free_bytes(&self.data_dir)? < self.budgets.disk_reserve_bytes {
            return Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::StorageFull,
                "free disk space is below the reserve",
            )));
        }
        let (admitted, count) = self.inbox.commit_import(key, &records).await?;
        if count != records.len() as i64 {
            return Err(Error::Incompatible(format!(
                "import chunk {key} was admitted with {count} records, now has {}",
                records.len()
            )));
        }
        if admitted {
            self.wake.notify_one();
        }
        Ok(admitted)
    }

    /// Follows the committed analytical watermark.
    pub fn watermark(&self) -> watch::Receiver<Watermark> {
        self.watermark.clone()
    }

    /// Age of the oldest record still waiting in the inbox: how far transfer lags behind
    /// admission. Zero when the inbox is empty.
    pub async fn inbox_age(&self) -> Result<Duration, Error> {
        self.inbox_age_after(Watermark::default()).await
    }

    /// Age of the oldest waiting record a snapshot at `watermark` does not show. Zero when
    /// it shows every admitted record.
    pub async fn inbox_age_after(&self, watermark: Watermark) -> Result<Duration, Error> {
        let oldest = self.inbox.oldest_received_ms(watermark.sequence).await?;
        Ok(Duration::from_millis(
            oldest.map_or(0, |at| now_ms().saturating_sub(at).max(0) as u64),
        ))
    }

    /// Free bytes on the data volume and the reserve below which admission is refused.
    pub fn disk_headroom(&self) -> std::io::Result<(u64, u64)> {
        Ok((free_bytes(&self.data_dir)?, self.budgets.disk_reserve_bytes))
    }

    /// File sizes of the pair, spill, and the volume's free space.
    pub fn disk(&self) -> std::io::Result<Disk> {
        let size = |name: &str| std::fs::metadata(self.data_dir.join(name)).map_or(0, |m| m.len());
        let spill = match std::fs::read_dir(self.data_dir.join(SPILL_DIR)) {
            Ok(entries) => entries
                .filter_map(|e| e.ok()?.metadata().ok())
                .map(|m| m.len())
                .sum(),
            Err(_) => 0,
        };
        Ok(Disk {
            analytics_bytes: size(ANALYTICS_FILE),
            analytics_wal_bytes: size(ANALYTICS_WAL_FILE),
            inbox_bytes: size(INBOX_FILE),
            inbox_wal_bytes: size("inbox.sqlite-wal"),
            spill_bytes: spill,
            free_bytes: free_bytes(&self.data_dir)?,
            reserve_bytes: self.budgets.disk_reserve_bytes,
        })
    }

    pub fn status(&self) -> Status {
        let since = self.transfer.failing_since_ms.load(Ordering::Relaxed);
        Status {
            inbox_records: self.inbox.backlog.records.load(Ordering::Relaxed),
            inbox_bytes: self.inbox.backlog.bytes.load(Ordering::Relaxed),
            watermark: *self.watermark.borrow(),
            transfer_failures: self.transfer.failures.load(Ordering::Relaxed),
            transfer_failing_since_ms: (since != 0).then_some(since),
        }
    }

    /// Whether a report read sent now would start at once: none waits and a worker is free.
    /// Background reads check it so they never queue ahead of a dashboard.
    pub fn reports_idle(&self) -> bool {
        self.reports.idle()
    }

    /// Runs `f` on a bounded read worker inside one DuckDB transaction.
    pub async fn read<T, F>(&self, lane: Lane, f: F) -> Result<Read<T>, ReadError>
    where
        T: Send + 'static,
        F: FnOnce(&Snapshot) -> duckdb::Result<T> + Send + 'static,
    {
        match lane {
            Lane::Report => self.reports.read(f).await,
            Lane::Live => self.live.read(f).await,
        }
    }

    /// Runs derived-data work (corrections, maintenance) on the single analytical
    /// writer, between transfer batches, in one transaction.
    pub async fn write<T, F>(&self, f: F) -> Result<T, Error>
    where
        T: Send + 'static,
        F: FnOnce(&duckdb::Transaction) -> duckdb::Result<T> + Send + 'static,
    {
        let writer = self.writer.clone();
        tokio::task::spawn_blocking(move || {
            let mut conn = writer.lock().expect("analytical writer");
            let tx = conn.transaction()?;
            let value = f(&tx)?;
            failpoint::hit("write.before_commit");
            tx.commit()?;
            Ok(value)
        })
        .await
        .expect("writer task")
    }

    /// Waits until every record admitted so far is applied and retired.
    pub async fn drain(&self) {
        let mut watermark = self.watermark();
        while self.inbox.backlog.records.load(Ordering::SeqCst) > 0 {
            let _ = tokio::time::timeout(Duration::from_millis(50), watermark.changed()).await;
        }
    }

    /// The last inbox sequence admission handed out: where a fence asked for later should end.
    pub async fn last_sequence(&self) -> Result<i64, Error> {
        Ok(self.inbox.last_sequence().await?)
    }

    /// Durably fences `erasure`: from the commit on, transfer leaves out every record it
    /// covers. A reset or profile fence covers records admitted up to `through_seq`, or up to
    /// now when it is `None`. Asking again with the same `key` returns the first fence, so an
    /// interrupted job resumes with the same bounds.
    pub async fn fence(
        &self,
        key: &str,
        erasure: Erasure,
        through_seq: Option<i64>,
    ) -> Result<Fence, Error> {
        let fence = self
            .inbox
            .fence(key, erasure, through_seq, now_ms())
            .await?;
        // Transfer reads the fences while it holds the writer, so an apply that started before
        // this line ends before any erase that follows can run.
        self.fences.write().expect("fences").insert(fence);
        Ok(fence)
    }

    /// Removes what `fence` covers from analytics. Fence first: records still in the inbox
    /// are then left out by transfer, so after both nothing covered can reach analytics.
    pub async fn erase(&self, fence: Fence) -> Result<Erased, Error> {
        self.write(move |tx| analytics::erase(tx, &fence)).await
    }

    /// Removes a site's analytical rows before its retention cutoff; see 0006 Retention cutoff.
    /// `cutoff_day` is the site-local date of `cutoff_ms`, in days since 1970-01-01.
    /// Returns the events removed.
    pub async fn expire(&self, site: Id, cutoff_ms: i64, cutoff_day: i32) -> Result<u64, Error> {
        self.write(move |tx| analytics::expire(tx, site, cutoff_ms, cutoff_day))
            .await
    }

    /// Forgets client event IDs whose 48-hour window ended before `now_ms`.
    pub async fn prune_event_ids(&self, now_ms: i64) -> Result<u64, Error> {
        Ok(self
            .inbox
            .prune_event_ids(now_ms - inbox::DEDUP_WINDOW_MS)
            .await?)
    }

    /// Copies the analytics file and the inbox into `dir` as one consistent pair. Transfer,
    /// correction and every other analytical write wait meanwhile; admission and reads go on.
    /// Returns how long the writer was paused.
    pub async fn snapshot(&self, dir: &Path) -> Result<Duration, Error> {
        let writer = self.writer.clone();
        let inbox = self.inbox.clone();
        let source = self.data_dir.clone();
        let dir = dir.to_path_buf();
        let runtime = tokio::runtime::Handle::current();
        tokio::task::spawn_blocking(move || {
            let conn = writer.lock().expect("analytical writer");
            let paused = Instant::now();
            // A checkpoint folds the WAL into the file. A reader inside a transaction can keep
            // it from finishing; then the WAL is copied too. Neither file changes while the
            // writer waits, so the two copies match either way.
            let _ = conn.execute_batch("CHECKPOINT");
            copy_analytics(&source, &dir)?;
            runtime.block_on(inbox.vacuum_into(&dir.join(INBOX_FILE)))?;
            drop(conn);
            Ok::<_, Error>(paused.elapsed())
        })
        .await
        .expect("snapshot task")
    }

    /// Stops in order: admission first (requests already queued still commit), then transfer
    /// after the batch in hand, then the read workers, then a DuckDB checkpoint. Admission
    /// answers unavailable from the first step on. The next open reconciles from the ledger.
    pub async fn shutdown(&self) -> Result<(), Error> {
        self.admissions.lock().expect("admissions").take();
        let _ = self.stop.send(true);
        let tasks = std::mem::take(&mut *self.tasks.lock().expect("tasks"));
        for task in tasks {
            let _ = task.await;
        }
        self.reports.close();
        self.live.close();
        self.inbox.close().await;
        let writer = self.writer.clone();
        tokio::task::spawn_blocking(move || {
            writer
                .lock()
                .expect("analytical writer")
                .execute_batch("CHECKPOINT")
        })
        .await
        .expect("checkpoint task")?;
        Ok(())
    }

    /// [`Storage::shutdown`] for an owner.
    pub async fn close(self) -> Result<(), Error> {
        self.shutdown().await
    }
}

/// Creates the pair's ID on new files and checks it on existing ones. A file that has none
/// yet (new, or from before pairing) takes the other's; the positions then decide whether
/// the two belong together.
async fn pair_up(inbox: &Inbox, conn: &Mutex<duckdb::Connection>) -> Result<(), Error> {
    let inbox_pair = inbox.pair().await?;
    let positions = inbox.positions().await?;
    let pair = pair_ledger(
        &conn.lock().expect("analytical writer"),
        inbox_pair.as_deref(),
        &positions,
    )?;
    if inbox_pair.is_none() {
        inbox.set_pair(&pair).await?;
    }
    Ok(())
}

/// The analytics half of [`pair_up`]: checks the pair and gives the ledger its ID when it has
/// none. Returns the pair's ID.
fn pair_ledger(
    conn: &duckdb::Connection,
    inbox_pair: Option<&str>,
    positions: &inbox::InboxPositions,
) -> Result<String, Error> {
    let ledger_pair = analytics::pair(conn)?;
    let pair = match (inbox_pair, ledger_pair.as_deref()) {
        (Some(a), Some(b)) if a != b => {
            return Err(Error::Mismatch(format!(
                "inbox.sqlite belongs to pair {a}, analytics.duckdb to pair {b}"
            )));
        }
        (Some(pair), _) | (None, Some(pair)) => pair.to_owned(),
        (None, None) => new_pair(),
    };
    let id = Some(pair.as_str());
    pairing::check(id, id, positions, conn)?;
    if ledger_pair.is_none() {
        analytics::set_pair(conn, &pair)?;
    }
    Ok(pair)
}

fn new_pair() -> String {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).expect("the OS provides randomness");
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The schema versions of the pair in `data_dir`, read without changing anything.
pub async fn stored_schemas(data_dir: &Path) -> Result<StoredSchemas, Error> {
    let inbox_path = data_dir.join(INBOX_FILE);
    let inbox = if inbox_path.exists() {
        let pool = sqlite_pool(&inbox_path, true).await?;
        let version = applied_migration(&pool).await?;
        pool.close().await;
        version
    } else {
        None
    };
    let analytics_path = data_dir.join(ANALYTICS_FILE);
    let analytics = if analytics_path.exists() {
        let conn = duckdb::Connection::open_with_flags(
            &analytics_path,
            duckdb::Config::default().access_mode(duckdb::AccessMode::ReadOnly)?,
        )?;
        analytics::stored_schema(&conn)?
    } else {
        None
    };
    Ok(StoredSchemas { inbox, analytics })
}

/// Brings the pair in `data_dir` to this release's schema: inbox migrations first, then
/// applied batches retired (their ledger entries name the old schema), then DuckDB. Each step
/// checks what the files already have, so an interrupted upgrade runs again from the start.
pub async fn upgrade(data_dir: &Path, budgets: &Budgets) -> Result<(), Error> {
    let spill = data_dir.join(SPILL_DIR);
    std::fs::create_dir_all(&spill)?;
    let inbox = Inbox::open(&data_dir.join(INBOX_FILE), Mode::Upgrade).await?;
    let path = data_dir.join(ANALYTICS_FILE);
    let ledger = duckdb::Connection::open(&path)?;
    if ledger.query_row(
        "SELECT EXISTS (SELECT 1 FROM duckdb_tables() WHERE table_name = 'applied_batches')",
        [],
        |r| r.get::<_, bool>(0),
    )? {
        let applied = applied_entries(&ledger)?;
        inbox::retire_applied(inbox.writer(), |[id, first, last, records]| {
            applied.get(id) == Some(&[*first, *last, *records])
        })
        .await?;
    }
    drop(ledger);
    let conn = Mutex::new(analytics::open(&path, &spill, budgets, Mode::Upgrade)?);
    pair_up(&inbox, &conn).await?;
    conn.into_inner()
        .expect("analytical writer")
        .execute_batch("CHECKPOINT")?;
    inbox.close().await;
    Ok(())
}

fn applied_entries(
    ledger: &duckdb::Connection,
) -> duckdb::Result<std::collections::HashMap<i64, [i64; 3]>> {
    ledger
        .prepare("SELECT batch_id, first_seq, last_seq, records FROM applied_batches")?
        .query_map([], |r| Ok((r.get(0)?, [r.get(1)?, r.get(2)?, r.get(3)?])))?
        .collect()
}

/// Opens a copy of the pair (a backup or a restored directory), checks that it is one pair,
/// and returns its positions. Opening folds a copied DuckDB WAL into the file. Works on files
/// of an older schema, which it does not migrate.
pub async fn inspect(data_dir: &Path) -> Result<Positions, Error> {
    let pool = sqlite_pool(&data_dir.join(INBOX_FILE), false).await?;
    let inbox = inbox::positions(&pool).await?;
    let inbox_pair = inbox::pair(&pool).await?;
    pool.close().await;
    let conn = duckdb::Connection::open(data_dir.join(ANALYTICS_FILE))?;
    let ledger_pair = analytics::pair(&conn)?;
    pairing::check(inbox_pair.as_deref(), ledger_pair.as_deref(), &inbox, &conn)?;
    let positions = pairing::positions(&inbox, &conn)?;
    conn.execute_batch("CHECKPOINT")?;
    Ok(positions)
}

/// Copies the pair of a stopped installation into `dir`, whatever its schema.
pub async fn snapshot_stopped(data_dir: &Path, dir: &Path) -> Result<(), Error> {
    copy_analytics(data_dir, dir)?;
    let pool = sqlite_pool(&data_dir.join(INBOX_FILE), true).await?;
    inbox::vacuum_into(&pool, &dir.join(INBOX_FILE)).await?;
    pool.close().await;
    Ok(())
}

/// The DuckDB file and, when one exists, its WAL.
fn copy_analytics(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::copy(from.join(ANALYTICS_FILE), to.join(ANALYTICS_FILE))?;
    match std::fs::copy(from.join(ANALYTICS_WAL_FILE), to.join(ANALYTICS_WAL_FILE)) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        result => result.map(|_| ()),
    }
}

async fn sqlite_pool(path: &Path, read_only: bool) -> Result<sqlx::SqlitePool, Error> {
    let options = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(path)
        .read_only(read_only);
    Ok(sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await?)
}

/// Group commit: takes every request queued at the moment the writer is free
/// and commits them together. No added delay; queueing alone forms the group.
async fn admit_loop(
    inbox: Inbox,
    mut requests: mpsc::UnboundedReceiver<Request>,
    queued: Arc<AtomicU64>,
    wake: Arc<Notify>,
    data_dir: PathBuf,
    reserve: u64,
) {
    const GROUP_BYTES: u64 = 16 * 1024 * 1024;
    while let Some(first) = requests.recv().await {
        let mut bytes = first.bytes;
        let mut group = vec![first];
        while bytes < GROUP_BYTES {
            match requests.try_recv() {
                Ok(next) => {
                    bytes += next.bytes;
                    group.push(next);
                }
                Err(_) => break,
            }
        }
        let result = if free_bytes(&data_dir).is_ok_and(|free| free >= reserve) {
            let parts: Vec<&[Encoded]> = group.iter().map(|r| r.records.as_slice()).collect();
            inbox.commit(&parts).await.map_err(|_| Refusal::Unavailable)
        } else {
            Err(Refusal::Unavailable)
        };
        queued.fetch_sub(bytes, Ordering::SeqCst);
        match result {
            Ok(outcomes) => {
                failpoint::hit("admission.after_commit");
                wake.notify_one();
                for (request, outcome) in group.into_iter().zip(outcomes) {
                    let _ = request.reply.send(Ok(outcome));
                }
            }
            Err(refusal) => {
                for request in group {
                    let _ = request.reply.send(Err(refusal));
                }
            }
        }
    }
}

/// What the single analytical writer needs to move batches.
struct Transfer {
    inbox: Inbox,
    writer: Arc<Mutex<duckdb::Connection>>,
    fences: Arc<RwLock<Fences>>,
    health: Arc<TransferHealth>,
    watermark: watch::Sender<Watermark>,
    budgets: Budgets,
}

/// The single analytical writer: assign or resume a stable batch, apply it
/// once, publish the watermark, retire the inbox records.
async fn transfer_loop(transfer: Transfer, wake: Arc<Notify>, mut stop: watch::Receiver<bool>) {
    while !*stop.borrow() {
        match transfer_one(&transfer).await {
            Ok(moved) => {
                transfer.health.failing_since_ms.store(0, Ordering::Relaxed);
                if moved {
                    continue;
                }
                tokio::select! {
                    _ = async {
                        wake.notified().await;
                        tokio::time::sleep(transfer.budgets.transfer_delay).await;
                    } => {}
                    _ = stop.changed() => {}
                }
            }
            Err(error) => {
                transfer.health.failures.fetch_add(1, Ordering::Relaxed);
                let _ = transfer.health.failing_since_ms.compare_exchange(
                    0,
                    now_ms(),
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                );
                eprintln!("holocron-storage: transfer failed, retrying the same batch: {error}");
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_secs(1)) => {}
                    _ = stop.changed() => {}
                }
            }
        }
    }
}

async fn transfer_one(transfer: &Transfer) -> Result<bool, Error> {
    let Some(batch) = transfer
        .inbox
        .next_batch(transfer.budgets.batch_records, transfer.budgets.batch_bytes)
        .await?
    else {
        return Ok(false);
    };
    let writer = transfer.writer.clone();
    let fences = transfer.fences.clone();
    let (batch, applied) = tokio::task::spawn_blocking(move || {
        let mut conn = writer.lock().expect("analytical writer");
        let fences = fences.read().expect("fences").clone();
        let applied = analytics::apply(&mut conn, &batch, &fences);
        (batch, applied)
    })
    .await
    .expect("apply task");
    let applied = applied?;
    failpoint::hit("transfer.before_notify");
    transfer.watermark.send_if_modified(|current| {
        let newer = applied > *current;
        if newer {
            *current = applied;
        }
        newer
    });
    failpoint::hit("transfer.before_retire");
    transfer.inbox.retire(&batch).await?;
    Ok(true)
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after 1970")
        .as_millis() as i64
}

/// Bytes available to an unprivileged writer on the file system holding `path`.
pub fn free_bytes(path: &Path) -> std::io::Result<u64> {
    let c_path = std::ffi::CString::new(path.as_os_str().as_encoded_bytes())?;
    // SAFETY: statvfs fills the zeroed struct for a valid NUL-terminated path.
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c_path.as_ptr(), &mut stat) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(stat.f_bavail as u64 * stat.f_frsize as u64)
}
