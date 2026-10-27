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
//! Each step table keeps the previous step's event time, so durations between steps are one
//! aggregate per table. A grouped funnel (version 2) ranks the values of the entering step-1
//! events once and carries each identity's rank `g` through the joins.
//!
//! Each step but the last also gets its leak: the identities in it but not in the next, by the
//! next page of the session their step event belongs to.

use holocron_protocol::exploration::{
    Cell, FunnelGroup, FunnelLeak, FunnelNext, FunnelResult, FunnelStep, IdentityScope, Metric,
    Reason, StepDuration,
};
use holocron_protocol::limits::REPORT_FUNNEL_GROUPS_MAX;
use holocron_storage::duckdb::{self, types::Value};

use super::matching::{self, Reads, key};
use super::{Input, Outcome};
use crate::explore::run::{cell_order, decode_cell};
use crate::explore::sql::{Sql, columns};
use crate::explore::validate::{CheckedFunnel, Pred};

const HOUR_MS: i64 = 3_600_000;
const DAY_MS: i64 = 24 * HOUR_MS;
/// Lower bounds of the duration buckets after the first: 1 h, 1 d, 7 d, 30 d.
const BUCKET_STARTS_MS: [i64; 4] = [HOUR_MS, DAY_MS, 7 * DAY_MS, 30 * DAY_MS];

pub fn run(input: &Input, report: &CheckedFunnel) -> Outcome<FunnelResult> {
    let definition = &report.definition;
    let person = definition.scope == IdentityScope::Person;
    let window = i64::from(definition.window_seconds) * 1000;
    let (start, end) = input.read;
    let steps: Vec<&Pred> = report.steps.iter().collect();
    let grouping: Vec<_> = report.group_by().cloned().into_iter().collect();
    let matched = matching::materialize(
        input,
        "fn",
        Reads {
            trees: &steps,
            fields: &grouping,
            narrow: true,
            ..Reads::default()
        },
        (start, end + window),
    )?;
    // The grouping field's columns keep their `fn_ev` names through `fn_c` and the entry.
    let group = grouping
        .first()
        .and_then(|field| Some((matched.index(field)?, field)));
    let group_columns = group.map_or_else(Vec::new, |(i, field)| columns(i, field));
    let carried: String = group_columns.iter().map(|c| format!(", {c}")).collect();

    // Daily scope follows the daily pseudonym, person scope the consented profile.
    let id = if person { "profile_id" } else { "daily_id" };
    let mut sql = Sql::default();
    sql.push(&format!(
        "CREATE OR REPLACE TEMP TABLE fn_c AS SELECT {id} AS id, t, {} AS k, ",
        key("t", "seq")
    ));
    matched.mask(&steps, &mut sql);
    sql.push(&format!(
        " AS mask{carried} FROM fn_ev WHERE {id} IS NOT NULL"
    ));
    matching::execute(input, &sql)?;

    // Grouped, the entry goes to `fn_e` and `fn_s0` replaces its columns by the group's rank.
    let entry_table = if group.is_some() { "fn_e" } else { "fn_s0" };
    let mut entry = Sql::default();
    entry
        .push(&format!(
            "CREATE OR REPLACE TEMP TABLE {entry_table} AS SELECT DISTINCT ON (id) id, k, t AS t1, t{carried}
             FROM fn_c WHERE mask & 1 <> 0 AND t >= "
        ))
        .bind(start)
        .push(" AND t < ")
        .bind(end)
        .push(" ORDER BY id, k");
    matching::execute(input, &entry)?;
    let (values, groups_truncated) = match group {
        Some((index, field)) => {
            ranked_groups(input, &group_columns, &cell_order(index, field, "ASC"))?
        }
        None => (Vec::new(), false),
    };
    let rank = if group.is_some() { ", p.g" } else { "" };
    for step in 1..steps.len() {
        let mut next = Sql::default();
        next.push(&format!(
            "CREATE OR REPLACE TEMP TABLE fn_s{step} AS SELECT p.id, p.t1{rank}, e.k, p.t AS tp, e.t AS t
             FROM fn_s{} p ASOF JOIN (SELECT id, k, t FROM fn_c WHERE mask & {} <> 0) e
               ON p.id = e.id AND p.k < e.k
             WHERE e.t < p.t1 + ",
            step - 1,
            1_u32 << step
        ))
        .bind(window);
        matching::execute(input, &next)?;
    }

    let overall = Summaries::read(input, steps.len(), false)?;
    let reached: Vec<i64> = (0..steps.len()).map(|s| overall.reached(s, 0)).collect();
    let groups = match group {
        Some(_) => {
            let by_group = Summaries::read(input, steps.len(), true)?;
            values
                .into_iter()
                .enumerate()
                .map(|(g, value)| {
                    let g = g as i64;
                    let reached: Vec<i64> =
                        (0..steps.len()).map(|s| by_group.reached(s, g)).collect();
                    FunnelGroup {
                        value,
                        conversion: conversion(&reached),
                        steps: funnel_steps(&reached),
                        durations: by_group.durations(g),
                    }
                })
                .collect()
        }
        None => Vec::new(),
    };
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
    tables.extend(["fn_e", "fn_g", "fn_c", "fn_ev"].map(str::to_owned));
    matching::drop(input, &tables)?;

    let coverage = matching::coverage(input, person)?;
    Ok(Ok(FunnelResult {
        steps: funnel_steps(&reached),
        conversion: conversion(&reached),
        daily_limit: !person,
        coverage,
        leaks,
        durations: overall.durations(0),
        groups,
        groups_truncated,
    }))
}

