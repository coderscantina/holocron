//! Typed properties and revenue from a payload's `data` object (decision 0002, Property type
//! system and Currencies). Numbers are read from their JSON text, never through a float.

use holocron_protocol::exploration::currency_exponent;
use holocron_storage::{Property, PropertyValue, Revenue};
use serde_json::value::RawValue;

use super::decimal::Decimal;

pub const MAX_PROPERTIES: usize = 50;
pub const MAX_DEPTH: usize = 5;
pub const MAX_KEY_BYTES: usize = 500;
pub const MAX_STRING_CHARS: usize = 500;
const MAX_INTEGER: i128 = i64::MAX as i128;
/// Decimal magnitude limit, `10^14`, in units of 0.0001.
const MAX_DECIMAL_UNITS: i128 = 100_000_000_000_000 * 10_000;

/// Counters a payload's properties contribute to (0002).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Notes {
    pub dropped: u64,
    pub coerced: u64,
    pub revenue_invalid: bool,
}

/// Flattens `data` into typed leaf properties with dot keys. `data` is a JSON object.
pub fn properties(data: &RawValue, notes: &mut Notes) -> Vec<Property> {
    let Some(Json::Object(members)) = parse(data.get()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    flatten(&members, "", 1, &mut out, notes);
    out
}

/// Revenue of a custom event: `data.revenue` in `data.currency` minor units. Zero is no revenue
/// and not an error; anything else unusable counts as `revenue_invalid`.
pub fn revenue(data: &RawValue, notes: &mut Notes) -> Option<Revenue> {
    let Some(Json::Object(members)) = parse(data.get()) else {
        return None;
    };
    let field = |name: &str| members.iter().find(|(key, _)| key == name).map(|(_, v)| v);
    let amount = field("revenue")?;
    let Json::Number(text) = amount else {
        notes.revenue_invalid = true;
        return None;
    };
    let amount = Decimal::parse(text)?;
    if amount.is_zero() {
        return None;
    }
    let code = match field("currency") {
        Some(Json::String(code)) => code.trim().to_ascii_uppercase(),
        _ => {
            notes.revenue_invalid = true;
            return None;
        }
    };
    let Some(exponent) = currency_exponent(&code).map(u32::from) else {
        notes.revenue_invalid = true;
        return None;
    };
    match amount.scaled(exponent).and_then(|v| i64::try_from(v).ok()) {
        Some(0) => None,
        Some(minor_units) => Some(Revenue {
            currency: code,
            minor_units,
        }),
        None => {
            notes.revenue_invalid = true;
            None
        }
    }
}

fn flatten(
    members: &[(String, Json)],
    prefix: &str,
    depth: usize,
    out: &mut Vec<Property>,
    notes: &mut Notes,
) {
    for (key, value) in members {
        let key = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}.{key}")
        };
        if let Json::Object(inner) = value {
            if depth < MAX_DEPTH {
                flatten(inner, &key, depth + 1, out, notes);
            } else {
                notes.dropped += leaves(inner);
            }
            continue;
        }
        if out.len() >= MAX_PROPERTIES || key.len() > MAX_KEY_BYTES {
            notes.dropped += 1;
            continue;
        }
        let value = match value {
            Json::Array(items) => PropertyValue::Array(
                items
                    .iter()
                    .map(|item| match item {
                        Json::Array(_) | Json::Object(_) => {
                            PropertyValue::String(truncate(&item.to_text()))
                        }
                        scalar => scalar_value(scalar, notes),
                    })
                    .collect(),
            ),
            scalar => scalar_value(scalar, notes),
        };
        out.push(Property { key, value });
    }
}

fn leaves(members: &[(String, Json)]) -> u64 {
    members
        .iter()
        .map(|(_, value)| match value {
            Json::Object(inner) => leaves(inner),
            _ => 1,
        })
        .sum()
}

fn scalar_value(value: &Json, notes: &mut Notes) -> PropertyValue {
    match value {
        Json::Null => PropertyValue::Null,
        Json::Bool(b) => PropertyValue::Bool(*b),
        Json::Number(text) => number_value(text, notes),
        Json::String(s) => match date_ms(s) {
            Some(ms) => PropertyValue::Date(ms),
            None => PropertyValue::String(truncate(s)),
        },
        Json::Array(_) | Json::Object(_) => unreachable!("only scalars reach here"),
    }
}

fn number_value(text: &str, notes: &mut Notes) -> PropertyValue {
    let Some(number) = Decimal::parse(text) else {
        return PropertyValue::String(truncate(text));
    };
    if number.is_whole() {
        if let Some(value) = number.scaled(0).filter(|v| v.abs() <= MAX_INTEGER) {
            return PropertyValue::Integer(value as i64);
        }
    } else if let Some(units) = number.scaled(4).filter(|v| v.abs() <= MAX_DECIMAL_UNITS) {
        return PropertyValue::Decimal(units as i64);
    }
    notes.coerced += 1;
    PropertyValue::String(truncate(text))
}

