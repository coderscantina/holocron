//! Site clustering of closed months (0008 Analytical layout). Live ingestion appends rows in
//! arrival order, so every row group holds every site; a one-site annual query then reads the
//! whole year. Compaction rewrites a closed month of `events`, `session_properties` and the
//! property partitions in (site, event time) order.
//!
//! The work is incremental. A month is cut into slices of about `step_events` events in
//! (site, event time) order, and each step moves one slice to the end of each table in one
//! transaction, together with the month's cursor in `compaction_months`. A step that does not
//! commit leaves the old rows and the old cursor, so an interrupted month resumes from its
//! last committed slice. Slices of one month land one after the other, so the month ends up
//! in site order no matter how many steps it took.
//!
//! A clustered month remembers the last sequence applied when it started. It is clustered
//! again when rows admitted later (late events) reach a twentieth of it. Freed blocks are
//! reused by later writes; the file does not shrink.

use duckdb::{Connection, OptionalExt, params};

use crate::{Error, PROPERTY_PARTS, Storage, failpoint};

const STATE: &str = "
CREATE TABLE IF NOT EXISTS compaction_months (
    -- first instant of the UTC month, microseconds since 1970
    month BIGINT PRIMARY KEY,
    -- last sequence applied when the month's pass started
    through_seq BIGINT NOT NULL,
    done BOOLEAN NOT NULL,
    -- first key of the next slice; NULL before the first one
    next_site UHUGEINT,
    next_time BIGINT
);";

/// One committed compaction step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Compacted {
    /// First instant of the UTC month the step worked on, microseconds since 1970.
    pub month: i64,
    /// Events the step moved.
    pub events: u64,
    /// Whether the month is clustered now.
    pub month_done: bool,
}

/// A position in (site, event time) order; the time in microseconds.
type Key = (u128, i64);

impl Storage {
    /// Moves the next slice of about `step_events` events of the oldest month that ended
    /// before the month of `before_ms` and needs clustering. `None` when every such month is
    /// clustered. Holds the analytical writer for the one step, like any derived write.
    pub async fn compact(
        &self,
        before_ms: i64,
        step_events: u64,
    ) -> Result<Option<Compacted>, Error> {
        let writer = self.writer.clone();
        tokio::task::spawn_blocking(move || {
            let mut conn = writer.lock().expect("analytical writer");
            // Slices are appended in order; the engine runs with this off for ingestion.
            conn.execute_batch("SET preserve_insertion_order = true")?;
            let result = (|| {
                let tx = conn.transaction()?;
                let step = step(&tx, before_ms, step_events)?;
                failpoint::hit("write.before_commit");
                tx.commit()?;
                Ok::<_, Error>(step)
            })();
            conn.execute_batch("SET preserve_insertion_order = false")?;
            result
        })
        .await
        .expect("compaction task")
    }
}

