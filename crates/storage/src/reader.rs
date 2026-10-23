//! Bounded read executor. Engine work runs on owned worker threads, never on
//! Tokio workers, with a fixed worker count and queue length per lane.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use duckdb::{Connection, InterruptHandle};
use tokio::sync::oneshot;

use crate::{Watermark, analytics};

/// A consistent analytical snapshot: the connection is inside one DuckDB
/// transaction, so query results and the watermark come from the same state.
pub struct Snapshot<'a> {
    conn: &'a Connection,
    watermark: Watermark,
}

impl Snapshot<'_> {
    pub fn connection(&self) -> &Connection {
        self.conn
    }

    pub fn watermark(&self) -> Watermark {
        self.watermark
    }
}

/// A completed read with its timing split into queue wait and execution.
#[derive(Debug)]
pub struct Read<T> {
    pub value: T,
    pub watermark: Watermark,
    pub queue_wait: Duration,
    pub execution: Duration,
}

#[derive(Debug, thiserror::Error)]
pub enum ReadError {
    #[error("read queue is full")]
    Busy,
    #[error("read cancelled after the query timeout")]
    Timeout,
    #[error("analytical engine: {0}")]
    Engine(#[from] duckdb::Error),
    #[error("storage is closed")]
    Closed,
}

type Work = Box<dyn FnOnce(&Connection) + Send>;

struct Job {
    cancel: Arc<Cancel>,
    work: Work,
}

#[derive(Default)]
struct Cancel {
    cancelled: AtomicBool,
    running: Mutex<Option<Arc<InterruptHandle>>>,
}

/// Cancels a read whose caller stopped waiting: a dropped request or a newer exploration
/// replacing it frees the worker instead of finishing work nobody reads.
struct CancelOnDrop(Option<Arc<Cancel>>);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        if let Some(cancel) = self.0.take() {
            cancel.cancel();
        }
    }
}

impl Cancel {
    fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        if let Some(handle) = self.running.lock().expect("cancel lock").as_ref() {
            handle.interrupt();
        }
    }
}

/// One read lane: `workers` threads sharing a queue of `queue` waiting reads.
pub(crate) struct Workers {
    sender: Mutex<Option<SyncSender<Job>>>,
    threads: Mutex<Vec<JoinHandle<()>>>,
    timeout: Duration,
    workers: usize,
    /// Reads queued or running.
    pending: Arc<AtomicUsize>,
}

impl Workers {
    pub fn start(
        name: &str,
        base: &Connection,
        workers: usize,
        queue: usize,
        timeout: Duration,
    ) -> duckdb::Result<Self> {
        let (sender, receiver) = mpsc::sync_channel::<Job>(queue);
        let receiver = Arc::new(Mutex::new(receiver));
        let pending = Arc::new(AtomicUsize::new(0));
        let mut threads = Vec::with_capacity(workers);
        for n in 0..workers {
            let conn = base.try_clone()?;
            let receiver = receiver.clone();
            let pending = pending.clone();
            let thread = std::thread::Builder::new()
                .name(format!("{name}-{n}"))
                .spawn(move || {
                    let interrupt = conn.interrupt_handle();
                    loop {
                        let job = match receiver.lock().expect("lane receiver").recv() {
                            Ok(job) => job,
                            Err(_) => return,
                        };
                        if !job.cancel.cancelled.load(Ordering::SeqCst) {
                            *job.cancel.running.lock().expect("cancel lock") =
                                Some(interrupt.clone());
                            // Interrupts that arrived before registration still apply.
                            if job.cancel.cancelled.load(Ordering::SeqCst) {
                                interrupt.interrupt();
                            }
                            (job.work)(&conn);
                            *job.cancel.running.lock().expect("cancel lock") = None;
                        }
                        pending.fetch_sub(1, Ordering::SeqCst);
                    }
                })
                .expect("spawn read worker");
            threads.push(thread);
        }
        Ok(Self {
            sender: Mutex::new(Some(sender)),
            threads: Mutex::new(threads),
            timeout,
            workers,
            pending,
        })
    }

    /// Whether a read sent now would start at once: no read waits and a worker is free.
    pub fn idle(&self) -> bool {
        self.pending.load(Ordering::SeqCst) < self.workers
    }

    pub async fn read<T, F>(&self, f: F) -> Result<Read<T>, ReadError>
    where
        T: Send + 'static,
        F: FnOnce(&Snapshot) -> duckdb::Result<T> + Send + 'static,
    {
        let sender = self
            .sender
            .lock()
            .expect("lane sender")
            .clone()
            .ok_or(ReadError::Closed)?;
        let (reply, result) = oneshot::channel();
        let enqueued = Instant::now();
        let cancel = Arc::new(Cancel::default());
        let work: Work = Box::new(move |conn| {
            let started = Instant::now();
            let value = in_snapshot(conn, f);
            let _ = reply.send((value, started - enqueued, started.elapsed()));
        });
        self.pending.fetch_add(1, Ordering::SeqCst);
        sender
            .try_send(Job {
                cancel: cancel.clone(),
                work,
            })
            .map_err(|e| {
                self.pending.fetch_sub(1, Ordering::SeqCst);
                match e {
                    TrySendError::Full(_) => ReadError::Busy,
                    TrySendError::Disconnected(_) => ReadError::Closed,
                }
            })?;
        // Workers stop once every sender is gone; this read must not keep them.
        drop(sender);
        let mut abandoned = CancelOnDrop(Some(cancel));
        match tokio::time::timeout(self.timeout, result).await {
            Ok(Ok((value, queue_wait, execution))) => {
                abandoned.0 = None;
                let (value, watermark) = value?;
                Ok(Read {
                    value,
                    watermark,
                    queue_wait,
                    execution,
                })
            }
            Ok(Err(_)) => Err(ReadError::Closed),
            Err(_) => Err(ReadError::Timeout),
        }
    }

    /// Refuses new reads, lets queued ones finish, and joins the workers.
    pub fn close(&self) {
        self.sender.lock().expect("lane sender").take();
        let threads = std::mem::take(&mut *self.threads.lock().expect("lane threads"));
        for thread in threads {
            let _ = thread.join();
        }
    }
}

fn in_snapshot<T>(
    conn: &Connection,
    f: impl FnOnce(&Snapshot) -> duckdb::Result<T>,
) -> duckdb::Result<(T, Watermark)> {
    conn.execute_batch("BEGIN TRANSACTION")?;
    let result = analytics::watermark(conn).and_then(|watermark| {
        let value = f(&Snapshot { conn, watermark })?;
        Ok((value, watermark))
    });
    let end = if result.is_ok() { "COMMIT" } else { "ROLLBACK" };
    match (result, conn.execute_batch(end)) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(e), _) | (Ok(_), Err(e)) => Err(e),
    }
}
