//! Report queries for agents (decision 0022): named blocks compiled onto exploration states.
//!
//! [`budget`] refuses a request over the block and sub-query limits before anything else.
//! [`plan`] checks every block up front: names, ranges, refs and derived operands, and the
//! compiled state through [`validate_with`] with the report query limits, refs filled with a
//! placeholder. [`run`] then runs the blocks in order inside one snapshot. Each run compiles
//! its state again with the refs resolved from earlier blocks and goes through [`explore`],
//! so a block returns the numbers of the equivalent exploration. [`table`] flattens results
//! into typed rows and [`derive`] adds derived columns.

mod derive;
mod table;

use std::collections::HashMap;

use holocron_protocol::errors::{ErrorCode, FieldErrorCode};
use holocron_protocol::exploration::{
    Compare, CondKind, Condition, DimensionField, ExplorationState, Field, Granularity, Group,
    GroupKind, GroupOp, Node, Operator, Page, Range, ReportDefinition, Value, Visualization,
};
use holocron_protocol::ids::SiteId;
use holocron_protocol::limits::{
    PAGE_SIZE_MAX, REPORT_QUERY_BLOCKS_MAX, REPORT_QUERY_CUT_LIMIT_DEFAULT,
    REPORT_QUERY_CUT_LIMIT_MAX, REPORT_QUERY_DERIVED_MAX, REPORT_QUERY_FOR_EACH_MAX,
    REPORT_QUERY_LIST_ITEMS_MAX, REPORT_QUERY_NAME_MAX_BYTES, REPORT_QUERY_SUBQUERIES_MAX,
};
use holocron_protocol::report_query::{
    Block, BlockBody, BlockGroup, BlockNode, BlockResult, BlockValue, ColumnRole, ForEachValues,
    Note, NoteCode, REPORT_QUERY_VERSION, RefSource, ReportCell, ReportQuery,
};
use holocron_protocol::retention::Retention;
use holocron_storage::{Id, Snapshot, duckdb};

use crate::METRIC_VERSION;
use crate::explore::{Context, DEFAULT_PAGE, Refusal, StateLimits, explore, validate_with};
use table::{Layout, Part};

/// The ID of the condition a `for_each` run adds to the block's filter.
const FOR_EACH_ID: &str = "for_each";

/// What a block's site adds to a run: its analytical ID, hostnames, retention and timezone.
pub struct Site {
    pub analytics: Id,
    pub hostnames: Vec<String>,
    pub retention: Retention,
    /// The site's IANA timezone, for blocks and queries that name none.
    pub timezone: String,
}

/// A checked report query, ready to [`run`].
pub struct Plan {
    query: ReportQuery,
    blocks: Vec<Checked>,
}

struct Checked {
    range: Range,
    timezone: String,
    layout: Layout,
}

/// The sub-queries of a request, one per block and one per `for_each` value. Refuses more than
/// 16 blocks or 200 sub-queries with `report_too_large`.
pub fn budget(query: &ReportQuery) -> Result<usize, Refusal> {
    if query.blocks.is_empty() {
        return Err(Refusal::new(
            ErrorCode::InvalidArguments,
            "blocks",
            FieldErrorCode::Required,
            "a report query has 1 to 16 blocks",
        ));
    }
    if query.blocks.len() > REPORT_QUERY_BLOCKS_MAX {
        return Err(Refusal::too_many(
            ErrorCode::ReportTooLarge,
            "blocks",
            REPORT_QUERY_BLOCKS_MAX,
            "a report query has at most 16 blocks; split it into several requests",
        ));
    }
    let sub_queries = query
        .blocks
        .iter()
        .map(|block| match &block.for_each {
            None => 1,
            Some(for_each) => match &for_each.values {
                ForEachValues::Values(values) => values.len(),
                ForEachValues::Ref(r) => ref_bound(query, &r.source),
            },
        })
        .sum();
    if sub_queries > REPORT_QUERY_SUBQUERIES_MAX {
        return Err(Refusal::too_many(
            ErrorCode::ReportTooLarge,
            "blocks",
            REPORT_QUERY_SUBQUERIES_MAX,
            format!(
                "the blocks need {sub_queries} sub-queries, counting each for_each value; at most 200 run in one request"
            ),
        ));
    }
    Ok(sub_queries)
}

