//! Bound SQL fragments compiled from the typed model: field columns, predicates and the channel
//! classification. Every user value is a bound parameter; only identifiers and constants of
//! this module reach the SQL text.

use holocron_protocol::exploration::{DimensionField, Field, PropertyScope};
use holocron_storage::duckdb::types::Value;

use super::glob::Glob;
use super::validate::{Cond, Order, Pred, Test};

/// SQL text with its positional parameters, in order.
#[derive(Debug, Default)]
pub struct Sql {
    pub text: String,
    pub params: Vec<Value>,
}

impl Sql {
    pub fn push(&mut self, text: &str) -> &mut Self {
        self.text.push_str(text);
        self
    }

    pub fn bind(&mut self, value: impl Into<Value>) -> &mut Self {
        self.text.push('?');
        self.params.push(value.into());
        self
    }
}

/// Columns of a property value, in the order a cell is decoded from.
pub const PROPERTY_PARTS: [&str; 7] = ["t", "s", "i", "d", "b", "dt", "a"];

/// The columns one field occupies in the materialized rows.
pub fn columns(index: usize, field: &Field) -> Vec<String> {
    match field {
        Field::Dimension(_) => vec![format!("f{index}")],
        Field::Property(_) => PROPERTY_PARTS
            .iter()
            .map(|part| format!("f{index}_{part}"))
            .collect(),
    }
}

