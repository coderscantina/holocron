//! The attribution report (0003 Attribution, 0004 Report definitions).
//!
//! Conversion events come from the matching events of the range: the first or every goal
//! event per session (sessions starting in range) or per profile, or every revenue event.
//! A touch is a session's first pageview, else its first eligible event; direct touches
//! (no outside referrer, UTM source or click ID) and internal referrers are not touches.
//! Each conversion takes the last or first touch of its identity strictly before it, within
//! the lookback, through an ASOF join: a sort-merge per identity instead of a join of every
//! touch with every conversion. A conversion without one is direct.

use std::collections::BTreeMap;

use holocron_protocol::exploration::{
    Attributed, AttributedConversion, AttributionModel, AttributionResult, Cell, Conversion,
    Coverage, GoalCounting, IdentityScope, Money, PopulationScope, TouchField, TouchRow,
};
use holocron_protocol::limits::REPORT_ATTRIBUTION_TOUCHES_MAX;

use super::matching::{self, Reads, key};
use super::{Input, Outcome};
use crate::explore::sql::{self, Sql};
use crate::explore::validate::{CheckedAttribution, Pred};

const DAY_MS: i64 = 86_400_000;

pub fn run(input: &Input, report: &CheckedAttribution) -> Outcome<AttributionResult> {
    let definition = &report.definition;
    let person = definition.scope == IdentityScope::Person;
    let id = if person { "profile_id" } else { "daily_id" };
    let lookback = i64::from(definition.lookback_days) * DAY_MS;
    let preds: Vec<&Pred> = report.goals.iter().flatten().collect();
    let sessions = definition
        .conversions
        .iter()
        .any(|c| matches!(c, Conversion::Goal(g) if g.goal.scope == PopulationScope::Session));
    let revenue = definition
        .conversions
        .iter()
        .any(|c| matches!(c, Conversion::Revenue));
    let matched = matching::materialize(
        input,
        "at",
        Reads {
            trees: &preds,
            sessions,
            revenue,
            ..Reads::default()
        },
        input.read,
    )?;
    let conn = input.snapshot.connection();

    let (with_identity, eligible): (i64, i64) = conn.query_row(
        &format!(
            "SELECT count(*) FILTER (WHERE {}), count(*) FROM at_ev",
            if person {
                "profile_id IS NOT NULL"
            } else {
                "daily_id IS NOT NULL OR profile_id IS NOT NULL"
            }
        ),
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;

    // Conversion events: `c` is the conversion's index, `k` its order key.
    let mut cv = Sql::default();
    cv.push("CREATE OR REPLACE TEMP TABLE at_cv AS SELECT * FROM (");
    for (c, (conversion, pred)) in definition.conversions.iter().zip(&report.goals).enumerate() {
        if c > 0 {
            cv.push(" UNION ALL ");
        }
        let k = key("t", "seq");
        match (conversion, pred) {
            (Conversion::Goal(goal), Some(pred)) => {
                let (table, unit) = match goal.goal.scope {
                    PopulationScope::Session => ("at_se", "session_id"),
                    PopulationScope::Person => ("at_ev", "profile_id"),
                };
                let columns = match goal.goal.counting {
                    GoalCounting::Every => format!("{id} AS id, t, {k} AS k"),
                    // The first matching event of the session or profile (0003 Attribution).
                    GoalCounting::Once => {
                        format!("arg_min({id}, {k}) AS id, arg_min(t, {k}) AS t, min({k}) AS k")
                    }
                };
                cv.push(&format!(
                    "SELECT {c} AS c, {columns}, NULL::VARCHAR AS currency, NULL::BIGINT AS minor
                     FROM {table} WHERE {unit} IS NOT NULL AND "
                ));
                matched.test(pred, &mut cv);
                if goal.goal.counting == GoalCounting::Once {
                    cv.push(&format!(" GROUP BY {unit}"));
                }
            }
            _ => {
                cv.push(&format!(
                    "SELECT {c} AS c, {id} AS id, t, {k} AS k, cur AS currency, rev AS minor
                     FROM at_ev WHERE rev IS NOT NULL"
                ));
            }
        }
    }
    cv.push(") WHERE id IS NOT NULL");
    matching::execute(input, &cv)?;

    let bounds: (Option<i64>, Option<i64>) =
        conn.query_row("SELECT min(t), max(t) FROM at_cv", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })?;
    let tables: Vec<String> = [
        "at_ev", "at_se", "at_cv", "at_ss", "at_tp", "at_r", "at_top",
    ]
    .map(String::from)
    .to_vec();
    let mut results: Vec<AttributedConversion> = definition
        .conversions
        .iter()
        .map(|_| AttributedConversion {
            touches: Vec::new(),
            touch_count: 0,
            direct: Attributed {
                conversions: 0,
                revenue: Vec::new(),
            },
        })
        .collect();
    if let (Some(first), Some(last)) = bounds {
        // Touches of sessions that can lie before a conversion: daily pseudonyms last one
        // local day, so two days cover every daily touch; persons look back the full window.
        let lo = first - if person { lookback } else { 2 * DAY_MS };
        touches(input, definition.touch_field, id, (lo, last + 1))?;
        attribute(input, definition.model, lookback)?;
        collect(input, &mut results)?;
    }
    matching::drop(input, &tables)?;

    Ok(Ok(AttributionResult {
        conversions: results,
        coverage: Coverage {
            with_identity,
            eligible_events: eligible,
        },
    }))
}

