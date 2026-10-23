//! The goals report (0003 Goals): completions of each goal over its denominator.
//!
//! The matching events of the range and the members of the sessions starting in it are
//! materialized once ([`super::matching`]) with a column per field the goals test. A session
//! goal counts over the members of sessions in range that match the filter; a person goal
//! over the consented profiles' matching events in range.
//!
//! Compared with a typical weekday (0015), every baseline window is counted the same way:
//! completions take the median per range day, the rate pools the baseline days, and a range
//! containing now projects its completions to the end of the range.

use holocron_protocol::exploration::{
    Coverage, GoalComparison, GoalCounting, GoalResult, GoalsResult, Metric, PopulationScope,
    Reason,
};

use holocron_storage::duckdb;

use super::matching::{self, Reads};
use super::{Input, Outcome};
use crate::explore::baseline::Windows;
use crate::explore::resolve;
use crate::explore::run;
use crate::explore::sql::Sql;
use crate::explore::validate::{CheckedGoals, Pred};

/// One goal over one span.
#[derive(Debug, Clone, Copy)]
struct Count {
    completions: i64,
    denominator: i64,
    coverage: Coverage,
}

pub fn run(
    input: &Input,
    report: &CheckedGoals,
    baseline: Option<&Windows>,
) -> Outcome<GoalsResult> {
    let current = count(input, report, input.read)?;
    let mut goals: Vec<GoalResult> = current
        .iter()
        .map(|c| GoalResult {
            completions: c.completions,
            denominator: c.denominator,
            rate: matching::rate(c.completions, c.denominator),
            coverage: c.coverage,
            compare: None,
            projection: None,
        })
        .collect();
    let Some(windows) = baseline else {
        return Ok(Ok(GoalsResult { goals }));
    };

    let parts = windows
        .windows
        .iter()
        .map(|w| count(input, report, (w.span.start_ms, w.span.end_ms)))
        .collect::<duckdb::Result<Vec<_>>>()?;
    // Today so far: the range itself when it starts today, as nothing is stored after now.
    let today = match windows.today {
        Some(0) => Some(current),
        Some(t) => {
            let midnight = windows.days[t].date.and_hms_opt(0, 0, 0).expect("midnight");
            let start = resolve::local_start(input.query.timezone, midnight);
            Some(count(input, report, (start, input.read.1))?)
        }
        None => None,
    };
    let remaining = windows.remaining();
    for (g, goal) in goals.iter_mut().enumerate() {
        let completions = |p: usize| parts[p][g].completions;
        let (pooled, denominator) = windows.counted().flatten().fold((0, 0), |(c, d), &p| {
            (c + parts[p][g].completions, d + parts[p][g].denominator)
        });
        let (typical, rate) = match windows.median_sum(completions) {
            Some(v) => (Metric::Count { v }, matching::rate(pooled, denominator)),
            None => {
                let insufficient = Metric::Unavailable {
                    reason: Reason::InsufficientHistory,
                };
                (insufficient.clone(), insufficient)
            }
        };
        let measured = Metric::Count {
            v: goal.completions,
        };
        goal.compare = Some(GoalComparison {
            completions: run::compare(&measured, typical),
            rate: run::compare(&goal.rate, rate),
        });
        goal.projection = today.as_ref().map(|today| {
            let observed = (goal.completions, today[g].completions);
            windows.pace(&remaining, observed, completions, completions)
        });
    }
    Ok(Ok(GoalsResult { goals }))
}

/// Every goal's completions, denominator and coverage over `[start, end)`.
fn count(input: &Input, report: &CheckedGoals, span: (i64, i64)) -> duckdb::Result<Vec<Count>> {
    let preds: Vec<&Pred> = report.goals.iter().collect();
    let matched = matching::materialize(
        input,
        "go",
        Reads {
            trees: &preds,
            sessions: true,
            ..Reads::default()
        },
        span,
    )?;
    let conn = input.snapshot.connection();
    let (identified, linked, eligible): (i64, i64, i64) = conn.query_row(
        "SELECT count(*) FILTER (WHERE daily_id IS NOT NULL OR profile_id IS NOT NULL),
                count(profile_id), count(*) FROM go_ev",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    let mut counts = Vec::with_capacity(preds.len());
    for (definition, pred) in report.definition.goals.iter().zip(&preds) {
        let mut sql = Sql::default();
        let (table, unit, with_identity) = match definition.scope {
            PopulationScope::Session => ("go_se", "session_id", identified),
            PopulationScope::Person => ("go_ev", "profile_id", linked),
        };
        sql.push(&format!("SELECT count(DISTINCT {unit}), "));
        match definition.counting {
            GoalCounting::Once => sql.push(&format!("count(DISTINCT {unit}) FILTER (WHERE ")),
            GoalCounting::Every => sql.push(&format!("count({unit}) FILTER (WHERE ")),
        };
        matched.test(pred, &mut sql);
        sql.push(&format!(") FROM {table}"));
        let (denominator, completions) = matching::rows(input, &sql, |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?))
        })?
        .pop()
        .unwrap_or_default();
        counts.push(Count {
            completions,
            denominator,
            coverage: Coverage {
                with_identity,
                eligible_events: eligible,
            },
        });
    }
    matching::drop(input, &["go_ev".to_owned(), "go_se".to_owned()])?;
    Ok(counts)
}
