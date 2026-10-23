//! Streaming generator for the retained 120M-event fixture.
//!
//! Every value is a closed-form function of a group or event ordinal, so a
//! month streams group by group without holding the dataset. Groups stream in
//! day-placement order (`v`), which keeps output roughly time-ordered.
//!
//! Where the manifest leaves a draw unspecified, this file states the choice
//! next to the code; the evidence report lists them.

use chrono::{Months, NaiveDate, NaiveTime, TimeZone};
use chrono_tz::Tz;

use crate::catalog::{Catalog, Dimension, Element, ListValue, PropertyKey, Rule};
use crate::prng::{Affine, Stream, permille, uniform};

pub const MONTHS: u64 = 12;
pub const GROUPS_PER_MONTH: u64 = 1_000_000;
pub const FIXTURE_START_S: i64 = 1_759_276_800; // 2025-10-01T00:00:00Z
pub const FIXTURE_END_S: i64 = 1_790_812_800; // 2026-10-01T00:00:00Z
const PAGEVIEW_RESIDUES: [u64; 15] = [0, 1, 2, 4, 5, 6, 8, 10, 11, 12, 14, 15, 16, 18, 19];
const CUSTOM_RESIDUES: [u64; 4] = [3, 7, 13, 17];
const PROPERTY_PAGEVIEWS: u64 = 56_000_000;
const REVENUE_RECORDS: u64 = 1_200_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Kind {
    Pageview,
    Custom,
    Performance,
}

/// A generated property value. Strings borrow the leaked catalog.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    /// 0.0001 units.
    Dec(i64),
    /// UTC milliseconds.
    Date(i64),
    Str(&'static str),
    Array(Vec<Value>),
}

#[derive(Debug, Clone)]
pub struct GenEvent {
    pub e: u64,
    pub slot: u64,
    pub time_s: i64,
    pub kind: Kind,
    pub path: &'static str,
    pub query: Option<&'static str>,
    pub event_name: Option<&'static str>,
    /// (currency index, signed minor units).
    pub revenue: Option<(usize, i64)>,
    /// (property key index, value) in slot order.
    pub properties: Vec<(usize, Value)>,
    /// lcp, fcp, ttfb, inp in ms and cls in 0.0001; `None` is missing.
    pub performance: Option<[Option<i64>; 5]>,
}

#[derive(Debug, Clone)]
pub struct Group {
    pub g: u64,
    pub month: u64,
    pub m: u64,
    pub v: u64,
    pub site: usize,
    pub local_day: NaiveDate,
    pub t0_s: i64,
    /// Supplied ID for identified (consented) groups.
    pub supplied_id: Option<String>,
    pub ip: std::net::Ipv6Addr,
    pub user_agent: String,
    pub device: usize,
    pub browser: usize,
    pub os: usize,
    pub country: usize,
    pub region: u64,
    pub referrer: Option<usize>,
    pub campaign: Option<usize>,
    pub planted: bool,
    pub events: Vec<GenEvent>,
    /// account_tier index and account_created ms, carried by the identify message at t0 - 1 s.
    pub identify: Option<(usize, i64)>,
}

struct Streams {
    start_time: Stream,
    device: Stream,
    browser: Stream,
    os: Stream,
    geo: Stream,
    referrer: Stream,
    campaign: Stream,
    path: Stream,
    query: Stream,
    event_name: Stream,
    slot_key: [Stream; 3],
    value: Stream,
    session_value: Stream,
    perf: Stream,
}

pub struct Fixture {
    pub cat: &'static Catalog,
    pub day: Affine,
    pageview_properties: Affine,
    revenue: Affine,
    s: Streams,
    /// [site][month] -> per local day (lo, hi) start-time bounds in seconds.
    bounds: Vec<Vec<Vec<(i64, i64)>>>,
}

pub fn month_start(month: u64) -> NaiveDate {
    NaiveDate::from_ymd_opt(2025, 10, 1).unwrap() + Months::new(month as u32)
}

pub fn days_in_month(month: u64) -> u64 {
    (month_start(month + 1) - month_start(month)).num_days() as u64
}

/// First instant whose local date is `date` at or after `time` (skips DST gaps).
pub fn local_instant(tz: Tz, date: NaiveDate, time: NaiveTime) -> i64 {
    let mut local = date.and_time(time);
    loop {
        if let Some(instant) = tz.from_local_datetime(&local).earliest() {
            return instant.timestamp();
        }
        local += chrono::Duration::minutes(1);
    }
}

pub fn local_date(tz: Tz, instant_s: i64) -> NaiveDate {
    tz.timestamp_opt(instant_s, 0).unwrap().date_naive()
}

