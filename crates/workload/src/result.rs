//! Canonical result documents for the early-gate cases (provisional shape until
//! milestone 03 fixes the wire results) and their RFC 8785 hash.
//!
//! The oracle and the engine probes each compute the raw numbers on their own;
//! only this rendering, the rounding rules of 0003/0004 and the calendar
//! bucket list are shared.

use chrono::{NaiveDateTime, TimeZone, Timelike};
use chrono_tz::Tz;
use serde_json::{Value as Json, json};
use sha2::{Digest, Sha256};

use crate::cases::{Case, Granularity, Range};

/// Exact aggregates for one range, one row, or one series bucket.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Totals {
    pub pageviews: u64,
    pub events: u64,
    pub visitor_days: u64,
    pub sessions: u64,
    pub bounces: u64,
    /// Sessions with at least one pageview: bounce and span denominator.
    pub engaged_sessions: u64,
    pub span_sum_ms: u64,
    pub span_median_ms: Option<u64>,
    pub with_identity: u64,
    pub eligible: u64,
}

/// One dimension cell: `None` is the not-set cell.
pub type Cell = Option<String>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub key: Vec<Cell>,
    pub current: Totals,
    pub previous: Option<Totals>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Raw {
    Overview {
        current: Totals,
        previous: Option<Totals>,
        series: Vec<Totals>,
    },
    /// `rows` is the requested page, already sorted; `row_count` counts every row.
    Table {
        current: Totals,
        previous: Option<Totals>,
        rows: Vec<Row>,
        row_count: u64,
    },
    Funnel {
        reached: Vec<u64>,
        with_identity: u64,
        eligible: u64,
    },
}

/// Series buckets: local wall-clock hours or days of the exploration timezone,
/// each with the first instant (ms) that reads it. A repeated fall-back hour is one bucket.
pub fn buckets(tz: Tz, range: Range, granularity: Granularity) -> Vec<(NaiveDateTime, i64)> {
    let mut out: Vec<(NaiveDateTime, i64)> = Vec::new();
    let mut t = range.start;
    while t < range.end {
        let label = bucket_label(tz, t, granularity);
        if out.last().is_none_or(|(l, _)| *l != label) && !out.iter().any(|(l, _)| *l == label) {
            out.push((label, t));
        }
        t += 900_000; // every timezone offset in use is a multiple of 15 minutes
    }
    out
}

pub fn bucket_label(tz: Tz, t_ms: i64, granularity: Granularity) -> NaiveDateTime {
    let local = tz.timestamp_millis_opt(t_ms).unwrap().naive_local();
    match granularity {
        Granularity::Hour => local.date().and_hms_opt(local.hour(), 0, 0).unwrap(),
        Granularity::Day => local.date().and_hms_opt(0, 0, 0).unwrap(),
    }
}

/// `num / den` rounded half to even to 4 places (0004 Encoding conventions).
pub fn ratio4(num: i128, den: i128) -> Json {
    if den == 0 {
        return Json::Null;
    }
    let negative = (num < 0) != (den < 0);
    let (n, d) = (num.abs() * 10_000, den.abs());
    let (mut q, r) = (n / d, n % d);
    if 2 * r > d || (2 * r == d && q % 2 == 1) {
        q += 1;
    }
    let value = q as f64 / 10_000.0;
    json!(if negative { -value } else { value })
}

/// Integer division rounded half to even.
pub fn div_half_even(num: u64, den: u64) -> u64 {
    let (q, r) = (num / den, num % den);
    if 2 * r > den || (2 * r == den && q % 2 == 1) {
        q + 1
    } else {
        q
    }
}

fn compare(current: u64, previous: Option<u64>) -> Json {
    match previous {
        None => json!(current),
        Some(previous) => {
            let mut out = json!({
                "current": current,
                "previous": previous,
                "change": current as i64 - previous as i64,
                "relative_change": ratio4(current as i128 - previous as i128, previous as i128),
            });
            if previous == 0 {
                out["reason"] = json!("zero_baseline");
            }
            out
        }
    }
}

fn rate(t: &Totals) -> Json {
    let mut out = json!({ "value": ratio4(t.bounces as i128, t.engaged_sessions as i128), "numerator": t.bounces, "denominator": t.engaged_sessions });
    if t.engaged_sessions == 0 {
        out["reason"] = json!("no_denominator");
    }
    out
}

fn span_mean(t: &Totals) -> Json {
    let value = (t.engaged_sessions > 0).then(|| div_half_even(t.span_sum_ms, t.engaged_sessions));
    json!({ "value": value, "sum_ms": t.span_sum_ms, "sessions": t.engaged_sessions })
}

fn measure(name: &str, current: &Totals, previous: Option<&Totals>) -> Json {
    let pair = |f: &dyn Fn(&Totals) -> Json| match previous {
        None => f(current),
        Some(p) => json!({ "current": f(current), "previous": f(p) }),
    };
    match name {
        "pageviews" => compare(current.pageviews, previous.map(|p| p.pageviews)),
        "events" => compare(current.events, previous.map(|p| p.events)),
        "visitor_days" => compare(current.visitor_days, previous.map(|p| p.visitor_days)),
        "sessions" => compare(current.sessions, previous.map(|p| p.sessions)),
        "bounce_rate" => pair(&rate),
        "span_mean" => pair(&span_mean),
        "span_median" => pair(&|t: &Totals| json!(t.span_median_ms)),
        other => panic!("measure {other} is outside the early-gate subset"),
    }
}

fn coverage(t: &Totals) -> Json {
    json!({ "with_identity": t.with_identity, "eligible_events": t.eligible, "rate": ratio4(t.with_identity as i128, t.eligible as i128) })
}

