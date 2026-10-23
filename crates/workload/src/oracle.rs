//! Independent streaming oracle. It reads generated groups directly and
//! computes golden results with plain Rust collections. It shares no code with
//! the storage crate or the engine probes: identities are labeled by their
//! inputs, sessions are rebuilt from group segments, and filters are evaluated
//! on generated values.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write as _;
use std::sync::Mutex;

use serde_json::{Value as Json, json};
use sha2::{Digest, Sha256};

use crate::cases::{Case, Cmp, Field, Node, Op, Range};
use crate::fixture::{Fixture, GenEvent, Group, Kind, MONTHS, Value};
use crate::result::{self, Cell, Raw, Row, Totals};

const SESSION_GAP_MS: i64 = 30 * 60 * 1000;

pub struct Golden {
    pub doc: Json,
    pub row_count: u64,
    pub matched_records: u64,
    pub population: u64,
    pub hash: String,
}

fn label(parts: &[&[u8]]) -> u128 {
    let mut hasher = blake3::Hasher::new();
    for part in parts {
        hasher.update(&(part.len() as u32).to_le_bytes());
        hasher.update(part);
    }
    u128::from_le_bytes(hasher.finalize().as_bytes()[..16].try_into().unwrap())
}

/// Daily identity label: the inputs of the daily pseudonym (0001), not its keyed value.
fn daily_key(group: &Group) -> u128 {
    let day = group.local_day.to_string();
    match &group.supplied_id {
        Some(id) => label(&[
            b"daily-id",
            &[group.site as u8],
            day.as_bytes(),
            id.as_bytes(),
        ]),
        None => label(&[
            b"anon",
            &[group.site as u8],
            day.as_bytes(),
            group.ip.to_string().as_bytes(),
            group.user_agent.as_bytes(),
        ]),
    }
}

/// Session identity: the persistent profile for consented groups, else the daily identity.
fn session_key(group: &Group) -> u128 {
    match &group.supplied_id {
        Some(id) => label(&[b"profile", &[group.site as u8], id.as_bytes()]),
        None => daily_key(group),
    }
}

fn eligible(e: &GenEvent) -> bool {
    matches!(e.kind, Kind::Pageview | Kind::Custom)
}

struct Ctx<'a> {
    fx: &'a Fixture,
    group: &'a Group,
    event: &'a GenEvent,
}

impl Ctx<'_> {
    fn dimension(&self, name: &str) -> Option<&'static str> {
        let (cat, g) = (self.fx.cat, self.group);
        match name {
            "path" => Some(self.event.path),
            "event_name" => self.event.event_name,
            "device" => Some(cat.device.values[g.device]),
            "browser" => Some(cat.browser.values[g.browser]),
            "os" => Some(cat.os.values[g.os]),
            "country" => Some(cat.countries.values[g.country]),
            "referrer_domain" => g.referrer.map(|r| cat.referrer.values[r]),
            other => panic!("dimension {other} is outside the early-gate subset"),
        }
    }

    fn string_property(&self, key: &str) -> Option<Option<&'static str>> {
        self.event
            .properties
            .iter()
            .find(|(k, _)| self.fx.cat.keys[*k].name == key)
            .map(|(_, v)| match v {
                Value::Str(s) => Some(*s),
                _ => None,
            })
    }

    fn matches(&self, node: &Node) -> bool {
        match node {
            Node::Group { op: Op::And, items } => items.iter().all(|n| self.matches(n)),
            Node::Group { op: Op::Or, items } => items.iter().any(|n| self.matches(n)),
            Node::Cond { field, cmp, value } => {
                let value = value.as_deref();
                // Present string value of the field, and whether the key is present at all.
                let (string, present) = match field {
                    Field::Dimension(name) => {
                        let v = self.dimension(name);
                        (v, v.is_some())
                    }
                    Field::EventProperty(key) => match self.string_property(key) {
                        Some(v) => (v, true),
                        None => (None, false),
                    },
                };
                let positive = |cmp: Cmp| match cmp {
                    Cmp::Eq => string == value,
                    Cmp::Contains => string.is_some_and(|s| s.contains(value.unwrap())),
                    Cmp::StartsWith => string.is_some_and(|s| s.starts_with(value.unwrap())),
                    Cmp::IsSet => present,
                    _ => unreachable!(),
                };
                match cmp {
                    Cmp::Eq | Cmp::Contains | Cmp::StartsWith | Cmp::IsSet => positive(*cmp),
                    Cmp::Neq => !positive(Cmp::Eq),
                    Cmp::NotContains => !positive(Cmp::Contains),
                    Cmp::IsNotSet => !positive(Cmp::IsSet),
                }
            }
        }
    }
}

#[derive(Default)]
struct Span {
    pageviews: u64,
    events: u64,
    keys: HashSet<u128>,
    with_identity: u64,
    eligible: u64,
}

