//! Provisional engine probes for the early-gate cases: typed, bounded SQL
//! compiled from the case state with bound parameters. Milestone 06 replaces
//! this with the analytics query compiler; the probes exist to measure the
//! storage layout with exact results, not to define the product API.
//!
//! Session fields (device, country, referrer) are read from each event's own
//! row. In the fixture every event of a session carries the same values;
//! milestone 05 resolves them from the session.

use std::time::Duration;

use chrono::{DateTime, TimeZone};
use chrono_tz::Tz;
use holocron_storage::duckdb::types::{TimeUnit, Value};
use holocron_storage::duckdb::{self, Connection, params_from_iter};
use holocron_storage::{Lane, ReadError, Storage};
use serde_json::Value as Json;

use crate::cases::{Case, Cmp, Field, Granularity, Node, Op, Range};
use crate::result::{self, Raw, Row, Totals};

pub struct ProbeRun {
    pub doc: Json,
    pub hash: String,
    pub row_count: u64,
    pub queue_wait: Duration,
    pub execution: Duration,
}

pub async fn run(storage: &Storage, case: Case, lane: Lane) -> Result<ProbeRun, ReadError> {
    let plan = Plan::new(&case);
    let read = storage
        .read(lane, move |snapshot| plan.execute(snapshot.connection()))
        .await?;
    let (doc, row_count) = result::document(&case, &read.value);
    let hash = result::sha256_hex(&result::canonical(&doc));
    Ok(ProbeRun {
        doc,
        hash,
        row_count,
        queue_wait: read.queue_wait,
        execution: read.execution,
    })
}

/// SQL text plus bound parameters.
#[derive(Default, Clone)]
struct Sql {
    text: String,
    params: Vec<Value>,
}

impl Sql {
    fn push(&mut self, text: &str) -> &mut Self {
        self.text.push_str(text);
        self
    }

    fn bind(&mut self, value: Value) -> &mut Self {
        self.text.push('?');
        self.params.push(value);
        self
    }
}

/// With `HOLOCRON_EXPLAIN_DIR` set, appends `EXPLAIN ANALYZE` of each statement
/// to `<dir>/<case>.txt` before running it. Evidence support only; off by default.
fn explain(conn: &Connection, case: &str, sql: &Sql) -> duckdb::Result<()> {
    let Ok(dir) = std::env::var("HOLOCRON_EXPLAIN_DIR") else {
        return Ok(());
    };
    let mut statement = conn.prepare(&format!("EXPLAIN ANALYZE {}", sql.text))?;
    let mut rows = statement.query(params_from_iter(sql.params.iter()))?;
    let mut text = String::new();
    while let Some(row) = rows.next()? {
        text.push_str(&row.get::<_, String>(1)?);
        text.push('\n');
    }
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(format!("{dir}/{case}.txt"))
        .expect("explain file");
    writeln!(file, "-- {}\n{text}", sql.text).expect("write explain");
    Ok(())
}

fn ts(ms: i64) -> Value {
    Value::Timestamp(TimeUnit::Millisecond, ms)
}

fn column(name: &str) -> &'static str {
    match name {
        "path" => "e.path",
        "event_name" => "e.event_name",
        "device" => "e.device",
        "browser" => "e.browser",
        "os" => "e.os",
        "country" => "e.country",
        "referrer_domain" => "e.referrer_domain",
        other => panic!("dimension {other} is outside the early-gate subset"),
    }
}

