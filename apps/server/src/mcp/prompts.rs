//! One prompt per report recipe. Each names the blocks to send as a concrete report query, what
//! to cross-check and which notes to cite. They are English; the agent writes the report in its
//! reader's language and joins external data itself.

use chrono::{Months, NaiveDate};
use serde::Serialize;
use serde_json::{Map, Value, json};

use super::resources::GUIDE_URI;
use super::{INVALID_PARAMS, RpcError, Text};

#[derive(Serialize)]
struct Prompt {
    name: &'static str,
    title: &'static str,
    description: &'static str,
    arguments: &'static [Argument],
}

#[derive(Serialize)]
struct Argument {
    name: &'static str,
    description: &'static str,
    required: bool,
}

const SITE: Argument = Argument {
    name: "site",
    description: "Site ID, or a site name to look up with list_sites.",
    required: true,
};

const RANGE: Argument = Argument {
    name: "range",
    description: "YYYY-MM-DD..YYYY-MM-DD, inclusive local dates. Default: the last 30 days.",
    required: false,
};

const PROMPTS: [Prompt; 4] = [
    Prompt {
        name: "event_sales_report",
        title: "Event or product sales report",
        description: "Views, buyer funnel per page with step durations, revenue per currency, \
            buyer sources, places and times for a set of event or product pages.",
        arguments: &[
            SITE,
            Argument {
                name: "pages",
                description: "A path glob such as /*/events/**, or a tag.",
                required: true,
            },
            RANGE,
        ],
    },
    Prompt {
        name: "campaign_report",
        title: "Campaign report",
        description: "Traffic, sources, landing pages, daily trend and attributed revenue of one \
            UTM campaign.",
        arguments: &[
            SITE,
            Argument {
                name: "campaign",
                description: "The utm_campaign value.",
                required: true,
            },
            RANGE,
        ],
    },
    Prompt {
        name: "monthly_site_report",
        title: "Monthly site report",
        description: "One month of a site against the month before: traffic, pages, channels, \
            referrers, countries, devices, events and revenue.",
        arguments: &[
            SITE,
            Argument {
                name: "month",
                description: "YYYY-MM.",
                required: true,
            },
        ],
    },
    Prompt {
        name: "content_performance",
        title: "Content performance",
        description: "Which pages of a section draw views, hold readers and lead on, against the \
            period before.",
        arguments: &[
            SITE,
            Argument {
                name: "section",
                description: "A path glob such as /blog/**. Default: every page.",
                required: false,
            },
            RANGE,
        ],
    },
];

pub(super) fn list() -> Value {
    json!({ "prompts": PROMPTS })
}

#[derive(Serialize)]
struct Message {
    role: &'static str,
    content: Text,
}

pub(super) fn get(params: &Map<String, Value>) -> Result<Value, RpcError> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let prompt = PROMPTS
        .iter()
        .find(|prompt| prompt.name == name)
        .ok_or_else(|| RpcError::new(INVALID_PARAMS, format!("Unknown prompt: {name}")))?;
    let args = Args(params.get("arguments").and_then(Value::as_object));
    for argument in prompt.arguments.iter().filter(|a| a.required) {
        if args.get(argument.name).is_none() {
            return Err(RpcError::new(
                INVALID_PARAMS,
                format!("{name} needs the argument {}.", argument.name),
            ));
        }
    }
    let text = match prompt.name {
        "event_sales_report" => event_sales(&args),
        "campaign_report" => campaign(&args),
        "monthly_site_report" => monthly(&args)?,
        _ => content(&args),
    };
    let message = Message {
        role: "user",
        content: Text::new(text),
    };
    Ok(json!({ "description": prompt.description, "messages": [message] }))
}

struct Args<'a>(Option<&'a Map<String, Value>>);

impl Args<'_> {
    fn get(&self, name: &str) -> Option<&str> {
        self.0?
            .get(name)?
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }

    /// Required arguments are checked before rendering.
    fn required(&self, name: &str) -> &str {
        self.get(name).unwrap_or_default()
    }
}