fn cell(c: &Cell) -> Json {
    match c {
        Some(v) => json!({ "t": "string", "v": v }),
        None => json!({ "t": "not_set" }),
    }
}

fn instant(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms)
        .unwrap()
        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
        .to_string()
}

/// Renders the canonical document and its row count.
pub fn document(case: &Case, raw: &Raw) -> (Json, u64) {
    let mut doc = json!({ "case": case.id, "metric_version": 1, "resolved": case.resolved });
    let row_count = match raw {
        Raw::Overview {
            current,
            previous,
            series,
        } => {
            let totals: serde_json::Map<String, Json> = case
                .measures
                .iter()
                .map(|m| (m.clone(), measure(m, current, previous.as_ref())))
                .collect();
            let starts = buckets(case.timezone, case.current, case.granularity);
            assert_eq!(starts.len(), series.len(), "{}: series length", case.id);
            let points: Vec<Json> = starts
                .iter()
                .zip(series)
                .map(|((_, start), t)| {
                    json!({
                        "start": instant(*start),
                        "pageviews": t.pageviews,
                        "visitor_days": t.visitor_days,
                        "sessions": t.sessions,
                        "bounces": t.bounces,
                        "engaged_sessions": t.engaged_sessions,
                        "span_sum_ms": t.span_sum_ms,
                    })
                })
                .collect();
            let series_sum: u64 = series.iter().map(|t| t.visitor_days).sum();
            doc["totals"] = Json::Object(totals);
            doc["series"] = Json::Array(points);
            doc["series_sum_differs"] = json!(series_sum > current.visitor_days);
            doc["identity_coverage"] = coverage(current);
            series.len() as u64
        }
        Raw::Table {
            current,
            previous,
            rows,
            row_count,
        } => {
            doc["totals"] = Json::Object(
                case.measures
                    .iter()
                    .map(|m| (m.clone(), measure(m, current, previous.as_ref())))
                    .collect(),
            );
            doc["row_count"] = json!(row_count);
            doc["page"] = json!({ "size": case.page_size, "index": case.page_index });
            doc["rows"] = Json::Array(
                rows.iter()
                    .map(|row| {
                        let mut out: serde_json::Map<String, Json> = case
                            .dimensions
                            .iter()
                            .zip(&row.key)
                            .map(|(d, c)| (d.clone(), cell(c)))
                            .collect();
                        for m in &case.measures {
                            out.insert(m.clone(), measure(m, &row.current, row.previous.as_ref()));
                        }
                        Json::Object(out)
                    })
                    .collect(),
            );
            doc["identity_coverage"] = coverage(current);
            *row_count
        }
        Raw::Funnel {
            reached,
            with_identity,
            eligible,
        } => {
            let funnel = case.funnel.as_ref().expect("funnel case");
            doc["window_seconds"] = json!(funnel.window_ms / 1000);
            doc["scope"] = json!("daily");
            doc["steps_reached"] = json!(reached);
            doc["step_drop"] = json!(reached.windows(2).map(|w| w[0] - w[1]).collect::<Vec<_>>());
            doc["conversion"] = json!({
                "value": ratio4(*reached.last().unwrap() as i128, reached[0] as i128),
                "numerator": reached.last(),
                "denominator": reached[0],
            });
            doc["limitation"] = json!("daily scope cannot cross local midnight");
            doc["identity_coverage"] = coverage(&Totals {
                with_identity: *with_identity,
                eligible: *eligible,
                ..Totals::default()
            });
            reached.len() as u64
        }
    };
    (doc, row_count)
}

/// RFC 8785 canonical JSON: sorted keys, no whitespace, shortest numbers.
pub fn canonical(value: &Json) -> String {
    let mut out = String::new();
    write_canonical(value, &mut out);
    out
}

fn write_canonical(value: &Json, out: &mut String) {
    match value {
        Json::Null | Json::Bool(_) | Json::String(_) => out.push_str(&value.to_string()),
        Json::Number(n) => match (n.as_i64(), n.as_u64(), n.as_f64()) {
            (Some(i), _, _) => out.push_str(&i.to_string()),
            (_, Some(u), _) => out.push_str(&u.to_string()),
            (_, _, Some(f)) if f.fract() == 0.0 && f.abs() < 1e21 => {
                out.push_str(&format!("{f:.0}"))
            }
            (_, _, Some(f)) => out.push_str(&format!("{f}")),
            _ => unreachable!("JSON numbers are i64, u64 or f64"),
        },
        Json::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_canonical(item, out);
            }
            out.push(']');
        }
        Json::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
            out.push('{');
            for (i, key) in keys.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&Json::String((*key).clone()).to_string());
                out.push(':');
                write_canonical(&map[*key], out);
            }
            out.push('}');
        }
    }
}

pub fn sha256_hex(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Dimension sort order of 0004: values ascending in UTF-8 byte order, not set last.
pub fn cell_order(a: &Cell, b: &Cell) -> std::cmp::Ordering {
    match (a, b) {
        (Some(a), Some(b)) => a.as_bytes().cmp(b.as_bytes()),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounding_is_half_even() {
        assert_eq!(ratio4(1, 8), json!(0.125));
        assert_eq!(ratio4(5, 2), json!(2.5));
        assert_eq!(ratio4(1, 3), json!(0.3333));
        assert_eq!(ratio4(-5, 3), json!(-1.6667));
        assert_eq!(ratio4(1, 0), Json::Null);
        assert_eq!(div_half_even(5, 2), 2);
        assert_eq!(div_half_even(7, 2), 4);
    }

    #[test]
    fn canonical_json_sorts_keys_and_trims_numbers() {
        let v = json!({ "b": 2.0, "a": [1, 0.5, "x"], "é": null });
        assert_eq!(canonical(&v), r#"{"a":[1,0.5,"x"],"b":2,"é":null}"#);
    }
}