/// The most values a `for_each` ref can return, known before the referenced block ran: the
/// ref's `limit`, the rows of a plain cut (its `limit`) or the fan-out cap, whichever is lowest.
/// The real count never exceeds it, so a request within the budget stays within it once refs
/// resolve and no recheck is needed.
fn ref_bound(query: &ReportQuery, source: &RefSource) -> usize {
    let rows = query
        .blocks
        .iter()
        .find(|b| b.name == source.block)
        .and_then(|b| match &b.body {
            BlockBody::Cut(cut) if b.for_each.is_none() && !cut.series => {
                Some(cut.limit.unwrap_or(REPORT_QUERY_CUT_LIMIT_DEFAULT) as usize)
            }
            _ => None,
        })
        .unwrap_or(REPORT_QUERY_FOR_EACH_MAX);
    let declared = source.limit.map_or(usize::MAX, |limit| limit as usize);
    rows.min(declared).min(REPORT_QUERY_FOR_EACH_MAX)
}

/// Checks a report query against the sites its blocks read. `sites` holds every block's site.
pub fn plan(query: ReportQuery, sites: &HashMap<SiteId, Site>) -> Result<Plan, Refusal> {
    budget(&query)?;
    if query.v != REPORT_QUERY_VERSION {
        return Err(Refusal::invalid(
            "v",
            format!("report query version {} is not supported; send 1", query.v),
        ));
    }
    let mut blocks: Vec<Checked> = Vec::with_capacity(query.blocks.len());
    for (i, block) in query.blocks.iter().enumerate() {
        let path = format!("blocks.{i}");
        if !valid_name(&block.name) {
            return Err(Refusal::invalid(
                format!("{path}.name"),
                "block names match ^[a-z][a-z0-9_]{0,31}$",
            ));
        }
        let earlier = &query.blocks[..i];
        if earlier.iter().any(|b| b.name == block.name) {
            return Err(Refusal::invalid(
                format!("{path}.name"),
                "another block has this name",
            ));
        }
        let site = sites
            .get(&block.site)
            .ok_or_else(|| Refusal::invalid(format!("{path}.site"), "unknown site"))?;
        let range = block
            .range
            .clone()
            .or_else(|| query.range.clone())
            .ok_or_else(|| {
                Refusal::new(
                    ErrorCode::InvalidArguments,
                    format!("{path}.range"),
                    FieldErrorCode::Required,
                    "give this block a range, or the query one for every block",
                )
            })?;
        let timezone = block
            .timezone
            .clone()
            .or_else(|| query.timezone.clone())
            .unwrap_or_else(|| site.timezone.clone());

        check_body(&path, block)?;
        let refs = |source: &RefSource, at: String, max: usize| {
            check_ref(&query.blocks[..i], &blocks, source, at, max)
        };
        if let Some(filter) = &block.filter {
            check_filter_refs(&path, filter, &refs)?;
        }
        if let Some(for_each) = &block.for_each {
            let at = format!("{path}.for_each.values");
            match &for_each.values {
                ForEachValues::Values(values)
                    if values.is_empty() || values.len() > REPORT_QUERY_FOR_EACH_MAX =>
                {
                    return Err(Refusal::too_many(
                        ErrorCode::InvalidArguments,
                        at,
                        REPORT_QUERY_FOR_EACH_MAX,
                        "for_each takes 1 to 50 values; prefer a dimension or a funnel group_by for more",
                    ));
                }
                ForEachValues::Values(_) => {}
                ForEachValues::Ref(r) => refs(&r.source, at, REPORT_QUERY_FOR_EACH_MAX)?,
            }
            if let BlockBody::Cut(cut) = &block.body
                && cut.dimensions.contains(&for_each.field)
            {
                return Err(Refusal::invalid(
                    format!("{path}.for_each.field"),
                    "this field is already a dimension of the cut",
                ));
            }
        }

        let layout = Layout::of(block);
        derive::check(&path, block, &layout, earlier, &blocks)?;

        // The compiled states with placeholders for refs: refuses everything the stored data
        // cannot change before any block runs.
        let filter = block
            .filter
            .as_ref()
            .map(|g| group(g, &mut |_| vec![Value::String(String::new())]));
        let values = match block.for_each.as_ref().map(|f| &f.values) {
            None => vec![None],
            Some(ForEachValues::Values(values)) => values.iter().cloned().map(Some).collect(),
            Some(ForEachValues::Ref(_)) => {
                vec![block.for_each.as_ref().map(|f| placeholder(&f.field))]
            }
        };
        for value in values {
            let for_each = block
                .for_each
                .as_ref()
                .zip(value)
                .map(|(f, v)| (&f.field, v));
            let state = state(block, &range, &timezone, filter.clone(), for_each);
            validate_with(&state, &[], StateLimits::REPORT_QUERY).map_err(|r| at(i, r))?;
        }
        blocks.push(Checked {
            range,
            timezone,
            layout,
        });
    }
    Ok(Plan { query, blocks })
}