fn event_sales(args: &Args) -> String {
    let site = args.required("site");
    let pages = args.required("pages");
    let (range, period) = range(args.get("range"));
    let on_pages = || group(vec![pages_condition("f1", pages)]);
    let purchase = || group(vec![cond("f1", "event_name", "eq", "<purchase event>")]);
    let query = json!({ "v": 1, "range": range, "blocks": [
        { "name": "pages", "site": site, "filter": on_pages(), "body": cut(
            json!(["pageviews", "visitors"]), json!(["path"]), 100) },
        { "name": "buyers", "site": site, "body": { "kind": "report", "report": {
            "kind": "funnel", "v": 2, "scope": "daily", "window_seconds": 86_400,
            "group_by": "path", "steps": [
                group(vec![pages_condition("s1", pages)]),
                group(vec![cond("s2", "event_name", "eq", "<checkout event>")]),
                group(vec![cond("s3", "event_name", "eq", "<purchase event>")]),
            ] } } },
        { "name": "revenue", "site": site, "body": cut(
            json!(["revenue_net", "revenue_gross", "revenue_refunds", "revenue_count"]), json!([]), 1) },
        { "name": "orders", "site": site, "filter": purchase(), "body": cut(
            json!(["revenue_count", "revenue_net"]), json!([{ "scope": "event", "key": "<order or event id property>" }]), 1000) },
        { "name": "sources", "site": site, "body": { "kind": "report", "report": {
            "kind": "attribution", "v": 1, "model": "last_touch", "lookback_days": 30,
            "scope": "daily", "touch_field": "channel", "conversions": [{ "kind": "revenue" }] } } },
        { "name": "places", "site": site, "filter": on_pages(), "body": cut(
            json!(["visitors"]), json!(["country", "city"]), 50) },
        { "name": "visit_times", "site": site, "filter": on_pages(), "body": cut(
            json!(["visitors"]), json!(["weekday", "hour"]), 168) },
        { "name": "purchase_times", "site": site, "filter": purchase(), "body": cut(
            json!(["revenue_count"]), json!(["weekday", "hour"]), 168) },
    ] });
    format!(
        "Write a sales report for the pages `{pages}` of site `{site}` over {period}.\n\n\
        {}\n\n\
        Cross-check before you write:\n\
        - `buyers` step 1 per group against `pages` visitors per path: the same order of \
        magnitude, or the step 1 condition differs from the pages filter.\n\
        - `sources` revenue over all touch and direct rows against `revenue` net, per currency.\n\
        - `revenue` and `orders` against the ticketing or shop totals for the same days. Name \
        refunds, offline sales and untracked orders as the likely gaps.\n\n\
        Read the funnel per group: `conversion` from step 1, `drop` per step, and the row from \
        step 1 to the last step for the time to buy (`median_ms`, buckets). Lead time before an \
        event date needs the event dates from the ticketing system: join them on `group` (the \
        path) yourself. Scope `daily` follows nobody past local midnight (note `daily_limit`); \
        use scope `person` only when the site records consent, and say which you used.\n\n\
        {}",
        steps(site, &query),
        closing()
    )
}

fn campaign(args: &Args) -> String {
    let site = args.required("site");
    let campaign = args.required("campaign");
    let (range, period) = range(args.get("range"));
    let only = || group(vec![cond("f1", "utm_campaign", "eq", campaign)]);
    let attribution = |model: &str| {
        json!({ "kind": "report", "report": {
            "kind": "attribution", "v": 1, "model": model, "lookback_days": 30, "scope": "daily",
            "touch_field": "utm_campaign", "conversions": [{ "kind": "revenue" }] } })
    };
    let mut overview = cut(
        json!([
            "visitors",
            "sessions",
            "pageviews",
            "bounce_rate",
            "revenue_net"
        ]),
        json!([]),
        1,
    );
    overview["compare"] = json!("previous_period");
    let query = json!({ "v": 1, "range": range, "blocks": [
        { "name": "overview", "site": site, "filter": only(), "body": overview },
        { "name": "by_source", "site": site, "filter": only(), "body": cut(
            json!(["visitors", "sessions", "revenue_net"]), json!(["utm_source", "utm_medium"]), 50) },
        { "name": "by_content", "site": site, "filter": only(), "body": cut(
            json!(["visitors", "sessions"]), json!(["utm_content"]), 50) },
        { "name": "landing", "site": site, "filter": only(), "body": cut(
            json!(["pageviews", "visitors"]), json!(["path"]), 20) },
        { "name": "daily", "site": site, "filter": only(), "body": {
            "kind": "cut", "measures": ["visitors", "sessions"], "series": true, "granularity": "day" } },
        { "name": "last_touch", "site": site, "body": attribution("last_touch") },
        { "name": "first_touch", "site": site, "body": attribution("first_touch") },
    ] });
    format!(
        "Write a report on the campaign `{campaign}` of site `{site}` over {period}.\n\n\
        {}\n\n\
        Cross-check before you write:\n\
        - `by_source` visitors add up to about `overview` visitors; a visitor seen from two \
        sources counts in both.\n\
        - `daily` sums to more visitors than `overview`, which counts each visitor once.\n\
        - The `{campaign}` rows of `last_touch` and `first_touch` against `overview` revenue: \
        attribution credits revenue to the campaign that touched the buyer, the overview counts \
        revenue inside campaign sessions.\n\
        - Spend, clicks and impressions come from the ad platforms: join them yourself on \
        utm_source, utm_medium and utm_content.\n\n\
        {}",
        steps(site, &query),
        closing()
    )
}