#[derive(Default)]
struct OverviewPart {
    current: Span,
    previous: Span,
    series_pageviews: Vec<u64>,
    series_keys: HashSet<(u32, u128)>,
}

#[derive(Default)]
struct TablePart {
    current: Span,
    previous: Span,
    rows: HashMap<Vec<Cell>, [u64; 2]>,
    row_keys: HashSet<(Vec<Cell>, u128)>,
    previous_rows: HashMap<Vec<Cell>, [u64; 2]>,
    previous_row_keys: HashSet<(Vec<Cell>, u128)>,
}

#[derive(Default)]
struct FunnelPart {
    events: HashMap<u128, Vec<(i64, u64, u8)>>,
    with_identity: u64,
    eligible: u64,
}

enum Part {
    Overview(OverviewPart),
    Table(TablePart),
    Funnel(FunnelPart),
}

#[derive(Clone, Copy)]
struct Segment {
    site: u8,
    key: u128,
    first: i64,
    last: i64,
    pageviews: u32,
    customs: u32,
}

/// Current and previous totals plus (pageviews, visitor-days) per series bucket.
type OverviewMonth = (Totals, Totals, Vec<(u64, u64)>);

/// Per-month results, merged after all months finish.
struct MonthOut {
    overview: Vec<Option<OverviewMonth>>,
    tables: Vec<Option<TableMonth>>,
    funnels: Vec<Option<(Vec<u64>, u64, u64)>>,
    matched: Vec<u64>,
    population: Vec<u64>,
    segments: Vec<Segment>,
    inventory: Inventory,
    hash: String,
}

#[derive(Default)]
struct TableMonth {
    current: Totals,
    previous: Totals,
    rows: HashMap<Vec<Cell>, Totals>,
    previous_rows: HashMap<Vec<Cell>, Totals>,
}

fn span_totals(span: &Span) -> Totals {
    Totals {
        pageviews: span.pageviews,
        events: span.events,
        visitor_days: span.keys.len() as u64,
        with_identity: span.with_identity,
        eligible: span.eligible,
        ..Totals::default()
    }
}

fn add(a: &mut Totals, b: &Totals) {
    a.pageviews += b.pageviews;
    a.events += b.events;
    a.visitor_days += b.visitor_days;
    a.with_identity += b.with_identity;
    a.eligible += b.eligible;
}

pub struct Oracle<'a> {
    fx: &'a Fixture,
    cases: &'a [Case],
    bucket_index: Vec<HashMap<chrono::NaiveDateTime, u32>>,
}

impl<'a> Oracle<'a> {
    pub fn new(fx: &'a Fixture, cases: &'a [Case]) -> Self {
        let bucket_index = cases
            .iter()
            .map(|c| {
                if c.has_series() {
                    result::buckets(c.timezone, c.current, c.granularity)
                        .into_iter()
                        .enumerate()
                        .map(|(i, (label, _))| (label, i as u32))
                        .collect()
                } else {
                    HashMap::new()
                }
            })
            .collect();
        Oracle {
            fx,
            cases,
            bucket_index,
        }
    }

    fn site_index(&self, case: &Case) -> usize {
        self.fx
            .cat
            .sites
            .iter()
            .position(|s| s.code == case.site)
            .unwrap()
    }

    /// Streams every month on `threads` workers and returns golden results and the inventory.
    pub fn run(
        &self,
        threads: usize,
        groups_per_month: u64,
        sample: &mut Vec<Json>,
    ) -> (Vec<Golden>, Json) {
        let next = Mutex::new(0u64);
        let outputs: Mutex<Vec<(u64, MonthOut)>> = Mutex::new(Vec::new());
        let sample_out = Mutex::new(Vec::new());
        std::thread::scope(|scope| {
            for _ in 0..threads {
                scope.spawn(|| {
                    loop {
                        let month = {
                            let mut n = next.lock().unwrap();
                            if *n == MONTHS {
                                return;
                            }
                            *n += 1;
                            *n - 1
                        };
                        let started = std::time::Instant::now();
                        let out = self.month(
                            month,
                            groups_per_month,
                            if month == 11 { Some(&sample_out) } else { None },
                        );
                        eprintln!("oracle: month {month} done in {:.0?}", started.elapsed());
                        outputs.lock().unwrap().push((month, out));
                    }
                });
            }
        });
        *sample = sample_out.into_inner().unwrap();
        let mut months = outputs.into_inner().unwrap();
        months.sort_by_key(|(m, _)| *m);
        self.merge(months.into_iter().map(|(_, out)| out).collect())
    }

