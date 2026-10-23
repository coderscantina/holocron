//! The hand-audited correctness fixture (fixtures/correctness) through real
//! admission, transfer, correction and the traffic and exploration reads: every
//! expected session in `sessions.json`, every case in `traffic.json`, the
//! acceptance exploration, typed property filters and cells, and realtime.
//!
//! The harness plays collection's part with the 0001 rules from
//! `holocron_analytics::identity`: identity day and epoch, keyless arrivals,
//! the retention cutoff, consent links by event time. Pseudonyms are stand-in
//! hashes of the fixture's identity labels, so results name identities exactly
//! as the fixture does. Dimensions follow `user-agents.json`, the fixed geo
//! lookup and collection's referrer rule; properties follow the 0002 type
//! system. Collection-surface checks belong to milestone 04.

mod common;

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

use chrono::{Months, TimeZone};
use chrono_tz::Tz;
use common::{budgets, ms, pseudonym};
use holocron_analytics::explore::Context;
use holocron_analytics::identity::{Consent, SessionPolicy, SitePolicies};
use holocron_analytics::traffic::{Range, Traffic, VisitorLabel, traffic};
use holocron_analytics::{Correction, Limits, jobs, run_until_clean};
use holocron_protocol::exploration::VisitorLabel as Label;
use holocron_protocol::exploration::{Cell, Comparison, ExplorationState, ExploreResult, Metric};
use holocron_storage::duckdb::types::Value as Db;
use holocron_storage::{
    ClientEventId, Dimensions, Event, EventKind, Identify, Lane, Outcome, Performance, Property,
    PropertyValue, Record, Revocation, Storage,
};
use serde_json::{Value, json};

fn fixture(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/correctness")
        .join(path)
}

fn read_json(path: &str) -> Value {
    serde_json::from_str(&std::fs::read_to_string(fixture(path)).unwrap()).unwrap()
}

struct FixtureSite {
    key: String,
    id: u128,
    policies: SitePolicies,
    persistent: bool,
    /// Labels carry the epoch only on a site whose timezone changed.
    epoch_labels: bool,
}

fn uuid(text: &str) -> u128 {
    u128::from_str_radix(&text.replace('-', ""), 16).unwrap()
}

fn tz(name: &str) -> Tz {
    name.parse().unwrap()
}

/// Collection's part: one record per stored input line, plus revocations.
struct Collection {
    sites: Vec<FixtureSite>,
    /// User agent to its label and fixed browser, OS and device.
    user_agents: HashMap<String, [String; 4]>,
    consents: HashMap<(u128, String), Vec<Consent>>,
    /// Pseudonym, profile and event IDs back to fixture labels.
    labels: HashMap<u128, String>,
    suppressed: Vec<String>,
}

impl Collection {
    fn new() -> Self {
        let sites = read_json("input/sites.json")["sites"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| {
                let versions = s["session_policy_versions"].as_array().unwrap();
                let policy = |v: &Value| {
                    SessionPolicy::new(
                        tz(v["timezone"].as_str().unwrap()),
                        v["session_window_minutes"].as_u64().unwrap() as u16,
                    )
                    .unwrap()
                };
                let mut policies = SitePolicies::new(policy(&versions[0]));
                for v in &versions[1..] {
                    let effective = ms(v["effective"].as_str().unwrap());
                    policies.change(policy(v), effective, effective).unwrap();
                }
                FixtureSite {
                    key: s["key"].as_str().unwrap().to_owned(),
                    id: uuid(s["id"].as_str().unwrap()),
                    epoch_labels: versions.len() > 1,
                    policies,
                    persistent: s["persistent_identity_enabled"].as_bool().unwrap(),
                }
            })
            .collect();
        let user_agents = read_json("input/user-agents.json")["user_agents"]
            .as_array()
            .unwrap()
            .iter()
            .map(|u| {
                let text = |key: &str| u[key].as_str().unwrap().to_owned();
                (
                    text("user_agent"),
                    [text("label"), text("browser"), text("os"), text("device")],
                )
            })
            .collect();
        Self {
            sites,
            user_agents,
            consents: HashMap::new(),
            labels: HashMap::new(),
            suppressed: Vec::new(),
        }
    }

    fn site(&self, key: &str) -> &FixtureSite {
        self.sites.iter().find(|s| s.key == key).unwrap()
    }

    fn label(&mut self, label: String) -> u128 {
        let id = pseudonym(&label);
        self.labels.insert(id, label);
        id
    }

    fn records(&mut self) -> Vec<Record> {
        let lines = std::fs::read_to_string(fixture("input/events.jsonl")).unwrap();
        let mut out = Vec::new();
        for line in lines.lines() {
            let input: Value = serde_json::from_str(line).unwrap();
            out.extend(self.collect(&input));
        }
        out
    }