fn step(conn: &Connection, before_ms: i64, step_events: u64) -> duckdb::Result<Option<Compacted>> {
    conn.execute_batch(STATE)?;
    let open: Option<(i64, Option<u128>, Option<i64>)> = conn
        .query_row(
            "SELECT month, next_site, next_time FROM compaction_months WHERE NOT done
             ORDER BY month LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    let (month, from) = match open {
        Some((month, site, time)) => (month, site.zip(time)),
        None => {
            let Some(month) = due(conn, before_ms)? else {
                // Months retention removed entirely.
                conn.execute_batch(
                    "DELETE FROM compaction_months WHERE month NOT IN
                     (SELECT DISTINCT epoch_us(date_trunc('month', event_time)) FROM events)",
                )?;
                return Ok(None);
            };
            conn.execute(
                "INSERT OR REPLACE INTO compaction_months
                 SELECT ?, coalesce(max(last_seq), 0), false, NULL, NULL FROM applied_batches",
                [month],
            )?;
            (month, None)
        }
    };
    let end: i64 = conn.query_row(
        "SELECT epoch_us(make_timestamp(?) + INTERVAL 1 MONTH)",
        [month],
        |r| r.get(0),
    )?;
    let to = slice_end(conn, (month, end), from, step_events)?;
    let slice = slice(month, end, from, to);
    let mut events = 0;
    let tables = ["events".to_owned(), "session_properties".to_owned()]
        .into_iter()
        .chain((0..PROPERTY_PARTS).map(|part| format!("event_properties_{part}")));
    for table in tables {
        conn.execute_batch(&format!(
            "CREATE OR REPLACE TEMP TABLE compaction_slice AS SELECT * FROM {table} WHERE {slice}"
        ))?;
        let moved = conn.execute(&format!("DELETE FROM {table} WHERE {slice}"), [])?;
        if table == "events" {
            events = moved as u64;
        }
        conn.execute_batch(&format!(
            "INSERT INTO {table} SELECT * FROM compaction_slice ORDER BY site, event_time, seq;
             DROP TABLE compaction_slice;"
        ))?;
    }
    conn.execute(
        "UPDATE compaction_months SET done = ?, next_site = ?, next_time = ? WHERE month = ?",
        params![to.is_none(), to.map(|k| k.0), to.map(|k| k.1), month],
    )?;
    Ok(Some(Compacted {
        month,
        events,
        month_done: to.is_none(),
    }))
}

/// The oldest month before the month of `before_ms` that was never clustered, or whose rows
/// admitted after its last pass reach a twentieth of it.
fn due(conn: &Connection, before_ms: i64) -> duckdb::Result<Option<i64>> {
    conn.query_row(
        "SELECT e.month FROM (
             SELECT epoch_us(date_trunc('month', event_time)) AS month, seq FROM events
             WHERE event_time < date_trunc('month', make_timestamp(? * 1000))
         ) e LEFT JOIN compaction_months c ON c.month = e.month
         GROUP BY e.month, c.through_seq
         HAVING c.through_seq IS NULL OR count(*) FILTER (WHERE e.seq > c.through_seq) * 20 >= count(*)
         ORDER BY e.month LIMIT 1",
        [before_ms],
        |r| r.get(0),
    )
    .optional()
}

/// The first key after a slice of about `step_events` events from `from`, `None` when the
/// rest of the month fits. Rows with one key always share a slice.
fn slice_end(
    conn: &Connection,
    (month, end): (i64, i64),
    from: Option<Key>,
    step_events: u64,
) -> duckdb::Result<Option<Key>> {
    let key = |sql: String| {
        conn.query_row(&sql, [], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()
    };
    let rest = slice(month, end, from, None);
    let Some(to) = key(format!(
        "SELECT site, epoch_us(event_time) FROM events WHERE {rest}
         ORDER BY site, event_time LIMIT 1 OFFSET {step_events}"
    ))?
    else {
        return Ok(None);
    };
    let before_to: i64 = conn.query_row(
        &format!(
            "SELECT count(*) FROM events WHERE {}",
            slice(month, end, from, Some(to))
        ),
        [],
        |r| r.get(0),
    )?;
    if before_to > 0 {
        return Ok(Some(to));
    }
    // More than a slice of rows share the key the slice starts with: they all go in this one.
    let (site, time) = to;
    key(format!(
        "SELECT site, epoch_us(event_time) FROM events WHERE {}
         AND (site > '{site}'::UHUGEINT OR (site = '{site}'::UHUGEINT AND event_time > make_timestamp({time})))
         ORDER BY site, event_time LIMIT 1",
        slice(month, end, None, None)
    ))
}

/// The rows of the month from `from` (inclusive) to `to` (exclusive). Every value is an
/// integer this module computed, so it is written into the statement.
fn slice(month: i64, end: i64, from: Option<Key>, to: Option<Key>) -> String {
    let mut sql =
        format!("event_time >= make_timestamp({month}) AND event_time < make_timestamp({end})");
    if let Some((site, time)) = from {
        sql += &format!(
            " AND (site > '{site}'::UHUGEINT OR (site = '{site}'::UHUGEINT AND event_time >= make_timestamp({time})))"
        );
    }
    if let Some((site, time)) = to {
        sql += &format!(
            " AND (site < '{site}'::UHUGEINT OR (site = '{site}'::UHUGEINT AND event_time < make_timestamp({time})))"
        );
    }
    sql
}
