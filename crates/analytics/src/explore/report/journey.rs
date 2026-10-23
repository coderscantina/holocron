//! The journey report (0003 Journeys, 0004 Report definitions).
//!
//! Entering sessions start in range, have at least one pageview (path journeys) or custom
//! event (event journeys), belong to a consented profile for person scope, and with a filter
//! have at least one matching event. Each session's values are read in `(event_time,
//! ingestion_sequence)` order, consecutive repeats are dropped over the whole session, and the
//! first `length` values form its sequence. Sequences are grouped and ranked in the engine.

use holocron_protocol::exploration::{
    JourneyReport, JourneyResult, JourneySequence, JourneyValue, PopulationScope,
};
use holocron_protocol::limits::REPORT_JOURNEY_SEQUENCES_MAX;

use super::matching::{self, Reads};
use super::{Input, Outcome};
use crate::explore::sql::Sql;

pub fn run(input: &Input, report: &JourneyReport) -> Outcome<JourneyResult> {
    let person = report.scope == PopulationScope::Person;
    let (start, end) = input.read;
    let site = input.ctx.site;
    let (kind, column, count) = match report.value {
        JourneyValue::Path => (1, "path", "pageviews"),
        JourneyValue::EventName => (2, "event_name", "custom_events"),
    };
    let mut tables = vec!["jy_entry".to_owned(), "jy_steps".to_owned()];
    if input.query.filter.is_some() {
        matching::materialize(
            input,
            "jy",
            Reads {
                sessions: true,
                ..Reads::default()
            },
            input.read,
        )?;
        tables.extend(["jy_ev".to_owned(), "jy_se".to_owned()]);
    }

    let mut entry = Sql::default();
    entry
        .push(
            "CREATE OR REPLACE TEMP TABLE jy_entry AS SELECT session_id, last_time FROM sessions
             WHERE site = ",
        )
        .bind(site)
        .push(" AND start_time >= epoch_ms(")
        .bind(start)
        .push(") AND start_time < epoch_ms(")
        .bind(end)
        .push(&format!(") AND {count} > 0"));
    if person {
        // Population 2 is a consented profile's sessions (0001).
        entry.push(" AND population = 2");
    }
    if input.query.filter.is_some() {
        entry.push(" AND session_id IN (SELECT session_id FROM jy_se)");
    }
    matching::execute(input, &entry)?;

    // Members of the entering sessions lie between the range start and their last event.
    let last: Option<i64> = input.snapshot.connection().query_row(
        "SELECT epoch_ms(max(last_time)) FROM jy_entry",
        [],
        |r| r.get(0),
    )?;
    let hi = last.unwrap_or(start);
    let mut steps = Sql::default();
    steps
        .push(&format!(
            "CREATE OR REPLACE TEMP TABLE jy_steps AS
             WITH v AS (
                SELECT m.session_id, coalesce(e.{column}, '') AS v, m.event_time AS t, m.seq
                FROM session_members m
                JOIN events e ON e.seq = m.seq AND e.site = "
        ))
        .bind(site)
        .push(&format!(" AND e.kind = {kind} AND e.event_time >= epoch_ms("))
        .bind(start)
        .push(") AND e.event_time <= epoch_ms(")
        .bind(hi)
        .push(") WHERE m.site = ")
        .bind(site)
        .push(" AND m.role = 1 AND m.event_time >= epoch_ms(")
        .bind(start)
        .push(") AND m.event_time <= epoch_ms(")
        .bind(hi)
        .push(&format!(
            ") AND m.session_id IN (SELECT session_id FROM jy_entry)
             ), compressed AS (
                SELECT session_id, v, t, seq FROM (
                    SELECT *, lag(v) OVER (PARTITION BY session_id ORDER BY t, seq) AS previous FROM v
                ) WHERE previous IS DISTINCT FROM v
             ), numbered AS (
                SELECT session_id, v, row_number() OVER (PARTITION BY session_id ORDER BY t, seq) AS n
                FROM compressed
             )
             SELECT session_id, list(v ORDER BY n) AS steps FROM numbered WHERE n <= {}
             GROUP BY session_id",
            report.length
        ));
    matching::execute(input, &steps)?;

    let conn = input.snapshot.connection();
    let (entry_sessions, distinct_sequences): (i64, i64) = conn.query_row(
        "SELECT (SELECT count(*) FROM jy_entry), (SELECT count(DISTINCT steps) FROM jy_steps)",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    // Lists compare element by element and a prefix sorts first; strings compare by bytes.
    let mut top = Sql::default();
    top.push(&format!(
        "SELECT steps::VARCHAR[], count(*) AS n FROM jy_steps GROUP BY steps
         ORDER BY n DESC, steps LIMIT {REPORT_JOURNEY_SEQUENCES_MAX}"
    ));
    let sequences = matching::rows(input, &top, |r| {
        let steps: holocron_storage::duckdb::types::Value = r.get(0)?;
        Ok(JourneySequence {
            steps: strings(steps),
            sessions: r.get(1)?,
        })
    })?;
    matching::drop(input, &tables)?;

    Ok(Ok(JourneyResult {
        sequences,
        distinct_sequences,
        entry_sessions,
        coverage: matching::coverage(input, person)?,
    }))
}

fn strings(list: holocron_storage::duckdb::types::Value) -> Vec<String> {
    use holocron_storage::duckdb::types::Value;
    match list {
        Value::List(items) => items
            .into_iter()
            .map(|item| match item {
                Value::Text(text) => text,
                other => panic!("unexpected journey value {other:?}"),
            })
            .collect(),
        other => panic!("unexpected journey sequence {other:?}"),
    }
}
