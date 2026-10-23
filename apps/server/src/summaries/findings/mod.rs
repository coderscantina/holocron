//! Noticed changes (decision 0015): four rule families, each a [`Rule`] that reads one site
//! through `analytics.explore` inside one snapshot and returns facts with the values their
//! templates need and the exploration state that shows them. Nothing here writes text.
//!
//! Dimension movers and goal pace run with every summary; performance and funnel steps at most
//! hourly per site (0015 decision 5, the fallback cadence). Between their runs, and when a
//! rule's read cannot start or times out, a rule keeps what it found last ([`Kept`]), so a
//! site's findings are always every family's latest. [`rank`] orders them across families.

mod funnel_step;
mod goal_pace;
mod mover;
mod performance;

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::bail;
use chrono::{Datelike, NaiveDate, Timelike};
use chrono_tz::Tz;
use holocron_analytics::explore::{self, Context, Refusal};
use holocron_protocol::exploration::{
    Cell, Compare, CondKind, Condition, DimensionField, ExplorationState, ExploreResult, Field,
    FunnelReport, GoalDefinition, Granularity, Group, GroupKind, GroupOp, Metric, Node, Operator,
    Range, STATE_VERSION, Value, Visualization,
};
use holocron_protocol::ids::{CardId, SiteId};
use holocron_protocol::limits::{FINDINGS_HOURLY_CADENCE_MS, FINDINGS_MAX, METRIC_VERSION};
use holocron_protocol::operations::collection::CollectionCounter;
use holocron_protocol::operations::summaries::{Finding, FindingRule, FindingValues};
use holocron_protocol::retention::Retention;
use holocron_storage::{Id, Lane, ReadError, Snapshot};

use crate::state::AppState;

/// A rule family: a pure function of one site's data at one snapshot.
pub trait Rule: Sync {
    fn rule(&self) -> FindingRule;
    /// Raised whenever its thresholds or values change.
    fn version(&self) -> u32;
    /// Runs at most every [`FINDINGS_HOURLY_CADENCE_MS`] instead of with every summary.
    fn hourly(&self) -> bool;
    /// The site's findings, unranked, with rank 0.
    fn findings(&self, site: &Site, snapshot: &Snapshot) -> anyhow::Result<Vec<Finding>>;
}

/// Every rule in family priority order (0015 Noticed changes).
const RULES: [&dyn Rule; 4] = [
    &goal_pace::GoalPace,
    &performance::Performance,
    &mover::Mover,
    &funnel_step::FunnelStep,
];

/// What the rules read about a site besides its analytical data.
pub struct Site {
    pub id: SiteId,
    pub analytics: Id,
    pub timezone: String,
    pub hostnames: Vec<String>,
    /// The site's effective event retention (0016).
    pub retention: Retention,
    pub now_ms: i64,
    /// Today's local date and the local hour now.
    pub today: NaiveDate,
    pub hour: u32,
    /// Today's typical-weekday baseline days, as the summary's own result names them.
    pub baseline_days: Vec<NaiveDate>,
    /// The site's active goals, oldest first, at their current versions.
    pub goals: Vec<GoalDefinition>,
    /// Funnels pinned to boards for the site.
    pub funnels: Vec<BoardFunnel>,
}

/// A funnel card: its definition and the filter it runs under.
pub struct BoardFunnel {
    pub card: CardId,
    pub title: String,
    pub funnel: FunnelReport,
    pub filter: Option<Group>,
}

impl Site {
    /// `None` for a timezone this build does not know; the summary would have been refused.
    pub fn local(timezone: &str, now_ms: i64) -> Option<(NaiveDate, u32)> {
        let tz: Tz = timezone.parse().ok()?;
        let local = chrono::DateTime::from_timestamp_millis(now_ms)?.with_timezone(&tz);
        Some((local.date_naive(), local.hour()))
    }

    /// ISO weekday 1 (Monday) to 7 of a local date.
    fn weekday(date: NaiveDate) -> i64 {
        i64::from(date.weekday().number_from_monday())
    }

    /// The first instant of a local date and hour; the earlier one of a repeated hour, the
    /// hour after a skipped one.
    fn instant(&self, date: NaiveDate, hour: u32) -> Option<i64> {
        use chrono::TimeZone;
        let tz: Tz = self.timezone.parse().ok()?;
        (hour..24).find_map(|h| {
            let local = date.and_hms_opt(h, 0, 0)?;
            tz.from_local_datetime(&local)
                .earliest()
                .map(|t| t.timestamp_millis())
        })
    }

    /// A plain exploration of the site in its timezone; rules change what they need.
    fn state(&self, range: Range) -> ExplorationState {
        ExplorationState {
            v: STATE_VERSION,
            site: self.id.clone(),
            measures: Vec::new(),
            dimensions: Vec::new(),
            filter: None,
            range,
            timezone: self.timezone.clone(),
            granularity: Granularity::Day,
            compare: Compare::None,
            metric_version: METRIC_VERSION,
            sort: Vec::new(),
            page: explore::DEFAULT_PAGE,
            visualization: Visualization::Table,
            report: None,
        }
    }

