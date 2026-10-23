//! `/api/send` request validation (collection compatibility contract, "POST /api/send").
//! A valid body becomes an [`Item`]; anything else is a 400 with the upstream envelope.

use holocron_analytics::identity::{MAX_TIME_MS, MIN_TIME_MS};
use holocron_storage::{Dimensions, Performance};
use serde::Deserialize;
use serde_json::value::RawValue;
use url::Url;
use uuid::Uuid;

use super::client::{Geo, classify};
use super::decimal::Decimal;
use super::text::{decode_component, decode_uri, truncate};

/// zod 4 `int()` stops at `Number.MAX_SAFE_INTEGER`.
const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;
const CLICK_IDS: [&str; 6] = [
    "gclid",
    "fbclid",
    "msclkid",
    "ttclid",
    "li_fat_id",
    "twclid",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Event,
    Identify,
    Performance,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Website(Uuid),
    Link(Uuid),
    Pixel(Uuid),
}

impl Source {
    pub fn id(self) -> Uuid {
        match self {
            Source::Website(id) | Source::Link(id) | Source::Pixel(id) => id,
        }
    }
}

/// Value of the top-level `consent` payload key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Consent {
    /// `"persistent"`: link this `id` to its persistent profile from now on.
    Persistent,
    /// `"none"`: revoke, following events return to daily mode.
    None,
    /// Any other value: ignored and counted, the payload is still accepted.
    Unrecognized,
}

/// One validated `/api/send` body.
#[derive(Debug)]
pub struct Item<'a> {
    pub kind: Kind,
    pub source: Source,
    pub data: Option<&'a RawValue>,
    pub hostname: Option<String>,
    pub language: Option<String>,
    pub screen: Option<String>,
    pub title: Option<String>,
    pub url: Option<String>,
    pub referrer: Option<String>,
    pub name: Option<String>,
    pub tag: Option<String>,
    pub browser: Option<String>,
    pub os: Option<String>,
    pub device: Option<String>,
    /// `ip` and `userAgent`, honored only on requests with a collection key for the item's
    /// site (0001 Transient inputs).
    pub ip: Option<String>,
    pub user_agent: Option<String>,
    /// Event time from `timestamp`, else receive time is used.
    pub timestamp_ms: Option<i64>,
    /// `id`, trimmed and truncated to 50 characters; empty is absent (0001).
    pub distinct_id: Option<String>,
    pub event_id: Option<String>,
    /// The consent extension key, when sent (0001 Persistent consent transport).
    pub consent: Option<Consent>,
    pub performance: Performance,
    /// Identifies the payload for duplicate-conflict counting (0002).
    pub fingerprint: u128,
}

#[derive(Deserialize)]
struct Body<'a> {
    #[serde(rename = "type")]
    kind: Kind,
    #[serde(borrow)]
    payload: &'a RawValue,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Payload<'a> {
    website: Option<String>,
    link: Option<String>,
    pixel: Option<String>,
    #[serde(borrow)]
    data: Option<&'a RawValue>,
    hostname: Option<String>,
    language: Option<String>,
    referrer: Option<String>,
    screen: Option<String>,
    title: Option<String>,
    url: Option<String>,
    name: Option<String>,
    tag: Option<String>,
    // Strings, as upstream validates them, but honored only with a collection key (0001).
    ip: Option<String>,
    user_agent: Option<String>,
    #[serde(borrow)]
    timestamp: Option<&'a RawValue>,
    id: Option<String>,
    browser: Option<String>,
    os: Option<String>,
    device: Option<String>,
    lcp: Option<f64>,
    inp: Option<f64>,
    #[serde(borrow)]
    cls: Option<&'a RawValue>,
    fcp: Option<f64>,
    ttfb: Option<f64>,
    event_id: Option<String>,
    #[serde(borrow)]
    consent: Option<&'a RawValue>,
}

/// A validation failure: upstream answers 400 `Bad request`.
#[derive(Debug, PartialEq, Eq)]
pub struct Invalid;

