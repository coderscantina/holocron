//! The funnel report (0003 Funnels, 0004 Report definitions).
//!
//! Candidate events, those matching the filter and at least one step, are materialized once
//! over `[start, end + window)` with a bit per step they match. An identity enters at its
//! first step-1 event in range. Each next step is an ASOF join: the identity's first event of
//! that step strictly after the previous step in `(event_time, ingestion_sequence)` order,
//! kept while it lies inside the half-open window from step 1. ASOF joins merge per identity
//! in order, so no identity's events are joined with each other and memory stays bounded by
//! the candidates.
//!
//! Each step but the last also gets its leak: the identities in it but not in the next, by the
//! next page of the session their step event belongs to.

use holocron_protocol::exploration::{
    FunnelLeak, FunnelNext, FunnelResult, FunnelStep, IdentityScope,
};

use holocron_storage::duckdb;

use super::matching::{self, Reads, key};
use super::{Input, Outcome};
use crate::explore::sql::Sql;
use crate::explore::validate::{CheckedFunnel, Pred};

pub fn run(input: &Input, report: &CheckedFunnel) -> Outcome<FunnelResult> {
    let definition = &report.definition;
    let person = definition.scope == IdentityScope::Person;
    let window = i64::from(definition.window_seconds) * 1000;
    let (start, end) = input.read;
    let steps: Vec<&Pred> = report.steps.iter().collect();
    let matched = matching::materialize(
        input,
        "fn",
        Reads {
            trees: &steps,
            narrow: true,
            ..Reads::default()
        },
        (start, end + window),
    )?;

    // Daily scope follows the daily pseudonym, person scope the consented profile.
    let id = if person { "profile_id" } else { "daily_id" };
    let mut sql = Sql::default();
    sql.push(&format!(
        "CREATE OR REPLACE TEMP TABLE fn_c AS SELECT {id} AS id, t, {} AS k, ",
        key("t", "seq")
    ));
    matched.mask(&steps, &mut sql);
    sql.push(&format!(" AS mask FROM fn_ev WHERE {id} IS NOT NULL"));
    matching::execute(input, &sql)?;

    let mut entry = Sql::default();
    entry
        .push(
            "CREATE OR REPLACE TEMP TABLE fn_s0 AS SELECT id, min(k) AS k, arg_min(t, k) AS t1
             FROM fn_c WHERE mask & 1 <> 0 AND t >= ",
        )
        .bind(start)
        .push(" AND t < ")
        .bind(end)
        .push(" GROUP BY id");
    matching::execute(input, &entry)?;
    for step in 1..steps.len() {
        let mut next = Sql::default();
        next.push(&format!(
            "CREATE OR REPLACE TEMP TABLE fn_s{step} AS SELECT p.id, p.t1, e.k
             FROM fn_s{} p ASOF JOIN (SELECT id, k, t FROM fn_c WHERE mask & {} <> 0) e
               ON p.id = e.id AND p.k < e.k
             WHERE e.t < p.t1 + ",
            step - 1,
            1_u32 << step
        ))
        .bind(window);
        matching::execute(input, &next)?;
    }
    let mut reached = Vec::with_capacity(steps.len());
    for step in 0..steps.len() {
        let n: i64 = input.snapshot.connection().query_row(
            &format!("SELECT count(*) FROM fn_s{step}"),
            [],
            |r| r.get(0),
        )?;
        reached.push(n);
    }
    let leaks = (0..steps.len().saturating_sub(1))
        .map(|step| {
            leak(
                input,
                step,
                reached[step] - reached[step + 1],
                (start, end + window),
            )
        })
        .collect::<duckdb::Result<Vec<_>>>()?;
    let mut tables: Vec<String> = (0..steps.len()).map(|s| format!("fn_s{s}")).collect();
    tables.extend(["fn_c".to_owned(), "fn_ev".to_owned()]);
    matching::drop(input, &tables)?;

    let coverage = matching::coverage(input, person)?;
    let first = reached.first().copied().unwrap_or(0);
    let last = reached.last().copied().unwrap_or(0);
    Ok(Ok(FunnelResult {
        steps: reached
            .iter()
            .enumerate()
            .map(|(i, n)| FunnelStep {
                reached: *n,
                drop: reached.get(i + 1).map(|next| n - next),
            })
            .collect(),
        conversion: matching::rate(last, first),
        daily_limit: !person,
        coverage,
        leaks,
    }))
}