    /// Runs a state, checked like any saved state. A refusal is returned, not raised: a
    /// pinned funnel can stop running when the data it filters on goes.
    fn explore(
        &self,
        snapshot: &Snapshot,
        state: &ExplorationState,
        panels: &[holocron_protocol::exploration::Panel],
    ) -> anyhow::Result<Result<ExploreResult, Refusal>> {
        let query = match explore::validate(state, panels) {
            Ok(query) => query,
            Err(refusal) => return Ok(Err(refusal)),
        };
        Ok(explore::explore(snapshot, &query, &self.context())?)
    }

    /// [`Site::explore`] for states the rules build themselves, which must run.
    fn run(&self, snapshot: &Snapshot, state: &ExplorationState) -> anyhow::Result<ExploreResult> {
        match self.explore(snapshot, state, &[])? {
            Ok(result) => Ok(result),
            Err(refusal) => bail!("finding state refused: {}", refusal.message),
        }
    }

    fn context(&self) -> Context<'_> {
        Context {
            site: self.analytics,
            now_ms: self.now_ms,
            hostnames: &self.hostnames,
            retention: self.retention,
        }
    }

    /// A finding of `rule` computed now, before ranking.
    fn finding(
        &self,
        rule: &dyn Rule,
        window_start: i64,
        share: f64,
        state: ExplorationState,
        values: FindingValues,
    ) -> Finding {
        Finding {
            rank: 0,
            rule: rule.rule(),
            rule_version: rule.version(),
            computed_at: self.now_ms,
            window_start,
            share,
            state,
            values,
        }
    }
}

/// One local day as a range.
fn day(date: NaiveDate) -> Range {
    days(date, date)
}

fn days(start: NaiveDate, end: NaiveDate) -> Range {
    Range::Days {
        start: start.to_string(),
        end: end.to_string(),
    }
}

fn group(op: GroupOp, items: Vec<Node>) -> Group {
    Group {
        kind: GroupKind::Group,
        op,
        items,
    }
}

fn cond(id: &str, field: DimensionField, cmp: Operator, value: Option<Value>) -> Node {
    Node::Cond(Condition {
        kind: CondKind::Cond,
        id: id.to_owned(),
        field: Field::Dimension(field),
        cmp,
        value,
    })
}

/// The condition selecting one result cell of `field`.
fn is(id: &str, field: DimensionField, cell: &Cell) -> Node {
    let (cmp, value) = match cell {
        Cell::String(v) => (Operator::Eq, Some(Value::String(v.clone()))),
        Cell::Integer(v) => (Operator::Eq, Some(Value::Integer(*v))),
        Cell::Decimal(v) => (Operator::Eq, Some(Value::Decimal(v.clone()))),
        Cell::Date(v) => (Operator::Eq, Some(Value::Date(v.clone()))),
        Cell::Bool(v) => (Operator::Eq, Some(Value::Bool(*v))),
        Cell::Null => (Operator::IsNull, None),
        Cell::NotSet => (Operator::IsNotSet, None),
    };
    cond(id, field, cmp, value)
}

fn count(metric: &Metric) -> Option<i64> {
    match metric {
        Metric::Count { v } => Some(*v),
        _ => None,
    }
}

/// Rounded to 4 places, as results round ratios.
fn round4(x: f64) -> f64 {
    (x * 10_000.0).round() / 10_000.0
}

/// What each rule last found for one site.
#[derive(Default)]
pub struct Kept {
    found: HashMap<FindingRule, (i64, Vec<Finding>)>,
}

impl Kept {
    fn due(&self, rule: &dyn Rule, now_ms: i64) -> bool {
        !rule.hourly()
            || self
                .found
                .get(&rule.rule())
                .is_none_or(|(at, _)| now_ms - at >= FINDINGS_HOURLY_CADENCE_MS)
    }

    /// Forgets everything, as after a reset of the site.
    pub fn clear(&mut self) {
        self.found.clear();
    }
}

/// Runs every due rule for the site, one read each, and returns the site's ranked findings.
/// A rule whose read waits behind a report, times out or fails keeps its last findings and
/// is due again next time.
pub async fn notice(state: &AppState, site: Site, kept: &mut Kept) -> Vec<Finding> {
    let storage = state.collector.storage();
    let site = Arc::new(site);
    for rule in RULES {
        if !kept.due(rule, site.now_ms) || !storage.reports_idle() {
            continue;
        }
        let reading = Arc::clone(&site);
        let read = storage
            .read(Lane::Report, move |snapshot| {
                Ok(rule.findings(&reading, snapshot))
            })
            .await;
        match read.map(|read| read.value) {
            Ok(Ok(found)) => {
                kept.found.insert(rule.rule(), (site.now_ms, found));
            }
            Ok(Err(error)) => {
                tracing::error!(site = %site.id, rule = ?rule.rule(), error = %error, "finding rule failed");
            }
            Err(ReadError::Timeout) => {
                tracing::warn!(site = %site.id, rule = ?rule.rule(), "finding rule timed out; retrying next run");
                state
                    .collector
                    .metrics()
                    .count(CollectionCounter::SummaryTimeouts, 1);
            }
            Err(error) => {
                tracing::warn!(site = %site.id, rule = ?rule.rule(), error = %error, "finding rule did not run");
            }
        }
    }
    rank(
        kept.found
            .values()
            .flat_map(|(_, f)| f.iter().cloned())
            .collect(),
    )
}

