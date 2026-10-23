//! Performance: a metric's p75 on one device over the last 24 whole hours above every daily
//! p75 of the 28 days before (0015).
//!
//! One exploration lists the metric and device pairs of the last 24 hours. A pair with
//! enough observations reads its 28 daily values as one day-granularity series. The last 24
//! hours are yesterday from the current local hour and today up to it, as a filter on
//! weekday and hour, so the state shows exactly the value the finding states.

use holocron_analytics::explore;
use holocron_protocol::exploration::{
    Cell, DimensionField, ExplorationState, Field, Granularity, Group, Measure, Metric,
    NamedMeasure, Node, Operator, Page, Value, Visualization,
};
use holocron_protocol::limits::{
    FINDING_PERFORMANCE_DAYS, FINDING_PERFORMANCE_DAYS_MIN, FINDING_PERFORMANCE_OBSERVATIONS_MIN,
};
use holocron_protocol::operations::summaries::{
    Finding, FindingRule, FindingValues, PerformanceValues,
};
use holocron_storage::Snapshot;

use super::{GroupOp, Rule, Site, cond, days, group, is};

/// Every performance metric (0003 Percentiles).
const METRICS: [&str; 5] = ["cls", "fcp", "inp", "lcp", "ttfb"];

const P75: Measure = Measure::Named(NamedMeasure::P75);

pub struct Performance;

impl Rule for Performance {
    fn rule(&self) -> FindingRule {
        FindingRule::Performance
    }

    fn version(&self) -> u32 {
        1
    }

    fn hourly(&self) -> bool {
        true
    }

    fn findings(&self, site: &Site, snapshot: &Snapshot) -> anyhow::Result<Vec<Finding>> {
        let yesterday = site.today.pred_opt().expect("a day before today");
        let any = cond(
            "m",
            DimensionField::PerfMetric,
            Operator::In,
            Some(Value::List(
                METRICS.iter().map(|m| Value::String((*m).into())).collect(),
            )),
        );
        let pairs = ExplorationState {
            measures: vec![P75],
            dimensions: vec![
                Field::Dimension(DimensionField::PerfMetric),
                Field::Dimension(DimensionField::Device),
            ],
            filter: Some(group(GroupOp::And, vec![any, last_day(site)])),
            page: Page {
                size: 100,
                index: 0,
            },
            ..site.state(days(yesterday, site.today))
        };
        let result = site.run(snapshot, &pairs)?;
        let window_start = site
            .instant(yesterday, site.hour)
            .unwrap_or(result.resolved.start);
        let mut findings = Vec::new();
        for row in &result.main.rows {
            let ([Cell::String(metric), device], Some(value)) =
                (row.key.as_slice(), row.values.first())
            else {
                continue;
            };
            let Some(units_now) = units(value) else {
                continue;
            };
            let pair = |extra: Vec<Node>| {
                let mut items = vec![
                    cond(
                        "m",
                        DimensionField::PerfMetric,
                        Operator::Eq,
                        Some(Value::String(metric.clone())),
                    ),
                    is("d", DimensionField::Device, device),
                ];
                items.extend(extra);
                group(GroupOp::And, items)
            };
            let shown = ExplorationState {
                measures: vec![P75],
                filter: Some(pair(vec![last_day(site)])),
                ..site.state(days(yesterday, site.today))
            };
            let query = match explore::validate(&shown, &[]) {
                Ok(query) => query,
                Err(refusal) => anyhow::bail!("performance state refused: {}", refusal.message),
            };
            let observations = match explore::observations(snapshot, &query, &site.context())? {
                Ok(n) => n,
                Err(refusal) => anyhow::bail!("performance state refused: {}", refusal.message),
            };
            if observations < FINDING_PERFORMANCE_OBSERVATIONS_MIN {
                continue;
            }
            let first = site.today - chrono::Days::new(u64::from(FINDING_PERFORMANCE_DAYS) + 1);
            let daily = ExplorationState {
                measures: vec![P75],
                filter: Some(pair(Vec::new())),
                visualization: Visualization::Line,
                granularity: Granularity::Day,
                ..site.state(days(first, yesterday.pred_opt().expect("a day before")))
            };
            let series = site.run(snapshot, &daily)?.main.series;
            let daily: Vec<(&str, &Metric, i64)> = series
                .iter()
                .flat_map(|s| &s.points)
                .filter_map(|p| {
                    let metric = p.values.first()?;
                    Some((&p.local[..10], metric, units(metric)?))
                })
                .collect();
            let Some(slowest) = slowest(&daily, units_now) else {
                continue;
            };
            let values = PerformanceValues {
                metric: metric.clone(),
                device: device.clone(),
                value: value.clone(),
                observations,
                slowest: slowest.1.clone(),
                slowest_day: slowest.0.to_owned(),
                days: daily.len() as u32,
            };
            findings.push(site.finding(
                self,
                window_start,
                0.0,
                shown,
                FindingValues::Performance(values),
            ));
        }
        Ok(findings)
    }
}

