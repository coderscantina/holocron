//! Dimension movers: one value of `path`, `referrer_domain`, `browser`, `os` or `device` whose
//! visitor-days today so far moved far from its typical weekday and explain a good part of the
//! site's own change (0015). One exploration reads all five as panels of today's top 100
//! values, so a value absent today is not seen.

use holocron_protocol::exploration::{
    Compare, Cut, DimensionField, ExplorationState, Field, Granularity, Measure, NamedMeasure,
    Page, Panel, Sort, SortDir, SortKey, Visualization,
};
use holocron_protocol::limits::{
    FINDING_MOVER_CHANGE_MIN, FINDING_MOVER_FALL_PERCENT, FINDING_MOVER_RISE_PERCENT,
    FINDING_MOVER_SHARE_PERCENT,
};
use holocron_protocol::operations::summaries::{Finding, FindingRule, FindingValues, MoverValues};
use holocron_storage::Snapshot;

use super::{GroupOp, Rule, Site, count, day, group, is, round4};

const DIMENSIONS: [DimensionField; 5] = [
    DimensionField::Path,
    DimensionField::ReferrerDomain,
    DimensionField::Browser,
    DimensionField::Os,
    DimensionField::Device,
];

/// The largest exploration page: the values read per dimension.
const TOP: u32 = 100;

const VISITOR_DAYS: Measure = Measure::Named(NamedMeasure::VisitorDays);

pub struct Mover;

impl Rule for Mover {
    fn rule(&self) -> FindingRule {
        FindingRule::DimensionMover
    }

    fn version(&self) -> u32 {
        1
    }

    fn hourly(&self) -> bool {
        false
    }

    fn findings(&self, site: &Site, snapshot: &Snapshot) -> anyhow::Result<Vec<Finding>> {
        let panel = |dimension| Panel {
            measures: vec![VISITOR_DAYS],
            dimensions: vec![Field::Dimension(dimension)],
            sort: vec![Sort {
                by: SortKey::Measure(VISITOR_DAYS),
                dir: SortDir::Desc,
            }],
            page: Page {
                size: TOP,
                index: 0,
            },
        };
        let main = panel(DIMENSIONS[0]);
        let state = ExplorationState {
            measures: main.measures,
            dimensions: main.dimensions,
            sort: main.sort,
            page: main.page,
            compare: Compare::TypicalWeekday,
            ..site.state(day(site.today))
        };
        let panels: Vec<Panel> = DIMENSIONS[1..].iter().map(|d| panel(*d)).collect();
        let result = match site.explore(snapshot, &state, &panels)? {
            Ok(result) => result,
            Err(refusal) => anyhow::bail!("mover state refused: {}", refusal.message),
        };
        let cuts = std::iter::once(&result.main).chain(&result.panels);
        let mut findings = Vec::new();
        for (dimension, cut) in DIMENSIONS.into_iter().zip(cuts) {
            let Some((site_today, site_typical)) = total(cut) else {
                continue;
            };
            for row in &cut.rows {
                let today = row.values.first().and_then(count);
                let typical = row
                    .compare
                    .as_ref()
                    .and_then(|c| c.first())
                    .and_then(|c| count(&c.previous));
                let (Some(today), Some(typical), Some(value)) = (today, typical, row.key.first())
                else {
                    continue;
                };
                let Some(share) = fires(today, typical, site_today - site_typical) else {
                    continue;
                };
                let values = MoverValues {
                    dimension,
                    value: value.clone(),
                    today,
                    typical,
                    ratio: round4(today as f64 / typical as f64),
                    site_today,
                    site_typical,
                };
                findings.push(site.finding(
                    self,
                    result.resolved.start,
                    share,
                    shown(site, dimension, value),
                    FindingValues::DimensionMover(values),
                ));
            }
        }
        Ok(findings)
    }
}

/// The site's visitor-days today so far and typical so far; `None` without enough history.
fn total(cut: &Cut) -> Option<(i64, i64)> {
    let today = cut.totals.first().and_then(count)?;
    let typical = cut
        .compare
        .as_ref()?
        .first()
        .and_then(|c| count(&c.previous))?;
    Some((today, typical))
}

/// The value's visitor-days today by the hour against a typical weekday.
fn shown(
    site: &Site,
    dimension: DimensionField,
    value: &holocron_protocol::exploration::Cell,
) -> ExplorationState {
    ExplorationState {
        measures: vec![VISITOR_DAYS],
        filter: Some(group(GroupOp::And, vec![is("f1", dimension, value)])),
        granularity: Granularity::Hour,
        compare: Compare::TypicalWeekday,
        visualization: Visualization::Line,
        ..site.state(day(site.today))
    }
}

/// The value's share of the site's change when it moved: by at least the minimum, to at least
/// 1.5 or at most 0.67 times a non-zero typical value, in the direction of the site's change
/// and by at least a quarter of it. A site without change has nothing to explain.
fn fires(today: i64, typical: i64, site_change: i64) -> Option<f64> {
    let change = today - typical;
    if typical <= 0
        || change.abs() < FINDING_MOVER_CHANGE_MIN
        || site_change == 0
        || change.signum() != site_change.signum()
    {
        return None;
    }
    let moved = if change > 0 {
        today * 100 >= typical * FINDING_MOVER_RISE_PERCENT
    } else {
        today * 100 <= typical * FINDING_MOVER_FALL_PERCENT
    };
    let explains = change.abs() * 100 >= site_change.abs() * FINDING_MOVER_SHARE_PERCENT;
    (moved && explains).then(|| round4((change as f64 / site_change as f64).min(1.0)))
}

#[cfg(test)]
mod tests {
    use super::fires;

    #[test]
    fn fires_at_every_threshold_and_not_just_below() {
        // 40 typical: 60 is 1.5 times and 20 more; 59 is neither.
        assert_eq!(fires(60, 40, 80), Some(0.25));
        assert_eq!(fires(59, 40, 76), None);
        // 20 more but only 1.4 times.
        assert_eq!(fires(70, 50, 20), None);
        // 1.5 times but 19 more.
        assert_eq!(fires(57, 38, 19), None);
        // Falls: 100 to 67 is 0.67 times, 100 to 68 is not.
        assert_eq!(fires(67, 100, -33), Some(1.0));
        assert_eq!(fires(68, 100, -32), None);
        // Explains a quarter of the site's change, not less.
        assert_eq!(fires(60, 40, 81), None);
        // Against the site's direction, or a site without change.
        assert_eq!(fires(60, 40, -10), None);
        assert_eq!(fires(60, 40, 0), None);
        // A value more than the site's change explains all of it.
        assert_eq!(fires(90, 40, 30), Some(1.0));
        assert_eq!(fires(30, 0, 30), None, "no typical value to compare with");
    }
}