fn truncate(s: &str) -> String {
    match s.char_indices().nth(MAX_STRING_CHARS) {
        Some((at, _)) => s[..at].to_owned(),
        None => s.to_owned(),
    }
}

/// Upstream `DATETIME_REGEX`: `YYYY-MM-DDTHH:MM:SS(.mmm)?(Z|+HH:MM)?`. Without an offset the
/// time is read as UTC. A matching string that is not a real date stays a string.
fn date_ms(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    let digits = |range: std::ops::Range<usize>| {
        b.get(range)
            .is_some_and(|r| r.iter().all(u8::is_ascii_digit))
    };
    let shape = b.len() >= 19
        && digits(0..4)
        && b[4] == b'-'
        && digits(5..7)
        && b[7] == b'-'
        && digits(8..10)
        && b[10] == b'T'
        && digits(11..13)
        && b[13] == b':'
        && digits(14..16)
        && b[16] == b':'
        && digits(17..19);
    if !shape {
        return None;
    }
    let mut rest = &s[19..];
    if rest.starts_with('.') {
        if !digits(20..23) {
            return None;
        }
        rest = &rest[4..];
    }
    let offset_ok = rest.is_empty()
        || rest == "Z"
        || (rest.len() == 6
            && rest.starts_with('+')
            && rest.as_bytes()[1..3].iter().all(u8::is_ascii_digit)
            && rest.as_bytes()[3] == b':'
            && rest.as_bytes()[4..6].iter().all(u8::is_ascii_digit));
    if !offset_ok {
        return None;
    }
    if rest.is_empty() {
        chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f")
            .ok()
            .map(|t| t.and_utc().timestamp_millis())
    } else {
        chrono::DateTime::parse_from_rfc3339(s)
            .ok()
            .map(|t| t.timestamp_millis())
    }
}

/// JSON with number text kept verbatim. Input is already validated by serde_json.
#[derive(Debug, Clone)]
enum Json<'a> {
    Null,
    Bool(bool),
    Number(&'a str),
    String(String),
    Array(Vec<Json<'a>>),
    Object(Vec<(String, Json<'a>)>),
}

impl Json<'_> {
    fn to_text(&self) -> String {
        match self {
            Json::Null => "null".into(),
            Json::Bool(b) => b.to_string(),
            Json::Number(text) => (*text).to_owned(),
            Json::String(s) => serde_json::to_string(s).expect("strings serialize"),
            Json::Array(items) => format!(
                "[{}]",
                items
                    .iter()
                    .map(Json::to_text)
                    .collect::<Vec<_>>()
                    .join(",")
            ),
            Json::Object(members) => format!(
                "{{{}}}",
                members
                    .iter()
                    .map(|(k, v)| format!(
                        "{}:{}",
                        serde_json::to_string(k).expect("strings serialize"),
                        v.to_text()
                    ))
                    .collect::<Vec<_>>()
                    .join(",")
            ),
        }
    }
}

fn parse(text: &str) -> Option<Json<'_>> {
    let mut parser = Parser { text, at: 0 };
    let value = parser.value()?;
    parser.skip_ws();
    (parser.at == text.len()).then_some(value)
}

struct Parser<'a> {
    text: &'a str,
    at: usize,
}

impl<'a> Parser<'a> {
    fn skip_ws(&mut self) {
        let rest = &self.text[self.at..];
        self.at += rest.len() - rest.trim_start_matches([' ', '\t', '\n', '\r']).len();
    }

    fn peek(&self) -> Option<u8> {
        self.text.as_bytes().get(self.at).copied()
    }

    fn eat(&mut self, literal: &str) -> bool {
        let found = self.text[self.at..].starts_with(literal);
        if found {
            self.at += literal.len();
        }
        found
    }

