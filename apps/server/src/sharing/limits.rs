//! The widening check: whether a viewer's state stays inside its share (R-SHARE-NOWIDEN).
//!
//! An exploration share bounds measures, fields, the span read and the filter. A board share
//! bounds the viewer to the board's current cards on the share's sites: the state must be one
//! of those cards, with only its range and page changed, as the board's range picker does.

use chrono::NaiveDate;
use holocron_protocol::errors::ErrorCode;
use holocron_protocol::exploration::{
    Compare, DimensionField, ExplorationState, Field, Group, GroupOp, Measure, NamedMeasure, Node,
    Panel, PropertyField, Range, RangeUnit, Sort, SortKey,
};
use holocron_protocol::ids::BoardId;
use holocron_protocol::limits::TYPICAL_WEEKDAY_WEEKS;
use holocron_protocol::operations::sharing::{ExplorationLimits, ShareTarget};
use sqlx::SqliteConnection;

use super::ShareViewer;
use crate::error::ApiError;

fn exceeded() -> ApiError {
    ApiError::new(ErrorCode::ShareScopeExceeded)
}

fn require(inside: bool) -> Result<(), ApiError> {
    if inside { Ok(()) } else { Err(exceeded()) }
}

/// `share_scope_exceeded` when the state or a panel reads outside the share: a longer span
/// (current plus comparison over `range_max_days`), a dropped or OR-widened pinned filter, a
/// measure, dimension, property or sort key outside the limits, another report, or for board
/// shares anything but a card of the board.
pub async fn admit_exploration(
    conn: &mut SqliteConnection,
    viewer: &ShareViewer,
    state: &ExplorationState,
    panels: &[Panel],
) -> Result<(), ApiError> {
    match &viewer.target {
        ShareTarget::Exploration {
            state: shared,
            limits,
            ..
        } => admit_state(shared, limits, state, panels),
        ShareTarget::Board { board, .. } => {
            require(panels.is_empty())?;
            require(is_card(conn, board, state).await?)
        }
    }
}

/// `share_scope_exceeded` when value suggestions would read a field outside the share's limits.
/// Board shares never edit filters, so they read no suggestions. Returns the shared state and
/// limits the suggestions must stay inside.
pub fn admit_field<'a>(
    viewer: &'a ShareViewer,
    field: &Field,
) -> Result<(&'a ExplorationState, &'a ExplorationLimits), ApiError> {
    match &viewer.target {
        ShareTarget::Exploration { state, limits, .. } if granted(limits, field) => {
            Ok((state, limits))
        }
        _ => Err(exceeded()),
    }
}

/// The checks of an exploration share, also used to test a new share against its own state.
pub(super) fn admit_state(
    shared: &ExplorationState,
    limits: &ExplorationLimits,
    state: &ExplorationState,
    panels: &[Panel],
) -> Result<(), ApiError> {
    require(state.report == shared.report)?;
    require(cut_inside(
        limits,
        &state.measures,
        &state.dimensions,
        &state.sort,
    ))?;
    for panel in panels {
        require(cut_inside(
            limits,
            &panel.measures,
            &panel.dimensions,
            &panel.sort,
        ))?;
    }
    require(filter_inside(limits, state.filter.as_ref()))?;
    let span = span_days(&state.range).ok_or_else(exceeded)?;
    let read = match state.compare {
        Compare::None => span,
        Compare::PreviousPeriod => span * 2,
        Compare::TypicalWeekday => span + 7 * TYPICAL_WEEKDAY_WEEKS,
    };
    require(read <= u32::from(limits.range_max_days))
}

fn cut_inside(
    limits: &ExplorationLimits,
    measures: &[Measure],
    dimensions: &[Field],
    sort: &[Sort],
) -> bool {
    measures.iter().all(|m| measure_granted(limits, m))
        && dimensions.iter().all(|f| granted(limits, f))
        && sort.iter().all(|s| match &s.by {
            SortKey::Measure(m) => measure_granted(limits, m),
            SortKey::Field(f) => granted(limits, f),
        })
}

