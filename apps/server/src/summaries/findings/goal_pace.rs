//! Goal pace: a goal's completions today so far against its typical weekday up to the same
//! local time, from one `goals` report over every active goal (0015).

use anyhow::bail;
use holocron_protocol::exploration::{
    Compare, ExplorationState, GoalDefinition, GoalsReport, Group, Node, REPORT_VERSION,
    ReportDefinition, ReportResult,
};
use holocron_protocol::limits::{FINDING_GOAL_CHANGE_MIN, FINDING_GOAL_CHANGE_PERCENT};
use holocron_protocol::operations::summaries::{
    Finding, FindingRule, FindingValues, GoalPaceValues,
};
use holocron_storage::Snapshot;

use super::{Rule, Site, count, day, round4};

pub struct GoalPace;

impl Rule for GoalPace {
    fn rule(&self) -> FindingRule {
        FindingRule::GoalPace
    }

    fn version(&self) -> u32 {
        1
    }

    fn hourly(&self) -> bool {
        false
    }

    fn findings(&self, site: &Site, snapshot: &Snapshot) -> anyhow::Result<Vec<Finding>> {
        if site.goals.is_empty() {
            return Ok(Vec::new());
        }
        // Saved goals number their conditions on their own; one report needs them unique.
        let goals: Vec<GoalDefinition> = site
            .goals
            .iter()
            .enumerate()
            .map(|(i, goal)| {
                let mut goal = goal.clone();
                renumber(&mut goal.predicate, &format!("g{i}c"), &mut 0);
                goal
            })
            .collect();
        let result = site.run(snapshot, &state(site, goals.clone()))?;
        let Some(ReportResult::Goals(report)) = &result.report else {
            bail!("goals report without goals result");
        };
        let mut findings = Vec::new();
        for (goal, measured) in goals.iter().zip(&report.goals) {
            let typical = measured
                .compare
                .as_ref()
                .and_then(|c| count(&c.completions.previous));
            let (Some(typical), Some(reference)) = (typical, &goal.goal_ref) else {
                continue;
            };
            if !fires(measured.completions, typical) {
                continue;
            }
            let values = GoalPaceValues {
                goal: reference.id.clone(),
                goal_version: reference.version,
                today: measured.completions,
                typical,
                relative_change: round4((measured.completions - typical) as f64 / typical as f64),
            };
            findings.push(site.finding(
                self,
                result.resolved.start,
                0.0,
                state(site, vec![goal.clone()]),
                FindingValues::GoalPace(values),
            ));
        }
        Ok(findings)
    }
}

/// Today's completions of `goals` against a typical weekday.
fn state(site: &Site, goals: Vec<GoalDefinition>) -> ExplorationState {
    ExplorationState {
        compare: Compare::TypicalWeekday,
        report: Some(ReportDefinition::Goals(GoalsReport {
            v: REPORT_VERSION,
            goals,
        })),
        ..site.state(day(site.today))
    }
}

/// Gives every condition of `group` the ID `<prefix><n>`, counting on from `n`.
fn renumber(group: &mut Group, prefix: &str, n: &mut usize) {
    for item in &mut group.items {
        match item {
            Node::Group(inner) => renumber(inner, prefix, n),
            Node::Cond(cond) => {
                cond.id = format!("{prefix}{n}");
                *n += 1;
            }
        }
    }
}

/// Completions differ from a non-zero typical value by at least the minimum count and
/// percentage. Without a typical value there is no percentage to state.
fn fires(today: i64, typical: i64) -> bool {
    let change = (today - typical).abs();
    typical > 0
        && change >= FINDING_GOAL_CHANGE_MIN
        && change * 100 >= typical * FINDING_GOAL_CHANGE_PERCENT
}

#[cfg(test)]
mod tests {
    use super::fires;

    #[test]
    fn fires_at_thirty_percent_and_five_completions() {
        // 30 % of 20 is 6: 26 and 14 fire, 25 and 15 do not.
        assert!(fires(26, 20) && fires(14, 20));
        assert!(!fires(25, 20) && !fires(15, 20));
        // 30 % of 10 is 3, but fewer than 5 completions apart stays silent.
        assert!(!fires(14, 10) && fires(15, 10) && fires(5, 10));
        assert!(!fires(6, 10));
        assert!(!fires(9, 0), "no typical value to compare with");
    }
}