    fn collect(&mut self, input: &Value) -> Vec<Record> {
        let name = input["id"].as_str().unwrap().to_owned();
        let site = self.site(input["site"].as_str().unwrap());
        let (site_id, persistent, epoch_labels, policies) = (
            site.id,
            site.persistent,
            site.epoch_labels,
            site.policies.clone(),
        );
        let body = &input["request"]["body"];
        let payload = &body["payload"];
        let received = ms(input["receive_time"].as_str().unwrap());
        let time = payload["timestamp"].as_i64().map_or(received, |s| s * 1000);

        // 0006 Retention cutoff: start of local date (today - 12 months).
        let today_tz = policies.at(received).policy.timezone();
        let today = today_tz
            .timestamp_millis_opt(received)
            .unwrap()
            .date_naive();
        let cutoff_date = today.checked_sub_months(Months::new(12)).unwrap();
        let cutoff = today_tz
            .from_local_datetime(&cutoff_date.and_hms_opt(0, 0, 0).unwrap())
            .earliest()
            .unwrap()
            .timestamp_millis();
        if time < cutoff {
            self.suppressed.push(name);
            return Vec::new();
        }

        let day = policies.identity_day(time);
        let keyless = received >= policies.key_destroyed_at(day);
        let suffix = if epoch_labels {
            format!(" epoch {}", day.epoch)
        } else {
            String::new()
        };
        let supplied = payload["id"]
            .as_str()
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .map(str::to_owned);
        let daily_id = (!keyless).then(|| match &supplied {
            Some(id) => format!("daily-id {id} {}{suffix}", day.date),
            None => {
                let ua = &self.user_agents[input["transient"]["user_agent"].as_str().unwrap()][0];
                format!(
                    "anon {} {ua} {}{suffix}",
                    input["transient"]["ip"].as_str().unwrap(),
                    day.date
                )
            }
        });
        let daily_id = daily_id.map(|label| self.label(label));

        let mut out = Vec::new();
        let kind = body["type"].as_str().unwrap();
        if kind == "identify"
            && persistent
            && let Some(id) = &supplied
        {
            let consents = self.consents.entry((site_id, id.clone())).or_default();
            match payload["consent"].as_str() {
                Some("persistent") => consents.push(Consent {
                    granted_at_ms: time,
                    revoked_at_ms: None,
                }),
                Some("none") => {
                    if let Some(open) = consents.iter_mut().find(|c| c.revoked_at_ms.is_none()) {
                        open.revoked_at_ms = Some(time);
                    }
                    let profile = self.label(format!("profile {id}"));
                    out.push(Record::Revocation(Revocation {
                        id: pseudonym(&format!("{name} revocation")),
                        site: site_id,
                        profile_id: Some(profile),
                        revoked_at_ms: time,
                        receive_time_ms: received,
                    }));
                }
                _ => {}
            }
        }
        let linked = supplied.as_ref().is_some_and(|id| {
            self.consents
                .get(&(site_id, id.clone()))
                .is_some_and(|c| c.iter().any(|c| c.links(time)))
        });
        let profile_id = match (&supplied, linked) {
            (Some(id), true) => Some(self.label(format!("profile {id}"))),
            _ => None,
        };
        let id = self.label(name.clone());
        let record = match kind {
            "identify" => Record::Identify(Identify {
                id,
                site: site_id,
                event_time_ms: time,
                receive_time_ms: received,
                local_day: day.local_day(),
                daily_id,
                profile_id,
                properties: Vec::new(),
            }),
            _ => {
                let url = payload["url"].as_str().unwrap();
                let path = url
                    .splitn(4, '/')
                    .nth(3)
                    .map(|rest| format!("/{}", rest.split('?').next().unwrap()));
                let event_name = payload["name"].as_str().map(str::to_owned);
                let text = |key: &str| payload[key].as_str().map(str::to_owned);
                let [_, browser, os, device] =
                    self.user_agents[input["transient"]["user_agent"].as_str().unwrap()].clone();
                let hostname = text("hostname");
                let strip = |host: &str| host.trim_start_matches("www.").to_owned();
                // Collection's rule: the referrer's host without `www.`, none for a self-referral.
                let referrer_domain = text("referrer")
                    .and_then(|r| r.split('/').nth(2).map(strip))
                    .filter(|r| Some(r.as_str()) != hostname.as_deref().map(strip).as_deref());
                // Collection's campaign fields: the page query's UTM parameters and the first
                // known click ID, as sent (0003 Attribution, UTM values).
                let query = url.split_once('?').map(|(_, q)| q.to_owned());
                let param = |name: &str| {
                    query.as_deref()?.split('&').find_map(|pair| {
                        let (key, value) = pair.split_once('=')?;
                        (key == name).then(|| value.replace('+', " "))
                    })
                };
                let click_id = [
                    "gclid",
                    "fbclid",
                    "msclkid",
                    "ttclid",
                    "li_fat_id",
                    "twclid",
                ]
                .iter()
                .find_map(|name| param(name));
                Record::Event(Event {
                    id,
                    site: site_id,
                    kind: match (kind, &event_name) {
                        ("performance", _) => EventKind::Performance,
                        (_, Some(_)) => EventKind::CustomEvent,
                        _ => EventKind::Pageview,
                    },
                    event_time_ms: time,
                    receive_time_ms: received,
                    local_day: day.local_day(),
                    daily_id,
                    profile_id,
                    client_event_id: payload["eventId"].as_str().map(|event_id| ClientEventId {
                        id: event_id.to_owned(),
                        fingerprint: pseudonym(&payload.to_string()),
                    }),
                    dimensions: Dimensions {
                        hostname,
                        path,
                        title: text("title"),
                        referrer_domain,
                        utm_source: param("utm_source"),
                        utm_medium: param("utm_medium"),
                        utm_campaign: param("utm_campaign"),
                        utm_content: param("utm_content"),
                        utm_term: param("utm_term"),
                        click_id,
                        query: query.clone(),
                        country: input["transient"]["country"].as_str().map(str::to_owned),
                        browser: Some(browser),
                        os: Some(os),
                        device: Some(device),
                        screen: text("screen"),
                        language: text("language"),
                        event_name,
                        ..Dimensions::default()
                    },
                    performance: (kind == "performance").then(|| Performance {
                        lcp: payload["lcp"].as_i64().map(|v| v as i32),
                        cls: payload["cls"]
                            .as_f64()
                            .map(|v| (v * 10_000.0).round() as i32),
                        ..Performance::default()
                    }),
                    revenue: revenue(&payload["data"]),
                    properties: payload["data"]
                        .as_object()
                        .map(|data| properties(data, ""))
                        .unwrap_or_default(),
                })
            }
        };
        out.insert(0, record);
        out
    }
}

/// Collection's revenue rule (0002 Currencies and revenue): a JSON number in a known ISO 4217
/// currency, rounded half to even to its minor unit from the number text; zero is no record.
fn revenue(data: &Value) -> Option<holocron_storage::Revenue> {
    let Value::Number(amount) = &data["revenue"] else {
        return None;
    };
    let currency = data["currency"].as_str()?.trim().to_ascii_uppercase();
    let exponent = holocron_protocol::exploration::currency_exponent(&currency)?;
    // The shortest text of the number is the text the fixture wrote.
    let text = amount.to_string();
    let (negative, digits) = text
        .strip_prefix('-')
        .map_or((false, text.as_str()), |rest| (true, rest));
    let (whole, fraction) = digits.split_once('.').unwrap_or((digits, ""));
    let scale = usize::from(exponent);
    let kept = format!("{fraction:0<scale$}");
    let (kept, dropped) = kept.split_at(scale);
    let mut units: i128 = format!("{whole}{kept}").parse().unwrap();
    let half = format!("5{}", "0".repeat(dropped.len().saturating_sub(1)));
    if !dropped.is_empty() {
        match dropped.cmp(&half) {
            std::cmp::Ordering::Greater => units += 1,
            std::cmp::Ordering::Equal if units % 2 == 1 => units += 1,
            _ => {}
        }
    }
    let units = if negative { -units } else { units };
    (units != 0).then_some(holocron_storage::Revenue {
        currency,
        minor_units: units as i64,
    })
}

