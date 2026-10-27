//! Umami segments, reports and shares as Holocron definitions.
//!
//! Each mapping returns the definition with the source settings it could not carry, or the reason
//! nothing maps. Every mapped state passes the planner's checks before it is stored, so an
//! imported definition opens like one saved in Holocron.

use holocron_analytics::explore::{self, DEFAULT_PAGE};
use holocron_protocol::exploration::{
    AttributionModel, AttributionReport, CondKind, Condition, Conversion, DimensionField,
    ExplorationState, Field, FunnelReport, GoalConversion, GoalCounting, GoalDefinition,
    GoalsReport, Granularity, Group, GroupKind, GroupOp, IdentityScope, JourneyReport,
    JourneyValue, Measure, NamedMeasure, Node, Operator, PopulationScope, PropertyField,
    PropertyScope, REPORT_VERSION, Range, RangeUnit, ReportDefinition, RetentionReport,
    STATE_VERSION, TouchField, Value as Cond, Visualization,
};
use holocron_protocol::ids::SiteId;
use holocron_protocol::operations::sharing::ExplorationLimits;
use serde_json::Value;

/// A mapped definition and the source settings it does not carry.
pub type Mapped<T> = Result<(T, Vec<String>), String>;

/// Conditions get IDs `c1`, `c2`, ... in source order.
struct Ids(usize);

impl Ids {
    fn next(&mut self) -> String {
        self.0 += 1;
        format!("c{}", self.0)
    }
}

/// An Umami filter field name as a Holocron dimension.
fn dimension(name: &str) -> Option<DimensionField> {
    use DimensionField::*;
    Some(match name {
        "path" | "url" => Path,
        "referrer" => ReferrerDomain,
        "title" => Title,
        "query" => Query,
        "host" | "hostname" => Hostname,
        "os" => Os,
        "browser" => Browser,
        "device" => Device,
        "country" => Country,
        "region" => Region,
        "city" => City,
        "language" => Language,
        "screen" => Screen,
        "event" => EventName,
        "tag" => Tag,
        "utmSource" => UtmSource,
        "utmMedium" => UtmMedium,
        "utmCampaign" => UtmCampaign,
        "utmContent" => UtmContent,
        "utmTerm" => UtmTerm,
        _ => return None,
    })
}

fn condition(
    ids: &mut Ids,
    field: Field,
    op: &str,
    value: Option<Cond>,
) -> Result<Condition, String> {
    let (cmp, value) = match op {
        "eq" => (Operator::Eq, value),
        "neq" => (Operator::Neq, value),
        "c" => (Operator::Contains, value),
        "dnc" => (Operator::NotContains, value),
        "gt" => (Operator::Gt, value),
        "gte" => (Operator::Gte, value),
        "lt" => (Operator::Lt, value),
        "lte" => (Operator::Lte, value),
        "s" => (Operator::IsSet, None),
        "ns" => (Operator::IsNotSet, None),
        "t" => (Operator::Eq, Some(Cond::Bool(true))),
        "f" => (Operator::Eq, Some(Cond::Bool(false))),
        other => return Err(format!("operator_not_supported:{other}")),
    };
    Ok(Condition {
        kind: CondKind::Cond,
        id: ids.next(),
        field,
        cmp,
        value,
    })
}

fn group(op: GroupOp, items: Vec<Condition>) -> Group {
    Group {
        kind: GroupKind::Group,
        op,
        items: items.into_iter().map(Node::Cond).collect(),
    }
}

/// `{ name, operator, value }` filters on dimensions.
fn filters(ids: &mut Ids, list: &[Value]) -> Result<Vec<Condition>, String> {
    list.iter()
        .map(|f| {
            let name = f["name"].as_str().unwrap_or_default();
            let field = dimension(name).ok_or_else(|| format!("field_not_supported:{name}"))?;
            let value = f["value"].as_str().map(|v| Cond::String(v.to_owned()));
            condition(
                ids,
                Field::Dimension(field),
                f["operator"].as_str().unwrap_or_default(),
                value,
            )
        })
        .collect()
}