/// Compiles a filter tree. Negative operators are exact complements (0004).
/// Property conditions become uncorrelated semi joins on `event_properties`,
/// restricted to the site and time span so zone maps prune row groups.
fn compile(node: &Node, site: u128, span: Range, sql: &mut Sql) {
    match node {
        Node::Group { op, items } => {
            sql.push("(");
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    sql.push(if *op == Op::And { " AND " } else { " OR " });
                }
                compile(item, site, span, sql);
            }
            sql.push(")");
        }
        Node::Cond { field, cmp, value } => {
            let negated = matches!(cmp, Cmp::Neq | Cmp::NotContains | Cmp::IsNotSet);
            if negated {
                sql.push("NOT ");
            }
            let value = || Value::Text(value.clone().expect("operator needs a value"));
            match field {
                Field::Dimension(name) => {
                    let col = column(name);
                    match cmp {
                        Cmp::Eq | Cmp::Neq => sql
                            .push(&format!("coalesce({col} = "))
                            .bind(value())
                            .push(", false)"),
                        Cmp::Contains | Cmp::NotContains => sql
                            .push(&format!("coalesce(contains({col}, "))
                            .bind(value())
                            .push("), false)"),
                        Cmp::StartsWith => sql
                            .push(&format!("coalesce(starts_with({col}, "))
                            .bind(value())
                            .push("), false)"),
                        Cmp::IsSet | Cmp::IsNotSet => sql.push(&format!("({col} IS NOT NULL)")),
                    };
                }
                Field::EventProperty(key) => {
                    sql.push("e.seq IN (SELECT seq FROM event_properties WHERE site = ")
                        .bind(Value::UHugeInt(site))
                        .push(" AND event_time >= ")
                        .bind(ts(span.start))
                        .push(" AND event_time < ")
                        .bind(ts(span.end))
                        .push(" AND key = ")
                        .bind(Value::Text(key.clone()));
                    match cmp {
                        Cmp::Eq | Cmp::Neq => {
                            sql.push(" AND type = 5 AND v_string = ").bind(value())
                        }
                        Cmp::Contains | Cmp::NotContains => sql
                            .push(" AND type = 5 AND contains(v_string, ")
                            .bind(value())
                            .push(")"),
                        Cmp::StartsWith => sql
                            .push(" AND type = 5 AND starts_with(v_string, ")
                            .bind(value())
                            .push(")"),
                        Cmp::IsSet | Cmp::IsNotSet => sql.push(""),
                    };
                    sql.push(")");
                }
            }
        }
    }
}

/// `event_time` shifted to the exploration timezone's wall clock. Offsets are
/// piecewise constant, so the shift is a CASE over the transitions in range.
fn local_time(tz: Tz, span: Range, column: &str) -> String {
    let offset = |ms: i64| {
        tz.offset_from_utc_datetime(&DateTime::from_timestamp_millis(ms).unwrap().naive_utc())
    };
    let seconds = |ms: i64| {
        use chrono::Offset;
        offset(ms).fix().local_minus_utc()
    };
    let mut segments = vec![(span.start, seconds(span.start))];
    let mut t = span.start;
    while t < span.end {
        t += 900_000;
        let s = seconds(t);
        if s != segments.last().unwrap().1 {
            // Find the exact transition instant within the last 15 minutes.
            let mut lo = t - 900_000;
            while lo + 1000 < t && seconds(lo + 1000) == segments.last().unwrap().1 {
                lo += 1000;
            }
            segments.push((lo + 1000, s));
        }
    }
    if segments.len() == 1 {
        return format!("({column} + to_seconds({}))", segments[0].1);
    }
    let mut case = String::from("CASE");
    for pair in segments.windows(2) {
        let until = DateTime::from_timestamp_millis(pair[1].0)
            .unwrap()
            .format("%Y-%m-%d %H:%M:%S");
        case.push_str(&format!(
            " WHEN {column} < TIMESTAMP '{until}' THEN {}",
            pair[0].1
        ));
    }
    case.push_str(&format!(" ELSE {} END", segments.last().unwrap().1));
    format!("({column} + to_seconds({case}))")
}

fn trunc(granularity: Granularity) -> &'static str {
    match granularity {
        Granularity::Hour => "hour",
        Granularity::Day => "day",
    }
}

#[derive(Clone)]
struct Plan {
    case: Case,
    site: u128,
    /// Union of current and previous ranges, used for scans.
    span: Range,
}

impl Plan {
    fn new(case: &Case) -> Self {
        let span = match case.previous {
            Some(p) => Range {
                start: p.start.min(case.current.start),
                end: p.end.max(case.current.end),
            },
            None => case.current,
        };
        Plan {
            case: case.clone(),
            site: case.site_id.as_u128(),
            span,
        }
    }