/// Event data typed by the 0002 rules: numbers with an integral value are integers, others decimals with 4
/// places, RFC 3339 strings dates, nested objects flattened with dots.
fn properties(data: &serde_json::Map<String, Value>, prefix: &str) -> Vec<Property> {
    let scalar = |value: &Value| match value {
        Value::Null => PropertyValue::Null,
        Value::Bool(b) => PropertyValue::Bool(*b),
        Value::Number(n) => match n.as_f64().unwrap() {
            f if f.fract() == 0.0 => PropertyValue::Integer(f as i64),
            f => PropertyValue::Decimal((f * 10_000.0).round() as i64),
        },
        Value::String(s) => match chrono::DateTime::parse_from_rfc3339(s) {
            Ok(date) => PropertyValue::Date(date.timestamp_millis()),
            Err(_) => PropertyValue::String(s.clone()),
        },
        other => panic!("nested value {other}"),
    };
    let mut out = Vec::new();
    for (key, value) in data {
        let key = format!("{prefix}{key}");
        match value {
            Value::Object(inner) => out.extend(properties(inner, &format!("{key}."))),
            Value::Array(items) => out.push(Property {
                key,
                value: PropertyValue::Array(items.iter().map(scalar).collect()),
            }),
            scalar_value => out.push(Property {
                key,
                value: scalar(scalar_value),
            }),
        }
    }
    out
}

async fn load() -> (tempfile::TempDir, Storage, Collection) {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::open(dir.path(), budgets()).await.unwrap();
    let mut collection = Collection::new();
    for site in &collection.sites {
        jobs::record_policies(&storage, site.id, site.policies.clone())
            .await
            .unwrap();
    }
    let records = collection.records();
    let outcomes = storage.admit(&records).await.unwrap();
    let duplicates = outcomes
        .iter()
        .filter(|o| matches!(o, Outcome::Duplicate { .. }))
        .count();
    assert_eq!(
        (collection.suppressed.as_slice(), duplicates),
        (&["ret-01".to_owned()][..], 2)
    );
    storage.drain().await;
    run_until_clean(&storage, Limits::default()).await.unwrap();
    (dir, storage, collection)
}

fn site_key(collection: &Collection, id: u128) -> &str {
    &collection.sites.iter().find(|s| s.id == id).unwrap().key
}