/// Runs a checked plan's blocks in order on one snapshot. The outer error is an engine failure;
/// the inner one a refusal that needed the stored data, such as an unknown property key.
pub fn run(
    snapshot: &Snapshot,
    plan: &Plan,
    sites: &HashMap<SiteId, Site>,
    now_ms: i64,
) -> duckdb::Result<Result<Vec<BlockResult>, Refusal>> {
    let mut done = Vec::with_capacity(plan.blocks.len());
    for (i, (block, checked)) in plan.query.blocks.iter().zip(&plan.blocks).enumerate() {
        let site = &sites[&block.site];
        let ctx = Context {
            site: site.analytics,
            now_ms,
            hostnames: &site.hostnames,
            retention: site.retention,
        };
        match run_block(snapshot, i, block, checked, &ctx, &done)? {
            Ok(result) => done.push(result),
            Err(refusal) => return Ok(Err(refusal)),
        }
    }
    Ok(Ok(done))
}

fn run_block(
    snapshot: &Snapshot,
    i: usize,
    block: &Block,
    checked: &Checked,
    ctx: &Context,
    done: &[BlockResult],
) -> duckdb::Result<Result<BlockResult, Refusal>> {
    let path = format!("blocks.{i}");
    let mut empty = false;
    let filter = block.filter.as_ref().map(|g| {
        group(g, &mut |source| {
            let values = resolve(done, source, REPORT_QUERY_LIST_ITEMS_MAX);
            empty |= values.is_empty();
            values
        })
    });
    let values = match block.for_each.as_ref().map(|f| &f.values) {
        None => vec![None],
        Some(ForEachValues::Values(values)) => values.iter().cloned().map(Some).collect(),
        Some(ForEachValues::Ref(r)) => resolve(done, &r.source, REPORT_QUERY_FOR_EACH_MAX)
            .into_iter()
            .map(Some)
            .collect(),
    };
    let mut result = BlockResult {
        name: block.name.clone(),
        site: block.site.clone(),
        resolved: None,
        timezone: checked.timezone.clone(),
        freshness: None,
        columns: Vec::new(),
        rows: Vec::new(),
        totals: None,
        notes: Vec::new(),
    };
    if empty || values.is_empty() {
        result.columns = checked.layout.columns(&[]);
        result.notes.push(Note {
            code: NoteCode::RefEmpty,
            message: "a ref found no values in its block, so this block did not run".into(),
            path: Some(path),
        });
        return Ok(Ok(result));
    }
    let limit = match &block.body {
        BlockBody::Cut(cut) => cut.limit.unwrap_or(REPORT_QUERY_CUT_LIMIT_DEFAULT),
        BlockBody::Report(_) => 0,
    };
    for value in values {
        let for_each = block
            .for_each
            .as_ref()
            .zip(value.clone())
            .map(|(f, v)| (&f.field, v));
        let state = state(
            block,
            &checked.range,
            &checked.timezone,
            filter.clone(),
            for_each,
        );
        let mut query = match validate_with(&state, &[], StateLimits::REPORT_QUERY) {
            Ok(query) => query,
            Err(refusal) => return Ok(Err(at(i, refusal))),
        };
        if let BlockBody::Cut(_) = block.body {
            // Up to 1,000 rows from the first page, inside the 10,000 addressable rows.
            query.main.size = limit;
        }
        let explored = match explore(snapshot, &query, ctx)? {
            Ok(explored) => explored,
            Err(refusal) => return Ok(Err(at(i, refusal))),
        };
        let Part {
            mut rows,
            mut totals,
            notes,
        } = table::flatten(block, &explored);
        derive::apply(block, &checked.layout, done, &mut rows, &mut totals);
        let label = value.as_ref().map(|v| {
            let field = &block
                .for_each
                .as_ref()
                .expect("a value comes from for_each")
                .field;
            format!("{} = {}: ", table::field_name(field), table::value_text(v))
        });
        if let Some(value) = &value {
            let cell = ReportCell::Key(table::cell_of(value));
            for row in &mut rows {
                row.insert(0, cell.clone());
            }
        }
        result
            .notes
            .extend(notes.into_iter().map(|(code, message)| Note {
                code,
                message: format!("{}{message}", label.as_deref().unwrap_or_default()),
                path: Some(path.clone()),
            }));
        result.rows.extend(rows);
        if block.for_each.is_none() {
            result.totals = totals;
        }
        if result.resolved.is_none() {
            result.resolved = Some(explored.resolved);
            result.freshness = Some(explored.freshness);
        }
    }
    result.columns = checked.layout.columns(&result.rows);
    Ok(Ok(result))
}