    fn month(
        &self,
        month: u64,
        groups_per_month: u64,
        sample: Option<&Mutex<Vec<Json>>>,
    ) -> MonthOut {
        let fx = self.fx;
        let session_sites: HashSet<usize> = self
            .cases
            .iter()
            .filter(|c| c.has_series())
            .map(|c| self.site_index(c))
            .collect();
        let sites: Vec<usize> = self.cases.iter().map(|c| self.site_index(c)).collect();
        let mut parts: Vec<Part> = self
            .cases
            .iter()
            .enumerate()
            .map(|(i, c)| {
                if c.funnel.is_some() {
                    Part::Funnel(FunnelPart::default())
                } else if c.has_series() {
                    Part::Overview(OverviewPart {
                        series_pageviews: vec![0; self.bucket_index[i].len()],
                        ..OverviewPart::default()
                    })
                } else {
                    Part::Table(TablePart::default())
                }
            })
            .collect();
        let mut matched = vec![0u64; self.cases.len()];
        let mut population = vec![0u64; self.cases.len()];
        let mut segments = Vec::new();
        let mut inventory = Inventory::default();
        let mut hasher = Sha256::new();
        let mut line = String::new();

        for group in fx.month(month, groups_per_month) {
            inventory.group(fx, &group);
            canonical_lines(fx, &group, &mut line);
            hasher.update(line.as_bytes());
            if let Some(sample) = sample
                && group.m < 1000
            {
                sample.lock().unwrap().push(sample_json(fx, &group));
            }
            let daily = daily_key(&group);
            if session_sites.contains(&group.site) {
                let times: Vec<&GenEvent> = group.events.iter().filter(|e| eligible(e)).collect();
                segments.push(Segment {
                    site: group.site as u8,
                    key: session_key(&group),
                    first: times.first().unwrap().time_s * 1000,
                    last: times.last().unwrap().time_s * 1000,
                    pageviews: times.iter().filter(|e| e.kind == Kind::Pageview).count() as u32,
                    customs: times.iter().filter(|e| e.kind == Kind::Custom).count() as u32,
                });
            }
            for (i, case) in self.cases.iter().enumerate() {
                if sites[i] != group.site {
                    continue;
                }
                for event in group.events.iter().filter(|e| eligible(e)) {
                    let t = event.time_s * 1000;
                    let ctx = Ctx {
                        fx,
                        group: &group,
                        event,
                    };
                    let in_current = case.current.contains(t);
                    if in_current {
                        population[i] += 1;
                    }
                    match &mut parts[i] {
                        Part::Overview(part) => {
                            for (range, span, current) in [
                                (case.current, &mut part.current, true),
                                (case.previous.unwrap_or(EMPTY), &mut part.previous, false),
                            ] {
                                if !range.contains(t) {
                                    continue;
                                }
                                count(span, event, daily);
                                if current {
                                    matched[i] += 1;
                                    let label =
                                        result::bucket_label(case.timezone, t, case.granularity);
                                    let bucket = self.bucket_index[i][&label];
                                    if event.kind == Kind::Pageview {
                                        part.series_pageviews[bucket as usize] += 1;
                                    }
                                    part.series_keys.insert((bucket, daily));
                                }
                            }
                        }
                        Part::Table(part) => {
                            let in_previous = case.previous.is_some_and(|p| p.contains(t));
                            if !(in_current || in_previous)
                                || !case.filter.as_ref().is_none_or(|f| ctx.matches(f))
                            {
                                continue;
                            }
                            let key: Vec<Cell> = case
                                .dimensions
                                .iter()
                                .map(|d| ctx.dimension(d).map(str::to_owned))
                                .collect();
                            let (span, rows, row_keys) = if in_current {
                                matched[i] += 1;
                                (&mut part.current, &mut part.rows, &mut part.row_keys)
                            } else {
                                (
                                    &mut part.previous,
                                    &mut part.previous_rows,
                                    &mut part.previous_row_keys,
                                )
                            };
                            count(span, event, daily);
                            let counts = rows.entry(key.clone()).or_default();
                            counts[0] += (event.kind == Kind::Pageview) as u64;
                            counts[1] += (event.kind == Kind::Custom) as u64;
                            row_keys.insert((key, daily));
                        }
                        Part::Funnel(part) => {
                            let funnel = case.funnel.as_ref().unwrap();
                            if in_current {
                                part.eligible += 1;
                                part.with_identity += 1;
                            }
                            if !(case.current.start <= t && t < case.current.end + funnel.window_ms)
                            {
                                continue;
                            }
                            let mask = funnel
                                .steps
                                .iter()
                                .enumerate()
                                .filter(|(_, step)| ctx.matches(step))
                                .fold(0u8, |m, (k, _)| m | 1 << k);
                            if mask != 0 {
                                if in_current {
                                    matched[i] += 1;
                                }
                                part.events
                                    .entry(daily)
                                    .or_default()
                                    .push((t, event.e, mask));
                            }
                        }
                    }
                }
            }
        }

        let mut out = MonthOut {
            overview: Vec::new(),
            tables: Vec::new(),
            funnels: Vec::new(),
            matched,
            population,
            segments,
            inventory,
            hash: hasher
                .finalize()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect(),
        };
        for (i, part) in parts.into_iter().enumerate() {
            let case = &self.cases[i];
            let (mut overview, mut table, mut funnel) = (None, None, None);
            match part {
                Part::Overview(p) => {
                    let mut series: Vec<(u64, u64)> =
                        p.series_pageviews.iter().map(|pv| (*pv, 0)).collect();
                    for (bucket, _) in &p.series_keys {
                        series[*bucket as usize].1 += 1;
                    }
                    overview = Some((span_totals(&p.current), span_totals(&p.previous), series));
                }
                Part::Table(p) => {
                    let rows = |rows: HashMap<Vec<Cell>, [u64; 2]>,
                                keys: HashSet<(Vec<Cell>, u128)>| {
                        let mut out: HashMap<Vec<Cell>, Totals> = rows
                            .into_iter()
                            .map(|(k, [pv, ev])| {
                                (
                                    k,
                                    Totals {
                                        pageviews: pv,
                                        events: ev,
                                        ..Totals::default()
                                    },
                                )
                            })
                            .collect();
                        for (k, _) in keys {
                            out.get_mut(&k).unwrap().visitor_days += 1;
                        }
                        out
                    };
                    table = Some(TableMonth {
                        current: span_totals(&p.current),
                        previous: span_totals(&p.previous),
                        rows: rows(p.rows, p.row_keys),
                        previous_rows: rows(p.previous_rows, p.previous_row_keys),
                    });
                }
                Part::Funnel(p) => {
                    let funnel_def = case.funnel.as_ref().unwrap();
                    let mut reached = vec![0u64; funnel_def.steps.len()];
                    for (_, mut events) in p.events {
                        events.sort_unstable();
                        for pair in events.windows(2) {
                            assert!(
                                pair[0].0 != pair[1].0,
                                "equal event times within one identity need ingestion order"
                            );
                        }
                        let Some(entry) = events
                            .iter()
                            .position(|(t, _, m)| m & 1 != 0 && case.current.contains(*t))
                        else {
                            continue;
                        };
                        let t1 = events[entry].0;
                        reached[0] += 1;
                        let mut at = entry;
                        for (k, slot) in reached.iter_mut().enumerate().skip(1) {
                            match events[at + 1..].iter().position(|(t, _, m)| {
                                m & (1 << k) != 0 && *t < t1 + funnel_def.window_ms
                            }) {
                                Some(offset) => {
                                    at += 1 + offset;
                                    *slot += 1;
                                }
                                None => break,
                            }
                        }
                    }
                    funnel = Some((reached, p.with_identity, p.eligible));
                }
            }
            out.overview.push(overview);
            out.tables.push(table);
            out.funnels.push(funnel);
        }
        out
    }

