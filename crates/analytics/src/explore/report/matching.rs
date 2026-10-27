//! Reads the behavioral reports share: the eligible events matching the state's filter and the
//! report's predicates, materialized once with a column per field the predicates test, the
//! identity coverage of the range, and the `(event_time, ingestion_sequence)` order key.

use holocron_protocol::exploration::{Coverage, Field, Metric, Reason};
use holocron_storage::duckdb::{self, Row, params_from_iter};

use super::Input;
use crate::explore::run::{self, Plan, ratio4};
use crate::explore::sql::{self, Sql};
use crate::explore::validate::Pred;

/// `(t, seq)` as one ordered HUGEINT, so a single inequality orders events as 0003 does: by
/// event time, then ingestion sequence. Both are non-negative BIGINTs.
pub fn key(t: &str, seq: &str) -> String {
    format!("({t}::HUGEINT * 9223372036854775808 + {seq})")
}

/// The fields of a materialized table's columns, in column order.
pub struct Matched {
    pub fields: Vec<Field>,
}

impl Matched {
    /// The index of a materialized field, which names its columns ([`sql::columns`]).
    pub fn index(&self, field: &Field) -> Option<usize> {
        self.fields.iter().position(|f| f == field)
    }

    /// `pred` as a boolean SQL expression over the materialized columns.
    pub fn test(&self, pred: &Pred, sql: &mut Sql) {
        sql::predicate(pred, &self.fields, sql);
    }

    /// `CASE WHEN <pred> THEN 1 ELSE 0 END + CASE WHEN … THEN 2 …`: one bit per predicate.
    pub fn mask(&self, preds: &[&Pred], sql: &mut Sql) {
        for (i, pred) in preds.iter().enumerate() {
            sql.push(if i == 0 {
                "(CASE WHEN "
            } else {
                " + (CASE WHEN "
            });
            self.test(pred, sql);
            sql.push(&format!(" THEN {} ELSE 0 END)", 1_u32 << i));
        }
    }
}

/// What [`materialize`] reads besides the filter.
#[derive(Default)]
pub struct Reads<'a> {
    /// Predicates whose fields become columns.
    pub trees: &'a [&'a Pred],
    /// More fields that become columns, such as a grouping.
    pub fields: &'a [Field],
    /// Keep only events matching at least one of `trees`.
    pub narrow: bool,
    /// Also `<prefix>_se`: the members of sessions starting in the span.
    pub sessions: bool,
    /// Rows carry their revenue record as `cur` and `rev`.
    pub revenue: bool,
}

/// Materializes `<prefix>_ev`, and with `sessions` also `<prefix>_se` ([`run::materialize`]),
/// over `span`: eligible events matching the state's filter and, with `narrow`, at least one
/// of the trees. Every field the trees test is a column, so their tests run on the result.
pub fn materialize(
    input: &Input,
    prefix: &str,
    reads: Reads,
    span: (i64, i64),
) -> duckdb::Result<Matched> {
    let Reads {
        trees,
        fields: extra,
        narrow,
        sessions,
        revenue,
    } = reads;
    let mut fields: Vec<Field> = Vec::new();
    let collect = |pred: &Pred, fields: &mut Vec<Field>| {
        pred.visit(&mut |cond| {
            if !fields.contains(&cond.field) {
                fields.push(cond.field.clone());
            }
        });
    };
    for tree in trees {
        collect(tree, &mut fields);
    }
    for field in extra {
        if !fields.contains(field) {
            fields.push(field.clone());
        }
    }
    let tested = fields.len();
    if let Some(filter) = &input.query.filter {
        collect(filter, &mut fields);
    }
    let filter = if narrow && !trees.is_empty() {
        let any = Pred::Any(trees.iter().map(|tree| (*tree).clone()).collect());
        Some(match &input.query.filter {
            Some(filter) => Pred::All(vec![filter.clone(), any]),
            None => any,
        })
    } else {
        input.query.filter.clone()
    };
    let plan = Plan {
        site: input.ctx.site,
        timezone: input.query.timezone,
        filter: filter.as_ref(),
        read: (0..fields.len()).map(|i| i < tested).collect(),
        fields: fields.clone(),
        sessions,
        rollup: false,
        hostnames: input.ctx.hostnames,
        performance: false,
        revenue,
        revenue_only: false,
    };
    run::materialize(input.snapshot.connection(), &plan, prefix, span)?;
    Ok(Matched { fields })
}

/// Eligible events in range matching the filter, with a daily pseudonym or profile, or for
/// person scope with a profile (consented coverage), out of all of them (0003 Common terms).
pub fn coverage(input: &Input, person: bool) -> duckdb::Result<Coverage> {
    materialize(input, "cov", Reads::default(), input.read)?;
    let with = if person {
        "profile_id IS NOT NULL"
    } else {
        "daily_id IS NOT NULL OR profile_id IS NOT NULL"
    };
    let coverage = input.snapshot.connection().query_row(
        &format!("SELECT count(*) FILTER (WHERE {with}), count(*) FROM cov_ev"),
        [],
        |r| {
            Ok(Coverage {
                with_identity: r.get(0)?,
                eligible_events: r.get(1)?,
            })
        },
    )?;
    run::drop_tables(input.snapshot.connection(), "cov")?;
    Ok(coverage)
}

/// Runs a statement, recording its plan for measurement tools.
pub fn execute(input: &Input, sql: &Sql) -> duckdb::Result<usize> {
    run::execute(input.snapshot.connection(), sql)
}

/// The rows of a query, each mapped by `f`.
pub fn rows<T>(
    input: &Input,
    sql: &Sql,
    f: impl FnMut(&Row<'_>) -> duckdb::Result<T>,
) -> duckdb::Result<Vec<T>> {
    let conn = input.snapshot.connection();
    run::note_plan(conn, sql)?;
    let mut statement = conn.prepare(&sql.text)?;
    statement
        .query_map(params_from_iter(sql.params.iter()), f)?
        .collect()
}

/// Drops temp tables of a report.
pub fn drop(input: &Input, tables: &[String]) -> duckdb::Result<()> {
    let statements: String = tables
        .iter()
        .map(|table| format!("DROP TABLE IF EXISTS {table};"))
        .collect();
    input.snapshot.connection().execute_batch(&statements)
}

/// `numerator / denominator`, unavailable when nothing is in the denominator (0003).
pub fn rate(numerator: i64, denominator: i64) -> Metric {
    match denominator {
        0 => Metric::Unavailable {
            reason: Reason::NoDenominator,
        },
        _ => Metric::Ratio {
            v: ratio4(i128::from(numerator), i128::from(denominator)),
            numerator,
            denominator,
        },
    }
}