/// Refusals of the compiled state, named by their place in the block.
fn at(block: usize, refusal: Refusal) -> Refusal {
    let field = &refusal.field;
    let part = if field == &format!("filter.{FOR_EACH_ID}") {
        "for_each".to_owned()
    } else if field.starts_with("filter") || field == "range" || field == "timezone" {
        field.clone()
    } else if field == "page" {
        "body.limit".to_owned()
    } else {
        format!("body.{field}")
    };
    Refusal {
        field: format!("blocks.{block}.{part}"),
        ..refusal
    }
}

fn valid_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    name.len() <= REPORT_QUERY_NAME_MAX_BYTES
        && bytes.next().is_some_and(|b| b.is_ascii_lowercase())
        && bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

fn check_body(path: &str, block: &Block) -> Result<(), Refusal> {
    match &block.body {
        BlockBody::Cut(cut) => {
            let limit = cut.limit.unwrap_or(REPORT_QUERY_CUT_LIMIT_DEFAULT);
            if !(1..=REPORT_QUERY_CUT_LIMIT_MAX).contains(&limit) {
                return Err(Refusal::too_many(
                    ErrorCode::InvalidArguments,
                    format!("{path}.body.limit"),
                    REPORT_QUERY_CUT_LIMIT_MAX as usize,
                    "a cut returns 1 to 1,000 rows",
                ));
            }
            if cut.series && !cut.dimensions.is_empty() {
                return Err(Refusal::invalid(
                    format!("{path}.body.series"),
                    "a series has no dimensions; drop them or the series",
                ));
            }
        }
        BlockBody::Report(report) => {
            if let ReportDefinition::SessionList(_) = report.report {
                return Err(Refusal::new(
                    ErrorCode::ReportFieldUnsupported,
                    format!("{path}.body.report.kind"),
                    FieldErrorCode::InvalidValue,
                    "report queries answer aggregates only; session lists are not available",
                ));
            }
            if !block.derive.is_empty() {
                return Err(Refusal::invalid(
                    format!("{path}.derive"),
                    "derived columns apply to cuts only",
                ));
            }
        }
    }
    if block.derive.len() > REPORT_QUERY_DERIVED_MAX {
        return Err(Refusal::too_many(
            ErrorCode::InvalidArguments,
            format!("{path}.derive"),
            REPORT_QUERY_DERIVED_MAX,
            "a block has at most 8 derived columns",
        ));
    }
    Ok(())
}

/// A ref names an earlier block and one of its `dimension` or `for_each` columns.
fn check_ref(
    earlier: &[Block],
    checked: &[Checked],
    source: &RefSource,
    at: String,
    max: usize,
) -> Result<(), Refusal> {
    let Some(j) = earlier.iter().position(|b| b.name == source.block) else {
        return Err(Refusal::invalid(
            at,
            format!("`{}` names no earlier block", source.block),
        ));
    };
    let keyed = checked[j].layout.specs().iter().any(|spec| {
        spec.name == source.column
            && matches!(spec.role, ColumnRole::Dimension | ColumnRole::ForEach)
    });
    if !keyed {
        return Err(Refusal::invalid(
            at,
            format!(
                "block `{}` has no dimension or for_each column `{}`",
                source.block, source.column
            ),
        ));
    }
    if source
        .limit
        .is_some_and(|limit| limit == 0 || limit as usize > max)
    {
        return Err(Refusal::too_many(
            ErrorCode::InvalidArguments,
            at,
            max,
            format!("a ref here takes 1 to {max} values"),
        ));
    }
    Ok(())
}

fn check_filter_refs(
    path: &str,
    group: &BlockGroup,
    refs: &impl Fn(&RefSource, String, usize) -> Result<(), Refusal>,
) -> Result<(), Refusal> {
    for item in &group.items {
        match item {
            BlockNode::Group(inner) => check_filter_refs(path, inner, refs)?,
            BlockNode::Cond(c) => {
                if let Some(BlockValue::Ref(r)) = &c.value {
                    let at = format!("{path}.filter.{}", c.id);
                    if !matches!(c.cmp, Operator::In | Operator::NotIn) {
                        return Err(Refusal::invalid(at, "a ref fills in and not_in lists only"));
                    }
                    refs(&r.source, at, REPORT_QUERY_LIST_ITEMS_MAX)?;
                }
            }
        }
    }
    Ok(())
}