fn granted(limits: &ExplorationLimits, field: &Field) -> bool {
    match field {
        Field::Dimension(dimension) => limits.dimensions.contains(dimension),
        Field::Property(property) => limits.properties.contains(property),
    }
}

fn measure_granted(limits: &ExplorationLimits, measure: &Measure) -> bool {
    match measure {
        Measure::Named(named) => limits.measures.contains(named),
        Measure::Property(stat) => limits.properties.contains(&stat.field),
    }
}

/// The filter must keep the pinned group under an AND at its root, either as one item or, for
/// an AND pin, item by item. Anything next to it narrows, which needs `narrowing_allowed` and
/// granted fields only.
fn filter_inside(limits: &ExplorationLimits, filter: Option<&Group>) -> bool {
    let added: Vec<&Node> = match (&limits.pinned_filter, filter) {
        (None, None) => return true,
        (Some(_), None) => return false,
        (Some(pinned), Some(root)) if pinned == root => return true,
        (None, Some(root)) => return limits.narrowing_allowed && group_granted(limits, root),
        (Some(pinned), Some(root)) => {
            if root.op != GroupOp::And {
                return false;
            }
            let mut rest: Vec<&Node> = root.items.iter().collect();
            let whole = Node::Group(pinned.clone());
            if let Some(index) = rest.iter().position(|node| **node == whole) {
                rest.remove(index);
            } else if pinned.op == GroupOp::And {
                for item in &pinned.items {
                    let Some(index) = rest.iter().position(|node| *node == item) else {
                        return false;
                    };
                    rest.remove(index);
                }
            } else {
                return false;
            }
            rest
        }
    };
    added.is_empty()
        || (limits.narrowing_allowed && added.iter().all(|node| node_granted(limits, node)))
}

fn group_granted(limits: &ExplorationLimits, group: &Group) -> bool {
    group.items.iter().all(|node| node_granted(limits, node))
}

fn node_granted(limits: &ExplorationLimits, node: &Node) -> bool {
    match node {
        Node::Group(group) => group_granted(limits, group),
        Node::Cond(condition) => granted(limits, &condition.field),
    }
}

/// The most days a range can cover. Weeks count 7 days and months 31, so a relative range is
/// never shorter here than on any calendar. `all` has no bound, so no share grants it.
fn span_days(range: &Range) -> Option<u32> {
    match range {
        Range::Relative { unit, count } => {
            let unit = match unit {
                RangeUnit::Day => 1,
                RangeUnit::Week => 7,
                RangeUnit::Month => 31,
            };
            Some(u32::from(*count) * unit)
        }
        Range::Days { start, end } => {
            let start = NaiveDate::parse_from_str(start, "%Y-%m-%d").ok()?;
            let end = NaiveDate::parse_from_str(end, "%Y-%m-%d").ok()?;
            u32::try_from((end - start).num_days() + 1).ok()
        }
        Range::All => None,
    }
}

