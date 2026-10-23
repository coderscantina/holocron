//! rrweb events as the unchanged recorder sends them, and how they come back for playback.
//!
//! Everything here is untrusted browser output. Ingest reads only the few fields it indexes
//! and keeps the events as sent. Playback restores events the recorder split into fragments,
//! strips active content (scripts, event handlers, script URLs, framed documents) and lists an
//! accessible timeline. The replay frame's sandbox and content security policy are the other
//! layers (decision 0011).

use std::collections::HashMap;

use holocron_protocol::operations::recordings::{TimelineEntry, TimelineKind};
use serde::Deserialize;
use serde_json::value::RawValue;
use serde_json::{Map, Value};

use crate::collect::text::truncate;

pub const FRAGMENT_TYPE: &str = "umami:rrweb-event-fragment";
/// Deepest JSON nesting a part may have. Playback parses whole events, so it runs on a thread
/// with a stack sized for this ([`PLAYBACK_STACK_BYTES`]).
pub const MAX_DEPTH: usize = 2048;
pub const PLAYBACK_STACK_BYTES: usize = 64 * 1024 * 1024;
/// Timeline entries one playback page lists at most.
const MAX_TIMELINE: usize = 1000;
/// Inputs and scrolls closer than this to the previous one share a timeline entry.
const COLLAPSE_MS: i64 = 1000;
const MAX_PATH_CHARS: usize = 500;
const MAX_FRAGMENTS: u32 = 100_000;

/// rrweb `EventType` and `IncrementalSource` numbers.
mod rrweb {
    pub const INCREMENTAL: u64 = 3;
    pub const META: u64 = 4;
    pub const CUSTOM: u64 = 5;
    pub const MAX_TYPE: u64 = 6;
    pub const MOUSE_INTERACTION: u64 = 2;
    pub const SCROLL: u64 = 3;
    pub const RESIZE: u64 = 4;
    pub const INPUT: u64 = 5;
    pub const CLICK: u64 = 2;
}

/// Not an event the supported recorder sends.
#[derive(Debug, PartialEq, Eq)]
pub struct Invalid;

/// What the server indexes of one replay part.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Summary {
    /// Events as upstream counts them: a split event counts once, at its first fragment.
    pub events: i64,
    /// Earliest and latest event time by the browser's clock.
    pub first_ms: Option<i64>,
    pub last_ms: Option<i64>,
    /// Paths of page loads and URL changes, in order.
    pub pages: Vec<String>,
}

#[derive(Deserialize)]
struct Head<'a> {
    #[serde(rename = "type")]
    kind: Value,
    timestamp: Option<f64>,
    #[serde(borrow)]
    data: Option<&'a RawValue>,
}

#[derive(Deserialize)]
struct Meta {
    href: Option<String>,
}

#[derive(Deserialize)]
struct Custom<'a> {
    tag: Option<String>,
    #[serde(borrow)]
    payload: Option<&'a RawValue>,
}

#[derive(Deserialize)]
struct UrlChange {
    url: Option<String>,
}

#[derive(Deserialize)]
struct FragmentData<'a> {
    id: String,
    index: u32,
    total: u32,
    #[serde(borrow)]
    value: &'a RawValue,
}

/// Validates the events of one part and reads what the server indexes. Event bodies stay
/// unparsed: serde_json skips raw values without recursion, so nesting cannot exhaust the stack.
pub fn summarize(events: &[&RawValue]) -> Result<Summary, Invalid> {
    let mut summary = Summary::default();
    for event in events {
        let head: Head = serde_json::from_str(event.get()).map_err(|_| Invalid)?;
        match &head.kind {
            Value::Number(n) => match n.as_u64() {
                Some(rrweb::META) => {
                    if let Some(meta) = parse::<Meta>(head.data)
                        && let Some(href) = meta.href
                    {
                        summary.pages.push(path_of(&href));
                    }
                    summary.events += 1;
                }
                Some(rrweb::CUSTOM) => {
                    if let Some(custom) = parse::<Custom>(head.data)
                        && custom.tag.as_deref() == Some("url-change")
                        && let Some(url) = parse::<UrlChange>(custom.payload).and_then(|c| c.url)
                    {
                        summary.pages.push(path_of(&url));
                    }
                    summary.events += 1;
                }
                Some(kind) if kind <= rrweb::MAX_TYPE => summary.events += 1,
                _ => return Err(Invalid),
            },
            Value::String(kind) if kind == FRAGMENT_TYPE => {
                let data = head.data.ok_or(Invalid)?;
                let fragment: FragmentData =
                    serde_json::from_str(data.get()).map_err(|_| Invalid)?;
                if fragment.index >= fragment.total
                    || fragment.total > MAX_FRAGMENTS
                    || !fragment.value.get().starts_with('"')
                    || fragment.id.is_empty()
                {
                    return Err(Invalid);
                }
                if fragment.index == 0 {
                    summary.events += 1;
                }
            }
            _ => return Err(Invalid),
        }
        if let Some(ms) = head.timestamp.filter(|t| t.is_finite()).map(|t| t as i64) {
            summary.first_ms = Some(summary.first_ms.map_or(ms, |m| m.min(ms)));
            summary.last_ms = Some(summary.last_ms.map_or(ms, |m| m.max(ms)));
        }
    }
    summary.pages.dedup();
    Ok(summary)
}

