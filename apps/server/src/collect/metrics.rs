//! Outcome and diagnostic counters. They hold counts only: no address, user agent, identifier
//! or payload ever becomes a label (0001 Transient inputs).
//!
//! Requests count in memory. [`Metrics::save`] writes the running totals to control storage,
//! which the background loop does every 10 seconds and once more at a clean stop, and
//! [`Metrics::load`] reads them back at start. A restart therefore keeps every count saved
//! before it; a crash loses at most the counts of the last interval. Collection itself never
//! waits for this write.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};

use holocron_protocol::operations::collection::{
    CollectionCounter, CollectionOutcome, CollectionRoute, CounterCount, OutcomeCount,
    OutcomeReason,
};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::db::Db;

/// Refusals newer than this mark collection busy or unavailable in status.
pub const RECENT_MS: i64 = 60_000;

type OutcomeKey = (CollectionRoute, CollectionOutcome, Option<OutcomeReason>);

pub struct Metrics {
    since: i64,
    outcomes: Mutex<BTreeMap<OutcomeKey, i64>>,
    counters: Mutex<BTreeMap<CollectionCounter, i64>>,
    /// A count changed since the last save.
    dirty: AtomicBool,
    last_busy: AtomicI64,
    last_unavailable: AtomicI64,
}

impl Metrics {
    /// The saved totals, counting on from them. The first start records `now` as `since`.
    /// Rows with a name this release does not know, left by a newer one, are skipped.
    pub async fn load(db: &Db, now: i64) -> anyhow::Result<Self> {
        sqlx::query("INSERT OR IGNORE INTO collection_counts_since (id, since) VALUES (1, ?)")
            .bind(now)
            .execute(&db.writer)
            .await?;
        let since: i64 = sqlx::query_scalar("SELECT since FROM collection_counts_since")
            .fetch_one(&db.reader)
            .await?;
        let rows: Vec<(String, String, String, i64)> =
            sqlx::query_as("SELECT route, outcome, reason, count FROM collection_outcome_counts")
                .fetch_all(&db.reader)
                .await?;
        let outcomes = rows
            .into_iter()
            .filter_map(|(route, outcome, reason, count)| {
                let reason = match reason.as_str() {
                    "" => None,
                    name => Some(parse(name)?),
                };
                Some(((parse(&route)?, parse(&outcome)?, reason), count))
            })
            .collect();
        let rows: Vec<(String, i64)> =
            sqlx::query_as("SELECT counter, count FROM collection_counter_counts")
                .fetch_all(&db.reader)
                .await?;
        let counters = rows
            .into_iter()
            .filter_map(|(counter, count)| Some((parse(&counter)?, count)))
            .collect();
        Ok(Self {
            since,
            outcomes: Mutex::new(outcomes),
            counters: Mutex::new(counters),
            dirty: AtomicBool::new(false),
            last_busy: AtomicI64::new(i64::MIN),
            last_unavailable: AtomicI64::new(i64::MIN),
        })
    }

    /// Writes the running totals in one transaction when a count changed since the last save.
    /// A failed write leaves the counts marked changed, so the next save retries.
    pub async fn save(&self, db: &Db) -> anyhow::Result<()> {
        if !self.dirty.swap(false, Ordering::AcqRel) {
            return Ok(());
        }
        let outcomes = self.outcomes.lock().expect("outcome counters").clone();
        let counters = self.counters.lock().expect("counters").clone();
        let written: anyhow::Result<()> = async {
            let mut tx = db.writer.begin().await?;
            for ((route, outcome, reason), count) in outcomes {
                sqlx::query(
                    "INSERT INTO collection_outcome_counts (route, outcome, reason, count)
                     VALUES (?, ?, ?, ?)
                     ON CONFLICT (route, outcome, reason) DO UPDATE SET count = excluded.count",
                )
                .bind(name(route))
                .bind(name(outcome))
                .bind(reason.map(name).unwrap_or_default())
                .bind(count)
                .execute(&mut *tx)
                .await?;
            }
            for (counter, count) in counters {
                sqlx::query(
                    "INSERT INTO collection_counter_counts (counter, count) VALUES (?, ?)
                     ON CONFLICT (counter) DO UPDATE SET count = excluded.count",
                )
                .bind(name(counter))
                .bind(count)
                .execute(&mut *tx)
                .await?;
            }
            tx.commit().await?;
            Ok(())
        }
        .await;
        if written.is_err() {
            self.dirty.store(true, Ordering::Release);
        }
        written
    }