#[tokio::test(flavor = "multi_thread")]
async fn sessions_match_the_fixture() {
    let (_dir, storage, collection) = load().await;
    let rows = storage
        .read(Lane::Report, |s| {
            let mut st = s.connection().prepare(
                "SELECT s.site, s.identity, list(e.id ORDER BY e.event_time, e.seq) FILTER (WHERE m.role = 1),
                        list(e.id) FILTER (WHERE m.role = 2),
                        any_value(epoch_ms(s.start_time)), any_value(epoch_ms(s.last_time) - epoch_ms(s.start_time)),
                        any_value(s.pageviews), any_value(s.custom_events), any_value(s.policy_version)
                 FROM sessions s
                 JOIN session_members m ON m.site = s.site AND m.session_id = s.session_id
                 JOIN events e ON e.seq = m.seq
                 GROUP BY s.site, s.session_id, s.identity",
            )?;
            st.query_map([], |r| {
                Ok((
                    r.get::<_, u128>(0)?,
                    r.get::<_, u128>(1)?,
                    r.get::<_, Db>(2)?,
                    r.get::<_, Db>(3)?,
                    [r.get::<_, i64>(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?],
                ))
            })?
            .collect::<holocron_storage::duckdb::Result<Vec<_>>>()
        })
        .await
        .unwrap()
        .value;

    let name = |id: &u128| collection.labels[id].clone();
    let names = |ids: Db| match ids {
        Db::Null => Vec::new(),
        ids => common::ids(ids).iter().map(name).collect::<Vec<_>>(),
    };
    let mut actual = Vec::new();
    let mut attached = BTreeMap::new();
    for (site, identity, members, performance, [start, span, pageviews, custom, policy]) in rows {
        let members = names(members);
        for perf in names(performance) {
            attached.insert(perf, members.clone());
        }
        let bounce = match pageviews {
            0 => Value::Null,
            n => json!(n == 1 && custom == 0),
        };
        actual.push(json!({
            "site": site_key(&collection, site), "identity": name(&identity), "events": members,
            "start": chrono::DateTime::from_timestamp_millis(start).unwrap().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string(),
            "span_ms": span, "pageviews": pageviews, "custom_events": custom, "bounce": bounce, "policy": policy,
        }));
    }
    let expected = read_json("expected/sessions.json");
    let key = |v: &Value| {
        (
            v["site"].to_string(),
            v["start"].to_string(),
            v["identity"].to_string(),
        )
    };
    let mut want: Vec<Value> = expected["sessions"].as_array().unwrap().clone();
    want.sort_by_key(key);
    actual.sort_by_key(key);
    assert_eq!(actual.len(), 78);
    for (got, want) in actual.iter().zip(&want) {
        assert_eq!(got, want);
    }

    for (perf, members) in expected["performance_attachment"].as_object().unwrap() {
        if perf == "note" {
            continue;
        }
        let got = attached.get(perf).map(|m| json!(m)).unwrap_or(Value::Null);
        assert_eq!(&got, members, "{perf}");
    }

    let keyless = storage
        .read(Lane::Report, |s| {
            let mut st = s.connection().prepare(
                "SELECT e.id FROM events e WHERE e.kind IN (1, 2) AND e.daily_id IS NULL AND e.profile_id IS NULL
                 AND NOT EXISTS (SELECT 1 FROM session_members m WHERE m.seq = e.seq) ORDER BY e.seq",
            )?;
            st.query_map([], |r| r.get::<_, u128>(0))?.collect::<holocron_storage::duckdb::Result<Vec<_>>>()
        })
        .await
        .unwrap()
        .value;
    let keyless: Vec<String> = keyless.iter().map(name).collect();
    assert_eq!(json!(keyless), expected["keyless_events"]["events"]);
}

fn ratio(numerator: u64, denominator: u64) -> Value {
    if denominator == 0 {
        return Value::Null;
    }
    // Round half to even to 4 places (0004 Encoding conventions).
    let scaled = numerator as u128 * 10_000;
    let (q, r) = (scaled / denominator as u128, scaled % denominator as u128);
    let q = match (2 * r).cmp(&(denominator as u128)) {
        std::cmp::Ordering::Less => q,
        std::cmp::Ordering::Greater => q + 1,
        std::cmp::Ordering::Equal => q + (q & 1),
    };
    json!(q as f64 / 10_000.0)
}

fn rendered(t: &Traffic) -> Value {
    let visitors = match t.visitor_label {
        VisitorLabel::Visitors => ("visitors", "visitors"),
        VisitorLabel::VisitorDays => ("visitor_days", "visitor-days"),
    };
    let mut out = json!({
        "session_policy_versions": t.freshness.session_policy_versions,
        "pageviews": t.pageviews,
        "events": t.events,
        "events_by_name": t.events_by_name,
        "sessions": t.sessions,
        "identified_people": {
            "value": t.identified_people,
            "coverage": { "linked_events": t.linked_events, "eligible_events": t.eligible_events, "rate": ratio(t.linked_events, t.eligible_events) },
        },
        "bounce_rate": { "bounces": t.bounces, "denominator": t.pageview_sessions, "value": ratio(t.bounces, t.pageview_sessions) },
        "observed_span": { "sessions": t.pageview_sessions, "sum_ms": t.span_sum_ms, "mean_ms": t.span_mean_ms(), "median_ms": t.span_median_ms },
        "visitor_days_series_by_local_day": t.visitor_series.iter().map(|(d, n)| (d.to_string(), json!(n))).collect::<serde_json::Map<_, _>>(),
        "series_sum_differs": t.series_sum_differs,
        "identity_coverage": { "with_identity": t.identified_events, "eligible_events": t.eligible_events, "rate": ratio(t.identified_events, t.eligible_events) },
    });
    out[visitors.0] = json!({ "label": visitors.1, "value": t.visitors });
    out
}

#[tokio::test(flavor = "multi_thread")]
async fn traffic_matches_the_fixture() {
    let (_dir, storage, collection) = load().await;
    let expected = read_json("expected/traffic.json");
    for case in expected["cases"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        let site = collection.site(case["site"].as_str().unwrap()).id;
        let range = &case["range"];
        let start = range["retention_start"]
            .as_str()
            .or(range["start"].as_str())
            .unwrap();
        let range = Range {
            start_ms: ms(start),
            end_ms: ms(range["end"].as_str().unwrap()),
        };
        let timezone = tz(case["range"]["timezone"].as_str().unwrap());
        let read = storage
            .read(Lane::Report, move |s| traffic(s, site, range, timezone))
            .await
            .unwrap()
            .value;
        assert_eq!(read.freshness.correction, Correction::Corrected, "{id}");
        let actual = rendered(&read);
        for (key, want) in case.as_object().unwrap() {
            if matches!(key.as_str(), "id" | "site" | "range" | "local_day_hours") {
                continue;
            }
            assert_eq!(&actual[key], want, "{id}: {key}");
        }
    }
}

const EVALUATED_AT: &str = "2026-10-03T10:00:00.000Z";

/// Validates and runs a state at the fixture's evaluation instant.
async fn explore(storage: &Storage, site: u128, state: Value) -> ExploreResult {
    let state: ExplorationState = serde_json::from_value(state).unwrap();
    let query = holocron_analytics::explore::validate(&state, &[]).unwrap();
    storage
        .read(Lane::Report, move |s| {
            let context = Context {
                site,
                now_ms: ms(EVALUATED_AT),
                hostnames: &[],
                retention: holocron_protocol::retention::Retention::OneYear,
            };
            holocron_analytics::explore::explore(s, &query, &context)
        })
        .await
        .unwrap()
        .value
        .unwrap()
}

fn state(
    measures: Value,
    dimensions: Value,
    filter: Value,
    range: Value,
    timezone: &str,
    compare: &str,
) -> Value {
    json!({
        "v": 1, "site": "0d2c9a6e-5a3b-4f1e-9b1c-7e2f1a4d8c01", "measures": measures, "dimensions": dimensions,
        "filter": filter, "range": range, "timezone": timezone, "granularity": "day", "compare": compare,
        "metric_version": 1, "sort": [{ "by": measures[0], "dir": "desc" }], "page": { "size": 50, "index": 0 },
        "visualization": "table"
    })
}

fn count(metric: &Metric) -> i64 {
    match metric {
        Metric::Count { v } => *v,
        other => panic!("not a count: {other:?}"),
    }
}

/// `{current, previous, change, relative_change, reason?}` as the fixture writes it.
fn compared(current: &Metric, comparison: &Comparison) -> Value {
    let mut out = json!({
        "current": count(current), "previous": count(&comparison.previous),
        "change": comparison.change.unwrap() as i64, "relative_change": comparison.relative_change,
    });
    if let Some(reason) = comparison.reason {
        out["reason"] = serde_json::to_value(reason).unwrap();
    }
    out
}

fn cell_text(cell: &Cell) -> &str {
    match cell {
        Cell::String(s) => s,
        other => panic!("not a string cell: {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn acceptance_exploration_matches_the_fixture() {
    let (_dir, storage, collection) = load().await;
    let site = collection.site("A").id;
    let expected = read_json("expected/acceptance-exploration.json");
    let condition = |id: &str, field: Value, v: &str| json!({ "kind": "cond", "id": id, "field": field, "cmp": "eq", "value": { "t": "string", "v": v } });
    let mut items = vec![
        condition("f1", json!("device"), "mobile"),
        condition("f2", json!("event_name"), "signup"),
        condition("f3", json!({ "scope": "event", "key": "plan" }), "pro"),
    ];
    let run = |items: Vec<Value>| {
        state(
            json!(["events", "visitor_days"]),
            json!(["country", "referrer_domain"]),
            json!({ "kind": "group", "op": "and", "items": items }),
            json!({ "kind": "relative", "unit": "day", "count": 30 }),
            "Europe/Vienna",
            "previous_period",
        )
    };

    let result = explore(&storage, site, run(items.clone())).await;
    let resolved = &expected["resolved"];
    assert_eq!(
        (
            result.resolved.start,
            result.resolved.end,
            result.resolved.compare_start,
            result.resolved.compare_end
        ),
        (
            ms(resolved["start"].as_str().unwrap()),
            ms(resolved["end"].as_str().unwrap()),
            Some(ms(resolved["compare_start"].as_str().unwrap())),
            Some(ms(resolved["compare_end"].as_str().unwrap()))
        )
    );
    let main = &result.main;
    let totals = main.compare.as_ref().unwrap();
    let breakdown = &expected["breakdown"];
    assert_eq!(
        compared(&main.totals[0], &totals[0]),
        breakdown["totals"]["events"]
    );
    assert_eq!(
        compared(&main.totals[1], &totals[1]),
        breakdown["totals"]["visitor_days"]
    );
    let rows: Vec<Value> = main
        .rows
        .iter()
        .map(|row| {
            let compare = row.compare.as_ref().unwrap();
            json!({
                "country": cell_text(&row.key[0]), "referrer_domain": cell_text(&row.key[1]),
                "events": compared(&row.values[0], &compare[0]),
                "visitor_days": compared(&row.values[1], &compare[1]),
            })
        })
        .collect();
    let want: Vec<Value> = breakdown["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            json!({ "country": row["country"], "referrer_domain": row["referrer_domain"], "events": row["events"], "visitor_days": row["visitor_days"] })
        })
        .collect();
    assert_eq!(rows, want);
    assert_eq!(main.row_count, 3);
    let coverage = &breakdown["identity_coverage"]["current"];
    assert_eq!(
        (
            main.identity_coverage.with_identity,
            main.identity_coverage.eligible_events
        ),
        (
            coverage["with_identity"].as_i64().unwrap(),
            coverage["eligible_events"].as_i64().unwrap()
        )
    );

    // Drilling into Austria appends f4; nothing else changes.
    items.push(condition("f4", json!("country"), "AT"));
    let drilled = explore(&storage, site, run(items)).await;
    let drill = &expected["drill_down_at"];
    let compare = drilled.main.compare.as_ref().unwrap();
    assert_eq!(
        compared(&drilled.main.totals[0], &compare[0]),
        drill["totals"]["events"]
    );
    assert_eq!(
        compared(&drilled.main.totals[1], &compare[1]),
        drill["totals"]["visitor_days"]
    );
    let rows: Vec<Value> = drilled
        .main
        .rows
        .iter()
        .map(|row| {
            json!({ "country": cell_text(&row.key[0]), "referrer_domain": cell_text(&row.key[1]),
                    "events": count(&row.values[0]), "visitor_days": count(&row.values[1]) })
        })
        .collect();
    assert_eq!(json!(rows), drill["rows"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn typed_property_filters_and_cells_match_the_fixture() {
    let (_dir, storage, collection) = load().await;
    let site = collection.site("A").id;
    let expected = read_json("expected/properties.json");
    let days = &expected["range"]["days"];
    let range = json!({ "kind": "days", "start": days[0], "end": days[1] });
    let base = json!({ "kind": "cond", "id": "base", "field": "event_name", "cmp": "eq", "value": { "t": "string", "v": "configure" } });
    let property = |key: &Value| json!({ "scope": "event", "key": key });

    let all = explore(
        &storage,
        site,
        state(
            json!(["events"]),
            json!([]),
            json!({ "kind": "group", "op": "and", "items": [base] }),
            range.clone(),
            "Europe/Vienna",
            "none",
        ),
    )
    .await;
    assert_eq!(
        count(&all.main.totals[0]),
        expected["base_count"].as_i64().unwrap()
    );

    for case in expected["filters"].as_array().unwrap() {
        let mut condition = json!({ "kind": "cond", "id": "c1", "field": property(&case["field"]), "cmp": case["cmp"] });
        if !case["value"].is_null() {
            condition["value"] = case["value"].clone();
        }
        let filter = json!({ "kind": "group", "op": "and", "items": [base, condition] });
        let result = explore(
            &storage,
            site,
            state(
                json!(["events"]),
                json!([]),
                filter,
                range.clone(),
                "Europe/Vienna",
                "none",
            ),
        )
        .await;
        assert_eq!(
            count(&result.main.totals[0]),
            case["events"].as_i64().unwrap(),
            "{}",
            case["id"]
        );
    }

    for (key, cells) in expected["group_by"].as_object().unwrap() {
        if key == "note" {
            continue;
        }
        let filter = json!({ "kind": "group", "op": "and", "items": [base] });
        let result = explore(
            &storage,
            site,
            state(
                json!(["events"]),
                json!([property(&json!(key))]),
                filter,
                range.clone(),
                "Europe/Vienna",
                "none",
            ),
        )
        .await;
        let mut got: Vec<Value> = result
            .main
            .rows
            .iter()
            .map(|row| json!({ "key": row.key[0], "events": count(&row.values[0]) }))
            .collect();
        let mut want = cells.as_array().unwrap().clone();
        let order = |v: &Value| v.to_string();
        got.sort_by_key(order);
        want.sort_by_key(order);
        assert_eq!(got, want, "{key}");
    }

    let catalog = storage
        .read(Lane::Report, move |s| {
            holocron_analytics::catalog::property_keys(s, site)
        })
        .await
        .unwrap()
        .value;
    let type_names = [
        "null", "bool", "integer", "decimal", "date", "string", "array",
    ];
    for (key, types) in expected["catalog"].as_object().unwrap() {
        let entry = catalog
            .iter()
            .find(|k| k.key == *key)
            .unwrap_or_else(|| panic!("{key} in the catalog"));
        let got: Vec<&str> = entry
            .types
            .iter()
            .map(|t| type_names[*t as usize])
            .collect();
        let mut want: Vec<&str> = types
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        want.sort_by_key(|t| type_names.iter().position(|n| n == t));
        assert_eq!(got, want, "{key}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn realtime_matches_the_fixture() {
    let (_dir, storage, collection) = load().await;
    let site = collection.site("A").id;
    let expected = read_json("expected/realtime.json");
    let now = ms(expected["server_time"].as_str().unwrap());
    let realtime = storage
        .read(Lane::Live, move |s| {
            holocron_analytics::realtime::realtime(s, site, now)
        })
        .await
        .unwrap()
        .value;
    assert_eq!(realtime.active, expected["active"].as_i64().unwrap());
    let series = &expected["series"];
    assert_eq!(
        realtime.series.len() as u64,
        series["buckets"].as_u64().unwrap()
    );
    assert_eq!(
        realtime.series[0].0,
        ms(series["first_bucket_start"].as_str().unwrap())
    );
    let non_empty: Vec<Value> = realtime
        .series
        .iter()
        .filter(|(_, pageviews, visitors)| pageviews + visitors > 0)
        .map(|(start, pageviews, visitors)| {
            json!({ "start": holocron_analytics::explore::instant(*start), "pageviews": pageviews, "visitors": visitors })
        })
        .collect();
    assert_eq!(json!(non_empty), series["non_empty"]);
    let coverage = &expected["identity_coverage"];
    assert_eq!(
        (realtime.with_identity, realtime.eligible),
        (
            coverage["with_identity"].as_i64().unwrap(),
            coverage["eligible_events"].as_i64().unwrap()
        )
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn explorations_reproduce_the_traffic_cases() {
    let (_dir, storage, collection) = load().await;
    let expected = read_json("expected/traffic.json");
    let measures = json!([
        "pageviews",
        "events",
        "sessions",
        "visitor_days",
        "identified_people",
        "bounce_rate",
        "span_mean",
        "span_median"
    ]);
    for case in expected["cases"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        let site = collection.site(case["site"].as_str().unwrap()).id;
        let days = &case["range"]["days"];
        let mut state = state(
            measures.clone(),
            json!([]),
            Value::Null,
            json!({ "kind": "days", "start": days[0], "end": days[1] }),
            case["range"]["timezone"].as_str().unwrap(),
            "none",
        );
        state["site"] = json!(site.to_string());
        let result = explore(&storage, site, state).await;
        let t = &result.main.totals;
        let millis = |m: &Metric| match m {
            Metric::Millis { v } => json!(v),
            _ => Value::Null,
        };
        let (bounces, denominator, rate) = match &t[5] {
            Metric::Ratio {
                v,
                numerator,
                denominator,
            } => (json!(numerator), json!(denominator), json!(v)),
            _ => (json!(0), json!(0), Value::Null),
        };
        let coverage = &result.main.identity_coverage;
        let label = match result.visitor_label {
            Label::Visitors => "visitors",
            Label::VisitorDays => "visitor-days",
        };
        let mut actual = json!({
            "pageviews": count(&t[0]), "events": count(&t[1]), "sessions": count(&t[2]),
            "identified_people": { "value": count(&t[4]) },
            "bounce_rate": { "bounces": bounces, "denominator": denominator, "value": rate },
            "observed_span": { "sessions": denominator, "mean_ms": millis(&t[6]), "median_ms": millis(&t[7]) },
            "identity_coverage": {
                "with_identity": coverage.with_identity, "eligible_events": coverage.eligible_events,
                "rate": ratio(coverage.with_identity as u64, coverage.eligible_events as u64),
            },
        });
        actual[if label == "visitors" {
            "visitors"
        } else {
            "visitor_days"
        }] = json!({ "label": label, "value": count(&t[3]) });
        // Fields the traffic summary adds that one exploration does not compute.
        let skipped = [
            "id",
            "site",
            "range",
            "local_day_hours",
            "events_by_name",
            "visitor_days_series_by_local_day",
            "series_sum_differs",
            "session_policy_versions",
        ];
        for (key, want) in case.as_object().unwrap() {
            if skipped.contains(&key.as_str()) {
                continue;
            }
            let mut want = want.clone();
            if let Some(object) = want.as_object_mut() {
                // Linked-event coverage and the span sum belong to the traffic summary.
                object.remove(if key == "identified_people" {
                    "coverage"
                } else {
                    "sum_ms"
                });
            }
            assert_eq!(&actual[key], &want, "{id}: {key}");
        }
    }
}

/// Per currency `{exponent, gross, refunds, net, count, net_display}` of one revenue row.
fn money_row(row: &holocron_protocol::exploration::Row) -> (String, Value) {
    let money = |metric: &Metric| match metric {
        Metric::Money { v } => match v.as_slice() {
            [one] => one.clone(),
            other => panic!("one currency per row, got {other:?}"),
        },
        other => panic!("not money: {other:?}"),
    };
    let (gross, refunds, net) = (
        money(&row.values[0]),
        money(&row.values[1]),
        money(&row.values[2]),
    );
    (
        cell_text(&row.key[0]).to_owned(),
        json!({
            "exponent": net.exponent, "gross": gross.minor_units as i64, "refunds": refunds.minor_units as i64,
            "net": net.minor_units as i64, "count": count(&row.values[3]), "net_display": net.amount,
        }),
    )
}

#[tokio::test(flavor = "multi_thread")]
async fn revenue_matches_the_fixture() {
    let (_dir, storage, collection) = load().await;
    let site = collection.site("A").id;
    let expected = read_json("expected/revenue.json");
    for case in expected["cases"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        let days = &case["range"]["days"];
        let mut state = state(
            json!([
                "revenue_gross",
                "revenue_refunds",
                "revenue_net",
                "revenue_count"
            ]),
            json!(["currency"]),
            Value::Null,
            json!({ "kind": "days", "start": days[0], "end": days[1] }),
            case["range"]["timezone"].as_str().unwrap(),
            "none",
        );
        state["sort"] = json!([{ "by": "currency", "dir": "asc" }]);
        let result = explore(&storage, site, state).await;
        let rows: serde_json::Map<String, Value> = result.main.rows.iter().map(money_row).collect();
        assert_eq!(Value::Object(rows), case["by_currency"], "{id}");

        // Totals keep every currency apart, in code order, never summed into one number.
        let Metric::Money { v } = &result.main.totals[2] else {
            panic!("{id}: net total is not money")
        };
        let totals: Vec<(&str, i64)> = v
            .iter()
            .map(|m| (m.currency.as_str(), m.minor_units as i64))
            .collect();
        let want: Vec<(&str, i64)> = case["by_currency"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(code, c)| (code.as_str(), c["net"].as_i64().unwrap()))
            .collect();
        assert_eq!(totals, want, "{id}");
        assert_eq!(
            count(&result.main.totals[3]),
            want.iter()
                .map(|(code, _)| case["by_currency"][code]["count"].as_i64().unwrap())
                .sum::<i64>(),
            "{id}"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn percentiles_match_the_fixture() {
    let (_dir, storage, collection) = load().await;
    let site = collection.site("A").id;
    let expected = read_json("expected/percentiles.json");
    let days = &expected["range"]["days"];
    let metric_in = |metrics: &[&str]| {
        json!({ "kind": "group", "op": "and", "items": [{
            "kind": "cond", "id": "m", "field": "perf_metric", "cmp": "in",
            "value": { "t": "list", "v": metrics.iter().map(|m| json!({ "t": "string", "v": m })).collect::<Vec<_>>() }
        }]})
    };
    let run = |dimensions: Value, filter: Value| {
        let mut state = state(
            json!(["p50", "p75", "p95"]),
            dimensions.clone(),
            filter,
            json!({ "kind": "days", "start": days[0], "end": days[1] }),
            "Europe/Vienna",
            "none",
        );
        state["sort"] = json!(
            dimensions
                .as_array()
                .unwrap()
                .iter()
                .map(|d| json!({ "by": d, "dir": "asc" }))
                .collect::<Vec<_>>()
        );
        state
    };
    let value = |metric: &Metric| match metric {
        Metric::Millis { v } => json!(v),
        Metric::Decimal { v } => json!(v),
        other => panic!("not a percentile: {other:?}"),
    };
    let actual = |key: &[Cell], values: &[Metric]| {
        let mut group = serde_json::Map::new();
        for (cell, name) in key.iter().skip(1).zip(["device"]) {
            group.insert(name.into(), json!(cell_text(cell)));
        }
        json!({
            "metric": cell_text(&key[0]),
            "group": if group.is_empty() { json!("all") } else { Value::Object(group) },
            "p50": value(&values[0]), "p75": value(&values[1]), "p95": value(&values[2]),
        })
    };

    let by_metric = explore(
        &storage,
        site,
        run(json!(["perf_metric"]), metric_in(&["lcp", "cls"])),
    )
    .await;
    // Over two metrics with different units the headline has no percentile.
    assert!(by_metric.main.totals.iter().all(|m| *m
        == Metric::Unavailable {
            reason: holocron_protocol::exploration::Reason::MixedMetrics
        }));
    let lcp_by_device = explore(
        &storage,
        site,
        run(json!(["perf_metric", "device"]), metric_in(&["lcp"])),
    )
    .await;
    let mut rows: Vec<Value> = by_metric
        .main
        .rows
        .iter()
        .chain(&lcp_by_device.main.rows)
        .map(|row| actual(&row.key, &row.values))
        .collect();
    rows.sort_by_key(|r| r.to_string());
    let mut want: Vec<Value> = expected["cases"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| json!({ "metric": c["metric"], "group": c["group"], "p50": c["p50"], "p75": c["p75"], "p95": c["p95"] }))
        .collect();
    want.sort_by_key(|r| r.to_string());
    assert_eq!(rows, want);
}

#[tokio::test(flavor = "multi_thread")]
async fn retention_matches_the_fixture() {
    let (_dir, storage, collection) = load().await;
    let site = collection.site("A").id;
    let expected = read_json("expected/retention.json");
    let days = &expected["cohort_range"]["days"];
    let mut state = state(
        json!([]),
        json!([]),
        Value::Null,
        json!({ "kind": "days", "start": days[0], "end": days[1] }),
        "Europe/Vienna",
        "none",
    );
    state["granularity"] = json!("week");
    state["sort"] = json!([]);
    state["visualization"] = json!("cohort");
    state["report"] = expected["report"].clone();
    let result = explore(&storage, site, state).await;
    let Some(holocron_protocol::exploration::ReportResult::Retention(retention)) = result.report
    else {
        panic!("no retention result")
    };
    let instant = |ms: i64| holocron_analytics::explore::instant(ms);
    let rate = |metric: &Metric| match metric {
        Metric::Ratio { v, .. } => json!(v),
        Metric::Unavailable { reason } => json!({ "rate": null, "reason": reason }),
        other => panic!("not a rate: {other:?}"),
    };
    let cohorts: Vec<Value> = retention
        .cohorts
        .iter()
        .map(|row| {
            json!({
                "cohort": row.local, "size": row.size,
                "coverage": {
                    "consented_events": row.coverage.with_identity, "eligible_events": row.coverage.eligible_events,
                    "rate": ratio(row.coverage.with_identity as u64, row.coverage.eligible_events as u64),
                },
                "intervals": row.cells.iter().enumerate().map(|(week, cell)| json!({
                    "week": week, "start": instant(cell.start), "end": instant(cell.end),
                    "active": cell.active, "rate": rate(&cell.rate), "complete": cell.complete,
                })).collect::<Vec<_>>(),
            })
        })
        .collect();
    let want: Vec<Value> = expected["cohorts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            let mut c = c.clone();
            c.as_object_mut().unwrap().remove("members");
            c
        })
        .collect();
    assert_eq!(cohorts, want);

    let averages: Vec<Value> = retention.averages.iter().map(rate).collect();
    let want: Vec<Value> = expected["average_over_complete"]
        .as_object()
        .unwrap()
        .values()
        .map(|a| {
            if a["rate"].is_null() {
                a.clone()
            } else {
                a["rate"].clone()
            }
        })
        .collect();
    assert_eq!(averages, want);
    assert_eq!(
        instant(retention.watermark),
        expected["watermark"].as_str().unwrap()
    );
}

/// A report state on site A over the expected case's local days, at the evaluation instant,
/// with site A's hostname for the internal-referrer rule.
async fn report(
    storage: &Storage,
    site: u128,
    case: &Value,
    visualization: &str,
    report: Value,
) -> Value {
    report_with(storage, site, case, visualization, report, |_| {}).await
}

/// [`report`] with the state changed by `edit` first (filter, page).
async fn report_with(
    storage: &Storage,
    site: u128,
    case: &Value,
    visualization: &str,
    report: Value,
    edit: impl FnOnce(&mut Value),
) -> Value {
    let range = &case["range"];
    let mut state = json!({
        "v": 1, "site": "0d2c9a6e-5a3b-4f1e-9b1c-7e2f1a4d8c01", "measures": [], "dimensions": [],
        "filter": null, "range": { "kind": "days", "start": range["days"][0], "end": range["days"][1] },
        "timezone": range["timezone"], "granularity": "day", "compare": "none", "metric_version": 1,
        "sort": [], "page": { "size": 50, "index": 0 }, "visualization": visualization, "report": report
    });
    edit(&mut state);
    let state: ExplorationState = serde_json::from_value(state).unwrap();
    let query = holocron_analytics::explore::validate(&state, &[]).unwrap();
    let result = storage
        .read(Lane::Report, move |s| {
            let hostnames = ["www.example.com".to_owned()];
            let context = Context {
                site,
                now_ms: ms(EVALUATED_AT),
                hostnames: &hostnames,
                retention: holocron_protocol::retention::Retention::OneYear,
            };
            holocron_analytics::explore::explore(s, &query, &context)
        })
        .await
        .unwrap()
        .value
        .unwrap();
    assert_eq!(
        (
            holocron_analytics::explore::instant(result.resolved.start),
            holocron_analytics::explore::instant(result.resolved.end)
        ),
        (
            range["start"].as_str().unwrap().to_owned(),
            range["end"].as_str().unwrap().to_owned()
        )
    );
    serde_json::to_value(result.report.unwrap()).unwrap()
}

/// A fixture predicate (`pageview_path`, `path` or `event_name`) as a group with IDs `<id>a`, …:
/// a pageview path also requires no event name, since custom events carry their page's path.
fn predicate(id: &str, spec: &Value) -> Value {
    let cond = |suffix: &str, field: &str, cmp: &str, value: Option<&Value>| {
        let mut c =
            json!({ "kind": "cond", "id": format!("{id}{suffix}"), "field": field, "cmp": cmp });
        if let Some(v) = value {
            c["value"] = json!({ "t": "string", "v": v });
        }
        c
    };
    let (kind, value) = spec.as_object().unwrap().iter().next().unwrap();
    let items = match (kind.as_str(), value) {
        ("pageview_path" | "path", path) => vec![
            cond("a", "path", "eq", Some(path)),
            cond("b", "event_name", "is_not_set", None),
        ],
        ("event_name", name) => vec![cond("a", "event_name", "eq", Some(name))],
        other => panic!("unknown predicate {other:?}"),
    };
    json!({ "kind": "group", "op": "and", "items": items })
}

fn coverage(value: &Value) -> Value {
    let (with, all) = (
        value["with_identity"].as_u64().unwrap(),
        value["eligible_events"].as_u64().unwrap(),
    );
    json!({ "with_identity": with, "eligible_events": all, "rate": ratio(with, all) })
}

#[tokio::test(flavor = "multi_thread")]
async fn funnels_match_the_fixture() {
    let (_dir, storage, collection) = load().await;
    let site = collection.site("A").id;
    for case in read_json("expected/funnels.json")["cases"]
        .as_array()
        .unwrap()
    {
        let steps: Vec<Value> = case["steps"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
            .map(|(i, step)| predicate(&format!("s{i}"), step))
            .collect();
        let definition = json!({ "kind": "funnel", "v": 1, "steps": steps,
            "window_seconds": case["window_minutes"].as_u64().unwrap() * 60, "scope": case["scope"] });
        let got = report(&storage, site, case, "funnel", definition).await;
        let reached: Vec<Value> = got["steps"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["reached"].clone())
            .collect();
        let drops: Vec<Value> = got["steps"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|s| s.get("drop").cloned())
            .collect();
        assert_eq!(
            Value::from(reached),
            case["steps_reached"],
            "{}",
            case["id"]
        );
        assert_eq!(Value::from(drops), case["step_drop"], "{}", case["id"]);
        assert_eq!(got["conversion"]["v"], case["conversion"], "{}", case["id"]);
        // Daily funnels state their midnight limitation; person funnels span days.
        assert_eq!(
            got["daily_limit"],
            case["scope"] == "daily",
            "{}",
            case["id"]
        );
        if case["identity_coverage"].is_object() {
            assert_eq!(
                coverage(&got["coverage"]),
                case["identity_coverage"],
                "{}",
                case["id"]
            );
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn goals_match_the_fixture() {
    let (_dir, storage, collection) = load().await;
    let site = collection.site("A").id;
    for case in read_json("expected/goals.json")["cases"]
        .as_array()
        .unwrap()
    {
        let goal = &case["goal"];
        let definition = json!({ "kind": "goals", "v": 1, "goals": [{
            "predicate": predicate("g", &goal["predicate"]), "scope": goal["scope"], "counting": goal["counting"],
            "ref": { "id": "goal-1", "version": goal["version"] }
        }] });
        let got = &report(&storage, site, case, "table", definition).await["goals"][0];
        assert_eq!(
            (&got["completions"], &got["denominator"], &got["rate"]["v"]),
            (&case["completions"], &case["denominator"], &case["rate"]),
            "{}",
            case["id"]
        );
        let want = if goal["scope"] == "person" {
            let c = &case["coverage"];
            json!({ "with_identity": c["linked_events"], "eligible_events": c["eligible_events"], "rate": c["rate"] })
        } else {
            case["identity_coverage"].clone()
        };
        assert_eq!(coverage(&got["coverage"]), want, "{}", case["id"]);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn journeys_match_the_fixture() {
    let (_dir, storage, collection) = load().await;
    let site = collection.site("A").id;
    for case in read_json("expected/journeys.json")["cases"]
        .as_array()
        .unwrap()
    {
        let got = report(&storage, site, case, "journey", case["report"].clone()).await;
        let want: Vec<Value> = case["sequences"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| json!({ "steps": s["steps"], "sessions": s["sessions"] }))
            .collect();
        assert_eq!(got["sequences"], Value::from(want), "{}", case["id"]);
        assert_eq!(
            got["entry_sessions"], case["entry_sessions"],
            "{}",
            case["id"]
        );
        assert_eq!(
            got["distinct_sequences"], case["distinct_sequences"],
            "{}",
            case["id"]
        );
        assert_eq!(
            coverage(&got["coverage"]),
            case["identity_coverage"],
            "{}",
            case["id"]
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn attribution_matches_the_fixture() {
    let (_dir, storage, collection) = load().await;
    let site = collection.site("A").id;
    for case in read_json("expected/attribution.json")["cases"]
        .as_array()
        .unwrap()
    {
        let revenue = case["conversion"].is_string();
        let conversion = if revenue {
            json!({ "kind": "revenue" })
        } else {
            let goal = &case["conversion"]["goal"];
            json!({ "kind": "goal", "goal": {
                "predicate": predicate("g", &goal["predicate"]), "scope": goal["scope"], "counting": goal["counting"]
            } })
        };
        for model in ["last_touch", "first_touch"] {
            let definition = json!({ "kind": "attribution", "v": 1, "model": model, "lookback_days": 30,
                "scope": case["scope"], "touch_field": "utm_campaign", "conversions": [conversion] });
            let got = report(&storage, site, case, "table", definition).await;
            let attributed = &got["conversions"][0];
            let mut campaigns = serde_json::Map::new();
            let mut total = attributed["direct"]["conversions"].as_i64().unwrap();
            for row in attributed["touches"].as_array().unwrap() {
                total += row["attributed"]["conversions"].as_i64().unwrap();
                if row["touch"]["t"] == "string" {
                    let value = if revenue {
                        let money = &row["attributed"]["revenue"];
                        json!({ money[0]["currency"].as_str().unwrap(): money[0]["minor_units"] })
                    } else {
                        row["attributed"]["conversions"].clone()
                    };
                    campaigns.insert(row["touch"]["v"].as_str().unwrap().to_owned(), value);
                }
            }
            let id = format!("{} {model}", case["id"]);
            // Every conversion event is attributed once: to a touch value or to direct.
            assert_eq!(
                total,
                case["conversions"].as_object().unwrap().len() as i64,
                "{id}"
            );
            let key = if revenue {
                format!("{model}_revenue_by_utm_campaign")
            } else {
                format!("{model}_by_utm_campaign")
            };
            if let Some(want) = case.get(&key) {
                assert_eq!(Value::Object(campaigns), *want, "{id}");
            }
            if let Some(direct) = case.get("direct") {
                assert_eq!(attributed["direct"]["conversions"], direct[model], "{id}");
            }
            assert_eq!(
                coverage(&got["coverage"]),
                case["identity_coverage"],
                "{id}"
            );
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn session_list_pages_the_sessions_of_the_range() {
    let (_dir, storage, collection) = load().await;
    let site = collection.site("A").id;
    // The five journey sessions of 2026-06-18 (`journeys.json`), oldest first.
    let case = &read_json("expected/journeys.json")["cases"][0];
    let definition = json!({ "kind": "session_list", "v": 1, "order": "start_asc",
        "columns": ["pageviews", "entry_path", "exit_path", "identity_kind", "channel"] });
    let cells = |got: &Value| -> Vec<Vec<Value>> {
        got["rows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| {
                row.as_array()
                    .unwrap()
                    .iter()
                    .map(|c| c["v"].clone())
                    .collect()
            })
            .collect()
    };
    let got = report(&storage, site, case, "table", definition.clone()).await;
    assert_eq!(got["row_count"], 5);
    assert_eq!(
        cells(&got),
        [
            json!([4, "/", "/docs", "daily", "direct"]),
            json!([4, "/", "/pricing", "daily", "direct"]),
            json!([3, "/", "/pricing", "daily", "direct"]),
            json!([3, "/", "/docs", "daily", "direct"]),
            json!([1, "/", "/", "daily", "direct"]),
        ]
        .iter()
        .map(|row| row.as_array().unwrap().clone())
        .collect::<Vec<_>>()
    );
    // A filter keeps sessions with at least one matching event; pages count over all of them.
    let docs = report_with(&storage, site, case, "table", definition, |state| {
        state["filter"] = json!({ "kind": "group", "op": "and", "items": [
            { "kind": "cond", "id": "f1", "field": "path", "cmp": "eq", "value": { "t": "string", "v": "/docs" } }
        ] });
        state["page"] = json!({ "size": 1, "index": 1 });
    })
    .await;
    assert_eq!(docs["row_count"], 2);
    assert_eq!(
        cells(&docs),
        vec![
            json!([3, "/", "/docs", "daily", "direct"])
                .as_array()
                .unwrap()
                .clone()
        ]
    );
}