pub fn parse(json: &str) -> Result<Item<'_>, Invalid> {
    let body: Body = serde_json::from_str(json).map_err(|_| Invalid)?;
    let raw = body.payload.get();
    if !is_object(raw) {
        return Err(Invalid);
    }
    let p: Payload = serde_json::from_str(raw).map_err(|_| Invalid)?;

    let source = match (p.website, p.link, p.pixel) {
        (Some(id), None, None) => Source::Website(uuid(&id)?),
        (None, Some(id), None) => Source::Link(uuid(&id)?),
        (None, None, Some(id)) => Source::Pixel(uuid(&id)?),
        _ => return Err(Invalid),
    };
    if body.kind == Kind::Identify && !matches!(source, Source::Website(_)) {
        return Err(Invalid);
    }
    if p.data.is_some_and(|data| !is_object(data.get())) {
        return Err(Invalid);
    }
    for value in [&p.url, &p.referrer].into_iter().flatten() {
        if base("localhost").join(value).is_err() {
            return Err(Invalid);
        }
    }
    for value in [&p.name, &p.tag].into_iter().flatten() {
        if value.starts_with(['=', '+', '-', '@', '\t', '\r']) {
            return Err(Invalid);
        }
    }
    if let Some(event_id) = &p.event_id
        && !(1..=64).contains(&event_id.len())
            | !event_id.bytes().all(|b| (0x20..=0x7e).contains(&b))
    {
        return Err(Invalid);
    }
    let metric = |value: Option<f64>, max: f64| match value {
        Some(v) if !(0.0..=max).contains(&v) => Err(Invalid),
        _ => Ok(value),
    };
    let ms = |value: Option<f64>| value.map(|v| v.round() as i32);
    // The range is checked on the float, as upstream's zod does; the stored value in units of
    // 0.0001 is rounded half to even from the number text, like revenue (0002).
    let cls = match p.cls {
        Some(raw) => {
            let value = serde_json::from_str::<f64>(raw.get()).map_err(|_| Invalid)?;
            metric(Some(value), 100.0)?;
            let units = Decimal::parse(raw.get())
                .and_then(|d| d.scaled(4))
                .ok_or(Invalid)?;
            Some(units as i32)
        }
        None => None,
    };
    let performance = Performance {
        lcp: ms(metric(p.lcp, 60_000.0)?),
        inp: ms(metric(p.inp, 60_000.0)?),
        fcp: ms(metric(p.fcp, 60_000.0)?),
        ttfb: ms(metric(p.ttfb, 60_000.0)?),
        cls,
    };

    Ok(Item {
        kind: body.kind,
        source,
        data: p.data,
        hostname: p.hostname,
        language: p.language,
        screen: p.screen,
        title: p.title,
        url: p.url,
        referrer: p.referrer,
        name: p.name,
        tag: p.tag,
        browser: p.browser,
        os: p.os,
        device: p.device,
        ip: p.ip,
        user_agent: p.user_agent,
        timestamp_ms: p.timestamp.map(timestamp_ms).transpose()?.flatten(),
        distinct_id: p
            .id
            .map(|id| truncate(id.trim(), 50))
            .filter(|id| !id.is_empty()),
        event_id: p.event_id,
        consent: p.consent.map(|raw| match raw.get() {
            r#""persistent""# => Consent::Persistent,
            r#""none""# => Consent::None,
            _ => Consent::Unrecognized,
        }),
        performance,
        fingerprint: u128::from_be_bytes(
            blake3::hash(raw.as_bytes()).as_bytes()[..16]
                .try_into()
                .expect("16 bytes"),
        ),
    })
}

fn is_object(raw: &str) -> bool {
    raw.trim_start().starts_with('{')
}

/// zod `uuid()`: the hyphenated 36-character form only.
fn uuid(value: &str) -> Result<Uuid, Invalid> {
    if value.len() != 36 {
        return Err(Invalid);
    }
    Uuid::try_parse(value).map_err(|_| Invalid)
}

/// `z.coerce.number().int()` in seconds. Zero counts as absent, as upstream's `timestamp ?`.
/// Values the engines cannot store are refused like an `i64` overflow (0002 Event time).
fn timestamp_ms(raw: &RawValue) -> Result<Option<i64>, Invalid> {
    let seconds = match serde_json::from_str::<serde_json::Value>(raw.get()).map_err(|_| Invalid)? {
        serde_json::Value::Number(n) => n.as_f64().ok_or(Invalid)?,
        serde_json::Value::String(s) => match s.trim() {
            "" => 0.0,
            text => text.parse::<f64>().map_err(|_| Invalid)?,
        },
        _ => return Err(Invalid),
    };
    if seconds.fract() != 0.0 || seconds.abs() > MAX_SAFE_INTEGER {
        return Err(Invalid);
    }
    if seconds == 0.0 {
        return Ok(None);
    }
    let ms = (seconds as i64).checked_mul(1000).ok_or(Invalid)?;
    if !(MIN_TIME_MS..=MAX_TIME_MS).contains(&ms) {
        return Err(Invalid);
    }
    Ok(Some(ms))
}

