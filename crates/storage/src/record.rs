//! Canonical records as they cross the durable inbox into analytics.
//!
//! Collection validates payloads and derives identities before admission, so a
//! record carries pseudonyms and typed values only, never raw IP or user agent
//! (0001 Transient inputs).

use serde::{Deserialize, Serialize};

/// Inbox payload encoding version. Ledger entries record it so a replay from
/// another encoding is refused (0006 Schema versions). Format 2 added the
/// client event ID fingerprint. Format 3 added the visitor coordinates.
pub const INBOX_FORMAT: i64 = 3;

/// 16-byte identifiers (site, event, pseudonym, capture) as plain integers.
pub type Id = u128;

// Records travel in batches by value; boxing events would add one allocation each.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Record {
    Event(Event),
    Identify(Identify),
    ReplayFragment(ReplayFragment),
    HeatmapInteraction(HeatmapInteraction),
    Revocation(Revocation),
}

impl Record {
    pub fn site(&self) -> Id {
        match self {
            Record::Event(e) => e.site,
            Record::Identify(i) => i.site,
            Record::ReplayFragment(f) => f.site,
            Record::HeatmapInteraction(h) => h.site,
            Record::Revocation(r) => r.site,
        }
    }

    pub(crate) fn encode(&self) -> Vec<u8> {
        postcard::to_stdvec(self).expect("records always serialize")
    }

    /// Decodes a record written in `format`: the current one, or format 2 rows a previous
    /// release left in the inbox. Any other format is refused.
    pub(crate) fn decode(format: i64, bytes: &[u8]) -> Result<Self, String> {
        match format {
            INBOX_FORMAT => postcard::from_bytes(bytes).map_err(|e| e.to_string()),
            format2::FORMAT => postcard::from_bytes::<format2::Record>(bytes)
                .map(Self::from)
                .map_err(|e| e.to_string()),
            _ => Err(format!("format {format} is not readable")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum EventKind {
    Pageview = 1,
    CustomEvent = 2,
    Performance = 3,
    /// Tracked link hit. `site` is the link ID; no session derivation (0002).
    LinkEvent = 4,
    /// Pixel hit. `site` is the pixel ID; no session derivation (0002).
    PixelEvent = 5,
}

/// A primary event: pageview, custom event, performance report, link or pixel hit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub id: Id,
    pub site: Id,
    pub kind: EventKind,
    pub event_time_ms: i64,
    pub receive_time_ms: i64,
    /// Identity day in the site timezone, days since 1970-01-01.
    pub local_day: i32,
    /// Daily pseudonym; `None` for keyless events (0001).
    pub daily_id: Option<Id>,
    /// Persistent profile when consent was active at event time.
    pub profile_id: Option<Id>,
    /// Optional client `eventId`, deduplicated per source for 48 hours (0002).
    pub client_event_id: Option<ClientEventId>,
    pub dimensions: Dimensions,
    pub performance: Option<Performance>,
    pub revenue: Option<Revenue>,
    pub properties: Vec<Property>,
}

/// A client `eventId` with a fingerprint of the payload that carried it. A later
/// copy with another fingerprint is still a duplicate, counted as a conflict.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientEventId {
    pub id: String,
    pub fingerprint: u128,
}

/// Request fields. `None` is "not set" (0003 Common terms).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Dimensions {
    pub hostname: Option<String>,
    pub path: Option<String>,
    pub query: Option<String>,
    pub title: Option<String>,
    pub referrer_domain: Option<String>,
    pub referrer_path: Option<String>,
    pub utm_source: Option<String>,
    pub utm_medium: Option<String>,
    pub utm_campaign: Option<String>,
    pub utm_content: Option<String>,
    pub utm_term: Option<String>,
    pub click_id: Option<String>,
    pub country: Option<String>,
    pub region: Option<String>,
    pub city: Option<String>,
    /// Tenths of a degree, from a trusted CDN header (0017).
    pub latitude: Option<i16>,
    /// Tenths of a degree, from a trusted CDN header (0017).
    pub longitude: Option<i16>,
    pub browser: Option<String>,
    pub os: Option<String>,
    pub device: Option<String>,
    pub screen: Option<String>,
    pub language: Option<String>,
    pub event_name: Option<String>,
    pub tag: Option<String>,
}

/// Web vitals. Milliseconds, except `cls` in units of 0.0001.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Performance {
    pub lcp: Option<i32>,
    pub fcp: Option<i32>,
    pub ttfb: Option<i32>,
    pub inp: Option<i32>,
    pub cls: Option<i32>,
}