/// The distinct set values of an earlier block's column, in row order, at most `limit` or `max`.
fn resolve(done: &[BlockResult], source: &RefSource, max: usize) -> Vec<Value> {
    let block = done
        .iter()
        .find(|b| b.name == source.block)
        .expect("refs name earlier blocks, checked in plan");
    let column = block
        .columns
        .iter()
        .position(|c| c.name == source.column)
        .expect("refs name a column, checked in plan");
    let limit = source.limit.map_or(max, |limit| limit as usize);
    let mut values: Vec<Value> = Vec::new();
    for row in &block.rows {
        if values.len() == limit {
            break;
        }
        if let ReportCell::Key(cell) = &row[column]
            && let Some(value) = table::value_of(cell)
            && !values.contains(&value)
        {
            values.push(value);
        }
    }
    values
}

/// A block filter as an exploration filter, refs filled in by `resolve`.
fn group(g: &BlockGroup, resolve: &mut dyn FnMut(&RefSource) -> Vec<Value>) -> Group {
    Group {
        kind: g.kind,
        op: g.op,
        items: g
            .items
            .iter()
            .map(|item| match item {
                BlockNode::Group(inner) => Node::Group(group(inner, resolve)),
                BlockNode::Cond(c) => Node::Cond(Condition {
                    kind: c.kind,
                    id: c.id.clone(),
                    field: c.field.clone(),
                    cmp: c.cmp,
                    value: c.value.as_ref().map(|value| match value {
                        BlockValue::Value(value) => value.clone(),
                        BlockValue::Ref(r) => Value::List(resolve(&r.source)),
                    }),
                }),
            })
            .collect(),
    }
}

/// A value of the field's type, standing in for refs while checking.
fn placeholder(field: &Field) -> Value {
    match field {
        Field::Dimension(DimensionField::Weekday | DimensionField::Hour) => Value::Integer(1),
        _ => Value::String(String::new()),
    }
}

/// The exploration state one run of a block computes.
fn state(
    block: &Block,
    range: &Range,
    timezone: &str,
    filter: Option<Group>,
    for_each: Option<(&Field, Value)>,
) -> ExplorationState {
    let filter = match for_each {
        None => filter,
        Some((field, value)) => {
            let cond = Node::Cond(Condition {
                kind: CondKind::Cond,
                id: FOR_EACH_ID.into(),
                field: field.clone(),
                cmp: Operator::Eq,
                value: Some(value),
            });
            Some(match filter {
                Some(mut g) if g.op == GroupOp::And => {
                    g.items.push(cond);
                    g
                }
                other => Group {
                    kind: GroupKind::Group,
                    op: GroupOp::And,
                    items: other.map(Node::Group).into_iter().chain([cond]).collect(),
                },
            })
        }
    };
    let base = ExplorationState {
        v: 1,
        site: block.site.clone(),
        measures: Vec::new(),
        dimensions: Vec::new(),
        filter,
        range: range.clone(),
        timezone: timezone.to_owned(),
        granularity: Granularity::Day,
        compare: Compare::None,
        metric_version: METRIC_VERSION,
        sort: Vec::new(),
        page: DEFAULT_PAGE,
        visualization: Visualization::Table,
        report: None,
    };
    match &block.body {
        BlockBody::Cut(cut) => {
            let limit = cut.limit.unwrap_or(REPORT_QUERY_CUT_LIMIT_DEFAULT);
            ExplorationState {
                measures: cut.measures.clone(),
                dimensions: cut.dimensions.clone(),
                granularity: cut.granularity.unwrap_or(Granularity::Day),
                compare: cut.compare.unwrap_or(Compare::None),
                sort: cut.sort.clone(),
                // The validator clamps a page to 100 rows; the run widens it to `limit`.
                page: Page {
                    size: limit.min(PAGE_SIZE_MAX),
                    index: 0,
                },
                visualization: if cut.series {
                    Visualization::Line
                } else {
                    Visualization::Table
                },
                ..base
            }
        }
        BlockBody::Report(report) => ExplorationState {
            granularity: report.granularity.unwrap_or(Granularity::Day),
            visualization: match report.report {
                ReportDefinition::Funnel(_) => Visualization::Funnel,
                ReportDefinition::Journey(_) => Visualization::Journey,
                ReportDefinition::Retention(_) => Visualization::Cohort,
                _ => Visualization::Table,
            },
            report: Some(report.report.clone()),
            ..base
        },
    }
}