fn base(host: &str) -> Url {
    Url::parse(&format!("https://{host}"))
        .unwrap_or_else(|_| Url::parse("https://localhost").expect("valid base"))
}

/// Stored hostname: lowercase IDNA ASCII without port or trailing dot, at most 100 characters
/// (0001 Hostname normalization).
fn normalize_host(host: &str) -> String {
    let host = host.trim();
    let normalized = match Url::parse(&format!("https://{host}")) {
        Ok(url) => url.host_str().unwrap_or(host).to_owned(),
        Err(_) => host.to_lowercase(),
    };
    truncate(normalized.trim_end_matches('.'), 100)
}

fn strip_www(host: &str) -> &str {
    host.strip_prefix("www.").unwrap_or(host)
}

fn not_empty(value: String) -> Option<String> {
    (!value.is_empty()).then_some(value)
}

impl Item<'_> {
    /// Request fields as stored dimensions, following upstream's `saveEvent` extraction.
    pub fn dimensions(&self, user_agent: &str, geo: &Geo) -> Dimensions {
        let page_base = base(self.hostname.as_deref().unwrap_or("localhost"));
        let current = page_base
            .join(self.url.as_deref().unwrap_or("undefined"))
            .unwrap_or(page_base);
        let pathname = current.path();
        let pathname = if pathname == "/undefined" {
            ""
        } else {
            pathname
        };
        let path = match (self.kind, current.fragment()) {
            (Kind::Event, Some(fragment)) => format!("{pathname}#{fragment}"),
            _ => pathname.to_owned(),
        };
        let hostname = match &self.hostname {
            Some(host) => normalize_host(host),
            None => current.host_str().map(normalize_host).unwrap_or_default(),
        };
        let param = |name: &str| {
            current
                .query_pairs()
                .find(|(key, _)| key == name)
                .map(|(_, value)| truncate(&value, 500))
        };
        let is_event = self.kind == Kind::Event;
        // UTM values are trimmed; an empty one is missing, the "not set" cell (0003 UTM values).
        let utm = |name: &str| {
            is_event
                .then(|| param(name))
                .flatten()
                .map(|value| value.trim().to_owned())
                .and_then(not_empty)
        };

        let event_domain = strip_www(&hostname).to_owned();
        let (referrer_domain, referrer_path) = match self.referrer.as_deref() {
            Some(referrer) if !referrer.is_empty() && self.kind == Kind::Event => {
                match base(&event_domain).join(referrer) {
                    Ok(url) => {
                        let domain = strip_www(url.host_str().unwrap_or_default()).to_owned();
                        (
                            not_empty(domain).filter(|d| *d != event_domain),
                            Some(truncate(&decode_uri(url.path()), 500)),
                        )
                    }
                    Err(_) => (None, None),
                }
            }
            _ => (None, None),
        };

        let (browser, os, device) = classify(user_agent, self.screen.as_deref());
        let field = |value: &Option<String>, max: usize| value.as_deref().map(|v| truncate(v, max));
        Dimensions {
            hostname: not_empty(hostname),
            path: Some(truncate(&decode_uri(&path), 500)),
            query: is_event
                .then(|| current.query().map(|q| truncate(q, 500)))
                .flatten()
                .and_then(not_empty),
            title: self
                .title
                .as_deref()
                .map(|t| truncate(&decode_component(t), 500)),
            referrer_domain: referrer_domain.map(|d| truncate(&d, 500)),
            referrer_path,
            utm_source: utm("utm_source"),
            utm_medium: utm("utm_medium"),
            utm_campaign: utm("utm_campaign"),
            utm_content: utm("utm_content"),
            utm_term: utm("utm_term"),
            click_id: is_event
                .then(|| CLICK_IDS.iter().find_map(|name| param(name)))
                .flatten(),
            country: geo.country.clone(),
            region: geo.region.clone(),
            city: geo.city.clone(),
            latitude: geo.latitude,
            longitude: geo.longitude,
            browser: field(&self.browser, 20).or(browser),
            os: field(&self.os, 20).or(os),
            device: field(&self.device, 20).or(device),
            screen: field(&self.screen, 11),
            language: field(&self.language, 35),
            event_name: field(&self.name, 50),
            tag: field(&self.tag, 50),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SITE: &str = "5e1a0000-0000-4000-8000-000000000001";

    fn item(payload: &str) -> Result<Item<'static>, Invalid> {
        let body: &'static str =
            Box::leak(format!(r#"{{"type":"event","payload":{payload}}}"#).into_boxed_str());
        parse(body)
    }

    #[test]
    fn exactly_one_source_is_required() {
        assert!(item(&format!(r#"{{"website":"{SITE}"}}"#)).is_ok());
        assert_eq!(item("{}").unwrap_err(), Invalid);
        assert_eq!(
            item(&format!(r#"{{"website":"{SITE}","link":"{SITE}"}}"#)).unwrap_err(),
            Invalid
        );
        assert_eq!(item(r#"{"website":"not-a-uuid"}"#).unwrap_err(), Invalid);
    }

    #[test]
    fn timestamps_coerce_like_upstream() {
        let ts = |value: &str| {
            item(&format!(r#"{{"website":"{SITE}","timestamp":{value}}}"#)).map(|i| i.timestamp_ms)
        };
        assert_eq!(ts("1790812800"), Ok(Some(1_790_812_800_000)));
        assert_eq!(ts(r#""1790812800""#), Ok(Some(1_790_812_800_000)));
        assert_eq!(ts("1.5"), Err(Invalid));
        assert_eq!(ts(r#""soon""#), Err(Invalid));
        assert_eq!(ts("9007199254740992"), Err(Invalid));
    }

    #[test]
    fn dimensions_follow_save_event() {
        let item = item(&format!(
            r#"{{"website":"{SITE}","hostname":"WWW.Example.com:8080","url":"/pricing?utm_source=news&gclid=abc#plans","referrer":"https://www.example.com/home","title":"Caf%C3%A9","name":"{}"}}"#,
            "x".repeat(60)
        ))
        .unwrap();
        let d = item.dimensions("", &Geo::default());
        assert_eq!(d.hostname.as_deref(), Some("www.example.com"));
        assert_eq!(d.path.as_deref(), Some("/pricing#plans"));
        assert_eq!(d.query.as_deref(), Some("utm_source=news&gclid=abc"));
        assert_eq!(d.utm_source.as_deref(), Some("news"));
        assert_eq!(d.click_id.as_deref(), Some("abc"));
        assert_eq!(d.referrer_domain, None, "self-referral");
        assert_eq!(d.title.as_deref(), Some("Café"));
        assert_eq!(d.event_name.map(|n| n.len()), Some(50));
    }

    #[test]
    fn utm_values_are_trimmed_and_empty_ones_are_missing() {
        let item = item(&format!(
            r#"{{"website":"{SITE}","url":"/?utm_source=+news%20&utm_medium=&utm_campaign=%20+&utm_content=a%20%20b"}}"#
        ))
        .unwrap();
        let d = item.dimensions("", &Geo::default());
        assert_eq!(d.utm_source.as_deref(), Some("news"));
        assert_eq!(d.utm_medium, None);
        assert_eq!(d.utm_campaign, None, "whitespace only");
        assert_eq!(
            d.utm_content.as_deref(),
            Some("a  b"),
            "inner whitespace kept"
        );
        assert_eq!(d.utm_term, None);
    }

    #[test]
    fn cls_rounds_half_to_even_from_the_number_text() {
        let cls = |value: &str| {
            let body: &'static str = Box::leak(
                format!(
                    r#"{{"type":"performance","payload":{{"website":"{SITE}","cls":{value}}}}}"#
                )
                .into_boxed_str(),
            );
            parse(body).map(|i| i.performance.cls)
        };
        // Through a float this is 1.4999999999999998 units, which would round to 1.
        assert_eq!(cls("0.00015"), Ok(Some(2)));
        assert_eq!(cls("0.00025"), Ok(Some(2)));
        assert_eq!(cls("0.12345"), Ok(Some(1234)));
        assert_eq!(cls("1e-1"), Ok(Some(1000)));
        assert_eq!(cls("100"), Ok(Some(1_000_000)));
        assert_eq!(cls("100.5"), Err(Invalid));
        assert_eq!(cls(r#""0.1""#), Err(Invalid));
        assert_eq!(cls("null"), Ok(None));
    }
}