/// `{ propertyName, dataType, operator, value }` filters on properties of `scope`.
fn property_filters(
    ids: &mut Ids,
    list: &[Value],
    scope: PropertyScope,
) -> Result<Vec<Condition>, String> {
    list.iter()
        .map(|f| {
            let key = f["propertyName"]
                .as_str()
                .or(f["property"].as_str())
                .unwrap_or_default()
                .to_owned();
            let text = f["value"].as_str().unwrap_or_default();
            let value = match f["dataType"].as_i64().unwrap_or(1) {
                1 => Cond::String(text.to_owned()),
                2 => match text.parse::<i64>() {
                    Ok(n) => Cond::Integer(n),
                    Err(_) => Cond::Decimal(text.to_owned()),
                },
                3 => Cond::Bool(text == "true"),
                other => return Err(format!("property_type_not_supported:{other}")),
            };
            condition(
                ids,
                Field::Property(PropertyField { scope, key }),
                f["operator"].as_str().unwrap_or_default(),
                Some(value),
            )
        })
        .collect()
}

/// A segment's `parameters` as one filter group.
pub fn segment_filter(params: &Value, site: &SiteId) -> Mapped<Group> {
    let mut ids = Ids(0);
    let mut items = filters(&mut ids, params["filters"].as_array().map_or(&[], |v| v))?;
    items.extend(property_filters(
        &mut ids,
        params["sessionPropertyFilters"]
            .as_array()
            .map_or(&[], |v| v),
        PropertyScope::Session,
    )?);
    if items.is_empty() {
        return Err("no_filters".into());
    }
    let op = match params["match"].as_str() {
        Some("any") => GroupOp::Or,
        _ => GroupOp::And,
    };
    let mut lost = Vec::new();
    for key in ["dateRange", "action"] {
        if !params[key].is_null() {
            lost.push(key.to_owned());
        }
    }
    let filter = group(op, items);
    let mut state = base(site, "UTC");
    state.filter = Some(filter.clone());
    checked(&state)?;
    Ok((filter, lost))
}

/// The state every mapped definition starts from: page views over the last 30 days.
fn base(site: &SiteId, timezone: &str) -> ExplorationState {
    ExplorationState {
        v: STATE_VERSION,
        site: site.clone(),
        measures: vec![Measure::Named(NamedMeasure::Pageviews)],
        dimensions: Vec::new(),
        filter: None,
        range: Range::Relative {
            unit: RangeUnit::Day,
            count: 30,
        },
        timezone: timezone.to_owned(),
        granularity: Granularity::Day,
        compare: holocron_protocol::exploration::Compare::None,
        metric_version: holocron_analytics::METRIC_VERSION,
        sort: Vec::new(),
        page: DEFAULT_PAGE,
        visualization: Visualization::Table,
        report: None,
    }
}

fn checked(state: &ExplorationState) -> Result<(), String> {
    explore::validate(state, &[])
        .map(drop)
        .map_err(|refusal| format!("planner_refused:{}:{}", refusal.field, refusal.message))
}

/// A path or event step of a funnel, goal or attribution.
fn step(ids: &mut Ids, kind: &str, value: &str) -> Result<Condition, String> {
    let field = match kind {
        "path" | "url" => DimensionField::Path,
        "event" => DimensionField::EventName,
        other => return Err(format!("step_type_not_supported:{other}")),
    };
    condition(
        ids,
        Field::Dimension(field),
        "eq",
        Some(Cond::String(value.to_owned())),
    )
}

