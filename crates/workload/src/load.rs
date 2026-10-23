//! Ingestion stand-in: turns generated groups into canonical storage records.
//!
//! Identity uses the 0001 construction (BLAKE3 keyed hash of IP and user
//! agent, or of the supplied ID, under a day key; profiles under a site key).
//! The day keys here are derived from a fixed gate secret instead of random
//! stored keys, so a later run can reach the same identities. Pseudonym
//! values never enter a golden result, only their equality does.

use chrono::NaiveDate;
use holocron_storage::{
    Dimensions, Event, EventKind, Identify, Performance, Property, PropertyValue, Record, Revenue,
};

use crate::catalog::{Catalog, fixture_uuid};
use crate::fixture::{Group, Kind, Value};

const GATE_ROOT: [u8; 32] = *b"holocron-storage-gate-root-key!!";

pub struct Identities {
    site_keys: Vec<[u8; 32]>,
}

impl Identities {
    pub fn new(cat: &Catalog) -> Self {
        let site_keys = cat
            .sites
            .iter()
            .map(|s| {
                blake3::derive_key(&format!("holocron/v1/site-persistent/{}", s.id), &GATE_ROOT)
            })
            .collect();
        Identities { site_keys }
    }

    fn day_key(&self, site: usize, day: NaiveDate) -> [u8; 32] {
        blake3::derive_key(&format!("holocron-gate/day-key/{site}/{day}"), &GATE_ROOT)
    }

    pub fn anonymous(&self, site: usize, day: NaiveDate, ip: &str, user_agent: &str) -> u128 {
        let mut h = blake3::Hasher::new_keyed(&self.day_key(site, day));
        h.update(&[0])
            .update(ip.as_bytes())
            .update(&[0])
            .update(user_agent.as_bytes());
        truncate(h.finalize())
    }

    pub fn daily_identified(&self, site: usize, day: NaiveDate, id: &str) -> u128 {
        let mut h = blake3::Hasher::new_keyed(&self.day_key(site, day));
        h.update(&[1]).update(id.as_bytes());
        truncate(h.finalize())
    }

    pub fn profile(&self, site: usize, id: &str) -> u128 {
        truncate(blake3::keyed_hash(&self.site_keys[site], id.as_bytes()))
    }
}

fn truncate(hash: blake3::Hash) -> u128 {
    u128::from_be_bytes(hash.as_bytes()[..16].try_into().unwrap())
}

pub fn days_since_epoch(day: NaiveDate) -> i32 {
    (day - NaiveDate::from_ymd_opt(1970, 1, 1).unwrap()).num_days() as i32
}

pub(crate) fn property_value(v: &Value) -> PropertyValue {
    match v {
        Value::Null => PropertyValue::Null,
        Value::Bool(b) => PropertyValue::Bool(*b),
        Value::Int(i) => PropertyValue::Integer(*i),
        Value::Dec(d) => PropertyValue::Decimal(*d),
        Value::Date(ms) => PropertyValue::Date(*ms),
        Value::Str(s) => PropertyValue::String((*s).to_owned()),
        Value::Array(items) => PropertyValue::Array(items.iter().map(property_value).collect()),
    }
}

/// The group's identify message (if identified) followed by its ten events.
pub fn records(cat: &Catalog, ids: &Identities, g: &Group) -> Vec<Record> {
    let site = &cat.sites[g.site];
    let site_id = site.id.as_u128();
    let local_day = days_since_epoch(g.local_day);
    let ip = g.ip.to_string();
    let (daily_id, profile_id) = match &g.supplied_id {
        Some(id) => (
            ids.daily_identified(g.site, g.local_day, id),
            Some(ids.profile(g.site, id)),
        ),
        None => (ids.anonymous(g.site, g.local_day, &ip, &g.user_agent), None),
    };
    let mut out = Vec::with_capacity(11);
    if let Some((tier, created)) = g.identify {
        out.push(Record::Identify(Identify {
            id: fixture_uuid(&format!("identify:{}", g.g)).as_u128(),
            site: site_id,
            event_time_ms: (g.t0_s - 1) * 1000,
            receive_time_ms: (g.t0_s - 1) * 1000,
            local_day,
            daily_id: Some(daily_id),
            profile_id,
            properties: vec![
                Property {
                    key: "account_tier".into(),
                    value: PropertyValue::String(cat.account_tiers.0[tier].into()),
                },
                Property {
                    key: "account_created".into(),
                    value: PropertyValue::Date(created),
                },
            ],
        }));
    }
    let country = cat.countries.values[g.country];
    let campaign = g.campaign.map(|c| cat.campaign.values[c].to_owned());
    for e in &g.events {
        let time = e.time_s * 1000;
        out.push(Record::Event(Event {
            id: fixture_uuid(&format!("event:{}", e.e)).as_u128(),
            site: site_id,
            kind: match e.kind {
                Kind::Pageview => EventKind::Pageview,
                Kind::Custom => EventKind::CustomEvent,
                Kind::Performance => EventKind::Performance,
            },
            event_time_ms: time,
            receive_time_ms: time,
            local_day,
            daily_id: Some(daily_id),
            profile_id,
            client_event_id: None,
            dimensions: Dimensions {
                hostname: Some(site.hostname.to_owned()),
                path: Some(e.path.to_owned()),
                query: e.query.map(str::to_owned),
                title: Some(e.path.to_owned()),
                referrer_domain: g.referrer.map(|r| cat.referrer.values[r].to_owned()),
                utm_source: campaign.as_ref().map(|_| "newsletter".to_owned()),
                utm_medium: campaign.as_ref().map(|_| "email".to_owned()),
                utm_campaign: campaign.clone(),
                country: Some(country.to_owned()),
                region: Some(format!("{country}-R{}", g.region)),
                browser: Some(cat.browser.values[g.browser].to_owned()),
                os: Some(cat.os.values[g.os].to_owned()),
                device: Some(cat.device.values[g.device].to_owned()),
                screen: Some(cat.screens[g.device].to_owned()),
                language: Some("en-US".to_owned()),
                event_name: e.event_name.map(str::to_owned),
                ..Dimensions::default()
            },
            performance: e.performance.map(|p| {
                let ms = |v: Option<i64>| v.map(|x| x as i32);
                Performance {
                    lcp: ms(p[0]),
                    fcp: ms(p[1]),
                    ttfb: ms(p[2]),
                    inp: ms(p[3]),
                    cls: ms(p[4]),
                }
            }),
            revenue: e.revenue.map(|(c, amount)| Revenue {
                currency: cat.currencies[c].code.to_owned(),
                minor_units: amount,
            }),
            properties: e
                .properties
                .iter()
                .map(|(k, v)| Property {
                    key: cat.keys[*k].name.to_owned(),
                    value: property_value(v),
                })
                .collect(),
        }));
    }
    out
}