    fn value(&mut self) -> Option<Json<'a>> {
        self.skip_ws();
        match self.peek()? {
            b'n' => self.eat("null").then_some(Json::Null),
            b't' => self.eat("true").then_some(Json::Bool(true)),
            b'f' => self.eat("false").then_some(Json::Bool(false)),
            b'"' => self.string().map(Json::String),
            b'[' => {
                self.at += 1;
                let mut items = Vec::new();
                self.skip_ws();
                if self.eat("]") {
                    return Some(Json::Array(items));
                }
                loop {
                    items.push(self.value()?);
                    self.skip_ws();
                    if self.eat("]") {
                        return Some(Json::Array(items));
                    }
                    if !self.eat(",") {
                        return None;
                    }
                }
            }
            b'{' => {
                self.at += 1;
                let mut members = Vec::new();
                self.skip_ws();
                if self.eat("}") {
                    return Some(Json::Object(members));
                }
                loop {
                    self.skip_ws();
                    let key = self.string()?;
                    self.skip_ws();
                    if !self.eat(":") {
                        return None;
                    }
                    members.push((key, self.value()?));
                    self.skip_ws();
                    if self.eat("}") {
                        return Some(Json::Object(members));
                    }
                    if !self.eat(",") {
                        return None;
                    }
                }
            }
            _ => {
                let start = self.at;
                let len = self.text[start..]
                    .find(|c: char| !matches!(c, '0'..='9' | '-' | '+' | '.' | 'e' | 'E'))
                    .unwrap_or(self.text.len() - start);
                self.at += len;
                (len > 0).then(|| Json::Number(&self.text[start..start + len]))
            }
        }
    }

    fn string(&mut self) -> Option<String> {
        let start = self.at;
        let bytes = self.text.as_bytes();
        let mut i = start + 1;
        while i < bytes.len() {
            match bytes[i] {
                b'\\' => i += 2,
                b'"' => {
                    self.at = i + 1;
                    return serde_json::from_str(&self.text[start..=i]).ok();
                }
                _ => i += 1,
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn props(json: &str) -> (Vec<Property>, Notes) {
        let raw: Box<RawValue> = serde_json::from_str(json).unwrap();
        let mut notes = Notes::default();
        (properties(&raw, &mut notes), notes)
    }

    fn value(json: &str) -> PropertyValue {
        props(&format!(r#"{{"k":{json}}}"#)).0.remove(0).value
    }

    #[test]
    fn numbers_are_typed_by_exact_value() {
        assert_eq!(value("1"), PropertyValue::Integer(1));
        assert_eq!(value("1.0"), PropertyValue::Integer(1));
        assert_eq!(value("1e3"), PropertyValue::Integer(1000));
        assert_eq!(value("0.12345"), PropertyValue::Decimal(1234));
        assert_eq!(value("2.00001"), PropertyValue::Decimal(20_000));
        assert_eq!(
            value("9223372036854775808"),
            PropertyValue::String("9223372036854775808".into())
        );
    }

    #[test]
    fn strings_dates_arrays_and_nesting() {
        assert_eq!(
            value(r#""2026-10-01T12:00:00Z""#),
            PropertyValue::Date(1_790_856_000_000)
        );
        assert_eq!(
            value(r#""2026-13-01T12:00:00Z""#),
            PropertyValue::String("2026-13-01T12:00:00Z".into())
        );
        assert_eq!(
            value(r#"[1, "a", [2]]"#),
            PropertyValue::Array(vec![
                PropertyValue::Integer(1),
                PropertyValue::String("a".into()),
                PropertyValue::String("[2]".into())
            ])
        );
        let (nested, _) = props(r#"{"a":{"b":{"c":true}}}"#);
        assert_eq!(nested[0].key, "a.b.c");
    }

    #[test]
    fn limits_drop_properties_and_count_them() {
        let many: Vec<String> = (0..52).map(|i| format!(r#""k{i}":{i}"#)).collect();
        let (kept, notes) = props(&format!("{{{}}}", many.join(",")));
        assert_eq!((kept.len(), notes.dropped), (50, 2));
        let (deep, notes) = props(r#"{"a":{"b":{"c":{"d":{"e":{"f":1}}}}}}"#);
        assert_eq!((deep.len(), notes.dropped), (0, 1));
    }

    #[test]
    fn revenue_uses_currency_minor_units() {
        let revenue_of = |json: &str| {
            let raw: Box<RawValue> = serde_json::from_str(json).unwrap();
            let mut notes = Notes::default();
            (revenue(&raw, &mut notes), notes.revenue_invalid)
        };
        let eur = |minor_units| {
            Some(Revenue {
                currency: "EUR".into(),
                minor_units,
            })
        };
        assert_eq!(
            revenue_of(r#"{"revenue":19.995,"currency":"eur"}"#),
            (eur(2000), false)
        );
        assert_eq!(
            revenue_of(r#"{"revenue":-5,"currency":"EUR"}"#),
            (eur(-500), false)
        );
        assert_eq!(
            revenue_of(r#"{"revenue":0.004,"currency":"EUR"}"#),
            (None, false)
        );
        assert_eq!(
            revenue_of(r#"{"revenue":12,"currency":"XYZ"}"#),
            (None, true)
        );
        assert_eq!(
            revenue_of(r#"{"revenue":"12","currency":"EUR"}"#),
            (None, true)
        );
        assert_eq!(
            revenue_of(r#"{"revenue":1500,"currency":"JPY"}"#),
            (
                Some(Revenue {
                    currency: "JPY".into(),
                    minor_units: 1500
                }),
                false
            )
        );
    }
}