/// An Umami report of `kind` as a saved exploration state.
pub fn report_state(
    site: &SiteId,
    timezone: &str,
    kind: &str,
    params: &Value,
) -> Mapped<ExplorationState> {
    let mut ids = Ids(0);
    let mut lost: Vec<String> = ["startDate", "endDate", "dateRange"]
        .into_iter()
        .filter(|key| !params[*key].is_null())
        .map(|key| format!("{key} (opens as the last 30 days)"))
        .collect();
    let mut state = base(site, timezone);
    state.measures.clear();
    let report = match kind {
        "funnel" => {
            let steps = params["steps"]
                .as_array()
                .ok_or("funnel_without_steps")?
                .iter()
                .map(|s| {
                    let mut items = vec![step(
                        &mut ids,
                        s["type"].as_str().unwrap_or_default(),
                        s["value"].as_str().unwrap_or_default(),
                    )?];
                    items.extend(property_filters(
                        &mut ids,
                        s["filters"].as_array().map_or(&[], |v| v),
                        PropertyScope::Event,
                    )?);
                    Ok(group(GroupOp::And, items))
                })
                .collect::<Result<_, String>>()?;
            let minutes = params["window"].as_f64().unwrap_or(60.0);
            state.visualization = Visualization::Funnel;
            ReportDefinition::Funnel(FunnelReport {
                v: REPORT_VERSION,
                steps,
                window_seconds: (minutes * 60.0).round() as u32,
                scope: IdentityScope::Daily,
                group_by: None,
            })
        }
        "journey" => {
            for key in ["startStep", "endStep"] {
                if !params[key].is_null() {
                    lost.push(key.to_owned());
                }
            }
            state.visualization = Visualization::Journey;
            ReportDefinition::Journey(JourneyReport {
                v: REPORT_VERSION,
                value: match params["eventType"].as_i64() {
                    Some(2) => JourneyValue::EventName,
                    _ => JourneyValue::Path,
                },
                length: params["steps"]
                    .as_u64()
                    .or_else(|| params["steps"].as_str().and_then(|s| s.parse().ok()))
                    .unwrap_or(3) as u8,
                scope: PopulationScope::Session,
            })
        }
        "retention" => {
            lost.push("population (consented profiles only, imported history excluded)".into());
            state.visualization = Visualization::Cohort;
            ReportDefinition::Retention(RetentionReport {
                v: REPORT_VERSION,
                intervals: 31,
            })
        }
        "goal" | "goals" => ReportDefinition::Goals(GoalsReport {
            v: REPORT_VERSION,
            goals: vec![goal(&mut ids, params)?],
        }),
        "attribution" => {
            if !params["currency"].is_null() {
                lost.push("currency".into());
            }
            ReportDefinition::Attribution(AttributionReport {
                v: REPORT_VERSION,
                model: match params["model"].as_str() {
                    Some("first-click") => AttributionModel::FirstTouch,
                    _ => AttributionModel::LastTouch,
                },
                lookback_days: 30,
                scope: IdentityScope::Daily,
                touch_field: TouchField::Channel,
                conversions: vec![Conversion::Goal(GoalConversion {
                    goal: GoalDefinition {
                        predicate: group(
                            GroupOp::And,
                            vec![step(
                                &mut ids,
                                params["type"].as_str().unwrap_or_default(),
                                params["step"].as_str().unwrap_or_default(),
                            )?],
                        ),
                        scope: PopulationScope::Session,
                        counting: GoalCounting::Once,
                        goal_ref: None,
                    },
                })],
            })
        }
        other => return Err(format!("report_type_not_supported:{other}")),
    };
    state.report = Some(report);
    checked(&state)?;
    Ok((state, lost))
}

fn goal(ids: &mut Ids, params: &Value) -> Result<GoalDefinition, String> {
    Ok(GoalDefinition {
        predicate: group(
            GroupOp::And,
            vec![step(
                ids,
                params["type"].as_str().unwrap_or_default(),
                params["value"].as_str().unwrap_or_default(),
            )?],
        ),
        scope: PopulationScope::Session,
        counting: GoalCounting::Once,
        goal_ref: None,
    })
}

/// The state a public site share opens at: page views and visitor-days by page, last 30 days.
pub fn share_state(site: &SiteId, timezone: &str) -> ExplorationState {
    let mut state = base(site, timezone);
    state.measures = vec![
        Measure::Named(NamedMeasure::Pageviews),
        Measure::Named(NamedMeasure::VisitorDays),
    ];
    state.dimensions = vec![Field::Dimension(DimensionField::Path)];
    state
}