    fn execute(&self, conn: &Connection) -> duckdb::Result<Raw> {
        if self.case.funnel.is_some() {
            self.funnel(conn)
        } else if self.case.has_series() {
            self.overview(conn)
        } else {
            self.table(conn)
        }
    }

    /// Eligible events of the site in the scan span, with `cur` marking the current range.
    fn matched(&self, extra_columns: &str, filter: bool) -> Sql {
        let mut sql = Sql::default();
        let previous = self.case.previous.unwrap_or(Range { start: 0, end: 0 });
        sql.push("SELECT e.kind, e.daily_id, e.profile_id, e.event_time, (e.event_time >= ")
            .bind(ts(self.case.current.start))
            .push(" AND e.event_time < ")
            .bind(ts(self.case.current.end))
            .push(") AS cur")
            .push(extra_columns)
            .push(" FROM events e WHERE e.site = ")
            .bind(Value::UHugeInt(self.site))
            .push(" AND e.kind IN (1, 2) AND ((e.event_time >= ")
            .bind(ts(self.case.current.start))
            .push(" AND e.event_time < ")
            .bind(ts(self.case.current.end))
            .push(") OR (e.event_time >= ")
            .bind(ts(previous.start))
            .push(" AND e.event_time < ")
            .bind(ts(previous.end))
            .push("))");
        if filter && let Some(node) = &self.case.filter {
            sql.push(" AND ");
            compile(node, self.site, self.span, &mut sql);
        }
        sql
    }