impl Fixture {
    pub fn new(cat: &'static Catalog) -> Self {
        let bounds = cat
            .sites
            .iter()
            .map(|site| {
                (0..MONTHS)
                    .map(|month| {
                        (0..days_in_month(month))
                            .map(|d| {
                                let date = month_start(month) + chrono::Days::new(d);
                                let start = local_instant(site.timezone, date, NaiveTime::MIN);
                                let latest = local_instant(
                                    site.timezone,
                                    date,
                                    NaiveTime::from_hms_opt(23, 50, 0).unwrap(),
                                );
                                (start.max(FIXTURE_START_S), latest.min(FIXTURE_END_S - 300))
                            })
                            .collect()
                    })
                    .collect()
            })
            .collect();
        Fixture {
            cat,
            day: Affine::new(GROUPS_PER_MONTH, "day.placement"),
            pageview_properties: Affine::new(90_000_000, "select.pageview_properties"),
            revenue: Affine::new(24_000_000, "select.revenue"),
            s: Streams {
                start_time: Stream::named("group.start_time"),
                device: Stream::named("dim.device"),
                browser: Stream::named("dim.browser"),
                os: Stream::named("dim.os"),
                geo: Stream::named("dim.geo"),
                referrer: Stream::named("dim.referrer_domain"),
                campaign: Stream::named("dim.utm_campaign"),
                path: Stream::named("dim.path"),
                query: Stream::named("dim.query"),
                event_name: Stream::named("dim.event_name"),
                slot_key: [
                    Stream::named("prop.slot0_key"),
                    Stream::named("prop.slot1_key"),
                    Stream::named("prop.slot2_key"),
                ],
                value: Stream::named("prop.value"),
                session_value: Stream::named("session_prop.value"),
                perf: Stream::named("perf.value"),
            },
            bounds,
        }
    }

    /// Local day index `d` and its first `v` for a day-placement value.
    pub fn day_of(&self, month: u64, v: u64) -> (u64, u64) {
        let days = days_in_month(month);
        let d = v * days / GROUPS_PER_MONTH;
        (d, (d * GROUPS_PER_MONTH).div_ceil(days))
    }