/// Where a built-in field's value comes from (0004 Field scope).
pub enum Source {
    /// The event's own column.
    Event(&'static str),
    /// The session's first eligible event.
    SessionFirst(&'static str),
    /// The session's touch: its first pageview, else its first eligible event.
    SessionTouch(&'static str),
    /// Computed from the session touch and the site's hostnames.
    Channel,
    /// Wall clock of the event time in the state's timezone.
    Weekday,
    Hour,
    /// The metric of a performance observation (0003 Percentiles).
    PerfMetric,
}

/// Performance metrics and their event columns. Each reported metric of a performance event
/// is one observation; `cls` is stored in units of 0.0001, the others in milliseconds.
pub const PERF_METRICS: [&str; 5] = ["cls", "fcp", "inp", "lcp", "ttfb"];

pub fn source(field: DimensionField) -> Source {
    use DimensionField::*;
    match field {
        Path => Source::Event("path"),
        Hostname => Source::Event("hostname"),
        Title => Source::Event("title"),
        Query => Source::Event("query"),
        EventName => Source::Event("event_name"),
        Tag => Source::Event("tag"),
        Currency => Source::Event("currency"),
        PerfMetric => Source::PerfMetric,
        ReferrerDomain => Source::SessionTouch("referrer_domain"),
        ReferrerPath => Source::SessionTouch("referrer_path"),
        UtmSource => Source::SessionTouch("utm_source"),
        UtmMedium => Source::SessionTouch("utm_medium"),
        UtmCampaign => Source::SessionTouch("utm_campaign"),
        UtmContent => Source::SessionTouch("utm_content"),
        UtmTerm => Source::SessionTouch("utm_term"),
        ClickId => Source::SessionTouch("click_id"),
        Channel => Source::Channel,
        Country => Source::SessionFirst("country"),
        Region => Source::SessionFirst("region"),
        City => Source::SessionFirst("city"),
        Browser => Source::SessionFirst("browser"),
        Os => Source::SessionFirst("os"),
        Device => Source::SessionFirst("device"),
        Language => Source::SessionFirst("language"),
        Screen => Source::SessionFirst("screen"),
        Weekday => Source::Weekday,
        Hour => Source::Hour,
    }
}

/// Session-scoped values correction stores on `sessions`: the event column, whether it comes
/// from the touch (else the first event), and the stored column.
pub const SESSION_FIELDS: [(&str, bool, &str); 17] = [
    ("country", false, "country"),
    ("region", false, "region"),
    ("city", false, "city"),
    ("browser", false, "browser"),
    ("os", false, "os"),
    ("device", false, "device"),
    ("language", false, "language"),
    ("screen", false, "screen"),
    ("referrer_domain", true, "referrer_domain"),
    ("referrer_path", true, "referrer_path"),
    ("utm_source", true, "utm_source"),
    ("utm_medium", true, "utm_medium"),
    ("utm_campaign", true, "utm_campaign"),
    ("utm_content", true, "utm_content"),
    ("utm_term", true, "utm_term"),
    ("click_id", true, "click_id"),
    // The touch's page query, for `channel`; not the event's own `query` field.
    ("query", true, "touch_query"),
];

/// Correction SQL selecting every [`SESSION_FIELDS`] value from the session's first event
/// joined as `f` and its touch event joined as `t`.
pub(crate) fn session_fields_sql() -> String {
    SESSION_FIELDS
        .iter()
        .map(|(column, touch, stored)| {
            let event = if *touch { "t" } else { "f" };
            format!("{event}.{column} AS {stored}")
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Whether a field needs the event's session.
pub fn needs_session(field: &Field) -> bool {
    match field {
        Field::Dimension(d) => matches!(
            source(*d),
            Source::SessionFirst(_) | Source::SessionTouch(_) | Source::Channel
        ),
        Field::Property(p) => p.scope == PropertyScope::Session,
    }
}

/// The filter as a boolean SQL expression over the materialized field columns.
pub fn predicate(pred: &Pred, fields: &[Field], sql: &mut Sql) {
    match pred {
        Pred::All(items) | Pred::Any(items) => {
            let joiner = if matches!(pred, Pred::All(_)) {
                " AND "
            } else {
                " OR "
            };
            sql.push("(");
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    sql.push(joiner);
                }
                predicate(item, fields, sql);
            }
            sql.push(")");
        }
        Pred::Cond(cond) => condition(cond, fields, sql),
    }
}

fn condition(cond: &Cond, fields: &[Field], sql: &mut Sql) {
    let index = fields
        .iter()
        .position(|f| *f == cond.field)
        .expect("filter fields are materialized");
    // A negative operator matches every row the positive one does not, missing values included.
    sql.push(if cond.negate { "NOT coalesce((" } else { "(" });
    match &cond.field {
        Field::Dimension(_) => scalar_test(&format!("f{index}"), &cond.test, sql),
        Field::Property(_) => property_test(&format!("f{index}_"), &cond.test, sql),
    }
    sql.push(if cond.negate { "), false)" } else { ")" });
}

fn op(order: Order) -> &'static str {
    match order {
        Order::Eq => " = ",
        Order::Gt => " > ",
        Order::Gte => " >= ",
        Order::Lt => " < ",
        Order::Lte => " <= ",
    }
}

fn list(items: &[String], sql: &mut Sql) {
    sql.push("(");
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            sql.push(", ");
        }
        sql.bind(item.clone());
    }
    sql.push(")");
}

/// `starts_with` on the literal prefix lets most rows skip the regular expression.
fn glob_test(c: &str, glob: &Glob, sql: &mut Sql) {
    if !glob.prefix.is_empty() {
        sql.push("starts_with(")
            .push(c)
            .push(", ")
            .bind(glob.prefix.clone())
            .push(") AND ");
    }
    sql.push("regexp_full_match(")
        .push(c)
        .push(", ")
        .bind(glob.regex.clone())
        .push(")");
}

fn scalar_test(c: &str, test: &Test, sql: &mut Sql) {
    match test {
        Test::StrEq(v) => sql.push(c).push(" = ").bind(v.clone()),
        Test::StrContains(v) => sql
            .push("contains(")
            .push(c)
            .push(", ")
            .bind(v.clone())
            .push(")"),
        Test::StrStartsWith(v) => sql
            .push("starts_with(")
            .push(c)
            .push(", ")
            .bind(v.clone())
            .push(")"),
        Test::StrIn(items) => {
            sql.push(c).push(" IN ");
            list(items, sql);
            sql
        }
        Test::StrMatches(glob) => {
            glob_test(c, glob, sql);
            sql
        }
        Test::Int(order, n) => sql.push(c).push(op(*order)).bind(*n),
        Test::IntBetween(a, b) => sql
            .push(c)
            .push(" BETWEEN ")
            .bind(*a)
            .push(" AND ")
            .bind(*b),
        Test::IsSet => sql.push(c).push(" IS NOT NULL"),
        other => unreachable!("validation keeps {other:?} off built-in fields"),
    };
}

/// Typed property rows (0002): string tests match string rows, numeric tests integer and
/// decimal rows, date tests date rows; arrays match on element equality.
pub fn property_test(p: &str, test: &Test, sql: &mut Sql) {
    let numeric = format!("(CASE WHEN {p}t = 2 THEN {p}i::HUGEINT * 10000 ELSE {p}d::HUGEINT END)");
    match test {
        Test::StrEq(v) => sql.push(&format!("{p}t = 5 AND {p}s = ")).bind(v.clone()),
        Test::StrStartsWith(v) => sql
            .push(&format!("{p}t = 5 AND starts_with({p}s, "))
            .bind(v.clone())
            .push(")"),
        Test::StrIn(items) => {
            sql.push(&format!("{p}t = 5 AND {p}s IN "));
            list(items, sql);
            sql
        }
        Test::StrMatches(glob) => {
            sql.push(&format!("{p}t = 5 AND "));
            glob_test(&format!("{p}s"), glob, sql);
            sql
        }
        Test::Contains { text, element } => {
            if let Some(text) = text {
                sql.push(&format!("({p}t = 5 AND contains({p}s, "))
                    .bind(text.clone())
                    .push(")) OR ");
            }
            // Arrays are stored as compact JSON, so `,"a",` inside `,<elements>,` is one element.
            sql.push(&format!(
                "({p}t = 6 AND contains(',' || substr({p}a, 2, length({p}a) - 2) || ',', "
            ))
            .bind(format!(",{element},"))
            .push("))")
        }
        Test::Num(order, n) => sql
            .push(&format!("{p}t IN (2, 3) AND {numeric}"))
            .push(op(*order))
            .bind(Value::HugeInt(*n)),
        Test::NumBetween(a, b) => sql
            .push(&format!("{p}t IN (2, 3) AND {numeric} BETWEEN "))
            .bind(Value::HugeInt(*a))
            .push(" AND ")
            .bind(Value::HugeInt(*b)),
        Test::Date(order, ms) => sql
            .push(&format!("{p}t = 4 AND {p}dt"))
            .push(op(*order))
            .bind(*ms),
        Test::DateBetween(a, b) => sql
            .push(&format!("{p}t = 4 AND {p}dt BETWEEN "))
            .bind(*a)
            .push(" AND ")
            .bind(*b),
        Test::Bool(v) => sql.push(&format!("{p}t = 1 AND {p}b = ")).bind(*v),
        Test::IsSet => sql.push(&format!("{p}t IS NOT NULL")),
        Test::IsNull => sql.push(&format!("{p}t = 0")),
        Test::StrContains(_) | Test::Int(..) | Test::IntBetween(..) => {
            unreachable!("validation keeps integer and text tests off properties")
        }
    };
}

/// Channel lists copied verbatim from Umami 3.4.0 `src/lib/constants.ts` (0003 Channels,
/// `fixtures/gaps/channel-classification.json`). Changing a list bumps the metric version.
const PAID_AD_PARAMS: &[&str] = &[
    "ad_id=",
    "aid=",
    "dclid=",
    "epik=",
    "gclid=",
    "li_fat_id=",
    "msclkid=",
    "ob_click_id=",
    "pc_id=",
    "rdt_cid=",
    "scid=",
    "ttclid=",
    "twclid=",
    "utm_medium=cpc",
    "utm_medium=paid",
    "utm_medium=paid_social",
    "utm_source=google",
];
const LLM_DOMAINS: &[&str] = &[
    "chatgpt.com",
    "claude.ai",
    "copilot.microsoft.com",
    "gemini.google.com",
    "meta.ai",
    "perplexity.ai",
];
const SEARCH_DOMAINS: &[&str] = &[
    "baidu.com",
    "bing.com",
    "duckduckgo.com",
    "ecosia.org",
    "google.",
    "msn.com",
    "search.brave.com",
    "yandex.",
];
const SOCIAL_DOMAINS: &[&str] = &[
    "bsky.app",
    "facebook.com",
    "fb.com",
    "ig.com",
    "instagram.com",
    "linkedin.",
    "news.ycombinator.com",
    "pinterest.",
    "reddit.",
    "snapchat.",
    "t.co",
    "threads.net",
    "tiktok.",
    "twitter.com",
    "x.com",
];
const EMAIL_DOMAINS: &[&str] = &[
    "gmail.",
    "hotmail.",
    "mail.yahoo.",
    "outlook.",
    "proton.me",
    "protonmail.",
];
const SHOPPING_DOMAINS: &[&str] = &[
    "alibaba.com",
    "aliexpress.com",
    "amazon.",
    "bestbuy.com",
    "ebay.com",
    "etsy.com",
    "newegg.com",
    "target.com",
    "walmart.com",
];
const VIDEO_DOMAINS: &[&str] = &["twitch.", "youtube."];
const PAID_MEDIUMS: &[&str] = &["cp", "ppc", "retargeting", "paid"];

/// `channel` from lower-cased touch columns `q` (page query), `r` (referrer domain), `m`
/// (utm_medium) and `s` (utm_source); `hostnames` are the site's hostnames.
pub fn channel(q: &str, r: &str, m: &str, s: &str, hostnames: &[String], sql: &mut Sql) {
    let any = |column: &str, items: &[&str]| {
        let parts: Vec<String> = items
            .iter()
            .map(|item| format!("contains(coalesce({column}, ''), '{item}')"))
            .collect();
        format!("({})", parts.join(" OR "))
    };
    let paid = any(m, PAID_MEDIUMS);
    let split = |paid_name: &str, organic: &str| {
        format!("CASE WHEN {paid} THEN '{paid_name}' ELSE '{organic}' END")
    };
    sql.push(&format!(
        "CASE WHEN {} THEN 'paidAds'
              WHEN {} THEN 'referral'
              WHEN {} THEN 'affiliate'
              WHEN {} OR {} THEN 'sms'
              WHEN {} THEN 'llm'
              WHEN {} OR {} THEN {}
              WHEN {} THEN {}
              WHEN {} OR {} THEN 'email'
              WHEN {} OR {} THEN {}
              WHEN {} OR {} THEN {}
              WHEN {r} IS NOT NULL AND regexp_replace({r}, '^www\\.', '') NOT IN (",
        any(q, PAID_AD_PARAMS),
        any(m, &["referral", "app", "link"]),
        any(m, &["affiliate"]),
        any(m, &["sms"]),
        any(s, &["sms"]),
        any(r, LLM_DOMAINS),
        any(r, SEARCH_DOMAINS),
        any(m, &["organic"]),
        split("paidSearch", "organicSearch"),
        any(r, SOCIAL_DOMAINS),
        split("paidSocial", "organicSocial"),
        any(r, EMAIL_DOMAINS),
        any(m, &["mail"]),
        any(r, SHOPPING_DOMAINS),
        any(m, &["shop"]),
        split("paidShopping", "organicShopping"),
        any(r, VIDEO_DOMAINS),
        any(m, &["video"]),
        split("paidVideo", "organicVideo"),
    ));
    sql.push("''");
    for host in hostnames {
        sql.push(", ")
            .bind(host.trim_start_matches("www.").to_lowercase());
    }
    sql.push(") THEN 'referral' ELSE 'direct' END");
}

#[cfg(test)]
mod tests {
    use holocron_storage::duckdb::Connection;
    use serde_json::Value as Json;

    use super::{Sql, channel};

    #[test]
    fn channels_match_the_classification_fixture() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/gaps/channel-classification.json"
        );
        let fixture: Json = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let hostnames: Vec<String> = fixture["site_hostnames"]
            .as_array()
            .unwrap()
            .iter()
            .map(|h| h.as_str().unwrap().to_owned())
            .collect();
        let conn = Connection::open_in_memory().unwrap();
        for case in fixture["touch_cases"].as_array().unwrap() {
            let touch = &case["touch"];
            let text = |key: &str| touch[key].as_str().map(str::to_lowercase);
            let mut sql = Sql::default();
            sql.push("SELECT ");
            channel("q", "r", "m", "s", &hostnames, &mut sql);
            sql.push(" FROM (SELECT ? AS q, ? AS r, ? AS m, ? AS s)");
            let mut params = vec![
                text("page_query"),
                text("referrer_domain"),
                text("utm_medium"),
                text("utm_source"),
            ];
            let mut all: Vec<holocron_storage::duckdb::types::Value> = sql.params;
            let tail: Vec<_> = params
                .drain(..)
                .map(|p| p.map_or(holocron_storage::duckdb::types::Value::Null, Into::into))
                .collect();
            all.extend(tail);
            let got: String = conn
                .query_row(
                    &sql.text,
                    holocron_storage::duckdb::params_from_iter(all.iter()),
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(
                got,
                case["expected_channel"].as_str().unwrap(),
                "{}",
                case["id"]
            );
        }
    }
}