fn funnel_steps(reached: &[i64]) -> Vec<FunnelStep> {
    reached
        .iter()
        .enumerate()
        .map(|(i, n)| FunnelStep {
            reached: *n,
            drop: reached.get(i + 1).map(|next| n - next),
        })
        .collect()
}

fn conversion(reached: &[i64]) -> Metric {
    let first = reached.first().copied().unwrap_or(0);
    let last = reached.last().copied().unwrap_or(0);
    matching::rate(last, first)
}

/// Ranks the values of the entering step-1 events in `fn_e` into `fn_g`: by identities
/// descending, then by value (0004 cell order), and writes `fn_s0` with each entry's rank
/// `g`. Returns the top values and whether more groups entered.
fn ranked_groups(
    input: &Input,
    group_columns: &[String],
    order: &[String],
) -> duckdb::Result<(Vec<Cell>, bool)> {
    let list = group_columns.join(", ");
    let mut ranked = Sql::default();
    ranked.push(&format!(
        "CREATE OR REPLACE TEMP TABLE fn_g AS
         SELECT {list}, row_number() OVER (ORDER BY count(*) DESC, {}) - 1 AS g
         FROM fn_e GROUP BY {list}",
        order.join(", ")
    ));
    matching::execute(input, &ranked)?;
    let same: Vec<String> = group_columns
        .iter()
        .map(|c| format!("e.{c} IS NOT DISTINCT FROM g.{c}"))
        .collect();
    let mut entry = Sql::default();
    entry.push(&format!(
        "CREATE OR REPLACE TEMP TABLE fn_s0 AS SELECT e.id, e.k, e.t1, e.t, g.g
         FROM fn_e e JOIN fn_g g ON {}",
        same.join(" AND ")
    ));
    matching::execute(input, &entry)?;

    let mut top = Sql::default();
    top.push(&format!("SELECT {list} FROM fn_g WHERE g < "))
        .bind(REPORT_FUNNEL_GROUPS_MAX as i64)
        .push(" ORDER BY g");
    let values = matching::rows(input, &top, |row| {
        let raw = (0..group_columns.len())
            .map(|c| row.get::<_, Value>(c))
            .collect::<duckdb::Result<Vec<_>>>()?;
        Ok(decode_cell(&raw))
    })?;
    let count: i64 =
        input
            .snapshot
            .connection()
            .query_row("SELECT count(*) FROM fn_g", [], |r| r.get(0))?;
    Ok((values, count > REPORT_FUNNEL_GROUPS_MAX as i64))
}