    pub fn since(&self) -> i64 {
        self.since
    }

    pub fn outcome(
        &self,
        route: CollectionRoute,
        outcome: CollectionOutcome,
        reason: Option<OutcomeReason>,
        count: i64,
        now_ms: i64,
    ) {
        if count == 0 {
            return;
        }
        match reason {
            Some(OutcomeReason::Busy) => self.last_busy.fetch_max(now_ms, Ordering::Relaxed),
            Some(OutcomeReason::Unavailable) => {
                self.last_unavailable.fetch_max(now_ms, Ordering::Relaxed)
            }
            _ => 0,
        };
        self.dirty.store(true, Ordering::Release);
        *self
            .outcomes
            .lock()
            .expect("outcome counters")
            .entry((route, outcome, reason))
            .or_default() += count;
    }

    pub fn count(&self, counter: CollectionCounter, count: u64) {
        if count > 0 {
            self.dirty.store(true, Ordering::Release);
            *self
                .counters
                .lock()
                .expect("counters")
                .entry(counter)
                .or_default() += count as i64;
        }
    }

    /// Whether busy and unavailable refusals happened within [`RECENT_MS`].
    pub fn recent_refusals(&self, now_ms: i64) -> (bool, bool) {
        let recent = |at: &AtomicI64| at.load(Ordering::Relaxed) > now_ms - RECENT_MS;
        (recent(&self.last_busy), recent(&self.last_unavailable))
    }

    pub fn outcomes(&self) -> Vec<OutcomeCount> {
        self.outcomes
            .lock()
            .expect("outcome counters")
            .iter()
            .map(|(&(route, outcome, reason), &count)| OutcomeCount {
                route,
                outcome,
                reason,
                count,
            })
            .collect()
    }

    pub fn counters(&self) -> Vec<CounterCount> {
        self.counters
            .lock()
            .expect("counters")
            .iter()
            .map(|(&counter, &count)| CounterCount { counter, count })
            .collect()
    }
}

/// The snake_case wire name of a protocol enum value.
fn name(value: impl Serialize) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(name)) => name,
        _ => unreachable!("collection enums serialize as strings"),
    }
}

fn parse<T: DeserializeOwned>(name: &str) -> Option<T> {
    serde_json::from_value(serde_json::Value::String(name.to_owned())).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn saved_counts_survive_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(dir.path()).await.unwrap();
        let first = Metrics::load(&db, 1_000).await.unwrap();
        first.outcome(
            CollectionRoute::Send,
            CollectionOutcome::Accepted,
            None,
            3,
            1_000,
        );
        first.outcome(
            CollectionRoute::Batch,
            CollectionOutcome::Refused,
            Some(OutcomeReason::Busy),
            2,
            1_000,
        );
        first.count(CollectionCounter::Keyless, 4);
        first.save(&db).await.unwrap();
        // Counted after the last save: lost by a crash, which this restart stands for.
        first.count(CollectionCounter::Keyless, 1);

        let second = Metrics::load(&db, 9_000).await.unwrap();
        assert_eq!(second.since(), 1_000);
        let outcomes: Vec<_> = second
            .outcomes()
            .into_iter()
            .map(|o| (o.route, o.outcome, o.reason, o.count))
            .collect();
        assert_eq!(
            outcomes,
            [
                (CollectionRoute::Send, CollectionOutcome::Accepted, None, 3),
                (
                    CollectionRoute::Batch,
                    CollectionOutcome::Refused,
                    Some(OutcomeReason::Busy),
                    2
                ),
            ]
        );
        let counters: Vec<_> = second
            .counters()
            .into_iter()
            .map(|c| (c.counter, c.count))
            .collect();
        assert_eq!(counters, [(CollectionCounter::Keyless, 4)]);
        assert!(
            !second.recent_refusals(9_000).0,
            "recent refusals are not saved"
        );
    }
}