fn parse<'a, T: Deserialize<'a>>(raw: Option<&'a RawValue>) -> Option<T> {
    serde_json::from_str(raw?.get()).ok()
}

/// The path of a page URL, without query or fragment, which can carry personal data.
pub fn path_of(url: &str) -> String {
    let path = match url::Url::parse(url) {
        Ok(parsed) => parsed.path().to_owned(),
        Err(_) if url.starts_with('/') => url.split(['?', '#']).next().unwrap_or("/").to_owned(),
        Err(_) => "/".to_owned(),
    };
    truncate(&path, MAX_PATH_CHARS)
}

/// Deepest nesting of arrays and objects in a JSON text, without parsing it.
pub fn depth(json: &[u8]) -> usize {
    let (mut depth, mut max, mut in_string, mut escaped) = (0usize, 0usize, false, false);
    for &byte in json {
        if in_string {
            match byte {
                _ if escaped => escaped = false,
                b'\\' => escaped = true,
                b'"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match byte {
            b'"' => in_string = true,
            b'[' | b'{' => {
                depth += 1;
                max = max.max(depth);
            }
            b']' | b'}' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    max
}

/// The events of one playback page.
pub struct Playback {
    /// A JSON array of sanitized events.
    pub events: String,
    pub timeline: Vec<TimelineEntry>,
    pub gaps: i64,
}

struct Pending {
    total: u32,
    values: Vec<Option<String>>,
    received: u32,
}

/// Turns stored parts, in recording order, into playable events. Must run on a thread with
/// [`PLAYBACK_STACK_BYTES`] of stack: events are parsed and sanitized as trees.
#[derive(Default)]
pub struct Assembler {
    events: String,
    count: usize,
    pending: HashMap<String, Pending>,
    timeline: Timeline,
    gaps: i64,
}

impl Assembler {
    /// Adds one stored part, a JSON array of events. A part that does not parse is skipped
    /// and counted as a gap; ingest validated it, so that means the store was damaged.
    pub fn push(&mut self, part: &[u8]) {
        match parse_unbounded::<Vec<Value>>(part) {
            Some(events) => events.into_iter().for_each(|event| self.event(event)),
            None => self.gaps += 1,
        }
    }

    /// Counts a stored part that could not be decoded.
    pub fn damaged(&mut self) {
        self.gaps += 1;
    }

    /// Split events still waiting for fragments.
    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    /// Bytes of event JSON so far.
    pub fn bytes(&self) -> usize {
        self.events.len()
    }

    /// Ends the page. Split events still missing a fragment are dropped and counted.
    pub fn finish(self) -> Playback {
        Playback {
            events: format!("[{}]", self.events),
            timeline: self.timeline.entries,
            gaps: self.gaps + self.pending.len() as i64,
        }
    }

    fn event(&mut self, event: Value) {
        if event["type"].as_str() == Some(FRAGMENT_TYPE) {
            return self.fragment(&event["data"]);
        }
        self.emit(event);
    }

    /// Like upstream's `restoreReplayEventFragments`: fragments collect by ID in any order,
    /// a repeated index is ignored, and the event takes its place at its last fragment.
    fn fragment(&mut self, data: &Value) {
        let (Some(id), Some(index), Some(total), Some(value)) = (
            data["id"].as_str(),
            data["index"].as_u64().and_then(|i| u32::try_from(i).ok()),
            data["total"].as_u64().and_then(|t| u32::try_from(t).ok()),
            data["value"].as_str(),
        ) else {
            self.gaps += 1;
            return;
        };
        if index >= total || total > MAX_FRAGMENTS {
            self.gaps += 1;
            return;
        }
        let pending = self
            .pending
            .entry(id.to_owned())
            .or_insert_with(|| Pending {
                total,
                values: vec![None; total as usize],
                received: 0,
            });
        if pending.total != total {
            // Another event reused the ID; the earlier one cannot complete any more.
            self.gaps += 1;
            *pending = Pending {
                total,
                values: vec![None; total as usize],
                received: 0,
            };
        }
        let slot = &mut pending.values[index as usize];
        if slot.is_none() {
            *slot = Some(value.to_owned());
            pending.received += 1;
        }
        if pending.received < pending.total {
            return;
        }
        let whole: String = self
            .pending
            .remove(id)
            .expect("pending group")
            .values
            .into_iter()
            .flatten()
            .collect();
        match parse_unbounded::<Value>(whole.as_bytes()) {
            Some(event @ Value::Object(_)) => self.emit(event),
            _ => self.gaps += 1,
        }
    }

    fn emit(&mut self, mut event: Value) {
        if !event.is_object() {
            return;
        }
        sanitize(&mut event);
        self.timeline.observe(&event);
        if self.count > 0 {
            self.events.push(',');
        }
        self.events.push_str(&event.to_string());
        self.count += 1;
    }
}

/// Parses without serde_json's 128-level limit. Callers bound depth with [`MAX_DEPTH`].
fn parse_unbounded<T: serde::de::DeserializeOwned>(json: &[u8]) -> Option<T> {
    if depth(json) > MAX_DEPTH {
        return None;
    }
    let mut deserializer = serde_json::Deserializer::from_slice(json);
    deserializer.disable_recursion_limit();
    let value = T::deserialize(&mut deserializer).ok()?;
    deserializer.end().ok()?;
    Some(value)
}

/// Attributes that run code, navigate, submit, or load a document, on any element.
const ACTIVE_ATTRIBUTES: [&str; 6] = [
    "srcdoc",
    "ping",
    "formaction",
    "action",
    "http-equiv",
    "xlink:href",
];
/// Elements whose resource attributes are dropped: they would load or frame other content.
const FRAMING_ELEMENTS: [&str; 6] = ["iframe", "frame", "object", "embed", "portal", "base"];

/// Removes active content from a serialized DOM, in snapshots and mutations alike: scripts
/// become empty `noscript` elements, event handler and script-URL attributes go, framed and
/// embedded documents lose their source, and links lose their target.
pub fn sanitize(value: &mut Value) {
    match value {
        Value::Array(items) => items.iter_mut().for_each(sanitize),
        Value::Object(map) => {
            if let Some(Value::Object(attributes)) = map.get_mut("attributes") {
                clean_attributes(attributes);
            }
            if let Some(tag) = map.get("tagName").and_then(Value::as_str) {
                element(&tag.to_ascii_lowercase(), map);
            }
            map.values_mut().for_each(sanitize);
        }
        _ => {}
    }
}

fn element(tag: &str, node: &mut Map<String, Value>) {
    let Some(Value::Object(attributes)) = node.get_mut("attributes") else {
        return;
    };
    let mut remove = |names: &[&str]| {
        attributes.retain(|name, _| !names.contains(&name.to_ascii_lowercase().as_str()));
    };
    match tag {
        "script" => {
            node.insert("tagName".into(), Value::String("noscript".into()));
            node.insert("attributes".into(), Value::Object(Map::new()));
            node.insert("childNodes".into(), Value::Array(Vec::new()));
        }
        "link" => remove(&["href", "imagesrcset"]),
        // SVG animations can set an event handler or a link target after load.
        "set" | "animate" => remove(&["attributename", "to", "values", "from", "by"]),
        _ if FRAMING_ELEMENTS.contains(&tag) => remove(&["src", "data", "href", "codebase"]),
        _ => {}
    }
}

fn clean_attributes(attributes: &mut Map<String, Value>) {
    attributes.retain(|name, value| {
        let name = name.to_ascii_lowercase();
        if name.starts_with("on") || ACTIVE_ATTRIBUTES.contains(&name.as_str()) {
            return false;
        }
        !value.as_str().is_some_and(is_script_url)
    });
}

/// `javascript:`, `vbscript:` and HTML `data:` URLs, after the whitespace and control
/// characters browsers ignore.
fn is_script_url(value: &str) -> bool {
    let compact: String = value
        .chars()
        .filter(|c| !c.is_whitespace() && !c.is_control())
        .take(16)
        .collect::<String>()
        .to_ascii_lowercase();
    compact.starts_with("javascript:")
        || compact.starts_with("vbscript:")
        || compact.starts_with("data:text/html")
}

#[derive(Default)]
struct Timeline {
    entries: Vec<TimelineEntry>,
}

impl Timeline {
    fn observe(&mut self, event: &Value) {
        if self.entries.len() >= MAX_TIMELINE {
            return;
        }
        let Some(at) = event["timestamp"]
            .as_f64()
            .filter(|t| t.is_finite())
            .map(|t| t as i64)
        else {
            return;
        };
        let data = &event["data"];
        let (kind, path) = match event["type"].as_u64() {
            Some(rrweb::META) => match data["href"].as_str() {
                Some(href) => (TimelineKind::Page, Some(path_of(href))),
                None => return,
            },
            Some(rrweb::CUSTOM) if data["tag"] == "url-change" => {
                match data["payload"]["url"].as_str() {
                    Some(url) => (TimelineKind::Page, Some(path_of(url))),
                    None => return,
                }
            }
            Some(rrweb::INCREMENTAL) => match data["source"].as_u64() {
                Some(rrweb::MOUSE_INTERACTION) if data["type"].as_u64() == Some(rrweb::CLICK) => {
                    (TimelineKind::Click, None)
                }
                Some(rrweb::INPUT) => (TimelineKind::Input, None),
                Some(rrweb::SCROLL) => (TimelineKind::Scroll, None),
                Some(rrweb::RESIZE) => (TimelineKind::Resize, None),
                _ => return,
            },
            _ => return,
        };
        let collapses = matches!(kind, TimelineKind::Input | TimelineKind::Scroll);
        if collapses
            && let Some(last) = self.entries.last_mut()
            && last.kind == kind
            && at - last.at < COLLAPSE_MS
        {
            last.at = at;
            return;
        }
        self.entries.push(TimelineEntry { at, kind, path });
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn raw(values: &[Value]) -> Vec<Box<RawValue>> {
        values
            .iter()
            .map(|v| RawValue::from_string(v.to_string()).unwrap())
            .collect()
    }

    fn summary(values: &[Value]) -> Result<Summary, Invalid> {
        let owned = raw(values);
        let borrowed: Vec<&RawValue> = owned.iter().map(AsRef::as_ref).collect();
        summarize(&borrowed)
    }

    fn fragments(event: &Value, parts: usize) -> Vec<Value> {
        let text = event.to_string();
        let size = text.len().div_ceil(parts);
        text.as_bytes()
            .chunks(size)
            .enumerate()
            .map(|(index, piece)| {
                json!({ "type": FRAGMENT_TYPE, "timestamp": 1, "data": {
                    "id": "f1", "index": index, "total": parts, "value": std::str::from_utf8(piece).unwrap() } })
            })
            .collect()
    }

    #[test]
    fn summary_counts_split_events_once_and_reads_pages_without_queries() {
        let snapshot = json!({ "type": 2, "timestamp": 1_000, "data": { "node": { "type": 0, "childNodes": [] } } });
        let mut events = vec![
            json!({ "type": 4, "timestamp": 900, "data": { "href": "https://shop.test/cart?email=a@b.test#x", "width": 1, "height": 1 } }),
            json!({ "type": 5, "timestamp": 2_000, "data": { "tag": "url-change", "payload": { "url": "https://shop.test/pay" } } }),
        ];
        events.extend(fragments(&snapshot, 3));
        let s = summary(&events).unwrap();
        assert_eq!(s.events, 3);
        assert_eq!((s.first_ms, s.last_ms), (Some(1), Some(2_000)));
        assert_eq!(s.pages, ["/cart", "/pay"]);
    }

    #[test]
    fn summary_refuses_what_the_recorder_never_sends() {
        assert_eq!(summary(&[json!(7)]), Err(Invalid));
        assert_eq!(summary(&[json!({ "type": 9 })]), Err(Invalid));
        assert_eq!(summary(&[json!({ "type": "other" })]), Err(Invalid));
        let bad_fragment = json!({ "type": FRAGMENT_TYPE, "data": { "id": "a", "index": 2, "total": 2, "value": "x" } });
        assert_eq!(summary(&[bad_fragment]), Err(Invalid));
    }

    #[test]
    fn fragments_restore_in_any_order_and_missing_ones_are_gaps() {
        let snapshot = json!({ "type": 2, "timestamp": 5, "data": { "node": { "type": 0, "childNodes": [] } } });
        let mut parts = fragments(&snapshot, 3);
        parts.swap(0, 2);
        parts.insert(2, parts[1].clone());
        let mut assembler = Assembler::default();
        assembler.push(json!(parts).to_string().as_bytes());
        assert!(!assembler.has_pending());
        let playback = assembler.finish();
        assert_eq!(
            serde_json::from_str::<Value>(&playback.events).unwrap(),
            json!([snapshot])
        );
        assert_eq!(playback.gaps, 0);

        let mut assembler = Assembler::default();
        assembler.push(json!(fragments(&snapshot, 3)[..2]).to_string().as_bytes());
        assert!(assembler.has_pending());
        let playback = assembler.finish();
        assert_eq!((playback.events.as_str(), playback.gaps), ("[]", 1));
    }

    #[test]
    fn hostile_fixtures_lose_every_active_part() {
        let dir =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/replay-hostile");
        let mut cases = 0;
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_none_or(|e| e != "json") {
                continue;
            }
            let case: Value =
                serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            let mut assembler = Assembler::default();
            assembler.push(case["events"].to_string().as_bytes());
            let played = assembler.finish().events.to_ascii_lowercase();
            for needle in case["must_not_contain"].as_array().unwrap() {
                let needle = needle.as_str().unwrap().to_ascii_lowercase();
                assert!(
                    !played.contains(&needle),
                    "{}: {needle} survived",
                    path.display()
                );
            }
            cases += 1;
        }
        assert!(cases >= 8, "{cases} hostile cases");
    }

    #[test]
    fn nesting_beyond_the_limit_is_refused_without_recursion() {
        let deep = format!(
            "[{}{}]",
            "[".repeat(MAX_DEPTH + 10),
            "]".repeat(MAX_DEPTH + 10)
        );
        assert_eq!(depth(deep.as_bytes()), MAX_DEPTH + 11);
        assert_eq!(depth(br#"["[[[", {"a": "]"}]"#), 2);
        let mut assembler = Assembler::default();
        assembler.push(deep.as_bytes());
        assert_eq!(assembler.finish().gaps, 1);
    }

    #[test]
    fn timeline_lists_pages_and_interactions_and_collapses_bursts() {
        let events = json!([
            { "type": 4, "timestamp": 0, "data": { "href": "https://shop.test/a?q=1" } },
            { "type": 3, "timestamp": 100, "data": { "source": 2, "type": 2, "id": 3, "x": 1, "y": 1 } },
            { "type": 3, "timestamp": 200, "data": { "source": 5, "id": 4, "text": "*****" } },
            { "type": 3, "timestamp": 300, "data": { "source": 5, "id": 4, "text": "******" } },
            { "type": 3, "timestamp": 2000, "data": { "source": 3, "id": 1, "x": 0, "y": 10 } },
            { "type": 3, "timestamp": 2100, "data": { "source": 1, "positions": [] } },
            { "type": 5, "timestamp": 3000, "data": { "tag": "url-change", "payload": { "url": "https://shop.test/b" } } }
        ]);
        let mut assembler = Assembler::default();
        assembler.push(events.to_string().as_bytes());
        let kinds: Vec<_> = assembler
            .finish()
            .timeline
            .into_iter()
            .map(|e| (e.kind, e.path))
            .collect();
        assert_eq!(
            kinds,
            [
                (TimelineKind::Page, Some("/a".to_owned())),
                (TimelineKind::Click, None),
                (TimelineKind::Input, None),
                (TimelineKind::Scroll, None),
                (TimelineKind::Page, Some("/b".to_owned())),
            ]
        );
    }
}