/// `at_tp`: the non-direct touches of sessions starting in `[lo, hi)`, by identity and key.
fn touches(
    input: &Input,
    field: TouchField,
    id: &str,
    (lo, hi): (i64, i64),
) -> holocron_storage::duckdb::Result<()> {
    let site = input.ctx.site;
    let mut ss = Sql::default();
    ss.push("CREATE OR REPLACE TEMP TABLE at_ss AS SELECT session_id, last_time FROM sessions WHERE site = ")
        .bind(site)
        .push(" AND start_time >= epoch_ms(")
        .bind(lo)
        .push(") AND start_time < epoch_ms(")
        .bind(hi)
        .push(")");
    matching::execute(input, &ss)?;
    // The touch can be any member, so members are read up to the sessions' last events.
    let last: Option<i64> = input.snapshot.connection().query_row(
        "SELECT epoch_ms(max(last_time)) FROM at_ss",
        [],
        |r| r.get(0),
    )?;
    let to = last.unwrap_or(lo);
    let within = |sql: &mut Sql, column: &str| {
        sql.push(&format!(" AND {column} >= epoch_ms("))
            .bind(lo)
            .push(&format!(") AND {column} <= epoch_ms("))
            .bind(to)
            .push(")");
    };
    let mut tp = Sql::default();
    tp.push(
        "CREATE OR REPLACE TEMP TABLE at_tp AS
         WITH touch AS (
            SELECT m.session_id, arg_min(m.seq, (e.kind <> 1, m.event_time, m.seq)) AS seq
            FROM session_members m JOIN events e ON e.seq = m.seq AND e.site = ",
    )
    .bind(site);
    within(&mut tp, "e.event_time");
    tp.push(" WHERE m.site = ")
        .bind(site)
        .push(" AND m.role = 1");
    within(&mut tp, "m.event_time");
    tp.push(&format!(
        " AND m.session_id IN (SELECT session_id FROM at_ss) GROUP BY m.session_id
         )
         SELECT t.{id} AS id, epoch_ms(t.event_time) AS t, {} AS k, ",
        key("epoch_ms(t.event_time)", "t.seq")
    ));
    match touch_column(field) {
        Some(column) => {
            tp.push(&format!("t.{column}"));
        }
        None => sql::channel(
            "lower(t.query)",
            "lower(t.referrer_domain)",
            "lower(t.utm_medium)",
            "lower(t.utm_source)",
            input.ctx.hostnames,
            &mut tp,
        ),
    }
    tp.push(" AS v FROM touch x JOIN events t ON t.seq = x.seq AND t.site = ")
        .bind(site);
    within(&mut tp, "t.event_time");
    // Direct: no outside referrer, no UTM source, no click ID. An internal referrer is a site
    // hostname after `www.` stripping and counts as no referrer (0003 Attribution).
    tp.push(&format!(
        " WHERE t.{id} IS NOT NULL AND (t.utm_source IS NOT NULL OR t.click_id IS NOT NULL
             OR (t.referrer_domain IS NOT NULL
                 AND regexp_replace(lower(t.referrer_domain), '^www\\.', '') NOT IN (''"
    ));
    for host in input.ctx.hostnames {
        tp.push(", ")
            .bind(host.trim_start_matches("www.").to_lowercase());
    }
    tp.push(")))");
    matching::execute(input, &tp).map(|_| ())
}

fn touch_column(field: TouchField) -> Option<&'static str> {
    Some(match field {
        TouchField::Channel => return None,
        TouchField::ReferrerDomain => "referrer_domain",
        TouchField::UtmSource => "utm_source",
        TouchField::UtmMedium => "utm_medium",
        TouchField::UtmCampaign => "utm_campaign",
        TouchField::UtmContent => "utm_content",
        TouchField::UtmTerm => "utm_term",
        TouchField::ClickId => "click_id",
    })
}

/// `at_r`: every conversion with its attributed touch value, or untouched (direct).
fn attribute(
    input: &Input,
    model: AttributionModel,
    lookback: i64,
) -> holocron_storage::duckdb::Result<()> {
    let mut sql = Sql::default();
    match model {
        // The latest touch strictly before the conversion, if it is inside the lookback.
        AttributionModel::LastTouch => sql
            .push(
                "CREATE OR REPLACE TEMP TABLE at_r AS
                 SELECT c.c, c.currency, c.minor, coalesce(p.t >= c.t - ",
            )
            .bind(lookback)
            .push(
                ", false) AS touched, p.v
                 FROM at_cv c ASOF LEFT JOIN at_tp p ON c.id = p.id AND c.k > p.k",
            ),
        // The earliest touch inside the lookback, if it is strictly before the conversion.
        AttributionModel::FirstTouch => sql
            .push(
                "CREATE OR REPLACE TEMP TABLE at_r AS
                 SELECT c.c, c.currency, c.minor, coalesce(p.k < c.k, false) AS touched, p.v
                 FROM (SELECT *, (t - ",
            )
            .bind(lookback)
            .push(
                ")::HUGEINT * 9223372036854775808 AS lo FROM at_cv) c
                 ASOF LEFT JOIN at_tp p ON c.id = p.id AND c.lo <= p.k",
            ),
    };
    matching::execute(input, &sql).map(|_| ())
}

/// Fills each conversion's top touch values, distinct count, direct count and revenue.
fn collect(
    input: &Input,
    results: &mut [AttributedConversion],
) -> holocron_storage::duckdb::Result<()> {
    let mut top = Sql::default();
    top.push(&format!(
        "CREATE OR REPLACE TEMP TABLE at_top AS SELECT c, v, n, distinct_values, rank FROM (
            SELECT c, v, count(*) AS n, count(*) OVER (PARTITION BY c) AS distinct_values,
                   row_number() OVER (PARTITION BY c ORDER BY count(*) DESC, v ASC NULLS LAST) AS rank
            FROM at_r WHERE touched GROUP BY c, v
         ) WHERE rank <= {REPORT_ATTRIBUTION_TOUCHES_MAX}"
    ));
    matching::execute(input, &top)?;

    let cell = |v: Option<String>| v.map_or(Cell::NotSet, Cell::String);
    let mut rows = Sql::default();
    rows.push("SELECT c, v, n, distinct_values FROM at_top ORDER BY c, rank");
    for (c, v, n, distinct) in matching::rows(input, &rows, |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, Option<String>>(1)?,
            r.get::<_, i64>(2)?,
            r.get::<_, i64>(3)?,
        ))
    })? {
        let result = &mut results[c as usize];
        result.touch_count = distinct;
        result.touches.push(TouchRow {
            touch: cell(v),
            attributed: Attributed {
                conversions: n,
                revenue: Vec::new(),
            },
        });
    }
    let mut direct = Sql::default();
    direct.push("SELECT c, count(*) FROM at_r WHERE NOT touched GROUP BY c");
    for (c, n) in matching::rows(input, &direct, |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?))
    })? {
        results[c as usize].direct.conversions = n;
    }

    // Net revenue per currency, never converted, for the shown touch values and direct.
    let mut revenue = Sql::default();
    revenue.push(
        "SELECT r.c, r.touched, r.v, r.currency, sum(r.minor)::HUGEINT FROM at_r r
         WHERE r.currency IS NOT NULL AND (NOT r.touched OR EXISTS (
            SELECT 1 FROM at_top t WHERE t.c = r.c AND t.v IS NOT DISTINCT FROM r.v))
         GROUP BY ALL ORDER BY r.currency",
    );
    let mut sums: BTreeMap<(i64, bool, Option<String>), Vec<Money>> = BTreeMap::new();
    for (c, touched, v, currency, minor) in matching::rows(input, &revenue, |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, bool>(1)?,
            r.get::<_, Option<String>>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, i128>(4)?,
        ))
    })? {
        if let Some(money) = Money::new(&currency, minor) {
            sums.entry((c, touched, v)).or_default().push(money);
        }
    }
    for ((c, touched, v), money) in sums {
        let result = &mut results[c as usize];
        if !touched {
            result.direct.revenue = money;
        } else if let Some(row) = result
            .touches
            .iter_mut()
            .find(|row| row.touch == cell(v.clone()))
        {
            row.attributed.revenue = money;
        }
    }
    Ok(())
}