/// One step table's identities, overall or of one group: how many reached it and how long
/// they took from an earlier step event.
struct Summary {
    reached: i64,
    quantiles: [Option<i64>; 3],
    buckets: [i64; 5],
}

/// Per step, then for the step-1-to-last span: summaries by group rank (`0` when ungrouped).
struct Summaries {
    steps: Vec<Vec<(i64, Summary)>>,
    total: Vec<(i64, Summary)>,
}

impl Summaries {
    /// Reads every `fn_s<n>`, by the rank `g` of the top groups when `grouped`.
    fn read(input: &Input, steps: usize, grouped: bool) -> duckdb::Result<Self> {
        let mut summaries = Vec::with_capacity(steps);
        for step in 0..steps {
            // Step 1 has no earlier event; its durations are zero and not reported.
            let from = if step == 0 { "t" } else { "tp" };
            summaries.push(summarize(input, &format!("fn_s{step}"), from, grouped)?);
        }
        let total = summarize(input, &format!("fn_s{}", steps - 1), "t1", grouped)?;
        Ok(Self {
            steps: summaries,
            total,
        })
    }

    fn find(rows: &[(i64, Summary)], g: i64) -> Option<&Summary> {
        rows.iter().find(|(rank, _)| *rank == g).map(|(_, s)| s)
    }

    fn reached(&self, step: usize, g: i64) -> i64 {
        Self::find(&self.steps[step], g).map_or(0, |s| s.reached)
    }

    /// One entry per step but the first, then the first step to the last.
    fn durations(&self, g: i64) -> Vec<StepDuration> {
        let last = self.steps.len() - 1;
        (1..=last)
            .map(|step| (step - 1, step, &self.steps[step]))
            .chain(std::iter::once((0, last, &self.total)))
            .map(|(from, to, rows)| {
                let summary = Self::find(rows, g);
                let quantile = |i: usize| match summary.and_then(|s| s.quantiles[i]) {
                    Some(v) => Metric::Millis { v },
                    None => Metric::Unavailable {
                        reason: Reason::NoValues,
                    },
                };
                StepDuration {
                    from_step: from as u8,
                    to_step: to as u8,
                    p25: quantile(0),
                    median: quantile(1),
                    p75: quantile(2),
                    buckets: summary.map_or([0; 5], |s| s.buckets),
                }
            })
            .collect()
    }
}

/// Reached, quantiles and bucket counts of `t − <from>` over `table`, by group or overall.
fn summarize(
    input: &Input,
    table: &str,
    from: &str,
    grouped: bool,
) -> duckdb::Result<Vec<(i64, Summary)>> {
    let [hour, day, week, month] = BUCKET_STARTS_MS;
    let buckets = format!(
        ", count(*) FILTER (WHERE d < {hour}), count(*) FILTER (WHERE d >= {hour} AND d < {day}),
         count(*) FILTER (WHERE d >= {day} AND d < {week}), count(*) FILTER (WHERE d >= {week} AND d < {month}),
         count(*) FILTER (WHERE d >= {month})"
    );
    let mut sql = Sql::default();
    sql.push(&format!(
        "SELECT {}, count(*), quantile_disc(d, 0.25), quantile_disc(d, 0.5), quantile_disc(d, 0.75){buckets}
         FROM (SELECT *, t - {from} AS d FROM {table})",
        if grouped { "g" } else { "0::BIGINT" }
    ));
    if grouped {
        sql.push(" WHERE g < ")
            .bind(REPORT_FUNNEL_GROUPS_MAX as i64)
            .push(" GROUP BY g");
    }
    matching::rows(input, &sql, |r| {
        let mut buckets = [0; 5];
        for (i, bucket) in buckets.iter_mut().enumerate() {
            *bucket = r.get(5 + i)?;
        }
        Ok((
            r.get(0)?,
            Summary {
                reached: r.get(1)?,
                quantiles: [r.get(2)?, r.get(3)?, r.get(4)?],
                buckets,
            },
        ))
    })
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
