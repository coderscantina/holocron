//! The inbox and the analytics file are one pair: transfer state lives half in each. Before
//! either is used, the pair is checked, so files from two installations, two backups or two
//! moments are refused instead of served (decision 0014).

use duckdb::{Connection, OptionalExt};

use crate::analytics;
use crate::inbox::InboxPositions;
use crate::{Error, Watermark};

/// Where the pair stands, recorded in backup manifests and compared at restore.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Positions {
    pub watermark_batch: i64,
    pub watermark_sequence: i64,
    /// Ledger entries and the inbox records they cover.
    pub ledger_batches: i64,
    pub ledger_records: i64,
    /// Records still in the inbox, assigned or pending.
    pub inbox_records: i64,
    pub inbox_bytes: i64,
    pub assigned_batches: i64,
    /// The highest batch ID and inbox sequence ever assigned.
    pub last_batch: i64,
    pub last_sequence: i64,
}

pub(crate) fn positions(inbox: &InboxPositions, ledger: &Connection) -> Result<Positions, Error> {
    let watermark = analytics::watermark(ledger)?;
    let (ledger_batches, ledger_records): (i64, i64) = ledger.query_row(
        "SELECT count(*), coalesce(sum(records), 0)::BIGINT FROM applied_batches",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    Ok(Positions {
        watermark_batch: watermark.batch,
        watermark_sequence: watermark.sequence,
        ledger_batches,
        ledger_records,
        inbox_records: inbox.records,
        inbox_bytes: inbox.bytes,
        assigned_batches: inbox.assigned.len() as i64,
        last_batch: inbox.last_batch,
        last_sequence: inbox.last_seq,
    })
}

/// Refuses an inbox and a ledger that cannot come from one installation at one moment.
pub(crate) fn check(
    inbox_pair: Option<&str>,
    ledger_pair: Option<&str>,
    inbox: &InboxPositions,
    ledger: &Connection,
) -> Result<(), Error> {
    if inbox_pair != ledger_pair {
        return Err(Error::Mismatch(format!(
            "inbox.sqlite belongs to pair {}, analytics.duckdb to pair {}",
            inbox_pair.unwrap_or("none"),
            ledger_pair.unwrap_or("none")
        )));
    }
    let watermark = analytics::watermark(ledger)?;
    check_positions(inbox, watermark, |id| {
        ledger
            .query_row(
                "SELECT first_seq, last_seq, records FROM applied_batches WHERE batch_id = ?",
                [id],
                |r| Ok([r.get(0)?, r.get(1)?, r.get(2)?]),
            )
            .optional()
    })
}

/// Every batch ever assigned is either in the ledger or still assigned in the inbox, and a
/// batch in both has the same membership. Batches apply in ID order, so the ledger's last
/// batch is its watermark.
fn check_positions(
    inbox: &InboxPositions,
    watermark: Watermark,
    entry: impl Fn(i64) -> duckdb::Result<Option<[i64; 3]>>,
) -> Result<(), Error> {
    let mismatch = |what: String| Err(Error::Mismatch(what));
    if inbox.last_batch < watermark.batch || inbox.last_seq < watermark.sequence {
        return mismatch(format!(
            "analytics applied batch {} (sequence {}) but the inbox only reached batch {} (sequence {}): the inbox is older",
            watermark.batch, watermark.sequence, inbox.last_batch, inbox.last_seq
        ));
    }
    for missing in watermark.batch + 1..=inbox.last_batch {
        if !inbox.assigned.iter().any(|[id, ..]| *id == missing) {
            return mismatch(format!(
                "batch {missing} left the inbox but analytics never applied it: analytics is older"
            ));
        }
    }
    for [id, first, last, records] in &inbox.assigned {
        if *id > watermark.batch {
            continue;
        }
        match entry(*id)? {
            Some(applied) if applied == [*first, *last, *records] => {}
            Some(applied) => {
                return mismatch(format!(
                    "batch {id} has inbox membership {:?} and ledger entry {applied:?}",
                    [first, last, records]
                ));
            }
            None => {
                return mismatch(format!(
                    "batch {id} is assigned in the inbox, older than applied batch {}, and missing from the ledger",
                    watermark.batch
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inbox(last_batch: i64, last_seq: i64, assigned: Vec<[i64; 4]>) -> InboxPositions {
        InboxPositions {
            last_batch,
            last_seq,
            assigned,
            ..InboxPositions::default()
        }
    }

    fn ledger(batch: i64, sequence: i64) -> Watermark {
        Watermark { batch, sequence }
    }

    fn entries(id: i64) -> duckdb::Result<Option<[i64; 3]>> {
        Ok((id <= 5).then_some([id * 10 - 9, id * 10, 10]))
    }

    #[test]
    fn a_pair_from_one_moment_passes() {
        // Batch 5 applied but not retired, batch 6 assigned, records pending after it.
        let state = inbox(6, 65, vec![[5, 41, 50, 10], [6, 51, 60, 10]]);
        assert!(check_positions(&state, ledger(5, 50), entries).is_ok());
        assert!(check_positions(&inbox(5, 50, vec![]), ledger(5, 50), entries).is_ok());
    }

    #[test]
    fn an_older_inbox_is_refused() {
        let error = check_positions(&inbox(4, 40, vec![]), ledger(5, 50), entries);
        assert!(matches!(error, Err(Error::Mismatch(_))));
    }

    #[test]
    fn an_older_ledger_is_refused() {
        // Batches 4 and 5 were retired after analytics applied them, but this ledger stops at 3.
        let error = check_positions(&inbox(5, 50, vec![]), ledger(3, 30), entries);
        assert!(matches!(error, Err(Error::Mismatch(_))));
    }

    #[test]
    fn different_membership_is_refused() {
        let error = check_positions(&inbox(5, 50, vec![[5, 41, 49, 9]]), ledger(5, 50), entries);
        assert!(matches!(error, Err(Error::Mismatch(_))));
    }
}