/// Orders findings by family priority, then within a family by share of the site's total
/// change and by the size of the change, keeps [`FINDINGS_MAX`] and numbers them from 1.
pub fn rank(mut findings: Vec<Finding>) -> Vec<Finding> {
    let family = |f: &Finding| RULES.iter().position(|rule| rule.rule() == f.rule);
    findings.sort_by(|a, b| {
        family(a)
            .cmp(&family(b))
            .then(b.share.total_cmp(&a.share))
            .then(size(&b.values).total_cmp(&size(&a.values)))
    });
    findings.truncate(FINDINGS_MAX);
    for (i, finding) in findings.iter_mut().enumerate() {
        finding.rank = i as u32 + 1;
    }
    findings
}

/// The absolute change within a family: visitor-days, completions, drop-off points, or how
/// far the p75 exceeds the slowest day relative to it.
fn size(values: &FindingValues) -> f64 {
    match values {
        FindingValues::DimensionMover(v) => (v.today - v.typical).abs() as f64,
        FindingValues::GoalPace(v) => (v.today - v.typical).abs() as f64,
        FindingValues::FunnelStep(v) => (v.drop_off - v.baseline_drop_off).abs(),
        FindingValues::Performance(v) => {
            match (performance::units(&v.value), performance::units(&v.slowest)) {
                (Some(value), Some(slowest)) => (value - slowest) as f64 / slowest.max(1) as f64,
                _ => 0.0,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use holocron_protocol::exploration::Cell;
    use holocron_protocol::ids::GoalId;
    use holocron_protocol::operations::summaries::{GoalPaceValues, MoverValues};

    fn site() -> Site {
        Site {
            id: SiteId::new("s1"),
            analytics: 1,
            timezone: "UTC".into(),
            hostnames: Vec::new(),
            retention: Retention::OneYear,
            now_ms: 0,
            today: NaiveDate::from_ymd_opt(2026, 10, 13).unwrap(),
            hour: 15,
            baseline_days: Vec::new(),
            goals: Vec::new(),
            funnels: Vec::new(),
        }
    }

    fn mover(share: f64, change: i64) -> Finding {
        let site = site();
        site.finding(
            &mover::Mover,
            0,
            share,
            site.state(day(site.today)),
            FindingValues::DimensionMover(MoverValues {
                dimension: DimensionField::Path,
                value: Cell::String(format!("/{change}")),
                today: 100 + change,
                typical: 100,
                ratio: 1.0,
                site_today: 0,
                site_typical: 0,
            }),
        )
    }

    fn goal(change: i64) -> Finding {
        let site = site();
        site.finding(
            &goal_pace::GoalPace,
            0,
            0.0,
            site.state(day(site.today)),
            FindingValues::GoalPace(GoalPaceValues {
                goal: GoalId::new("g"),
                goal_version: 1,
                today: 10 + change,
                typical: 10,
                relative_change: 0.0,
            }),
        )
    }

    #[test]
    fn findings_rank_by_family_then_share_then_change() {
        let ranked = rank(vec![
            mover(0.5, 30),
            mover(0.9, 25),
            goal(-6),
            mover(0.5, -40),
            goal(9),
        ]);
        let order: Vec<(u32, FindingRule, i64)> = ranked
            .iter()
            .map(|f| {
                let change = match &f.values {
                    FindingValues::DimensionMover(v) => v.today - v.typical,
                    FindingValues::GoalPace(v) => v.today - v.typical,
                    _ => unreachable!(),
                };
                (f.rank, f.rule, change)
            })
            .collect();
        assert_eq!(
            order,
            vec![
                (1, FindingRule::GoalPace, 9),
                (2, FindingRule::GoalPace, -6),
                (3, FindingRule::DimensionMover, 25),
                (4, FindingRule::DimensionMover, -40),
                (5, FindingRule::DimensionMover, 30),
            ]
        );
        let many = rank(
            (0..FINDINGS_MAX as i64 + 3)
                .map(|c| mover(0.5, c))
                .collect(),
        );
        assert_eq!(many.len(), FINDINGS_MAX);
    }

    #[test]
    fn hourly_rules_wait_out_their_cadence() {
        let mut kept = Kept::default();
        let hourly: &dyn Rule = &performance::Performance;
        let every: &dyn Rule = &mover::Mover;
        assert!(kept.due(hourly, 0) && kept.due(every, 0));
        kept.found.insert(hourly.rule(), (0, Vec::new()));
        kept.found.insert(every.rule(), (0, Vec::new()));
        assert!(!kept.due(hourly, FINDINGS_HOURLY_CADENCE_MS - 1));
        assert!(kept.due(hourly, FINDINGS_HOURLY_CADENCE_MS));
        assert!(kept.due(every, 1));
    }
}