/// Revenue in integer minor units of an ISO 4217 currency (0002).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Revenue {
    pub currency: String,
    pub minor_units: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Property {
    pub key: String,
    pub value: PropertyValue,
}

/// One typed property value (0002 Property type system).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PropertyValue {
    Null,
    Bool(bool),
    Integer(i64),
    /// Units of 0.0001, the DECIMAL(18,4) scale.
    Decimal(i64),
    /// UTC milliseconds.
    Date(i64),
    String(String),
    Array(Vec<PropertyValue>),
}

/// Consent revocation (0001 Revocation and returning to anonymous). Closes open
/// profile sessions at `revoked_at_ms`. `profile_id` `None` revokes every
/// profile of the site, when persistent identity is disabled for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Revocation {
    pub id: Id,
    pub site: Id,
    pub profile_id: Option<Id>,
    pub revoked_at_ms: i64,
    pub receive_time_ms: i64,
}

/// `identify` message: session properties for an identity, never a session boundary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Identify {
    pub id: Id,
    pub site: Id,
    pub event_time_ms: i64,
    pub receive_time_ms: i64,
    pub local_day: i32,
    pub daily_id: Option<Id>,
    pub profile_id: Option<Id>,
    pub properties: Vec<Property>,
}

/// One recorder fragment, kept by capture ID and chunk order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReplayFragment {
    pub id: Id,
    pub site: Id,
    pub capture_id: Id,
    pub index: u16,
    pub total: u16,
    pub is_final: bool,
    pub event_time_ms: i64,
    pub receive_time_ms: i64,
    #[serde(with = "serde_bytes_compat")]
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HeatmapInteraction {
    pub id: Id,
    pub site: Id,
    pub capture_id: Id,
    pub event_time_ms: i64,
    pub receive_time_ms: i64,
    pub path: String,
    pub viewport_width: u16,
    pub viewport_height: u16,
    pub kind: Interaction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Interaction {
    Click { x: u16, page_y: u32 },
    Scroll { percent: u8 },
}

/// Inbox format 2, before the visitor coordinates. Only `Event` and `Dimensions` differ, so
/// pending rows of the previous release decode here and convert with no coordinates.
mod format2 {
    use super::{ClientEventId, EventKind, Id, Performance, Property, Revenue};
    use serde::{Deserialize, Serialize};

    pub const FORMAT: i64 = 2;

    #[allow(clippy::large_enum_variant)]
    #[derive(Serialize, Deserialize)]
    pub enum Record {
        Event(Event),
        Identify(super::Identify),
        ReplayFragment(super::ReplayFragment),
        HeatmapInteraction(super::HeatmapInteraction),
        Revocation(super::Revocation),
    }

    #[derive(Serialize, Deserialize)]
    pub struct Event {
        pub id: Id,
        pub site: Id,
        pub kind: EventKind,
        pub event_time_ms: i64,
        pub receive_time_ms: i64,
        pub local_day: i32,
        pub daily_id: Option<Id>,
        pub profile_id: Option<Id>,
        pub client_event_id: Option<ClientEventId>,
        pub dimensions: Dimensions,
        pub performance: Option<Performance>,
        pub revenue: Option<Revenue>,
        pub properties: Vec<Property>,
    }

    #[derive(Serialize, Deserialize)]
    pub struct Dimensions {
        pub hostname: Option<String>,
        pub path: Option<String>,
        pub query: Option<String>,
        pub title: Option<String>,
        pub referrer_domain: Option<String>,
        pub referrer_path: Option<String>,
        pub utm_source: Option<String>,
        pub utm_medium: Option<String>,
        pub utm_campaign: Option<String>,
        pub utm_content: Option<String>,
        pub utm_term: Option<String>,
        pub click_id: Option<String>,
        pub country: Option<String>,
        pub region: Option<String>,
        pub city: Option<String>,
        pub browser: Option<String>,
        pub os: Option<String>,
        pub device: Option<String>,
        pub screen: Option<String>,
        pub language: Option<String>,
        pub event_name: Option<String>,
        pub tag: Option<String>,
    }

    impl From<Record> for super::Record {
        fn from(record: Record) -> Self {
            match record {
                Record::Event(e) => Self::Event(super::Event {
                    id: e.id,
                    site: e.site,
                    kind: e.kind,
                    event_time_ms: e.event_time_ms,
                    receive_time_ms: e.receive_time_ms,
                    local_day: e.local_day,
                    daily_id: e.daily_id,
                    profile_id: e.profile_id,
                    client_event_id: e.client_event_id,
                    dimensions: e.dimensions.into(),
                    performance: e.performance,
                    revenue: e.revenue,
                    properties: e.properties,
                }),
                Record::Identify(r) => Self::Identify(r),
                Record::ReplayFragment(r) => Self::ReplayFragment(r),
                Record::HeatmapInteraction(r) => Self::HeatmapInteraction(r),
                Record::Revocation(r) => Self::Revocation(r),
            }
        }
    }

    impl From<Dimensions> for super::Dimensions {
        fn from(d: Dimensions) -> Self {
            Self {
                hostname: d.hostname,
                path: d.path,
                query: d.query,
                title: d.title,
                referrer_domain: d.referrer_domain,
                referrer_path: d.referrer_path,
                utm_source: d.utm_source,
                utm_medium: d.utm_medium,
                utm_campaign: d.utm_campaign,
                utm_content: d.utm_content,
                utm_term: d.utm_term,
                click_id: d.click_id,
                country: d.country,
                region: d.region,
                city: d.city,
                latitude: None,
                longitude: None,
                browser: d.browser,
                os: d.os,
                device: d.device,
                screen: d.screen,
                language: d.language,
                event_name: d.event_name,
                tag: d.tag,
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::record::Record as Current;

        #[test]
        fn format_2_rows_decode_without_coordinates() {
            let legacy = Record::Event(Event {
                id: 1,
                site: 2,
                kind: EventKind::Pageview,
                event_time_ms: 3,
                receive_time_ms: 4,
                local_day: 5,
                daily_id: Some(6),
                profile_id: None,
                client_event_id: None,
                dimensions: Dimensions {
                    hostname: None,
                    path: Some("/".into()),
                    query: None,
                    title: None,
                    referrer_domain: None,
                    referrer_path: None,
                    utm_source: None,
                    utm_medium: None,
                    utm_campaign: None,
                    utm_content: None,
                    utm_term: None,
                    click_id: None,
                    country: Some("AT".into()),
                    region: None,
                    city: Some("Vienna".into()),
                    browser: Some("chrome".into()),
                    os: None,
                    device: None,
                    screen: None,
                    language: None,
                    event_name: None,
                    tag: None,
                },
                performance: None,
                revenue: None,
                properties: vec![],
            });
            let bytes = postcard::to_stdvec(&legacy).unwrap();
            let Current::Event(event) = Current::decode(FORMAT, &bytes).unwrap() else {
                panic!("expected an event");
            };
            assert_eq!(event.dimensions.city.as_deref(), Some("Vienna"));
            assert_eq!(event.dimensions.browser.as_deref(), Some("chrome"));
            assert_eq!(
                (event.dimensions.latitude, event.dimensions.longitude),
                (None, None)
            );
            assert!(Current::decode(1, &bytes).is_err());
        }
    }
}

/// Serializes byte payloads as one length-prefixed slice instead of a sequence of u8.
mod serde_bytes_compat {
    use serde::{Deserializer, Serializer, de::Visitor};

    pub fn serialize<S: Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_bytes(bytes)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
        struct Bytes;
        impl<'de> Visitor<'de> for Bytes {
            type Value = Vec<u8>;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("bytes")
            }
            fn visit_bytes<E>(self, v: &[u8]) -> Result<Vec<u8>, E> {
                Ok(v.to_vec())
            }
            fn visit_byte_buf<E>(self, v: Vec<u8>) -> Result<Vec<u8>, E> {
                Ok(v)
            }
        }
        deserializer.deserialize_byte_buf(Bytes)
    }
}
