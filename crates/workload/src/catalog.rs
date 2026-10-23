//! Typed view of the frozen workload manifest: sites, dimension catalogs,
//! property keys and their value rules. Built once and leaked, so generated
//! records can borrow `&'static str` values without per-event allocation.

use std::path::Path;

use chrono_tz::Tz;
use serde_json::Value as Json;
use uuid::Uuid;

use crate::prng::Weights;

pub const FIXTURE_NAMESPACE: Uuid = uuid::uuid!("63fbcd3b-48d6-5d90-aa04-53ffd8a4c059");

pub fn fixture_uuid(name: &str) -> Uuid {
    Uuid::new_v5(&FIXTURE_NAMESPACE, name.as_bytes())
}

pub struct Site {
    pub code: &'static str,
    pub id: Uuid,
    pub hostname: &'static str,
    pub timezone: Tz,
}

/// A catalog dimension: values plus the draw rule of `dimension_rules`.
pub struct Dimension {
    pub values: Vec<&'static str>,
    pub weights: Weights,
    pub population: u64,
    pub empty_permille: u64,
    pub head_permille: u64,
}

impl Dimension {
    pub fn cardinality(&self) -> u64 {
        self.values.len() as u64
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyType {
    String,
    Integer,
    Decimal,
    Date,
    Bool,
    Array,
    /// `revenue`: decimal for EUR/USD/GBP, integer for JPY.
    Revenue,
}

pub enum Element {
    Strings(Vec<&'static str>),
    Integers { lo: i64, hi: i64 },
    Decimals { lo: i64, hi: i64 },
    Bools { true_permille: u64 },
}

pub enum Rule {
    List {
        values: Vec<ListValue>,
        weights: Weights,
    },
    Zipf {
        values: Vec<&'static str>,
        weights: Weights,
        empty_permille: u64,
    },
    /// Linear interpolation over `uniform(u, 1_000_000)`; `decimal` values are in 0.0001 units.
    Knots {
        knots: Vec<(i64, i64)>,
        decimal: bool,
    },
    UniformInt {
        lo: i64,
        hi: i64,
        decimal: bool,
    },
    UniformDate {
        start_ms: i64,
        steps: u64,
        step_ms: i64,
    },
    Bool {
        true_permille: u64,
    },
    Array {
        min_len: u64,
        max_len: u64,
        element: Element,
    },
    /// Never drawn: `currency` and `revenue` follow the revenue rule.
    Forced,
}

#[derive(Debug, Clone, Copy)]
pub enum ListValue {
    Str(&'static str),
    Int(i64),
}

pub struct PropertyKey {
    pub name: &'static str,
    pub ty: KeyType,
    pub slot: usize,
    pub null_permille: u64,
    pub rule: Rule,
}

pub struct Metric {
    pub name: &'static str,
    pub missing_permille: u64,
    pub zero_permille: u64,
    pub knots: Vec<(i64, i64)>,
}

pub struct Currency {
    pub code: &'static str,
    pub exponent: u32,
    pub tiers: Vec<i64>,
}

pub struct Catalog {
    pub sites: Vec<Site>,
    pub device: Dimension,
    pub mobile: usize,
    pub screens: Vec<&'static str>,
    pub browser: Dimension,
    pub os: Dimension,
    pub countries: Dimension,
    pub regions_per_country: u64,
    pub geo_population: u64,
    pub referrer: Dimension,
    pub campaign: Dimension,
    pub path: Dimension,
    pub query: Dimension,
    pub event_name: Dimension,
    pub keys: Vec<PropertyKey>,
    /// Per slot: drawable key indexes and their draw weights.
    pub slot_keys: [(Vec<usize>, Weights); 3],
    pub key_currency: usize,
    pub key_revenue: usize,
    pub key_plan: usize,
    pub plan_pro: usize,
    pub account_tiers: (Vec<&'static str>, Weights),
    pub account_created: (i64, u64),
    pub currencies: Vec<Currency>,
    pub metrics: Vec<Metric>,
    pub device_multiplier_permille: Vec<i64>,
    pub manifest: Json,
}

fn leak(s: &str) -> &'static str {
    Box::leak(s.to_owned().into_boxed_str())
}

fn str_list(j: &Json) -> Vec<&'static str> {
    j.as_array()
        .expect("array")
        .iter()
        .map(|v| leak(v.as_str().expect("string")))
        .collect()
}

fn u64_list(j: &Json) -> Vec<u64> {
    j.as_array()
        .expect("array")
        .iter()
        .map(|v| v.as_u64().expect("u64"))
        .collect()
}

fn knots(j: &Json) -> Vec<(i64, i64)> {
    j.as_array()
        .expect("knots")
        .iter()
        .map(|k| (k[0].as_i64().expect("x"), k[1].as_i64().expect("y")))
        .collect()
}

fn instant_ms(j: &Json) -> i64 {
    chrono::DateTime::parse_from_rfc3339(j.as_str().expect("instant"))
        .expect("rfc3339")
        .timestamp_millis()
}

fn templated(template: &str, n: usize, head: &[&'static str]) -> Vec<&'static str> {
    (0..n)
        .map(|i| match head.get(i) {
            Some(named) => *named,
            None => leak(&fill(template, i)),
        })
        .collect()
}

/// Expands the manifest's value templates: `{i}`, `{i:05}` and the two special forms.
fn fill(template: &str, i: usize) -> String {
    match template {
        "s{i}{suffix[i mod 4]}" => format!("s{i}{}", ["", " für", " 日本", " 😀"][i % 4]),
        t if t.starts_with("note {i}") => format!("note {i} {}", "lorem ".repeat(15 + i % 60)),
        t => {
            let start = t.find('{').expect("template placeholder");
            let end = t.find('}').expect("template placeholder");
            let width: usize = t[start + 1..end]
                .strip_prefix("i:")
                .map_or(0, |w| w.parse().expect("width"));
            format!("{}{:0width$}{}", &t[..start], i, &t[end + 1..])
        }
    }
}

impl Catalog {
    pub fn load(manifest_path: &Path) -> &'static Catalog {
        let text = std::fs::read_to_string(manifest_path).expect("read manifest");
        let m: Json = serde_json::from_str(&text).expect("parse manifest");
        let fixture = &m["retained_fixture"];
        let dims = &fixture["dimensions"];

        let sites = m["sites"]
            .as_array()
            .expect("sites")
            .iter()
            .map(|s| Site {
                code: leak(s["code"].as_str().unwrap()),
                id: s["id"].as_str().unwrap().parse().unwrap(),
                hostname: leak(s["hostname"].as_str().unwrap()),
                timezone: s["timezone"]
                    .as_str()
                    .unwrap()
                    .parse()
                    .expect("IANA timezone"),
            })
            .collect::<Vec<_>>();
        for (s, site) in sites.iter().enumerate() {
            assert_eq!(
                site.id,
                fixture_uuid(&format!("site:{}", site.code)),
                "site {s} id"
            );
        }

        let list = |d: &Json| Dimension {
            values: str_list(&d["values"]),
            weights: Weights::list(&u64_list(&d["weights"])),
            population: d["population"].as_u64().unwrap(),
            empty_permille: 0,
            head_permille: 0,
        };
        let zipf = |d: &Json, values: Vec<&'static str>| Dimension {
            weights: Weights::zipf(values.len()),
            values,
            population: d["population"].as_u64().unwrap(),
            empty_permille: d["empty_permille"].as_u64().unwrap_or(0),
            head_permille: d["head_permille"].as_u64().unwrap_or(0),
        };
        let device = list(&dims["device"]);
        let screens = device
            .values
            .iter()
            .map(|v| leak(dims["device"]["screen_by_value"][*v].as_str().unwrap()))
            .collect();
        let geo = &dims["geo"];
        let countries = Dimension {
            values: str_list(&geo["countries_in_rank_order"]),
            weights: Weights::zipf(64),
            population: geo["population"].as_u64().unwrap(),
            empty_permille: 0,
            head_permille: 0,
        };
        let referrer_head = str_list(&dims["referrer_domain"]["named_head"]);
        let referrer = zipf(
            &dims["referrer_domain"],
            templated("ref-{i:05}.example", 10_000, &referrer_head),
        );
        let campaign = zipf(&dims["utm_campaign"], templated("cmp-{i:04}", 1_000, &[]));
        let path_head = str_list(&dims["path"]["named_head"]);
        let path = zipf(&dims["path"], templated("/p/{i:05}", 100_000, &path_head));
        let query = zipf(
            &dims["query"],
            templated("?id={i:07}&ref=fx", 1_000_000, &[]),
        );
        let event_name = zipf(
            &dims["event_name"],
            str_list(&dims["event_name"]["values_in_rank_order"]),
        );

        let keys: Vec<PropertyKey> = fixture["event_properties"]["keys"]
            .as_array()
            .unwrap()
            .iter()
            .map(property_key)
            .collect();
        let slot_keys = std::array::from_fn(|slot| {
            let drawable: Vec<usize> = (0..keys.len())
                .filter(|k| keys[*k].slot == slot && !matches!(keys[*k].rule, Rule::Forced))
                .collect();
            let weights = fixture["event_properties"]["keys"]
                .as_array()
                .unwrap()
                .iter()
                .enumerate()
                .filter(|(k, _)| drawable.contains(k))
                .map(|(_, key)| key["draw_weight"].as_u64().unwrap())
                .collect::<Vec<_>>();
            (drawable, Weights::list(&weights))
        });
        let key = |name: &str| {
            keys.iter()
                .position(|k| k.name == name)
                .expect("property key")
        };
        let (key_currency, key_revenue, key_plan) = (key("currency"), key("revenue"), key("plan"));
        let plan_pro = match &keys[key_plan].rule {
            Rule::List { values, .. } => values
                .iter()
                .position(|v| matches!(v, ListValue::Str("pro")))
                .unwrap(),
            _ => unreachable!("plan is a list"),
        };

        let session = &fixture["session_properties"]["keys"];
        let account_tiers = (
            str_list(&session[0]["values"]["values"]),
            Weights::list(&u64_list(&session[0]["values"]["weights"])),
        );
        let range = &session[1]["values"]["range"];
        let created_start = instant_ms(&range[0]);
        let account_created = (
            created_start,
            ((instant_ms(&range[1]) - created_start) / 86_400_000) as u64,
        );

        let currencies = fixture["revenue"]["currencies"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| Currency {
                code: leak(c["code"].as_str().unwrap()),
                exponent: c["exponent"].as_u64().unwrap() as u32,
                tiers: c["tiers_minor"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|t| t.as_i64().unwrap())
                    .collect(),
            })
            .collect();
        let perf = &fixture["performance"];
        let metrics = ["lcp", "fcp", "ttfb", "inp", "cls"]
            .iter()
            .map(|name| {
                let metric = &perf["metrics"][*name];
                Metric {
                    name,
                    missing_permille: metric["missing_permille"].as_u64().unwrap(),
                    zero_permille: metric["zero_permille"].as_u64().unwrap(),
                    knots: knots(&metric["knots"]),
                }
            })
            .collect();
        let device_multiplier_permille = device
            .values
            .iter()
            .map(|d| perf["device_multiplier_permille"][*d].as_i64().unwrap())
            .collect();

        Box::leak(Box::new(Catalog {
            sites,
            mobile: device
                .values
                .iter()
                .position(|d| *d == "mobile")
                .expect("mobile device"),
            device,
            screens,
            browser: list(&dims["browser"]),
            os: list(&dims["os"]),
            countries,
            regions_per_country: geo["regions_per_country"].as_u64().unwrap(),
            geo_population: geo["population"].as_u64().unwrap(),
            referrer,
            campaign,
            path,
            query,
            event_name,
            keys,
            slot_keys,
            key_currency,
            key_revenue,
            key_plan,
            plan_pro,
            account_tiers,
            account_created,
            currencies,
            metrics,
            device_multiplier_permille,
            manifest: m,
        }))
    }

    pub fn site_of_group(&self, g: u64) -> usize {
        let residue = (g % 40) as usize;
        match residue {
            0..=19 => 0,
            20..=29 => 1,
            r => r - 28,
        }
    }
}

fn property_key(j: &Json) -> PropertyKey {
    let name = leak(j["key"].as_str().unwrap());
    let ty = match j["type"].as_str().unwrap() {
        "string" => KeyType::String,
        "integer" => KeyType::Integer,
        "decimal" => KeyType::Decimal,
        "date" => KeyType::Date,
        "bool" => KeyType::Bool,
        "array" => KeyType::Array,
        "integer|decimal" => KeyType::Revenue,
        other => panic!("unknown property type {other}"),
    };
    let v = &j["values"];
    let decimal = v["unit"].as_str() == Some("0.0001");
    let rule = if j["draw_weight"].as_u64() == Some(0) {
        Rule::Forced
    } else {
        match v["dist"].as_str().unwrap() {
            "list" => Rule::List {
                values: v["values"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|x| match x {
                        Json::String(s) => ListValue::Str(leak(s)),
                        n => ListValue::Int(n.as_i64().unwrap()),
                    })
                    .collect(),
                weights: Weights::list(&u64_list(&v["weights"])),
            },
            "zipf" => {
                let n = v["cardinality"].as_u64().unwrap() as usize;
                Rule::Zipf {
                    values: templated(v["template"].as_str().unwrap(), n, &[]),
                    weights: Weights::zipf(n),
                    empty_permille: v["empty_string_permille"].as_u64().unwrap_or(0),
                }
            }
            "knots" => Rule::Knots {
                knots: knots(&v["knots"]),
                decimal,
            },
            "uniform_int" => Rule::UniformInt {
                lo: v["range"][0].as_i64().unwrap(),
                hi: v["range"][1].as_i64().unwrap(),
                decimal,
            },
            "uniform_date" => {
                let start = instant_ms(&v["range"][0]);
                let span = instant_ms(&v["range"][1]) - start;
                let step_ms = if v["granularity"] == "day" {
                    86_400_000
                } else {
                    1000
                };
                Rule::UniformDate {
                    start_ms: start,
                    steps: (span / step_ms) as u64,
                    step_ms,
                }
            }
            "bool" => Rule::Bool {
                true_permille: v["true_permille"].as_u64().unwrap(),
            },
            "array" => {
                let e = &v["element"];
                let element = match (e["type"].as_str().unwrap(), e["dist"].as_str().unwrap()) {
                    ("string", "list_uniform") => {
                        let spec = e["values"].as_str().unwrap();
                        let (first, last) = spec.split_once("..").unwrap();
                        let prefix = &first[..first.rfind('-').unwrap() + 1];
                        let width = first.len() - prefix.len();
                        let count: usize = last[prefix.len()..].parse::<usize>().unwrap() + 1;
                        Element::Strings(
                            (0..count)
                                .map(|i| leak(&format!("{prefix}{i:0width$}")))
                                .collect(),
                        )
                    }
                    ("integer", _) => Element::Integers {
                        lo: e["range"][0].as_i64().unwrap(),
                        hi: e["range"][1].as_i64().unwrap(),
                    },
                    ("decimal", _) => Element::Decimals {
                        lo: e["range"][0].as_i64().unwrap(),
                        hi: e["range"][1].as_i64().unwrap(),
                    },
                    ("bool", _) => Element::Bools {
                        true_permille: e["true_permille"].as_u64().unwrap(),
                    },
                    other => panic!("unknown array element {other:?}"),
                };
                Rule::Array {
                    min_len: v["length"][0].as_u64().unwrap(),
                    max_len: v["length"][1].as_u64().unwrap(),
                    element,
                }
            }
            other => panic!("unknown value distribution {other}"),
        }
    };
    PropertyKey {
        name,
        ty,
        slot: j["slot"].as_u64().unwrap() as usize,
        null_permille: j["null_permille"].as_u64().unwrap(),
        rule,
    }
}