    fn merge(&self, months: Vec<MonthOut>) -> (Vec<Golden>, Json) {
        let mut segments: Vec<Segment> = months
            .iter()
            .flat_map(|m| m.segments.iter().copied())
            .collect();
        segments.sort_unstable_by_key(|s| (s.site, s.key, s.first));
        let mut sessions: Vec<Segment> = Vec::new();
        for s in segments {
            match sessions.last_mut() {
                Some(open)
                    if open.site == s.site
                        && open.key == s.key
                        && s.first - open.last < SESSION_GAP_MS =>
                {
                    open.last = open.last.max(s.last);
                    open.pageviews += s.pageviews;
                    open.customs += s.customs;
                }
                _ => sessions.push(s),
            }
        }

        let mut goldens = Vec::new();
        for (i, case) in self.cases.iter().enumerate() {
            let site = self.site_index(case) as u8;
            let matched: u64 = months.iter().map(|m| m.matched[i]).sum();
            let population: u64 = months.iter().map(|m| m.population[i]).sum();
            let raw = if let Some(funnel) = &case.funnel {
                let mut reached = vec![0u64; funnel.steps.len()];
                let (mut with_identity, mut eligible) = (0, 0);
                for m in &months {
                    let (r, w, e) = m.funnels[i].as_ref().unwrap();
                    reached.iter_mut().zip(r).for_each(|(a, b)| *a += b);
                    with_identity += w;
                    eligible += e;
                }
                Raw::Funnel {
                    reached,
                    with_identity,
                    eligible,
                }
            } else if case.has_series() {
                let n = self.bucket_index[i].len();
                let mut current = Totals::default();
                let mut previous = Totals::default();
                let mut series = vec![Totals::default(); n];
                for m in &months {
                    let (c, p, s) = m.overview[i].as_ref().unwrap();
                    add(&mut current, c);
                    add(&mut previous, p);
                    for (point, (pv, vd)) in series.iter_mut().zip(s) {
                        point.pageviews += pv;
                        point.visitor_days += vd;
                    }
                }
                let mut spans = Vec::new();
                let mut previous_spans = Vec::new();
                for s in sessions.iter().filter(|s| s.site == site) {
                    let span = (s.last - s.first) as u64;
                    let bounce = s.pageviews == 1 && s.customs == 0;
                    if case.current.contains(s.first) {
                        let bucket = self.bucket_index[i]
                            [&result::bucket_label(case.timezone, s.first, case.granularity)];
                        let point = &mut series[bucket as usize];
                        current.sessions += 1;
                        point.sessions += 1;
                        if s.pageviews > 0 {
                            for t in [&mut current, &mut *point] {
                                t.engaged_sessions += 1;
                                t.span_sum_ms += span;
                                t.bounces += bounce as u64;
                            }
                            spans.push(span);
                        }
                    } else if case.previous.is_some_and(|p| p.contains(s.first)) {
                        previous.sessions += 1;
                        if s.pageviews > 0 {
                            previous.engaged_sessions += 1;
                            previous.span_sum_ms += span;
                            previous.bounces += bounce as u64;
                            previous_spans.push(span);
                        }
                    }
                }
                current.span_median_ms = nearest_rank_median(&mut spans);
                previous.span_median_ms = nearest_rank_median(&mut previous_spans);
                Raw::Overview {
                    current,
                    previous: case.previous.map(|_| previous),
                    series,
                }
            } else {
                let mut current = Totals::default();
                let mut previous = Totals::default();
                let mut rows: HashMap<Vec<Cell>, (Totals, Totals)> = HashMap::new();
                for m in &months {
                    let t = m.tables[i].as_ref().unwrap();
                    add(&mut current, &t.current);
                    add(&mut previous, &t.previous);
                    for (k, v) in &t.rows {
                        add(&mut rows.entry(k.clone()).or_default().0, v);
                    }
                    for (k, v) in &t.previous_rows {
                        add(&mut rows.entry(k.clone()).or_default().1, v);
                    }
                }
                let sort = case.sort_by.as_deref().unwrap_or("events");
                let metric = |t: &Totals| match sort {
                    "pageviews" => t.pageviews,
                    "events" => t.events,
                    "visitor_days" => t.visitor_days,
                    other => panic!("sort by {other} is outside the early-gate subset"),
                };
                let mut rows: Vec<Row> = rows
                    .into_iter()
                    .map(|(key, (c, p))| Row {
                        key,
                        current: c,
                        previous: case.previous.map(|_| p),
                    })
                    .collect();
                rows.sort_by(|a, b| {
                    metric(&b.current).cmp(&metric(&a.current)).then_with(|| {
                        a.key
                            .iter()
                            .zip(&b.key)
                            .map(|(x, y)| result::cell_order(x, y))
                            .find(|o| o.is_ne())
                            .unwrap_or(std::cmp::Ordering::Equal)
                    })
                });
                let row_count = rows.len() as u64;
                let page: Vec<Row> = rows
                    .into_iter()
                    .skip(case.page_index * case.page_size)
                    .take(case.page_size)
                    .collect();
                Raw::Table {
                    current,
                    previous: case.previous.map(|_| previous),
                    rows: page,
                    row_count,
                }
            };
            let (doc, row_count) = result::document(case, &raw);
            let hash = result::sha256_hex(&result::canonical(&doc));
            goldens.push(Golden {
                doc,
                row_count,
                matched_records: matched,
                population,
                hash,
            });
        }

        let mut inventory = Inventory::default();
        let mut hashes = BTreeMap::new();
        let mut all = Sha256::new();
        for (month, m) in months.into_iter().enumerate() {
            inventory.merge(m.inventory);
            all.update(m.hash.as_bytes());
            hashes.insert(format!("{month:02}"), m.hash);
        }
        let mut report = inventory.report(self.fx);
        report["canonical_input_sha256"] = json!({
            "per_month": hashes,
            "all_months": all.finalize().iter().map(|b| format!("{b:02x}")).collect::<String>(),
            "line_format": "docs/evidence/m02-storage-gate.md, canonical input lines",
        });
        (goldens, report)
    }
}