    /// Streams the first `limit` groups of a month in day-placement order.
    /// `GROUPS_PER_MONTH` is the full month; smaller limits cover its first days only.
    pub fn month(&self, month: u64, limit: u64) -> impl Iterator<Item = Group> + '_ {
        (0..limit).map(move |v| self.group(month * GROUPS_PER_MONTH + self.day.index(v)))
    }

    pub fn group(&self, g: u64) -> Group {
        let cat = self.cat;
        let (month, m) = (g / GROUPS_PER_MONTH, g % GROUPS_PER_MONTH);
        let v = self.day.value(m);
        let (d, day_start) = self.day_of(month, v);
        let site = cat.site_of_group(g);
        let local_day = month_start(month) + chrono::Days::new(d);
        let (lo, hi) = self.bounds[site][month as usize][d as usize];
        let t0_s = lo + uniform(self.s.start_time.at(g, 1), (hi - lo + 1) as u64) as i64;

        let identified_limit = if month < 8 { 333_333 } else { 333_334 };
        let supplied_id = (m < identified_limit).then(|| {
            let earlier = 333_333 * month.min(8) + 333_334 * month.saturating_sub(8);
            format!("user-{:06}", (earlier + m) % 400_000)
        });

        let device_coverage = coverage(&cat.device, g);
        let mut device = dim(&cat.device, self.s.device, g).expect("device is never empty");
        let browser = dim(&cat.browser, self.s.browser, g).expect("browser is never empty");
        let os = dim(&cat.os, self.s.os, g).expect("os is never empty");
        // Geo: coverage rows run over geo_index (country * 4 + region). The
        // manifest's generic rule puts zipf ranks on sub 2, but its geo rule
        // also puts the region on sub 2; this generator draws the country rank
        // from sub 1 ("value") and the region from sub 2 ("secondary value").
        let geo_cells = cat.countries.cardinality() * cat.regions_per_country;
        let stride = cat.geo_population / geo_cells;
        let (country, region) = if g.is_multiple_of(stride) && g / stride < geo_cells {
            let cell = g / stride;
            (
                (cell / cat.regions_per_country) as usize,
                cell % cat.regions_per_country,
            )
        } else {
            (
                cat.countries.weights.pick(self.s.geo.at(g, 1)),
                uniform(self.s.geo.at(g, 2), cat.regions_per_country),
            )
        };
        let referrer = dim(&cat.referrer, self.s.referrer, g);
        let campaign = dim(&cat.campaign, self.s.campaign, g);

        // Acceptance plant: first 40 placement values of each local day bucket.
        let plant_eligible = v - day_start < 40 && device_coverage.is_none();
        let mut planted = false;

        let mut events = Vec::with_capacity(10);
        let (mut last_path, mut last_query) = ("", None);
        for slot in 0..10 {
            let e = g * 10 + slot;
            let (r, block) = (e % 20, e / 20);
            let time_s = t0_s + 30 * slot as i64;
            if let Some(idx) = PAGEVIEW_RESIDUES.iter().position(|x| *x == r) {
                let pv = block * 15 + idx as u64;
                let path =
                    cat.path.values[dim(&cat.path, self.s.path, pv).expect("path is never empty")];
                let query = dim(&cat.query, self.s.query, pv).map(|q| cat.query.values[q]);
                let properties = if self.pageview_properties.value(pv) < PROPERTY_PAGEVIEWS {
                    (0..3).map(|s| self.drawn_property(e, s)).collect()
                } else {
                    Vec::new()
                };
                (last_path, last_query) = (path, query);
                events.push(GenEvent {
                    e,
                    slot,
                    time_s,
                    kind: Kind::Pageview,
                    path,
                    query,
                    event_name: None,
                    revenue: None,
                    properties,
                    performance: None,
                });
            } else if let Some(idx) = CUSTOM_RESIDUES.iter().position(|x| *x == r) {
                let c = block * 4 + idx as u64;
                let rv = self.revenue.value(c);
                // Custom events take the URL of the pageview before them (slot - 1);
                // the manifest gives paths to pageviews and performance events only.
                let mut event = GenEvent {
                    e,
                    slot,
                    time_s,
                    kind: Kind::Custom,
                    path: last_path,
                    query: last_query,
                    event_name: None,
                    revenue: None,
                    properties: Vec::new(),
                    performance: None,
                };
                if rv < REVENUE_RECORDS {
                    let currency = (rv % 4) as usize;
                    let q = rv / 4;
                    let tier = cat.currencies[currency].tiers[(q % 10) as usize];
                    let amount = if q % 500 >= 490 { -tier } else { tier };
                    let revenue_value = if cat.currencies[currency].exponent == 0 {
                        Value::Int(amount)
                    } else {
                        Value::Dec(amount * 100)
                    };
                    event.event_name = Some("purchase");
                    event.revenue = Some((currency, amount));
                    event.properties = vec![
                        (cat.key_currency, Value::Str(cat.currencies[currency].code)),
                        (cat.key_revenue, revenue_value),
                        self.drawn_property(e, 2),
                    ];
                } else {
                    let j = rv - REVENUE_RECORDS;
                    event.properties = (0..3).map(|s| self.drawn_property(e, s)).collect();
                    if plant_eligible && !planted && coverage(&cat.event_name, j).is_none() {
                        event.event_name = Some("signup");
                        event.properties[0] = (cat.key_plan, Value::Str("pro"));
                        planted = true;
                    } else {
                        let name = dim(&cat.event_name, self.s.event_name, j)
                            .expect("event name is never empty");
                        event.event_name = Some(cat.event_name.values[name]);
                    }
                }
                events.push(event);
            } else {
                events.push(GenEvent {
                    e,
                    slot,
                    time_s,
                    kind: Kind::Performance,
                    path: last_path,
                    query: last_query,
                    event_name: None,
                    revenue: None,
                    properties: Vec::new(),
                    performance: None,
                });
            }
        }
        if planted {
            device = cat.mobile;
        }
        for event in events.iter_mut().filter(|e| e.kind == Kind::Performance) {
            event.performance = Some(self.performance(event.e / 20, device));
        }

        let identify = supplied_id.as_ref().map(|_| {
            let tier = cat.account_tiers.1.pick(self.s.session_value.at(g * 2, 1));
            let (start, days) = cat.account_created;
            (
                tier,
                start + uniform(self.s.session_value.at(g * 2 + 1, 1), days) as i64 * 86_400_000,
            )
        });

        let ip = std::net::Ipv6Addr::new(
            0x2001,
            0xdb8,
            0,
            0,
            0,
            0,
            ((g >> 16) & 0xffff) as u16,
            (g & 0xffff) as u16,
        );
        let user_agent = format!(
            "Mozilla/5.0 (fixture; {}) holocron-fixture/1 {}/{}",
            cat.os.values[os],
            cat.browser.values[browser],
            g % 97
        );
        Group {
            g,
            month,
            m,
            v,
            site,
            local_day,
            t0_s,
            supplied_id,
            ip,
            user_agent,
            device,
            browser,
            os,
            country,
            region,
            referrer,
            campaign,
            planted,
            events,
            identify,
        }
    }

    pub(crate) fn drawn_property(&self, e: u64, slot: u64) -> (usize, Value) {
        let cat = self.cat;
        let (keys, weights) = &cat.slot_keys[slot as usize];
        // The manifest names the stream and ordinal for the key draw but not the sub; this uses sub 1.
        let key = keys[weights.pick(self.s.slot_key[slot as usize].at(e, 1))];
        (key, self.value(&cat.keys[key], e * 3 + slot))
    }

    fn value(&self, key: &PropertyKey, ordinal: u64) -> Value {
        let at = |sub| self.s.value.at(ordinal, sub);
        if key.null_permille > 0 && permille(at(0), key.null_permille) {
            return Value::Null;
        }
        match &key.rule {
            Rule::List { values, weights } => match values[weights.pick(at(1))] {
                ListValue::Str(s) => Value::Str(s),
                ListValue::Int(i) => Value::Int(i),
            },
            // The empty-string test for search_term uses sub 2, which string keys leave free.
            Rule::Zipf {
                values,
                weights,
                empty_permille,
            } => {
                if *empty_permille > 0 && permille(at(2), *empty_permille) {
                    Value::Str("")
                } else {
                    Value::Str(values[weights.pick(at(1))])
                }
            }
            Rule::Knots { knots, decimal } => {
                let x = interpolate(knots, uniform(at(1), 1_000_000) as i64);
                if *decimal {
                    Value::Dec(x)
                } else {
                    Value::Int(x)
                }
            }
            Rule::UniformInt { lo, hi, decimal } => {
                let x = lo + uniform(at(1), (hi - lo + 1) as u64) as i64;
                if *decimal {
                    Value::Dec(x)
                } else {
                    Value::Int(x)
                }
            }
            Rule::UniformDate {
                start_ms,
                steps,
                step_ms,
            } => Value::Date(start_ms + uniform(at(1), *steps) as i64 * step_ms),
            Rule::Bool { true_permille } => Value::Bool(permille(at(1), *true_permille)),
            Rule::Array {
                min_len,
                max_len,
                element,
            } => {
                let len = min_len + uniform(at(2), max_len - min_len + 1);
                Value::Array(
                    (0..len)
                        .map(|k| {
                            let u = at(3 + k);
                            match element {
                                Element::Strings(values) => {
                                    Value::Str(values[uniform(u, values.len() as u64) as usize])
                                }
                                Element::Integers { lo, hi } => {
                                    Value::Int(lo + uniform(u, (hi - lo + 1) as u64) as i64)
                                }
                                Element::Decimals { lo, hi } => {
                                    Value::Dec(lo + uniform(u, (hi - lo + 1) as u64) as i64)
                                }
                                Element::Bools { true_permille } => {
                                    Value::Bool(permille(u, *true_permille))
                                }
                            }
                        })
                        .collect(),
                )
            }
            Rule::Forced => unreachable!("forced keys are never drawn"),
        }
    }

    pub(crate) fn performance(&self, ordinal: u64, device: usize) -> [Option<i64>; 5] {
        let cat = self.cat;
        std::array::from_fn(|i| {
            let metric = &cat.metrics[i];
            let at = |sub| self.s.perf.at(ordinal * 5 + i as u64, sub);
            if permille(at(0), metric.missing_permille) {
                None
            } else if permille(at(1), metric.zero_permille) {
                Some(0)
            } else {
                let value = interpolate(&metric.knots, uniform(at(2), 1_000_000) as i64);
                Some(value * cat.device_multiplier_permille[device] / 1000)
            }
        })
    }
}

/// Catalog index of a coverage row, if `j` is one.
fn coverage(dim: &Dimension, j: u64) -> Option<usize> {
    let stride = dim.population / dim.cardinality();
    (j.is_multiple_of(stride) && j / stride < dim.cardinality()).then_some((j / stride) as usize)
}

/// `dimension_rules`: coverage row, else sub 0 empty test, sub 1 head test, sub 2 rank.
fn dim(dim: &Dimension, stream: Stream, j: u64) -> Option<usize> {
    if let Some(index) = coverage(dim, j) {
        return Some(index);
    }
    if dim.empty_permille > 0 && permille(stream.at(j, 0), dim.empty_permille) {
        return None;
    }
    if dim.head_permille > 0 && permille(stream.at(j, 1), dim.head_permille) {
        return Some(0);
    }
    Some(dim.weights.pick(stream.at(j, 2)))
}

fn interpolate(knots: &[(i64, i64)], x: i64) -> i64 {
    let i = knots.partition_point(|k| k.0 <= x) - 1;
    let ((x0, y0), (x1, y1)) = (knots[i], knots[i + 1]);
    y0 + (y1 - y0) * (x - x0) / (x1 - x0)
}