/// The last 24 whole hours of a range from yesterday to today: yesterday from the current
/// local hour on, today before it.
fn last_day(site: &Site) -> Node {
    let hour = Value::Integer(i64::from(site.hour));
    let on = |id: &str, date, cmp| {
        Node::Group(Group {
            items: vec![
                cond(
                    &format!("{id}w"),
                    DimensionField::Weekday,
                    Operator::Eq,
                    Some(Value::Integer(Site::weekday(date))),
                ),
                cond(
                    &format!("{id}h"),
                    DimensionField::Hour,
                    cmp,
                    Some(hour.clone()),
                ),
            ],
            ..group(GroupOp::And, Vec::new())
        })
    };
    let yesterday = site.today.pred_opt().expect("a day before today");
    Node::Group(group(
        GroupOp::Or,
        vec![
            on("y", yesterday, Operator::Gte),
            on("t", site.today, Operator::Lt),
        ],
    ))
}

/// The slowest day when it has enough days and the last 24 hours exceed it.
fn slowest<'a>(daily: &[(&'a str, &'a Metric, i64)], now: i64) -> Option<(&'a str, &'a Metric)> {
    if daily.len() < FINDING_PERFORMANCE_DAYS_MIN as usize {
        return None;
    }
    // The earliest of equally slow days.
    let (day, metric, units) = daily
        .iter()
        .copied()
        .reduce(|a, b| if b.2 > a.2 { b } else { a })?;
    (now > units).then_some((day, metric))
}

/// A p75 in its stored integer units: milliseconds, or `cls` times 10^4.
pub fn units(metric: &Metric) -> Option<i64> {
    match metric {
        Metric::Millis { v } => Some(*v),
        Metric::Decimal { v } => {
            let (whole, fraction) = v.split_once('.').unwrap_or((v, ""));
            let fraction = format!("{fraction:0<4}");
            Some(whole.parse::<i64>().ok()? * 10_000 + fraction.get(..4)?.parse::<i64>().ok()?)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fires_only_above_the_slowest_of_enough_days() {
        let ms = |v| Metric::Millis { v };
        let (a, b, c, d) = (ms(2000), ms(2400), ms(2400), ms(1800));
        let days = [
            ("2026-09-15", &a, 2000),
            ("2026-09-16", &b, 2400),
            ("2026-09-17", &c, 2400),
            ("2026-09-18", &d, 1800),
        ];
        assert_eq!(slowest(&days, 2401), Some(("2026-09-16", &b)));
        assert_eq!(slowest(&days, 2400), None, "equal is not slower");
        assert_eq!(slowest(&days[..3], 5000), None, "three days are too few");
    }

    #[test]
    fn cls_reads_in_units_of_one_ten_thousandth() {
        let decimal = |v: &str| Metric::Decimal { v: v.into() };
        assert_eq!(units(&decimal("0.2500")), Some(2500));
        assert_eq!(units(&decimal("1.0001")), Some(10_001));
        assert_eq!(units(&Metric::Millis { v: 3400 }), Some(3400));
    }
}