const EMPTY: Range = Range { start: 0, end: 0 };

fn count(span: &mut Span, event: &GenEvent, daily: u128) {
    span.pageviews += (event.kind == Kind::Pageview) as u64;
    span.events += (event.kind == Kind::Custom) as u64;
    span.eligible += 1;
    span.with_identity += 1;
    span.keys.insert(daily);
}

fn nearest_rank_median(values: &mut [u64]) -> Option<u64> {
    if values.is_empty() {
        return None;
    }
    values.sort_unstable();
    Some(values[values.len().div_ceil(2) - 1])
}

fn value_text(v: &Value) -> String {
    match v {
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Int(i) => i.to_string(),
        Value::Dec(d) => format!(
            "{}{}.{:04}",
            if *d < 0 { "-" } else { "" },
            d.abs() / 10_000,
            d.abs() % 10_000
        ),
        Value::Date(ms) => chrono::DateTime::from_timestamp_millis(*ms)
            .unwrap()
            .format("%Y-%m-%dT%H:%M:%S%.3fZ")
            .to_string(),
        Value::Str(s) => s.to_string(),
        Value::Array(items) => format!(
            "[{}]",
            items.iter().map(value_text).collect::<Vec<_>>().join(",")
        ),
    }
}

fn type_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Int(_) => "integer",
        Value::Dec(_) => "decimal",
        Value::Date(_) => "date",
        Value::Str(_) => "string",
        Value::Array(_) => "array",
    }
}