    fn overview(&self, conn: &Connection) -> duckdb::Result<Raw> {
        let case = &self.case;
        let buckets = result::buckets(case.timezone, case.current, case.granularity);
        let index: std::collections::HashMap<i64, usize> = buckets
            .iter()
            .enumerate()
            .map(|(i, (label, _))| (label.and_utc().timestamp_millis(), i))
            .collect();
        let mut series = vec![Totals::default(); buckets.len()];
        let local = local_time(case.timezone, self.span, "event_time");

        // Event measures per range.
        let mut sql = self.matched("", false);
        sql.text = format!(
            "SELECT cur, count(*) FILTER (WHERE kind = 1), count(DISTINCT daily_id), count(*),
                    count(*) FILTER (WHERE daily_id IS NOT NULL OR profile_id IS NOT NULL)
             FROM ({}) GROUP BY cur",
            sql.text
        );
        let mut ranges = [Totals::default(), Totals::default()];
        explain(conn, &self.case.id, &sql)?;
        let mut statement = conn.prepare(&sql.text)?;
        let mut rows = statement.query(params_from_iter(sql.params.iter()))?;
        while let Some(row) = rows.next()? {
            let t = &mut ranges[!row.get::<_, bool>(0)? as usize];
            t.pageviews = row.get::<_, i64>(1)? as u64;
            t.visitor_days = row.get::<_, i64>(2)? as u64;
            t.eligible = row.get::<_, i64>(3)? as u64;
            t.with_identity = row.get::<_, i64>(4)? as u64;
        }

        // Event series in the current range.
        let mut sql = self.matched("", false);
        sql.text = format!(
            "SELECT epoch_ms(date_trunc('{}', {})), count(*) FILTER (WHERE kind = 1), count(DISTINCT daily_id)
             FROM ({}) WHERE cur GROUP BY 1",
            trunc(case.granularity),
            local,
            sql.text
        );
        explain(conn, &self.case.id, &sql)?;
        let mut statement = conn.prepare(&sql.text)?;
        let mut rows = statement.query(params_from_iter(sql.params.iter()))?;
        while let Some(row) = rows.next()? {
            let point = &mut series[index[&row.get::<_, i64>(0)?]];
            point.pageviews = row.get::<_, i64>(1)? as u64;
            point.visitor_days = row.get::<_, i64>(2)? as u64;
        }

        // Sessions in start mode.
        let previous = case.previous.unwrap_or(Range { start: 0, end: 0 });
        let session_columns = "count(*), count(*) FILTER (WHERE pageviews > 0),
             count(*) FILTER (WHERE pageviews = 1 AND custom_events = 0),
             coalesce(sum(epoch_ms(last_time) - epoch_ms(start_time)) FILTER (WHERE pageviews > 0), 0)::BIGINT";
        let mut sql = Sql::default();
        sql.push(&format!(
            "SELECT start_time >= ? AND start_time < ? AS cur, {session_columns},
                    quantile_disc(epoch_ms(last_time) - epoch_ms(start_time), 0.5) FILTER (WHERE pageviews > 0)
             FROM sessions WHERE site = ? AND ((start_time >= ? AND start_time < ?) OR (start_time >= ? AND start_time < ?))
             GROUP BY cur"
        ));
        sql.params = vec![
            ts(case.current.start),
            ts(case.current.end),
            Value::UHugeInt(self.site),
            ts(case.current.start),
            ts(case.current.end),
            ts(previous.start),
            ts(previous.end),
        ];
        explain(conn, &self.case.id, &sql)?;
        let mut statement = conn.prepare(&sql.text)?;
        let mut rows = statement.query(params_from_iter(sql.params.iter()))?;
        while let Some(row) = rows.next()? {
            let t = &mut ranges[!row.get::<_, bool>(0)? as usize];
            t.sessions = row.get::<_, i64>(1)? as u64;
            t.engaged_sessions = row.get::<_, i64>(2)? as u64;
            t.bounces = row.get::<_, i64>(3)? as u64;
            t.span_sum_ms = row.get::<_, i64>(4)? as u64;
            t.span_median_ms = row.get::<_, Option<i64>>(5)?.map(|v| v as u64);
        }

        let local_start = local_time(case.timezone, self.span, "start_time");
        let sql = Sql {
            text: format!(
                "SELECT epoch_ms(date_trunc('{}', {local_start})), {session_columns}
                 FROM sessions WHERE site = ? AND start_time >= ? AND start_time < ? GROUP BY 1",
                trunc(case.granularity)
            ),
            params: vec![
                Value::UHugeInt(self.site),
                ts(case.current.start),
                ts(case.current.end),
            ],
        };
        explain(conn, &self.case.id, &sql)?;
        let mut statement = conn.prepare(&sql.text)?;
        let mut rows = statement.query(params_from_iter(sql.params.iter()))?;
        while let Some(row) = rows.next()? {
            let point = &mut series[index[&row.get::<_, i64>(0)?]];
            point.sessions = row.get::<_, i64>(1)? as u64;
            point.engaged_sessions = row.get::<_, i64>(2)? as u64;
            point.bounces = row.get::<_, i64>(3)? as u64;
            point.span_sum_ms = row.get::<_, i64>(4)? as u64;
        }

        let [current, previous] = ranges;
        Ok(Raw::Overview {
            current,
            previous: case.previous.map(|_| previous),
            series,
        })
    }