/// Limits of an imported public share: the measures and fields of the Umami sections it
/// showed, up to a year, with viewer filters allowed as upstream allowed them. `sections` is the
/// 3.x share `parameters`; `None` is a 2.x website share, which showed the whole dashboard.
pub fn share_limits(sections: Option<&Value>) -> (ExplorationLimits, Vec<String>) {
    use DimensionField::*;
    use NamedMeasure::*;
    let shown = |name: &str| sections.is_none_or(|s| s[name].as_bool().unwrap_or(false));
    let mut measures = vec![
        Pageviews,
        Visitors,
        VisitorDays,
        Sessions,
        BounceRate,
        SpanMean,
    ];
    let mut dimensions = vec![
        Path,
        Title,
        Hostname,
        ReferrerDomain,
        Channel,
        Browser,
        Os,
        Device,
        Country,
        Region,
        City,
        Language,
        Screen,
    ];
    if shown("events") {
        measures.push(Events);
        dimensions.push(EventName);
    }
    if shown("utm") {
        dimensions.extend([UtmSource, UtmMedium, UtmCampaign, UtmContent, UtmTerm]);
    }
    if shown("revenue") {
        measures.extend([RevenueNet, RevenueGross, RevenueRefunds, RevenueCount]);
        dimensions.push(Currency);
    }
    if shown("performance") {
        dimensions.push(PerfMetric);
    }
    let mut lost: Vec<String> = sections
        .and_then(Value::as_object)
        .map(|s| {
            s.iter()
                .filter(|(name, on)| {
                    on.as_bool() == Some(true)
                        && ![
                            "overview",
                            "events",
                            "utm",
                            "revenue",
                            "performance",
                            "compare",
                        ]
                        .contains(&name.as_str())
                })
                .map(|(name, _)| format!("section {name}"))
                .collect()
        })
        .unwrap_or_default();
    if sections.is_none() {
        lost.push("realtime".into());
    }
    (
        ExplorationLimits {
            measures,
            dimensions,
            properties: Vec::new(),
            range_max_days: 366,
            pinned_filter: None,
            narrowing_allowed: true,
        },
        lost,
    )
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn site() -> SiteId {
        SiteId::new("00000000-0000-4000-8000-000000000201".to_owned())
    }

    #[test]
    fn segment_filters_map_to_one_checked_group() {
        let params = json!({ "filters": [
            { "name": "device", "operator": "eq", "value": "desktop" },
            { "name": "utmSource", "operator": "c", "value": "news" }
        ], "match": "any", "dateRange": "30d" });
        let (filter, lost) = segment_filter(&params, &site()).unwrap();
        assert_eq!(filter.op, GroupOp::Or);
        assert_eq!(filter.items.len(), 2);
        assert_eq!(lost, ["dateRange"]);
        let regex = json!({ "filters": [{ "name": "path", "operator": "re", "value": "^/a" }] });
        assert_eq!(
            segment_filter(&regex, &site()).unwrap_err(),
            "operator_not_supported:re"
        );
    }

    #[test]
    fn funnels_of_both_versions_and_goals_become_checked_reports() {
        for kind in ["path", "url"] {
            let params = json!({ "window": 60, "steps": [
                { "type": kind, "value": "/" }, { "type": "event", "value": "purchase" }
            ] });
            let (state, lost) = report_state(&site(), "UTC", "funnel", &params).unwrap();
            let Some(ReportDefinition::Funnel(funnel)) = state.report else {
                panic!("not a funnel");
            };
            assert_eq!((funnel.steps.len(), funnel.window_seconds), (2, 3600));
            assert!(lost.is_empty());
        }
        let goal = json!({ "type": "event", "value": "signup", "startDate": "2026-01-01" });
        let (_, lost) = report_state(&site(), "UTC", "goal", &goal).unwrap();
        assert_eq!(lost.len(), 1);
        assert!(report_state(&site(), "UTC", "utm", &json!({})).is_err());
    }
}