/// Canonical input lines: one `G` line per group, one `E` line per event,
/// tab-separated, `-` for absent values, properties as `key=type:value` joined by `;`.
fn canonical_lines(fx: &Fixture, g: &Group, out: &mut String) {
    let cat = fx.cat;
    out.clear();
    let opt = |v: Option<&str>| v.unwrap_or("-").to_owned();
    let _ = writeln!(
        out,
        "G\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
        g.g,
        cat.sites[g.site].code,
        g.local_day,
        g.t0_s,
        g.ip,
        g.user_agent,
        opt(g.supplied_id.as_deref()),
        cat.device.values[g.device],
        cat.browser.values[g.browser],
        cat.os.values[g.os],
        cat.countries.values[g.country],
        g.region,
        opt(g.referrer.map(|r| cat.referrer.values[r])),
        opt(g.campaign.map(|c| cat.campaign.values[c])),
        g.identify.map_or("-".into(), |(tier, created)| format!(
            "{}:{}",
            cat.account_tiers.0[tier], created
        )),
    );
    for e in &g.events {
        let props: Vec<String> = e
            .properties
            .iter()
            .map(|(k, v)| format!("{}={}:{}", cat.keys[*k].name, type_name(v), value_text(v)))
            .collect();
        let _ = writeln!(
            out,
            "E\t{}\t{}\t{:?}\t{}\t{}\t{}\t{}\t{}\t{}",
            e.e,
            e.time_s,
            e.kind,
            e.path,
            opt(e.query),
            opt(e.event_name),
            e.revenue.map_or("-".into(), |(c, a)| format!(
                "{}:{}",
                cat.currencies[c].code, a
            )),
            e.performance.map_or("-".into(), |p| p
                .iter()
                .map(|v| v.map_or("-".into(), |x| x.to_string()))
                .collect::<Vec<_>>()
                .join(",")),
            props.join(";"),
        );
    }
}