    fn table(&self, conn: &Connection) -> duckdb::Result<Raw> {
        let case = &self.case;
        let dims: Vec<String> = case
            .dimensions
            .iter()
            .enumerate()
            .map(|(i, d)| format!(", {} AS d{i}", column(d)))
            .collect();
        let keys: Vec<String> = (0..case.dimensions.len())
            .map(|i| format!("d{i}"))
            .collect();
        let matched = self.matched(&dims.concat(), true);
        // Rows are ranked by a count, so exact distinct identities are only
        // computed for the requested page and the totals, not for every row.
        let sort = match case.sort_by.as_deref().unwrap_or("events") {
            "pageviews" => "pv",
            "events" => "ev",
            other => panic!("sort by {other} is outside the early-gate subset"),
        };
        let order: Vec<String> = keys.iter().map(|k| format!("{k} ASC NULLS LAST")).collect();
        let same_key = |a: &str, b: &str| {
            keys.iter()
                .map(|k| format!("{a}.{k} IS NOT DISTINCT FROM {b}.{k}"))
                .collect::<Vec<_>>()
                .join(" AND ")
        };
        let counts = "count(*) FILTER (WHERE cur AND kind = 1) AS pv, count(*) FILTER (WHERE cur AND kind = 2) AS ev,
             count(*) FILTER (WHERE NOT cur AND kind = 1) AS ppv, count(*) FILTER (WHERE NOT cur AND kind = 2) AS pev,
             count(*) FILTER (WHERE cur) AS eligible,
             count(*) FILTER (WHERE cur AND (daily_id IS NOT NULL OR profile_id IS NOT NULL)) AS identified";
        let distinct = "count(DISTINCT daily_id) FILTER (WHERE cur) AS vd, count(DISTINCT daily_id) FILTER (WHERE NOT cur) AS pvd";
        // A filtered match set is evaluated once and kept. An unfiltered one is
        // cheaper to rescan from the site-clustered table than to materialize:
        // reading back 57M kept rows cost 4.7 to 6.4 s per pass (decision 0008).
        let (head, source, params) = if case.filter.is_some() {
            (
                format!("m AS MATERIALIZED ({}),", matched.text),
                "m".to_owned(),
                matched.params,
            )
        } else {
            let p = matched.params;
            (
                String::new(),
                format!("({}) m", matched.text),
                [p.clone(), p.clone(), p].concat(),
            )
        };
        let sql = Sql {
            params,
            text: format!(
                "WITH {head}
                 ranked AS MATERIALIZED (SELECT {keys}, {counts} FROM {source} GROUP BY ALL),
                 page AS MATERIALIZED (SELECT * FROM ranked ORDER BY {sort} DESC, {order} LIMIT {limit} OFFSET {offset}),
                 page_distinct AS (SELECT {keys}, {distinct} FROM {source}
                                   WHERE EXISTS (SELECT 1 FROM page p WHERE {page_match}) GROUP BY ALL)
                 SELECT true AS total, {null_keys}, t.pv, t.ev, t.vd, t.ppv, t.pev, t.pvd, t.eligible, t.identified,
                        (SELECT count(*) FROM ranked)
                 FROM (SELECT {counts}, {distinct} FROM {source}) t
                 UNION ALL
                 SELECT false, {page_keys}, p.pv, p.ev, coalesce(d.vd, 0), p.ppv, p.pev, coalesce(d.pvd, 0), p.eligible, p.identified, NULL
                 FROM page p LEFT JOIN page_distinct d ON {join}",
                keys = keys.join(", "),
                order = order.join(", "),
                limit = case.page_size,
                offset = case.page_size * case.page_index,
                page_match = same_key("p", "m"),
                join = same_key("p", "d"),
                page_keys = keys.iter().map(|k| format!("p.{k}")).collect::<Vec<_>>().join(", "),
                null_keys = keys.iter().map(|k| format!("NULL::VARCHAR AS {k}")).collect::<Vec<_>>().join(", "),
            ),
        };
        explain(conn, &self.case.id, &sql)?;
        let mut statement = conn.prepare(&sql.text)?;
        let mut rows = statement.query(params_from_iter(sql.params.iter()))?;
        let n = keys.len();
        let (mut current, mut previous, mut page, mut row_count) =
            (Totals::default(), Totals::default(), Vec::new(), 0);
        while let Some(row) = rows.next()? {
            let get =
                |i: usize| -> duckdb::Result<u64> { Ok(row.get::<_, i64>(1 + n + i)? as u64) };
            let c = Totals {
                pageviews: get(0)?,
                events: get(1)?,
                visitor_days: get(2)?,
                eligible: get(6)?,
                with_identity: get(7)?,
                ..Totals::default()
            };
            let p = Totals {
                pageviews: get(3)?,
                events: get(4)?,
                visitor_days: get(5)?,
                ..Totals::default()
            };
            if row.get::<_, bool>(0)? {
                (current, previous) = (c, p);
                row_count = row.get::<_, i64>(1 + n + 8)? as u64;
            } else {
                let key = (0..n)
                    .map(|i| row.get::<_, Option<String>>(1 + i))
                    .collect::<duckdb::Result<Vec<_>>>()?;
                page.push(Row {
                    key,
                    current: c,
                    previous: case.previous.map(|_| p),
                });
            }
        }
        page.sort_by(|a, b| {
            let metric = |t: &Totals| match sort {
                "pv" => t.pageviews,
                "ev" => t.events,
                _ => t.visitor_days,
            };
            metric(&b.current).cmp(&metric(&a.current)).then_with(|| {
                a.key
                    .iter()
                    .zip(&b.key)
                    .map(|(x, y)| result::cell_order(x, y))
                    .find(|o| o.is_ne())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
        });
        Ok(Raw::Table {
            current,
            previous: case.previous.map(|_| previous),
            rows: page,
            row_count,
        })
    }

    /// Daily funnel (0003 Funnels): entry at the first step-1 event in range,
    /// each next step the first match strictly after the previous step in
    /// (event_time, ingestion sequence) order and inside the window from step 1.
    fn funnel(&self, conn: &Connection) -> duckdb::Result<Raw> {
        let case = &self.case;
        let funnel = case.funnel.as_ref().unwrap();
        let span = Range {
            start: case.current.start,
            end: case.current.end + funnel.window_ms,
        };
        let mut sql = Sql::default();
        sql.push("WITH e AS MATERIALIZED (SELECT e.daily_id, epoch_ms(e.event_time) AS t, epoch_ms(e.event_time)::HUGEINT * 4294967296 + e.seq AS k");
        for (i, step) in funnel.steps.iter().enumerate() {
            sql.push(", ");
            compile(step, self.site, span, &mut sql);
            sql.push(&format!(" AS s{i}"));
        }
        sql.push(" FROM events e WHERE e.site = ")
            .bind(Value::UHugeInt(self.site))
            .push(" AND e.kind IN (1, 2) AND e.daily_id IS NOT NULL AND e.event_time >= ")
            .bind(ts(span.start))
            .push(" AND e.event_time < ")
            .bind(ts(span.end))
            .push(")");
        sql.push(&format!(
            ", f0 AS (SELECT daily_id, min(k) AS k, arg_min(t, k) AS t1 FROM e WHERE s0 AND t < {} GROUP BY daily_id)",
            case.current.end
        ));
        for i in 1..funnel.steps.len() {
            sql.push(&format!(
                ", f{i} AS (SELECT f.daily_id, min(e.k) AS k, any_value(f.t1) AS t1 FROM f{p} f JOIN e ON e.daily_id = f.daily_id
                   WHERE e.s{i} AND e.k > f.k AND e.t < f.t1 + {w} GROUP BY f.daily_id)",
                p = i - 1,
                w = funnel.window_ms
            ));
        }
        let counts: Vec<String> = (0..funnel.steps.len())
            .map(|i| format!("(SELECT count(*) FROM f{i})"))
            .collect();
        sql.push(&format!(
            " SELECT {}, (SELECT count(*) FROM events WHERE site = ? AND kind IN (1, 2) AND event_time >= ? AND event_time < ?),
                 (SELECT count(*) FROM events WHERE site = ? AND kind IN (1, 2) AND event_time >= ? AND event_time < ?
                  AND (daily_id IS NOT NULL OR profile_id IS NOT NULL))",
            counts.join(", ")
        ));
        for _ in 0..2 {
            sql.params.extend([
                Value::UHugeInt(self.site),
                ts(case.current.start),
                ts(case.current.end),
            ]);
        }
        let steps = funnel.steps.len();
        explain(conn, &case.id, &sql)?;
        conn.query_row(&sql.text, params_from_iter(sql.params.iter()), |row| {
            let reached = (0..steps)
                .map(|i| row.get::<_, i64>(i).map(|v| v as u64))
                .collect::<duckdb::Result<Vec<_>>>()?;
            Ok(Raw::Funnel {
                reached,
                eligible: row.get::<_, i64>(steps)? as u64,
                with_identity: row.get::<_, i64>(steps + 1)? as u64,
            })
        })
    }
}