/// Whether `state` is a current card of `board` with at most its range and page changed.
async fn is_card(
    conn: &mut SqliteConnection,
    board: &BoardId,
    state: &ExplorationState,
) -> Result<bool, ApiError> {
    let cards: Vec<String> =
        sqlx::query_scalar("SELECT state FROM board_cards WHERE board_id = ? AND site_id = ?")
            .bind(board.as_str())
            .bind(state.site.as_str())
            .fetch_all(conn)
            .await?;
    for card in cards {
        let card: ExplorationState = serde_json::from_str(&card).map_err(ApiError::internal)?;
        let candidate = ExplorationState {
            range: card.range.clone(),
            page: card.page,
            ..state.clone()
        };
        if candidate == card {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Every measure, dimension and property a state reads.
#[derive(Default)]
pub(super) struct StateFields {
    pub(super) measures: Vec<NamedMeasure>,
    pub(super) dimensions: Vec<DimensionField>,
    pub(super) properties: Vec<PropertyField>,
}

impl StateFields {
    fn field(&mut self, field: &Field) {
        match field {
            Field::Dimension(dimension) => self.dimensions.push(*dimension),
            Field::Property(property) => self.properties.push(property.clone()),
        }
    }
}

/// What a state reads, so new limits can be checked to be no wider than the shared state.
pub(super) fn state_fields(state: &ExplorationState) -> StateFields {
    let mut fields = StateFields::default();
    for measure in &state.measures {
        match measure {
            Measure::Named(named) => fields.measures.push(*named),
            Measure::Property(stat) => fields.field(&Field::Property(stat.field.clone())),
        }
    }
    for field in &state.dimensions {
        fields.field(field);
    }
    for sort in &state.sort {
        if let SortKey::Field(field) = &sort.by {
            fields.field(field);
        }
    }
    let mut stack: Vec<&Group> = state.filter.iter().collect();
    while let Some(group) = stack.pop() {
        for node in &group.items {
            match node {
                Node::Group(inner) => stack.push(inner),
                Node::Cond(condition) => fields.field(&condition.field),
            }
        }
    }
    fields
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(json: serde_json::Value) -> ExplorationState {
        let mut base = serde_json::json!({
            "v": 1, "site": "s", "measures": ["pageviews"], "dimensions": ["path"],
            "filter": {"kind": "group", "op": "and", "items": [
                {"kind": "cond", "id": "c1", "field": "device", "cmp": "eq", "value": {"t": "string", "v": "mobile"}}
            ]},
            "range": {"kind": "relative", "unit": "day", "count": 30},
            "timezone": "UTC", "granularity": "day", "compare": "none", "metric_version": 1,
            "sort": [], "page": {"size": 50, "index": 0}, "visualization": "table"
        });
        base.as_object_mut()
            .unwrap()
            .extend(json.as_object().unwrap().clone());
        serde_json::from_value(base).unwrap()
    }

    fn limits() -> ExplorationLimits {
        let shared = state(serde_json::json!({}));
        ExplorationLimits {
            measures: vec![NamedMeasure::Pageviews, NamedMeasure::VisitorDays],
            dimensions: vec![DimensionField::Path, DimensionField::Country],
            properties: Vec::new(),
            range_max_days: 30,
            pinned_filter: shared.filter,
            narrowing_allowed: true,
        }
    }

    fn admit(json: serde_json::Value) -> bool {
        admit_state(&state(serde_json::json!({})), &limits(), &state(json), &[]).is_ok()
    }

    #[test]
    fn the_shared_state_and_narrower_ones_pass() {
        assert!(admit(serde_json::json!({})));
        assert!(admit(
            serde_json::json!({"range": {"kind": "relative", "unit": "day", "count": 15}, "compare": "previous_period"})
        ));
        assert!(admit(
            serde_json::json!({"filter": {"kind": "group", "op": "and", "items": [
                {"kind": "cond", "id": "c1", "field": "device", "cmp": "eq", "value": {"t": "string", "v": "mobile"}},
                {"kind": "cond", "id": "c2", "field": "country", "cmp": "eq", "value": {"t": "string", "v": "AT"}}
            ]}})
        ));
    }

    #[test]
    fn widening_is_refused() {
        let pinned = serde_json::json!({"kind": "cond", "id": "c1", "field": "device", "cmp": "eq", "value": {"t": "string", "v": "mobile"}});
        for widened in [
            serde_json::json!({"range": {"kind": "relative", "unit": "day", "count": 16}, "compare": "previous_period"}),
            serde_json::json!({"range": {"kind": "days", "start": "2026-01-01", "end": "2026-03-01"}}),
            serde_json::json!({"filter": null}),
            serde_json::json!({"filter": {"kind": "group", "op": "or", "items": [pinned,
                {"kind": "cond", "id": "c2", "field": "country", "cmp": "eq", "value": {"t": "string", "v": "AT"}}]}}),
            serde_json::json!({"measures": ["revenue_net"]}),
            serde_json::json!({"dimensions": ["city"]}),
            serde_json::json!({"dimensions": [{"scope": "event", "key": "plan"}]}),
        ] {
            assert!(!admit(widened.clone()), "{widened}");
        }
    }
}