fn sample_json(fx: &Fixture, g: &Group) -> Json {
    let cat = fx.cat;
    json!({
        "group": g.g,
        "site": cat.sites[g.site].code,
        "local_day": g.local_day.to_string(),
        "client": { "ip": g.ip.to_string(), "user_agent": g.user_agent },
        "id": g.supplied_id,
        "device": cat.device.values[g.device],
        "browser": cat.browser.values[g.browser],
        "os": cat.os.values[g.os],
        "country": cat.countries.values[g.country],
        "region": format!("{}-R{}", cat.countries.values[g.country], g.region),
        "referrer_domain": g.referrer.map(|r| cat.referrer.values[r]),
        "utm_campaign": g.campaign.map(|c| cat.campaign.values[c]),
        "planted": g.planted,
        "identify": g.identify.map(|(tier, created)| json!({ "account_tier": cat.account_tiers.0[tier], "account_created": value_text(&Value::Date(created)) })),
        "events": g.events.iter().map(|e| json!({
            "ordinal": e.e,
            "id": crate::catalog::fixture_uuid(&format!("event:{}", e.e)).to_string(),
            "time": value_text(&Value::Date(e.time_s * 1000)),
            "kind": format!("{:?}", e.kind).to_lowercase(),
            "path": e.path,
            "query": e.query,
            "event_name": e.event_name,
            "revenue": e.revenue.map(|(c, a)| json!({ "currency": cat.currencies[c].code, "minor_units": a })),
            "performance": e.performance,
            "properties": e.properties.iter().map(|(k, v)| json!({ "key": cat.keys[*k].name, "type": type_name(v), "value": value_text(v) })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    })
}

/// Generation inventory: exact counts the manifest leaves to first generation.
#[derive(Default)]
struct Inventory {
    kinds: BTreeMap<String, u64>,
    sites: BTreeMap<String, u64>,
    site_local_day: BTreeMap<String, u64>,
    utc_day: BTreeMap<String, u64>,
    site_utc_month: BTreeMap<String, u64>,
    groups: u64,
    identified_groups: u64,
    identified_events: u64,
    planted: BTreeMap<String, u64>,
    records_with_properties: u64,
    property_values: u64,
    keys: BTreeMap<String, KeyStats>,
    revenue: BTreeMap<String, [i64; 4]>,
    perf: BTreeMap<(usize, usize), Vec<i64>>,
    joint: BTreeMap<String, BTreeMap<String, u64>>,
    session_property_values: u64,
    /// Distinct supplied IDs per site: the site-scoped person count.
    people: BTreeMap<String, HashSet<String>>,
    groups_per_local_day: BTreeMap<u64, Vec<u64>>,
}

#[derive(Default)]
struct KeyStats {
    rows: u64,
    nulls: u64,
    types: BTreeMap<&'static str, u64>,
    distinct: HashSet<u64>,
    min_bytes: Option<usize>,
    max_bytes: usize,
    total_bytes: u64,
}

impl Inventory {
    fn group(&mut self, fx: &Fixture, g: &Group) {
        let cat = fx.cat;
        let site = cat.sites[g.site].code;
        self.groups += 1;
        let day_counts = self
            .groups_per_local_day
            .entry(g.month)
            .or_insert_with(|| vec![0; crate::fixture::days_in_month(g.month) as usize]);
        day_counts[fx.day_of(g.month, g.v).0 as usize] += 1;
        if let Some(id) = &g.supplied_id {
            self.people
                .entry(site.into())
                .or_default()
                .insert(id.clone());
            self.identified_groups += 1;
            self.identified_events += 10;
            self.session_property_values += 2;
        }
        if g.planted {
            *self.planted.entry(site.into()).or_default() += 1;
        }
        *self.sites.entry(site.into()).or_default() += 10;
        *self
            .site_local_day
            .entry(format!("{site} {}", g.local_day))
            .or_default() += 10;
        let device = cat.device.values[g.device];
        let country = cat.countries.values[g.country];
        let referrer = match g.referrer {
            Some(r) if r < 8 => cat.referrer.values[r],
            Some(_) => "(other)",
            None => "(not set)",
        };
        let top_country = if g.country < 8 { country } else { "(other)" };
        *self
            .joint
            .entry("country_by_device_groups".into())
            .or_default()
            .entry(format!("{country} {device}"))
            .or_default() += 1;
        *self
            .joint
            .entry("referrer_by_country_groups".into())
            .or_default()
            .entry(format!("{referrer} {top_country}"))
            .or_default() += 1;
        for e in &g.events {
            let day = chrono::DateTime::from_timestamp(e.time_s, 0).unwrap();
            *self
                .utc_day
                .entry(day.format("%Y-%m-%d").to_string())
                .or_default() += 1;
            *self
                .site_utc_month
                .entry(format!("{site} {}", day.format("%Y-%m")))
                .or_default() += 1;
            *self
                .kinds
                .entry(format!("{:?}", e.kind).to_lowercase())
                .or_default() += 1;
            if let Some(name) = e.event_name {
                *self
                    .joint
                    .entry("device_by_event_name".into())
                    .or_default()
                    .entry(format!("{device} {name}"))
                    .or_default() += 1;
            }
            if let Some((c, amount)) = e.revenue {
                let r = self
                    .revenue
                    .entry(cat.currencies[c].code.into())
                    .or_default();
                if amount > 0 {
                    r[0] += amount
                } else {
                    r[1] += amount
                }
                r[2] += 1;
                r[3] += (amount < 0) as i64;
            }
            if let Some(perf) = e.performance {
                for (metric, value) in perf.iter().enumerate() {
                    if let Some(v) = value {
                        self.perf.entry((metric, g.device)).or_default().push(*v);
                    }
                }
            }
            if !e.properties.is_empty() {
                self.records_with_properties += 1;
            }
            for (k, v) in &e.properties {
                self.property_values += 1;
                let key = &cat.keys[*k];
                if key.name == "plan" {
                    let plan = match v {
                        Value::Str(s) => s,
                        _ => "(null)",
                    };
                    *self
                        .joint
                        .entry("device_by_plan".into())
                        .or_default()
                        .entry(format!("{device} {plan}"))
                        .or_default() += 1;
                }
                let stats = self.keys.entry(key.name.into()).or_default();
                stats.rows += 1;
                *stats.types.entry(type_name(v)).or_default() += 1;
                if *v == Value::Null {
                    stats.nulls += 1;
                    continue;
                }
                let text = value_text(v);
                let bytes = match v {
                    Value::Str(s) => s.len(),
                    _ => text.len(),
                };
                stats.min_bytes = Some(stats.min_bytes.map_or(bytes, |m| m.min(bytes)));
                stats.max_bytes = stats.max_bytes.max(bytes);
                stats.total_bytes += bytes as u64;
                stats
                    .distinct
                    .insert(value_hash(&format!("{}:{}", type_name(v), text)));
            }
        }
    }

    fn merge(&mut self, o: Inventory) {
        fn sum<K: Ord>(a: &mut BTreeMap<K, u64>, b: BTreeMap<K, u64>) {
            for (k, v) in b {
                *a.entry(k).or_default() += v;
            }
        }
        sum(&mut self.kinds, o.kinds);
        sum(&mut self.sites, o.sites);
        sum(&mut self.site_local_day, o.site_local_day);
        sum(&mut self.utc_day, o.utc_day);
        sum(&mut self.site_utc_month, o.site_utc_month);
        sum(&mut self.planted, o.planted);
        self.groups += o.groups;
        self.identified_groups += o.identified_groups;
        self.identified_events += o.identified_events;
        self.records_with_properties += o.records_with_properties;
        self.property_values += o.property_values;
        self.session_property_values += o.session_property_values;
        for (site, ids) in o.people {
            self.people.entry(site).or_default().extend(ids);
        }
        for (k, v) in o.keys {
            let s = self.keys.entry(k).or_default();
            s.rows += v.rows;
            s.nulls += v.nulls;
            for (t, n) in v.types {
                *s.types.entry(t).or_default() += n;
            }
            s.distinct.extend(v.distinct);
            s.min_bytes = match (s.min_bytes, v.min_bytes) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, b) => a.or(b),
            };
            s.max_bytes = s.max_bytes.max(v.max_bytes);
            s.total_bytes += v.total_bytes;
        }
        for (k, v) in o.revenue {
            let r = self.revenue.entry(k).or_default();
            for i in 0..4 {
                r[i] += v[i];
            }
        }
        for (k, mut v) in o.perf {
            self.perf.entry(k).or_default().append(&mut v);
        }
        for (k, v) in o.joint {
            sum(self.joint.entry(k).or_default(), v);
        }
        for (k, v) in o.groups_per_local_day {
            let days = self
                .groups_per_local_day
                .entry(k)
                .or_insert_with(|| vec![0; v.len()]);
            days.iter_mut().zip(v).for_each(|(a, b)| *a += b);
        }
    }

    fn report(mut self, fx: &Fixture) -> Json {
        let cat = fx.cat;
        let total_records = self.kinds.values().sum::<u64>();
        let missing = |stats: &KeyStats| total_records - stats.rows;
        let keys: BTreeMap<String, Json> = self
            .keys
            .iter()
            .map(|(k, s)| {
                (k.clone(), json!({
                    "rows": s.rows,
                    "missing": missing(s),
                    "null": s.nulls,
                    "types": s.types,
                    "observed_cardinality": s.distinct.len(),
                    "utf8_bytes": { "min": s.min_bytes, "max": s.max_bytes, "total": s.total_bytes },
                }))
            })
            .collect();
        let revenue: BTreeMap<String, Json> = self
            .revenue
            .iter()
            .map(|(c, [gross, refunds, records, refund_records])| {
                (c.clone(), json!({ "gross_minor": gross, "refunds_minor": refunds, "net_minor": gross + refunds, "records": records, "refund_records": refund_records }))
            })
            .collect();
        let mut percentiles = BTreeMap::new();
        for ((metric, device), values) in self.perf.iter_mut() {
            values.sort_unstable();
            let rank = |p: f64| values[((p * values.len() as f64).ceil() as usize).max(1) - 1];
            percentiles.insert(
                format!("{} {}", cat.metrics[*metric].name, cat.device.values[*device]),
                json!({ "n": values.len(), "p50": rank(0.50), "p75": rank(0.75), "p95": rank(0.95) }),
            );
        }
        json!({
            "primary_events": total_records,
            "by_kind": self.kinds,
            "by_site": self.sites,
            "capture_groups": self.groups,
            "groups_per_local_day_by_month": self.groups_per_local_day,
            "identified_groups": self.identified_groups,
            "consent_coverage": { "linked_events": self.identified_events, "primary_events": total_records },
            "planted_groups_by_site": self.planted,
            "records_with_properties": self.records_with_properties,
            "property_values": self.property_values,
            "session_property_values": self.session_property_values,
            "identify_messages": self.identified_groups,
            "site_scoped_person_counts": self.people.iter().map(|(s, ids)| (s.clone(), ids.len())).collect::<BTreeMap<_, _>>(),
            "property_keys": keys,
            "revenue": revenue,
            "performance_percentiles_nearest_rank": percentiles,
            "joint_distributions": self.joint,
            "events_by_site_local_day": self.site_local_day,
            "events_by_utc_day": self.utc_day,
            "events_by_site_utc_month": self.site_utc_month,
        })
    }
}

fn value_hash(text: &str) -> u64 {
    u64::from_le_bytes(
        blake3::hash(text.as_bytes()).as_bytes()[..8]
            .try_into()
            .unwrap(),
    )
}