fn monthly(args: &Args) -> Result<String, RpcError> {
    let site = args.required("site");
    let month = args.required("month");
    let start = NaiveDate::parse_from_str(&format!("{month}-01"), "%Y-%m-%d")
        .map_err(|_| RpcError::new(INVALID_PARAMS, "month must be YYYY-MM."))?;
    let end = start
        .checked_add_months(Months::new(1))
        .and_then(|next| next.pred_opt())
        .ok_or_else(|| RpcError::new(INVALID_PARAMS, "month is out of range."))?;
    let range = json!({ "kind": "days", "start": start.to_string(), "end": end.to_string() });
    let mut overview = cut(
        json!([
            "visitors",
            "sessions",
            "pageviews",
            "bounce_rate",
            "span_median",
            "revenue_net"
        ]),
        json!([]),
        1,
    );
    overview["compare"] = json!("previous_period");
    let top = |dimension: &str, limit: u32| {
        cut(json!(["visitors", "pageviews"]), json!([dimension]), limit)
    };
    let query = json!({ "v": 1, "range": range, "blocks": [
        { "name": "overview", "site": site, "body": overview },
        { "name": "daily", "site": site, "body": {
            "kind": "cut", "measures": ["visitors", "pageviews"], "series": true, "granularity": "day" } },
        { "name": "pages", "site": site, "body": top("path", 20) },
        { "name": "channels", "site": site, "body": top("channel", 20) },
        { "name": "referrers", "site": site, "body": top("referrer_domain", 20) },
        { "name": "countries", "site": site, "body": top("country", 20) },
        { "name": "devices", "site": site, "body": top("device", 10) },
        { "name": "events", "site": site, "body": cut(json!(["events", "visitors"]), json!(["event_name"]), 20) },
    ] });
    Ok(format!(
        "Write the monthly report of site `{site}` for {month}, against the month before.\n\n\
        {}\n\n\
        Cross-check before you write:\n\
        - `overview` has a `<measure>_previous` column per measure: report changes from it, not \
        from memory.\n\
        - `daily` visitors add up to more than `overview` visitors, which counts each visitor \
        once; over a month `visitors` are visitor-days (note `visitor_days`).\n\
        - `channels` and `referrers` visitors stay at or below `overview` visitors per row.\n\
        - Goals and targets come from the person who asked; Holocron has no targets.\n\n\
        {}",
        steps(site, &query),
        closing()
    ))
}

