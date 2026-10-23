//! Funnel steps: a step of a funnel pinned to a board whose drop-off today moved against the
//! same local weekdays of the previous 8 weeks, both read from local midnight to the start of
//! the current hour (0015).
//!
//! Reports have no typical-weekday comparison, so the rule runs each funnel twice: today, and
//! once over every baseline day together, filtered to their weekday. Only daily-scope funnels
//! qualify: the daily pseudonym ends at local midnight, so pooling the baseline days counts
//! each day's funnel on its own. Drop-off is a rate, so the baseline days pool as one
//! population, as typical-weekday rates do.

use holocron_analytics::explore::Refusal;
use holocron_protocol::exploration::{
    DimensionField, ExplorationState, ExploreResult, FunnelReport, FunnelResult, Group,
    IdentityScope, Node, Operator, ReportDefinition, ReportResult, Value, Visualization,
};
use holocron_protocol::limits::{
    FINDING_FUNNEL_ENTERED_MIN, FINDING_FUNNEL_POINTS_MIN, FINDING_FUNNELS_MAX,
    TYPICAL_WEEKDAY_DAYS_MIN,
};
use holocron_protocol::operations::summaries::{
    Finding, FindingRule, FindingValues, FunnelStepValues,
};
use holocron_storage::Snapshot;

use super::{BoardFunnel, GroupOp, Range, Rule, Site, cond, day, days, group, round4};

pub struct FunnelStep;

impl Rule for FunnelStep {
    fn rule(&self) -> FindingRule {
        FindingRule::FunnelStep
    }

    fn version(&self) -> u32 {
        1
    }

    fn hourly(&self) -> bool {
        true
    }

    fn findings(&self, site: &Site, snapshot: &Snapshot) -> anyhow::Result<Vec<Finding>> {
        let (Some(first), Some(last)) = (site.baseline_days.first(), site.baseline_days.last())
        else {
            return Ok(Vec::new());
        };
        if site.hour == 0 || site.baseline_days.len() < TYPICAL_WEEKDAY_DAYS_MIN as usize {
            return Ok(Vec::new());
        }
        let mut seen: Vec<(&FunnelReport, &Option<Group>)> = Vec::new();
        let mut findings = Vec::new();
        for funnel in &site.funnels {
            if funnel.funnel.scope != IdentityScope::Daily
                || seen.contains(&(&funnel.funnel, &funnel.filter))
            {
                continue;
            }
            if seen.len() == FINDING_FUNNELS_MAX {
                break;
            }
            seen.push((&funnel.funnel, &funnel.filter));
            let today = state(site, funnel, day(site.today), false);
            let baseline = state(site, funnel, days(*first, *last), true);
            // A pinned funnel stops running when a property it filters on goes.
            let (Some((measured, window_start)), Some((typical, _))) = (
                steps(site.explore(snapshot, &today, &[])?),
                steps(site.explore(snapshot, &baseline, &[])?),
            ) else {
                continue;
            };
            for (i, (now, then)) in measured.steps.iter().zip(&typical.steps).enumerate() {
                let (Some(dropped), Some(baseline_dropped)) = (now.drop, then.drop) else {
                    continue;
                };
                if !fires(now.reached, dropped, then.reached, baseline_dropped) {
                    continue;
                }
                let values = FunnelStepValues {
                    card: funnel.card.clone(),
                    title: funnel.title.clone(),
                    step: i as u32 + 1,
                    until_hour: site.hour,
                    entered: now.reached,
                    dropped,
                    baseline_entered: then.reached,
                    baseline_dropped,
                    baseline_days: site.baseline_days.len() as u32,
                    drop_off: round4(dropped as f64 / now.reached as f64),
                    baseline_drop_off: round4(baseline_dropped as f64 / then.reached as f64),
                };
                findings.push(site.finding(
                    self,
                    window_start,
                    0.0,
                    today.clone(),
                    FindingValues::FunnelStep(values),
                ));
            }
        }
        Ok(findings)
    }
}

/// The funnel over `range` from local midnight to the current hour, its own filter kept;
/// over the baseline days also only on today's weekday.
fn state(site: &Site, funnel: &BoardFunnel, range: Range, baseline: bool) -> ExplorationState {
    let mut taken = Vec::new();
    funnel.filter.iter().for_each(|g| ids(g, &mut taken));
    funnel.funnel.steps.iter().for_each(|g| ids(g, &mut taken));
    let fresh = |base: &str| {
        (0..)
            .map(|n| format!("{base}{n}"))
            .find(|id| !taken.contains(id))
            .expect("an unused ID")
    };
    let mut items = vec![cond(
        &fresh("sh"),
        DimensionField::Hour,
        Operator::Lt,
        Some(Value::Integer(i64::from(site.hour))),
    )];
    if baseline {
        items.push(cond(
            &fresh("sw"),
            DimensionField::Weekday,
            Operator::Eq,
            Some(Value::Integer(Site::weekday(site.today))),
        ));
    }
    if let Some(filter) = &funnel.filter {
        items.push(Node::Group(filter.clone()));
    }
    ExplorationState {
        filter: Some(group(GroupOp::And, items)),
        visualization: Visualization::Funnel,
        report: Some(ReportDefinition::Funnel(funnel.funnel.clone())),
        ..site.state(range)
    }
}

/// Every condition ID of a group.
fn ids(group: &Group, into: &mut Vec<String>) {
    for item in &group.items {
        match item {
            Node::Group(inner) => ids(inner, into),
            Node::Cond(cond) => into.push(cond.id.clone()),
        }
    }
}

/// The funnel's steps and the start of its range; `None` when it was refused.
fn steps(result: Result<ExploreResult, Refusal>) -> Option<(FunnelResult, i64)> {
    let result = result.ok()?;
    match result.report {
        Some(ReportResult::Funnel(funnel)) => Some((funnel, result.resolved.start)),
        _ => None,
    }
}

/// Enough visitors entered the step today and on the baseline days, and its drop-off moved by
/// at least the minimum of percentage points. Exact in integers: `|d/e − bd/be| ≥ p/100`.
fn fires(entered: i64, dropped: i64, baseline_entered: i64, baseline_dropped: i64) -> bool {
    if entered < FINDING_FUNNEL_ENTERED_MIN || baseline_entered < FINDING_FUNNEL_ENTERED_MIN {
        return false;
    }
    let (e, d, be, bd) = (
        i128::from(entered),
        i128::from(dropped),
        i128::from(baseline_entered),
        i128::from(baseline_dropped),
    );
    (d * be - bd * e).abs() * 100 >= i128::from(FINDING_FUNNEL_POINTS_MIN) * e * be
}

#[cfg(test)]
mod tests {
    use super::fires;

    #[test]
    fn fires_at_five_points_with_fifty_entering() {
        // 50 entering: 20 % against 15 % of 200 is 5 points; 14.5 % is 5.5, 15.5 % is 4.5.
        assert!(fires(50, 10, 200, 30));
        assert!(fires(50, 10, 200, 29));
        assert!(!fires(50, 10, 200, 31));
        // Falling drop-off counts the same.
        assert!(fires(50, 5, 200, 30));
        assert!(!fires(50, 6, 200, 31));
        // 49 entering today, or on the baseline days, is too few.
        assert!(!fires(49, 20, 200, 30));
        assert!(!fires(100, 50, 49, 10));
    }
}
