//! `POST /api/record` request validation (collection compatibility contract, "POST /api/record").
//! A valid body becomes a [`Request`]; anything else is a 400 with the upstream envelope.

use serde::{Deserialize, Deserializer};
use serde_json::value::RawValue;
use uuid::Uuid;

/// Upstream's request limit, unchanged (0005 Request body caps).
pub const RECORD_MAX_BYTES: usize = 1_000_000;
const MAX_EVENTS: usize = 200;

/// A validation failure: upstream answers 400 `Bad request`.
#[derive(Debug, PartialEq, Eq)]
pub struct Invalid;

#[derive(Debug)]
pub struct Request<'a> {
    pub website: Uuid,
    /// `payload.timestamp`: the replay part's chunk index, Unix seconds by the browser's clock.
    pub timestamp: Option<i64>,
    pub payload: Payload<'a>,
}

#[derive(Debug)]
pub enum Payload<'a> {
    /// rrweb events, kept unparsed.
    Replay(Vec<&'a RawValue>),
    Heatmap(Vec<HeatmapEvent>),
}

impl Payload<'_> {
    pub fn is_empty(&self) -> bool {
        match self {
            Payload::Replay(events) => events.is_empty(),
            Payload::Heatmap(events) => events.is_empty(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HeatmapKind {
    Click,
    Scroll,
}

/// One heatmap interaction. Numbers are coerced from strings as upstream's zod schema does.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HeatmapEvent {
    #[serde(rename = "type")]
    pub kind: HeatmapKind,
    pub url: String,
    #[serde(default, deserialize_with = "number")]
    pub page_x: Option<f64>,
    #[serde(default, deserialize_with = "number")]
    pub page_y: Option<f64>,
    #[serde(default, deserialize_with = "number")]
    pub viewport_w: Option<f64>,
    #[serde(default, deserialize_with = "number")]
    pub viewport_h: Option<f64>,
    #[serde(default, deserialize_with = "number")]
    pub scroll_pct: Option<f64>,
    /// Unix milliseconds by the browser's clock.
    #[serde(default, deserialize_with = "number")]
    pub timestamp: Option<f64>,
}

#[derive(Deserialize)]
struct Body<'a> {
    #[serde(rename = "type")]
    kind: String,
    #[serde(borrow)]
    payload: &'a RawValue,
}

#[derive(Deserialize)]
struct Envelope<'a> {
    website: String,
    #[serde(borrow)]
    events: Vec<&'a RawValue>,
    #[serde(default, deserialize_with = "number")]
    timestamp: Option<f64>,
}

pub fn parse(json: &str) -> Result<Request<'_>, Invalid> {
    let body: Body = serde_json::from_str(json).map_err(|_| Invalid)?;
    let envelope: Envelope = serde_json::from_str(body.payload.get()).map_err(|_| Invalid)?;
    if envelope.website.len() != 36 || envelope.events.len() > MAX_EVENTS {
        return Err(Invalid);
    }
    let website = Uuid::try_parse(&envelope.website).map_err(|_| Invalid)?;
    let timestamp = match envelope.timestamp {
        Some(t) if t.fract() != 0.0 || t.abs() > 9_007_199_254_740_991.0 => return Err(Invalid),
        other => other.map(|t| t as i64),
    };
    let payload = match body.kind.as_str() {
        "record" => Payload::Replay(envelope.events),
        "heatmap" => Payload::Heatmap(
            envelope
                .events
                .iter()
                .map(|event| serde_json::from_str(event.get()).map_err(|_| Invalid))
                .collect::<Result<_, _>>()?,
        ),
        _ => return Err(Invalid),
    };
    Ok(Request {
        website,
        timestamp,
        payload,
    })
}

/// `z.coerce.number()`: a finite number, or a string holding one.
fn number<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<f64>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Raw {
        Number(f64),
        Text(String),
    }
    let value = match Option::<Raw>::deserialize(deserializer)? {
        None => return Ok(None),
        Some(Raw::Number(n)) => n,
        Some(Raw::Text(text)) => text
            .trim()
            .parse::<f64>()
            .map_err(|_| serde::de::Error::custom("not a number"))?,
    };
    if value.is_finite() {
        Ok(Some(value))
    } else {
        Err(serde::de::Error::custom("not finite"))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const SITE: &str = "5e1a0000-0000-4000-8000-000000000002";

    fn body(kind: &str, events: serde_json::Value) -> String {
        json!({ "type": kind, "payload": { "website": SITE, "events": events, "timestamp": "1790000000" } }).to_string()
    }

    #[test]
    fn heatmap_numbers_are_coerced_and_url_is_required() {
        let text = body(
            "heatmap",
            json!([{ "type": "click", "url": "/a", "pageX": "12", "pageY": 30, "viewportW": 1280 }]),
        );
        let request = parse(&text).unwrap();
        assert_eq!(request.timestamp, Some(1_790_000_000));
        let Payload::Heatmap(events) = request.payload else {
            panic!("heatmap")
        };
        assert_eq!(
            (events[0].page_x, events[0].page_y),
            (Some(12.0), Some(30.0))
        );

        assert_eq!(
            parse(&body("heatmap", json!([{ "type": "click" }]))).unwrap_err(),
            Invalid
        );
        assert_eq!(
            parse(&body("heatmap", json!([{ "type": "hover", "url": "/" }]))).unwrap_err(),
            Invalid
        );
    }

    #[test]
    fn envelope_limits_follow_upstream() {
        assert!(
            matches!(parse(&body("record", json!([]))).unwrap().payload, Payload::Replay(e) if e.is_empty())
        );
        assert_eq!(
            parse(&body("record", json!(vec![json!({ "type": 3 }); 201]))).unwrap_err(),
            Invalid
        );
        assert_eq!(parse(&body("other", json!([]))).unwrap_err(), Invalid);
        let short = json!({ "type": "record", "payload": { "website": "5e1a0000", "events": [] } })
            .to_string();
        assert_eq!(parse(&short).unwrap_err(), Invalid);
    }
}