fn content(args: &Args) -> String {
    let site = args.required("site");
    let section = args.get("section").unwrap_or("/**");
    let (range, period) = range(args.get("range"));
    let within = || group(vec![cond("f1", "path", "matches", section)]);
    let mut pages = cut(
        json!(["pageviews", "visitors", "span_median", "bounce_rate"]),
        json!(["path"]),
        50,
    );
    pages["compare"] = json!("previous_period");
    let query = json!({ "v": 1, "range": range, "blocks": [
        { "name": "pages", "site": site, "filter": within(), "body": pages,
          "derive": [{ "name": "view_share", "op": "share", "a": { "column": "pageviews" } }] },
        { "name": "titles", "site": site, "filter": within(), "body": cut(
            json!(["pageviews", "visitors"]), json!(["title"]), 50) },
        { "name": "referrers", "site": site, "filter": within(), "body": cut(
            json!(["visitors"]), json!(["referrer_domain"]), 20) },
        { "name": "weekly", "site": site, "filter": within(), "body": {
            "kind": "cut", "measures": ["pageviews", "visitors"], "series": true, "granularity": "week" } },
        { "name": "paths", "site": site, "filter": within(), "body": { "kind": "report", "report": {
            "kind": "journey", "v": 1, "value": "path", "length": 3, "scope": "session" } } },
    ] });
    format!(
        "Write a content performance report for the pages `{section}` of site `{site}` over \
        {period}.\n\n\
        {}\n\n\
        Cross-check before you write:\n\
        - `pages` `view_share` sums to about 1 over the returned rows; when `rows_truncated` is \
        noted, say the rest is left out.\n\
        - Rank by views and by `span_median` (time on page) separately: a page that is read long \
        but seldom is a different finding from a popular page that is left quickly.\n\
        - `paths` shows where readers go next; name the pages that lead on and those that end \
        sessions.\n\
        - Publication dates and authors come from the CMS: join them yourself on `path`.\n\n\
        {}",
        steps(site, &query),
        closing()
    )
}

/// The query to send and how to fill it in.
fn steps(site: &str, query: &Value) -> String {
    let query = serde_json::to_string_pretty(query).unwrap_or_default();
    format!(
        "If `{site}` is not a site ID, call `list_sites` and use the matching site's `id`. \
        Replace every `<...>` placeholder first: `field_values` with field `event_name` gives \
        event names, `catalog` gives property keys. Drop a block whose data the site does not \
        record. Then send this with the `query` tool:\n\n```json\n{query}\n```"
    )
}

fn closing() -> String {
    format!(
        "When you write:\n\
        - Cite each block's `notes` (code and message) as method notes, and say so when \
        `watermark.stale` is true.\n\
        - Write in the language of the person who asked. These instructions are English; the \
        report need not be.\n\
        - Holocron holds analytics only. Join external data such as ticketing, orders or CRM \
        yourself on the keys in the rows: paths, event names, event properties.\n\
        - A refusal names the path to fix, such as `blocks.2.filter.f3`. The rules are in \
        {GUIDE_URI}."
    )
}

/// `YYYY-MM-DD..YYYY-MM-DD` as a days range, else the last 30 days. Text in another form stays
/// a placeholder for the agent to convert.
fn range(text: Option<&str>) -> (Value, String) {
    let Some(text) = text else {
        return (
            json!({ "kind": "relative", "unit": "day", "count": 30 }),
            "the last 30 days".into(),
        );
    };
    let day = |s: &str| NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d").ok();
    match text.split_once("..").map(|(a, b)| (day(a), day(b))) {
        Some((Some(start), Some(end))) => (
            json!({ "kind": "days", "start": start.to_string(), "end": end.to_string() }),
            format!("{start} to {end}"),
        ),
        _ => (json!(format!("<range: {text}>")), text.to_owned()),
    }
}

fn cut(measures: Value, dimensions: Value, limit: u32) -> Value {
    let mut body = json!({ "kind": "cut", "dimensions": dimensions });
    if let Some(first) = measures.get(0).filter(|_| limit > 1) {
        body["sort"] = json!([{ "by": first, "dir": "desc" }]);
        body["limit"] = json!(limit);
    }
    body["measures"] = measures;
    body
}

/// A path glob, or else a tag.
fn pages_condition(id: &str, pages: &str) -> Value {
    if pages.starts_with('/') {
        cond(id, "path", "matches", pages)
    } else {
        cond(id, "tag", "eq", pages)
    }
}

fn cond(id: &str, field: &str, cmp: &str, value: &str) -> Value {
    json!({ "kind": "cond", "id": id, "field": field, "cmp": cmp,
        "value": { "t": "string", "v": value } })
}

fn group(items: Vec<Value>) -> Value {
    json!({ "kind": "group", "op": "and", "items": items })
}