/// Next pages a leak names before folding the rest into `other`.
const LEAK_PAGES: usize = 3;
/// Sessions a leak lists.
const LEAK_SESSIONS: usize = 20;

/// The `leaving` identities of `fn_s<step>` missing from the next step: whether their session
/// showed no further page, went back to one it had shown, or moved on to another, within `span`.
fn leak(
    input: &Input,
    step: usize,
    leaving: i64,
    (start, end): (i64, i64),
) -> duckdb::Result<FunnelLeak> {
    let site = input.ctx.site;
    let mut sql = Sql::default();
    sql.push(&format!(
        "CREATE OR REPLACE TEMP TABLE fn_m AS
         WITH placed AS (
             SELECT p.id, p.k, m.session_id
             FROM (SELECT * FROM fn_s{step} ANTI JOIN fn_s{} n USING (id)) p
             JOIN session_members m ON m.seq = (p.k % 9223372036854775808)::BIGINT AND m.site = ",
        step + 1
    ))
    .bind(site)
    .push(
        "),
         pages AS (
             SELECT m.session_id, e.path, ",
    )
    .push(&key("epoch_ms(e.event_time)", "e.seq"))
    .push(
        " AS k FROM session_members m JOIN events e ON e.seq = m.seq AND e.site = m.site
             WHERE m.site = ",
    )
    .bind(site)
    .push(" AND e.kind = 1 AND e.event_time >= epoch_ms(")
    .bind(start)
    .push(") AND e.event_time < epoch_ms(")
    .bind(end)
    .push(
        ") AND m.session_id IN (SELECT session_id FROM placed)
         )
         SELECT p.id, p.session_id, p.k,
                arg_min(g.path, g.k) FILTER (WHERE g.k > p.k) AS next,
                list(g.path) FILTER (WHERE g.k <= p.k) AS seen
         FROM placed p LEFT JOIN pages g ON g.session_id = p.session_id
         GROUP BY p.id, p.session_id, p.k",
    );
    matching::execute(input, &sql)?;

    let conn = input.snapshot.connection();
    let mut statement = conn.prepare(
        "SELECT CASE WHEN next IS NULL THEN 0 WHEN list_contains(seen, next) THEN 1 ELSE 2 END AS move,
                CASE WHEN move = 2 THEN next END, count(*)
         FROM fn_m GROUP BY ALL",
    )?;
    let groups = statement
        .query_map([], |r| {
            Ok((
                r.get::<_, u8>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, i64>(2)?,
            ))
        })?
        .collect::<duckdb::Result<Vec<_>>>()?;
    let mut statement = conn.prepare(&format!(
        "SELECT session_id FROM fn_m GROUP BY session_id ORDER BY max(k) DESC LIMIT {LEAK_SESSIONS}"
    ))?;
    let sessions = statement
        .query_map([], |r| r.get::<_, i64>(0))?
        .collect::<duckdb::Result<Vec<_>>>()?;
    matching::drop(input, &["fn_m".to_owned()])?;

    let (mut exited, mut back) = (0, 0);
    let mut next = Vec::new();
    for (movement, page, identities) in groups {
        match (movement, page) {
            (0, _) => exited += identities,
            (1, _) => back += identities,
            (_, Some(path)) => next.push(FunnelNext { path, identities }),
            _ => {}
        }
    }
    next.sort_by(|a, b| {
        b.identities
            .cmp(&a.identities)
            .then_with(|| a.path.cmp(&b.path))
    });
    next.truncate(LEAK_PAGES);
    let named = exited + back + next.iter().map(|page| page.identities).sum::<i64>();
    Ok(FunnelLeak {
        exited,
        back,
        next,
        other: leaving - named,
        sessions,
    })
}
