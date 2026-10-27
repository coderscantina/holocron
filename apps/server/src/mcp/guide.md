# Report queries

A report query asks Holocron for several tables at once. Send it with the MCP `query` tool or as the body of `POST /api/queries/analytics.report`. The JSON Schema is the resource `holocron://report-query/schema`; this guide explains how the parts work together. The rules are in decision 0022.

Holocron answers aggregates only: counts, rates, durations and money. It has no session lists, no raw events and no SQL. Join external data, such as ticketing, orders or CRM, yourself on the keys in the rows: paths, event names and event properties.

## Shape

```json
{
  "v": 1,
  "range": { "kind": "relative", "unit": "day", "count": 30 },
  "timezone": "Europe/Vienna",
  "blocks": [
    { "name": "pages", "site": "<site id>", "body": { "kind": "cut", "measures": ["pageviews"], "dimensions": ["path"] } }
  ]
}
```

- `v` is always 1. Unknown fields are refused.
- `range` and `timezone` are defaults for blocks that name none. Without a timezone a block reads its site's timezone, which also decides where days, weekdays and hours start.
- `blocks`: 1 to 16. Each has a unique `name` (`^[a-z][a-z0-9_]{0,31}$`) and one `site`. Blocks may read different sites. They run in order on one snapshot, so every block reports the same `watermark`.

Ranges:

- `{ "kind": "relative", "unit": "day" | "week" | "month", "count": n }`: the last `n` units ending with the current one. `count: 1` with `month` is this month so far.
- `{ "kind": "days", "start": "2026-09-01", "end": "2026-09-30" }`: inclusive local dates.
- `{ "kind": "all" }`: every retained day through today.

## Fields, values and filters

A field is a built-in name or a custom property:

- Event fields: `path`, `hostname`, `title`, `query`, `event_name`, `tag`, `perf_metric`, `currency`, `weekday` (1 Monday to 7), `hour` (0 to 23).
- Session fields: `entry_path` (the path of the session's first pageview, else first event), `referrer_domain`, `referrer_path`, `utm_source`, `utm_medium`, `utm_campaign`, `utm_content`, `utm_term`, `click_id`, `channel`, `country`, `region`, `city`, `browser`, `os`, `device`, `language`, `screen`.
- Properties: `{ "scope": "event", "key": "checkout_id" }` or `{ "scope": "session", "key": "plan" }`. Their column is named `event.checkout_id`. The `catalog` tool lists the keys a site has; `field_values` lists the most frequent values of any field.

Values are typed: `{ "t": "string", "v": "/pricing" }`, `{ "t": "integer", "v": 3 }`, `{ "t": "decimal", "v": "9.5" }`, `{ "t": "bool", "v": true }`, `{ "t": "date", "v": "2026-09-01T00:00:00Z" }`, `{ "t": "list", "v": [ ... ] }`.

A block's `filter` is a group of conditions:

```json
{ "kind": "group", "op": "and", "items": [
  { "kind": "cond", "id": "f1", "field": "path", "cmp": "matches", "value": { "t": "string", "v": "/*/events/**" } },
  { "kind": "group", "op": "or", "items": [
    { "kind": "cond", "id": "f2", "field": "country", "cmp": "eq", "value": { "t": "string", "v": "AT" } },
    { "kind": "cond", "id": "f3", "field": "country", "cmp": "eq", "value": { "t": "string", "v": "DE" } }
  ] }
] }
```

- `op` is `and` or `or`. A group holds 1 to 32 items and nests at most three levels deep.
- Each condition has an `id` (`^[a-z0-9_]{1,16}$`) unique within the block. Refusals name it: `blocks.0.filter.f3`.
- `cmp`: `eq`, `neq`, `contains`, `not_contains`, `starts_with`, `in`, `not_in`, `gt`, `gte`, `lt`, `lte`, `between` (a list of exactly 2, both ends inclusive), `is_set`, `is_not_set`, `is_null` (these three take no value), `matches`, `not_matches`.
- `in` and `not_in` take a list of up to 1,000 values, or a ref (below).

### Globs

`matches` compares the whole value with a glob, case-sensitively:

- `*` matches any run of characters except `/`.
- `**` matches any run, including `/`.
- `\` escapes the next character.

`/*/events/**` matches `/en/events/summer-fest` and `/de/events/2026/opening`, not `/events/x`. Patterns are 1 to 500 bytes with at most 8 wildcards. `matches` works on `path`, `entry_path`, `referrer_path`, `title`, `query`, `hostname` and string properties. `not_matches` is its exact complement.

## Cuts

```json
{ "kind": "cut", "measures": ["pageviews", "visitors"], "dimensions": ["path"],
  "sort": [{ "by": "pageviews", "dir": "desc" }], "limit": 100 }
```

- `measures`: 1 to 8 of `pageviews`, `events`, `visitors`, `visitor_days`, `identified_people`, `sessions`, `bounce_rate`, `span_mean`, `span_median`, `revenue_net`, `revenue_gross`, `revenue_refunds`, `revenue_count`, `p50`, `p75`, `p95` (performance percentiles, which need a `perf_metric` condition), or a statistic over a numeric property: `{ "stat": "sum", "field": { "scope": "event", "key": "amount" } }` with `count`, `sum`, `min`, `max` or `mean`. Its column is `sum(event.amount)`.
- `dimensions`: 0 to 3 fields. Without dimensions or a series, the one row holds the totals.
- `series: true` makes the rows the time buckets of the totals, in a leading `time` column, by `granularity` (`hour`, `day`, `week`, `month`; default `day`). A series takes no dimensions.
- `compare`: `previous_period` or `typical_weekday` adds a `<measure>_previous` column after each measure.
- `sort`: 0 to 2 keys, by a measure or a field. Ties fall back to the dimension values ascending. Sorting by a revenue measure needs the `currency` dimension.
- `limit`: 1 to 1,000 rows, default 100. The note `rows_truncated` says when more exist. For more rows, use the `export` tool, which returns up to 10,000.

Visitors are daily pseudonyms. Over a range longer than one identity day `visitors` counts visitor-days, and the block says so with the note `visitor_days`. Revenue is per currency and never summed across currencies: a money cell is `{ "t": "money", "v": [{ "currency": "EUR", "exponent": 2, "minor_units": 4991, "amount": "49.91" }] }`, one entry per currency in code order.

## Reports

A report block computes one report and flattens it into rows:

```json
{ "kind": "report", "report": { "kind": "funnel", ... } }
```

| Kind | Settings | Columns |
| --- | --- | --- |
| `funnel` | `v` 1, or 2 with `group_by`; `steps` 2 to 8 groups; `window_seconds` 60 to 2,592,000; `scope` `daily` or `person` | `group` (with `group_by`), `step`, `from_step`, `reached`, `drop`, `conversion`, `p25_ms`, `median_ms`, `p75_ms`, `bucket_lt_1h`, `bucket_1h_24h`, `bucket_1d_7d`, `bucket_7d_30d`, `bucket_ge_30d` |
| `journey` | `v` 1; `value` `path` or `event_name`; `length` 2 to 7; `scope` `session` or `person` | `step_1` to `step_<length>`, `identities` |
| `attribution` | `v` 1; `model` `last_touch` or `first_touch`; `lookback_days` 30; `scope`; `touch_field` such as `channel` or `utm_campaign`; `conversions` 1 to 4, each `{ "kind": "revenue" }` or `{ "kind": "goal", "goal": <goal> }` | `conversion`, `touch`, `direct`, `conversions`, `revenue` |
| `retention` | `v` 1; `intervals` 1 to 32 for day cohorts, 1 to 53 for week cohorts. The block's `granularity` (`day` or `week`) is the cohort period | `cohort`, `offset`, `cohort_size`, `retained`, `rate`, `complete` |
| `goals` | `v` 1; `goals` 1 to 8, each `{ "predicate": <group>, "scope": "session" or "person", "counting": "once" or "every" }` | `goal`, `completions`, `denominator`, `rate` |

Steps, goals and conversions count from 1.

### Funnels

- An identity enters at its first step-1 event and has `window_seconds` from there to reach the later steps in order. `conversion` is from step 1.
- Each step row carries the time from the previous step: p25, median and p75 in milliseconds, and counts in the five buckets. After the step rows comes one row with `from_step` 1 and `step` the last step: the time to convert.
- `group_by` (version 2) splits identities by one field's value on their entering step-1 event, as `group_by: "path"` gives one funnel per landing page. Groups are ordered by step 1 reached, descending, at most 50; the rest are left out with the note `groups_truncated`. With `group_by` the rows are per group only.
- `scope: "daily"` follows the daily pseudonym, so nobody progresses across local midnight (note `daily_limit`). `scope: "person"` follows consented profiles across days, and counts only those.

## Fan-out and refs

`for_each` runs a block once per value with an added `eq` condition, and puts the value in a leading column named after the field:

```json
"for_each": { "field": "utm_campaign", "values": [{ "t": "string", "v": "spring" }, { "t": "string", "v": "summer" }] }
```

Up to 50 values. Prefer a dimension or a funnel `group_by` where one exists: they run in one pass. `for_each` blocks have no `totals`, since visitors do not add up across runs.

A ref takes the values of a `dimension` or `for_each` column of an earlier block, in that block's row order, without null and not-set cells:

```json
{ "ref": { "block": "top_pages", "column": "path", "limit": 10 } }
```

A ref fills an `in` or `not_in` value (limit 1 to 1,000, default 1,000) or the `for_each` values (limit 1 to 50, default 50). A ref that finds no values skips its block, which answers no rows and the note `ref_empty`.

## Derived columns

Cuts may add 0 to 8 computed columns:

```json
"derive": [
  { "name": "views_per_visitor", "op": "ratio", "a": { "column": "pageviews" }, "b": { "column": "visitors" } },
  { "name": "view_share", "op": "share", "a": { "column": "pageviews" } },
  { "name": "vs_all", "op": "ratio", "a": { "column": "visitors" }, "b": { "block": "overview", "column": "visitors", "total": true } }
]
```

- `ratio` is `a / b`, `share` is `a` over the total of the same column, both rounded to 4 places and `{ "t": "unavailable", "reason": "no_denominator" }` when the denominator is 0. `difference` is `a − b` of two columns of the same type.
- Operands are measure columns other than ratios. A `total` operand names a cut block without `for_each`. Money pairs only with money of one currency per cell; otherwise the cell is unavailable with the reason `mixed_currencies`.

## Limits

- 16 blocks and 200 sub-queries per request, counting each `for_each` value as one and a `for_each` ref at its limit. A larger request is refused with `report_too_large` before anything runs.
- The whole request shares one 30 second budget; past it the answer is `query_timeout`. Split a slow request, or narrow its ranges.
- Session lists are refused (`report_field_unsupported`).

## Answers

```text
{ v, watermark: { batch, sequence, at, stale }, metric_version, blocks: [
  { name, site, resolved, timezone, freshness, columns: [{ name, type, role }], rows, totals, notes } ] }
```

- `columns` give each column's `type` (`string`, `integer`, `decimal`, `money`, `ratio`, `duration_ms`, `date`, `instant`, `bool`) and `role` (`for_each`, `dimension`, `time`, `measure`, `derived`).
- `rows` hold one cell per column. Keys are typed cells such as `{ "t": "string", "v": "/a" }`, or `{ "t": "not_set" }` when the event has no value. Values are metrics: `{ "t": "count", "v": 12 }`, `{ "t": "ratio", "v": 0.25, "numerator": 1, "denominator": 4 }`, `{ "t": "millis", "v": 5400 }`, `{ "t": "decimal", "v": "9.5000" }`, money, or `{ "t": "unavailable", "reason": ... }`.
- `totals` are over every matching row, not only the returned ones, with `null` for dimensions. Only cuts without `for_each` have them.
- `notes` are method notes to cite: `coverage` (some events carry no identity, so visitor counts cover less than all traffic), `visitor_days`, `daily_limit`, `groups_truncated`, `rows_truncated`, `ref_empty`. `watermark.stale` means analytics is over a minute behind collection.
- A refusal has an `error_code`, a `message` and, for `invalid_arguments`, field errors by path such as `blocks.0.derive.1.b`. A body that does not parse is `malformed_request` with the path leading its message.

## Examples

### Top pages and their share

```json
{ "v": 1, "range": { "kind": "relative", "unit": "day", "count": 30 }, "blocks": [
  { "name": "pages", "site": "<site id>", "body": {
      "kind": "cut", "measures": ["pageviews", "visitors"], "dimensions": ["path"],
      "sort": [{ "by": "pageviews", "dir": "desc" }], "limit": 20, "compare": "previous_period" },
    "derive": [{ "name": "view_share", "op": "share", "a": { "column": "pageviews" } }] }
] }
```

### Event sales for a venue

Views of every event page, a buyer funnel per event page with step durations, revenue per currency, buyer sources and when people buy. Find the real event names with `field_values` on `event_name` first.

```json
{ "v": 1, "range": { "kind": "days", "start": "2026-09-01", "end": "2026-09-30" }, "blocks": [
  { "name": "pages", "site": "<site id>",
    "filter": { "kind": "group", "op": "and", "items": [
      { "kind": "cond", "id": "f1", "field": "path", "cmp": "matches", "value": { "t": "string", "v": "/*/events/**" } } ] },
    "body": { "kind": "cut", "measures": ["pageviews", "visitors"], "dimensions": ["path"],
      "sort": [{ "by": "pageviews", "dir": "desc" }], "limit": 100 } },
  { "name": "buyers", "site": "<site id>", "body": { "kind": "report", "report": {
      "kind": "funnel", "v": 2, "scope": "daily", "window_seconds": 86400, "group_by": "path",
      "steps": [
        { "kind": "group", "op": "and", "items": [
          { "kind": "cond", "id": "s1", "field": "path", "cmp": "matches", "value": { "t": "string", "v": "/*/events/**" } } ] },
        { "kind": "group", "op": "and", "items": [
          { "kind": "cond", "id": "s2", "field": "event_name", "cmp": "eq", "value": { "t": "string", "v": "checkout" } } ] },
        { "kind": "group", "op": "and", "items": [
          { "kind": "cond", "id": "s3", "field": "event_name", "cmp": "eq", "value": { "t": "string", "v": "purchase" } } ] }
      ] } } },
  { "name": "revenue", "site": "<site id>", "body": { "kind": "cut", "measures": ["revenue_net", "revenue_count"] } },
  { "name": "sources", "site": "<site id>", "body": { "kind": "report", "report": {
      "kind": "attribution", "v": 1, "model": "last_touch", "lookback_days": 30, "scope": "daily",
      "touch_field": "channel", "conversions": [{ "kind": "revenue" }] } } },
  { "name": "places", "site": "<site id>",
    "filter": { "kind": "group", "op": "and", "items": [
      { "kind": "cond", "id": "f1", "field": "path", "cmp": "matches", "value": { "t": "string", "v": "/*/events/**" } } ] },
    "body": { "kind": "cut", "measures": ["visitors"], "dimensions": ["country"],
      "sort": [{ "by": "visitors", "dir": "desc" }], "limit": 20 } },
  { "name": "purchase_times", "site": "<site id>", "body": {
      "kind": "cut", "measures": ["revenue_count"], "dimensions": ["weekday", "hour"], "limit": 168 } }
] }
```

Read `buyers` per `group`: the last row of each group (from step 1 to step 3) is the time from the event page to the purchase. The lead time before the event date needs the event dates from the ticketing system, joined on the path.

### One series per top campaign

A ref feeds the top five campaigns of the first block into `for_each`, so each campaign gets its own daily series.

```json
{ "v": 1, "range": { "kind": "relative", "unit": "week", "count": 4 }, "blocks": [
  { "name": "campaigns", "site": "<site id>", "body": {
      "kind": "cut", "measures": ["visitors"], "dimensions": ["utm_campaign"],
      "sort": [{ "by": "visitors", "dir": "desc" }], "limit": 5 } },
  { "name": "daily", "site": "<site id>",
    "for_each": { "field": "utm_campaign", "values": { "ref": { "block": "campaigns", "column": "utm_campaign" } } },
    "body": { "kind": "cut", "measures": ["visitors", "revenue_net"], "series": true, "granularity": "day" } }
] }
```

### Signups and returning visitors

```json
{ "v": 1, "range": { "kind": "relative", "unit": "week", "count": 8 }, "blocks": [
  { "name": "signups", "site": "<site id>", "body": { "kind": "report", "report": {
      "kind": "goals", "v": 1, "goals": [{
        "predicate": { "kind": "group", "op": "and", "items": [
          { "kind": "cond", "id": "g1", "field": "event_name", "cmp": "eq", "value": { "t": "string", "v": "signup" } } ] },
        "scope": "session", "counting": "once" }] } } },
  { "name": "returning", "site": "<site id>", "body": { "kind": "report", "granularity": "week",
      "report": { "kind": "retention", "v": 1, "intervals": 8 } } }
] }
```
