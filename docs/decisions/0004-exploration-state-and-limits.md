# 0004: Exploration state, limits, and encoding

Status: Adopted (milestone 01, team decision). Limits that cap what the gate promises are "supported complexity", which the briefing assigns to milestone 01. They do not remove features.

## Problem

URLs, saved explorations, board cards, exports, Vue Query keys, and the Q01 to Q14 query suite all need one typed state. The planner needs declared limits so a query can be refused up front instead of timing out.

## Constraints

Confirmed: one versioned typed state; repeated conditions on one field; grouped AND/OR; relative and absolute dates resolved once per request; comparison; metric version carried; unsupported combinations refused visibly; no user SQL.

## Evidence

Upstream encodes filters as flat query parameters with numeric suffixes (`browser1`, `epf2`) and disables segment saving when event-property filters are active (`FilterBar.tsx`). Kit's `useTableQueryState` keeps readable URL params and omits defaults. The acceptance workflow needs two split dimensions, three predicates, relative dates, and a comparison.

## Choice

### State shape, version 1

```text
ExplorationState {
  v: 1
  site: Uuid                     the website, or the link or pixel when source is not "website"
  source?: "website" | "link" | "pixel"     omitted means "website"
  measures: Measure[]            1..8, unique; exactly 0 when report is present
  dimensions: Field[]            0..3, unique, event or session dimensions or properties; exactly 0 when report is present
  filter: Group | null
  range: Range
  timezone: IanaTimezone         always explicit; default is the site timezone at creation
  granularity: "hour" | "day" | "week" | "month"
  compare: "none" | "previous_period"
  metric_version: integer        pinned at creation
  sort: Sort[]                   0..2
  page: { size: 1..100, index: 0.. }
  visualization: "table" | "line" | "bar" | "map" | "grid" | "funnel" | "journey" | "cohort"
  report?: ReportDefinition      omitted for plain explorations; see Report definitions
}

Measure = "pageviews" | "events" | "visitors" | "visitor_days" | "identified_people"
        | "sessions" | "bounce_rate" | "span_mean" | "span_median"
        | "revenue_net" | "revenue_gross" | "revenue_refunds" | "revenue_count"
        | "p50" | "p75" | "p95"          (performance only; need a perf_metric predicate)
        | "hits"                         (link and pixel sources only)
        | PropertyStat

PropertyStat = { stat: "count" | "sum" | "min" | "max" | "mean", field: PropertyField }

Field = DimensionField | PropertyField
DimensionField = "path" | "hostname" | "title" | "referrer_domain" | "referrer_path" | "query"
               | "country" | "region" | "city" | "browser" | "os" | "device" | "language" | "screen"
               | "event_name" | "tag" | "utm_source" | "utm_medium" | "utm_campaign" | "utm_content" | "utm_term"
               | "click_id" | "perf_metric" | "currency"
               | "channel" | "weekday" | "hour"
PropertyField = { scope: "event" | "session", key: string (1..500 bytes) }

Group = { kind: "group", op: "and" | "or", items: (Group | Condition)[] }   1..32 items
Condition = { kind: "cond", id: PredicateId, field: Field, cmp: Operator, value?: Value }
PredicateId = string matching ^[a-z0-9_]{1,16}$, unique within the state, assigned by the client when the condition is created and never renumbered

Operator by value type:
  string:  eq, neq, contains, not_contains, starts_with, in, not_in
  integer, decimal, date: eq, neq, gt, gte, lt, lte, between
  bool:    eq
  array:   contains, not_contains
  any:     is_set, is_not_set, is_null        (no value)

Value = { t: "string",  v: string }
      | { t: "integer", v: integer or decimal string when |v| > 2^53 − 1 }
      | { t: "decimal", v: decimal string, up to 4 fractional digits }
      | { t: "date",    v: RFC 3339 instant }
      | { t: "bool",    v: boolean }
      | { t: "list",    v: Value[] }           (for in, not_in, between; between has exactly 2)

Range = { kind: "relative", unit: "day" | "week" | "month", count: 1..366 }
      | { kind: "days", start: "YYYY-MM-DD", end: "YYYY-MM-DD" }     inclusive local dates, end >= start

Sort = { by: Measure | Field, dir: "asc" | "desc" }
```

Semantics:

- `relative` means the last `count` units ending with the current unit, in `timezone`, including the current partial unit. `{"kind":"relative","unit":"day","count":30}` on 2026-10-03 resolves to local days 2026-09-04 through 2026-10-03.
- Resolution happens once per request on the server: `range` becomes `[start, end)` instants and the response echoes them with the comparison bounds.
- Dimension fields are typed by the catalog; `path`, `referrer_domain`, and the like are strings. Property fields take their operators from the typed value in the condition, so a mixed-type key is filtered per type as in [0002](0002-sessions-and-events.md).
- Conditions on the same field repeat freely. Groups nest. Nothing is merged on serialization.
- Sorting on a measure sorts the aggregate; ties fall back to the dimension values ascending in UTF-8 byte order, so pages are stable.
- Pagination is offset-based (`index` × `size`) over one result snapshot; totals are not affected by paging.
- A drill-down (include, exclude, split-by from a cell) appends a condition with a new `id` or appends a dimension; it never rewrites existing predicate IDs.
- Changing `site` revalidates every field against the new site's catalog; invalid conditions are reported and the state is refused until fixed.
- `between` with `[a, b]` matches `a <= value <= b`, both ends inclusive, as a user reading "between 10 and 20" expects.

### Field scope

Each dimension is either per event or per session. Grouping or filtering events by a session field uses the value of the event's session; the field catalog shows the scope next to each field.

| Scope | Fields |
| --- | --- |
| Event | `path`, `hostname`, `title`, `query`, `event_name`, `tag`, `perf_metric`, `currency`, `weekday`, `hour`, event-scope properties |
| Session | `referrer_domain`, `referrer_path`, `utm_source`, `utm_medium`, `utm_campaign`, `utm_content`, `utm_term`, `click_id`, `channel` (all from the session's touch: its first pageview, or its first eligible event when it has no pageview); `country`, `region`, `city`, `browser`, `os`, `device`, `language`, `screen` (from the session's first eligible event); session-scope properties (0002) |

Events without a session (unattached performance, link and pixel hits) take session fields from their own request. Referrer and campaign fields are per session because the tracker reduces same-origin referrers to a path, so per-event referrers are empty after the landing page and would put most custom events in the "not set" cell. A self-referral keeps its stored domain as the cell value; no referrer is the "not set" cell, labeled "(none)" in the UI.

### Negative operators and missing values

`neq`, `not_contains`, `not_in` and `is_not_set` match exactly the rows that `eq`, `contains`, `in` and `is_set` with the same value do not match. That includes events where the key is missing, where it is `null`, and where it has another type. So on a mixed-type key, `qty neq 1` (integer) matches `"1"`, `2.5`, `null` and missing. `gt`, `gte`, `lt`, `lte` and `between` have no complement: they match numeric or date rows only (0002). This keeps include and exclude drill-downs on a cell an exact partition of the total.

### Weekday and hour

`weekday` (1 Monday to 7 Sunday, ISO 8601) and `hour` (0 to 23) are integer event fields computed from event time as wall-clock values in the state's `timezone`. On a fall-back day the repeated hour collects both instants that read it, and on a spring-forward day the skipped hour is empty (hour 2 in Europe/Vienna). The weekly traffic grid is `dimensions: ["weekday", "hour"]` with `visualization: "grid"`; `grid` requires exactly those two dimensions. Sessions grouped by them follow the common grouping rule in [0003](0003-metric-formulas.md), so a session appears in every hour in which it has an event, which matches upstream's distinct sessions per hour.

### Sources

`source` selects what the state reports on. With `"link"` or `"pixel"`, `site` holds that link or pixel ID, authorization uses the workspace that owns it, and only the measures and dimensions of [0003](0003-metric-formulas.md) Links and pixels are valid. `report` is refused for these sources. `timezone` stays explicit; the default at creation is the user's preference timezone, else UTC.

### Segments

A segment is a saved `Group` with a name, scoped to one site: it is validated against that site's field catalog and visible to everyone who can read the site. Names are trimmed, 1 to 100 characters, and unique per site after case folding, so a picker never shows two "Mobile" entries. Applying a segment copies its conditions into the state's `filter` with fresh predicate IDs; the state never references the segment, so editing a segment later does not change saved explorations. Workspace-wide segments were rejected because property keys and their types differ per site.

### Report definitions

Report-specific settings live in an optional typed `report` part, tagged by `kind`. Saved explorations and board cards store the whole state, so they carry these definitions without a second schema. Each kind has its own version `v`: changing a kind's meaning bumps that kind's `v` and migrates saved states of that kind only, while the state stays `v: 1`. Plain explorations omit `report`, so every existing v1 instance stays valid and byte-identical.

```text
ReportDefinition = Funnel | Journey | Attribution | Retention | SessionList | Goals

Funnel      = { kind: "funnel", v: 1, steps: Group[] 2..8, window_seconds: 60..2592000, scope: "daily" | "person" }
Journey     = { kind: "journey", v: 1, value: "path" | "event_name", length: 2..7, scope: "session" | "person" }
Attribution = { kind: "attribution", v: 1, model: "last_touch" | "first_touch", lookback_days: 30,
                scope: "daily" | "person", touch_field: TouchField, conversions: Conversion[] 1..4 }
TouchField  = "channel" | "referrer_domain" | "utm_source" | "utm_medium" | "utm_campaign"
            | "utm_content" | "utm_term" | "click_id"
Conversion  = { kind: "goal", goal: GoalDefinition } | { kind: "revenue" }
GoalDefinition = { predicate: Group, scope: "session" | "person", counting: "once" | "every",
                   ref?: { id: Uuid, version: integer } }
Retention   = { kind: "retention", v: 1, intervals: 1..53 }
SessionList = { kind: "session_list", v: 1, columns: SessionColumn[] 1..16 unique, order: "start_desc" | "start_asc" }
Goals       = { kind: "goals", v: 1, goals: GoalDefinition[] 1..8 }
SessionColumn = "session_id" | "identity_kind" | "start" | "end" | "span" | "pageviews" | "custom_events"
              | "entry_path" | "exit_path" | "referrer_domain" | "channel" | "utm_source" | "utm_campaign"
              | "country" | "device" | "browser" | "os"
```

How each kind uses the rest of the state. "Default" means the value must be exactly that, else the state is refused with `report_field_unsupported`; nothing is silently ignored.

| Kind | `visualization` | `granularity` | `compare` | `sort` | `page` | `filter` selects |
| --- | --- | --- | --- | --- | --- | --- |
| `funnel` | `funnel` | `day` | `none` | `[]` | default | events; every step event must match the filter and its step predicate |
| `journey` | `journey` | `day` | `none` | `[]` | default | entering sessions (or profiles): those with at least one matching event |
| `attribution` | `table` or `bar` | `day` | `none` | `[]` | default | conversion events; touches come from all sessions of the identity |
| `retention` | `cohort` | `day` or `week`, the cohort period | `none` | `[]` | default | cohort entry, on the profile's events in the cohort period ([0003](0003-metric-formulas.md)) |
| `session_list` | `table` | `day` | `none` | `[]` | pages over sessions | sessions with at least one matching event |
| `goals` | `table` or `bar` | `day` | `none` | `[]` | default | events: goal events match the filter and the goal; session goals count sessions with at least one matching event, person goals profiles with one |

- `measures` and `dimensions` are empty for every kind; the kind defines its output. Default `page` is `{ "size": 50, "index": 0 }`.
- Output order is fixed per kind: funnel by step; journey by session count, then sequence ([0003](0003-metric-formulas.md)); attribution per conversion by conversions descending, then touch value ascending, top 100 touch values per conversion; retention by cohort start, then offset; session list by `order`, ties by session ID ascending; goals in definition order.
- `visualization: "funnel"`, `"journey"` or `"cohort"` without the matching `report` is refused with `report_missing`.
- Funnel steps and goal predicates are `Group` trees. Predicate IDs are unique across the whole state, and the condition limit counts conditions in `filter` and in every report predicate together. Nesting depth applies to each tree on its own.
- A goal is stored inline, so a saved report keeps the exact goal it was built with (0003 Goals). `ref` only records which saved goal version it came from, so the UI can offer an upgrade when that goal has a newer version.
- Session list `span` is integer milliseconds, like every span in 0003. Retention offsets are `0` to `intervals − 1`.
- An unknown `kind` or `v` is refused with `report_version_unsupported`. Parsing stays strict, so a known kind whose later version adds fields fails as `malformed_request` before the version is read.
- A state with `report` takes no panels (`report_field_unsupported` on `panels`). Its result keeps the plain envelope: `main` is the state's cut without totals, rows or series, and the kind's output is in `report`.
- Refusals name the tree: `report.steps.2`, `report.conversions.0.goal.predicate`, and a condition inside it by ID, as `report.steps.2.f7`. The retention cell limit needs the resolved range, so it is checked per request rather than when the state is saved.
- `p50`, `p75` and `p95` need at least one condition on `perf_metric` in `filter`.
- A state that measures a percentile or names `perf_metric` reads performance observations, one row per metric a performance event reported, and measures only `p50`, `p75` and `p95` (refused on `measures` otherwise). Report trees never name `perf_metric`. A percentile over observations of more than one metric is `null` with `reason: "mixed_metrics"`, since their units differ.
- `revenue_net`, `revenue_gross` and `revenue_refunds` are a `money` value: one amount per currency in code order, never summed across currencies. `revenue_count` is a count. A state whose measures are all revenue measures reads revenue records only, so a `currency` cut has no "not set" cell. Sorting by an amount needs `currency` among the cut's dimensions and orders rows by currency first, then by the amount.

### Limits

| Limit | Value | Behavior when exceeded |
| --- | --- | --- |
| Group nesting depth | 3 (root group plus two nested levels) | refused: `filter_too_deep` |
| Conditions per state | 32, counting `filter` and all report predicates | refused: `too_many_conditions` |
| Items per `in`/`not_in` list | 50 | refused |
| Dimensions | 3 | refused |
| Measures | 8 | refused |
| Page size | 100 (default 50) | clamped to 100 |
| Addressable result rows | 10,000 per query (page index × size ≤ 10,000) | refused: `page_out_of_range`; totals still exact |
| Series points | 744 (31 days hourly) | refused: choose coarser granularity |
| Relative range | 366 days | refused |
| Query runtime | cancelled at 30 s; gate targets 2 s and 10 s p95 | result `query_timeout` |
| Encoded URL | 8,192 bytes total | refused on load with the oversize notice; saving to a board or saved exploration has no URL limit |
| Funnel steps / journey steps | 8 / 7 | refused: `report_invalid` |
| Funnel window | 60 s to 30 days | refused: `report_invalid` |
| Attribution conversions | 4, top 100 touch values each | refused: `report_invalid` |
| Retention intervals | 32 for day cohorts, 53 for week cohorts | refused: `report_invalid` |
| Retention cells | cohort periods in range × intervals ≤ 3,000 | refused: `report_too_large` |
| Session list columns | 16 | refused: `report_invalid` |
| Goals per goals report | 8 | refused: `report_invalid` |

Limits live in the generated protocol as constants so the Vue client can validate before sending.

### Amendment: glob matching and funnel v2

Adopted in [0022](0022-agent-report-queries.md) on 2026-10-27, which holds the full rules.

- String operators gain `matches` and `not_matches` with a glob value: `*` any run without `/`, `**` any run, `\` escapes. Whole value, case-sensitive, 1 to 500 bytes, at most 8 wildcards. `not_matches` is the exact complement of `matches`, like the other negative operators.
- `Funnel` v2 adds `group_by?: Field`: an identity's group is the value of its entering step-1 event, at most 50 groups by step 1 reached, `groups_truncated` beyond that. v1 funnels stay valid.
- Every funnel result carries step durations: p25, median and p75 milliseconds from the previous step and counts in the buckets `< 1 h`, `1–24 h`, `1–7 d`, `7–30 d`, `≥ 30 d`, plus step 1 to the last step.
- A report query allows 1,000 items in an `in` or `not_in` list. The 50-item limit below stays for exploration states.

### Timestamp conventions

- Storage: UTC instants, millisecond precision, both `event_time` and `receive_time`. Tracker `timestamp` (integer seconds) is multiplied by 1000.
- Wire: RFC 3339 with explicit offset, milliseconds always present. Server output uses `Z`: `2026-10-03T07:15:30.000Z`. Input accepts any offset and converts.
- Local dates: `YYYY-MM-DD` with the state's `timezone`.
- Resolved bounds in responses: `{ "start": instant, "end": instant, "compare_start"?, "compare_end"? }`, half-open.

### Encoding conventions

- JSON, UTF-8, no BOM.
- Integers with `|n| <= 2^53 − 1` are JSON numbers. Larger integers are decimal strings. Counts in this product fit in numbers; the rule exists for property values and minor units.
- Decimals (property decimals, CLS, revenue display amounts) are strings with fixed scale, e.g. `"12.5000"`, `"0.0412"`. Ratios (bounce rate, relative change) are JSON numbers rounded half to even to 4 decimals, since they are display values, not exact facts.
- Revenue is `{ currency, exponent, minor_units, amount }`: `minor_units` is an integer under the rule above (a decimal string beyond 2^53 − 1), `amount` the decimal string with `exponent` fractional digits, `"49.91"` or `"4500"`.
- `null` with a sibling `reason` field marks unavailable values ([0003](0003-metric-formulas.md)).
- Result cell keys are a `Value`, `{"t":"null"}` for a present JSON `null`, or `{"t":"not_set"}` for no value. In tie-breaks and dimension sorting, `(null)` sorts after every value and not set sorts last. A drill-down on the not-set cell appends `is_not_set` (include) or `is_set` (exclude).
- Canonical form for hashing and query keys: RFC 8785 (JCS): object keys sorted by UTF-16 code units, no whitespace, shortest number form. Query key = JCS of the state with `range` replaced by the resolved bounds, plus authorization scope and dataset watermark. Saved explorations and golden-result hashes use JCS of the state as given.
- URL form: `?s=<base64url without padding of the JCS state>`. One parameter keeps Back and Share reliable and the 8 KiB limit easy to check. Human-readable duplication of fields in the URL is not done.

### Complete example: the acceptance workflow

Last 30 days, mobile traffic, `signup` events with `plan = pro`, split by country and referrer, compared with the previous period. Drilling into Austria appends one condition (`id: "f4"`).

```json
{
  "v": 1,
  "site": "0d2c9a6e-5a3b-4f1e-9b1c-7e2f1a4d8c01",
  "measures": ["events", "visitor_days"],
  "dimensions": ["country", "referrer_domain"],
  "filter": {
    "kind": "group",
    "op": "and",
    "items": [
      { "kind": "cond", "id": "f1", "field": "device", "cmp": "eq", "value": { "t": "string", "v": "mobile" } },
      { "kind": "cond", "id": "f2", "field": "event_name", "cmp": "eq", "value": { "t": "string", "v": "signup" } },
      { "kind": "cond", "id": "f3", "field": { "scope": "event", "key": "plan" }, "cmp": "eq", "value": { "t": "string", "v": "pro" } },
      { "kind": "cond", "id": "f4", "field": "country", "cmp": "eq", "value": { "t": "string", "v": "AT" } }
    ]
  },
  "range": { "kind": "relative", "unit": "day", "count": 30 },
  "timezone": "Europe/Vienna",
  "granularity": "day",
  "compare": "previous_period",
  "metric_version": 1,
  "sort": [{ "by": "events", "dir": "desc" }],
  "page": { "size": 50, "index": 0 },
  "visualization": "table"
}
```

Resolved by the server on 2026-10-03 in Europe/Vienna:

```json
{
  "start": "2026-09-03T22:00:00.000Z",
  "end": "2026-10-03T22:00:00.000Z",
  "compare_start": "2026-08-04T22:00:00.000Z",
  "compare_end": "2026-09-03T22:00:00.000Z",
  "metric_version": 1,
  "session_policy_versions": [3],
  "watermark": 184233011,
  "correction": "complete"
}
```

The query suite instance Q03 uses this state without `f4` for the breakdown case and with `f4` for the drill-down case.

## Rejected alternatives

- Readable per-field URL parameters (Kit style): nice for simple tables, but a nested filter tree does not fit and two encodings invite drift. One `s` parameter wins.
- Cursor pagination: unnecessary when results come from one snapshot and rows are capped at 10,000.
- Absolute instant ranges in the state: local-date ranges cover the product use; instants appear in the resolved response.
- Floating-point revenue or decimals in results: exact values must survive JavaScript parsing.
- A `v: 2` state for report settings: it would force a migration of every saved plain exploration to add fields they do not use. An optional tagged part keeps old instances valid.
- One generic `parameters` object: untyped settings cannot be validated before sending or migrated per kind.
- Property statistics as a report kind: they are measures over the same rows and want dimensions, comparison and sorting like any other measure.
- Comparison for report kinds in v1: no confirmed workflow needs it, and refusing it visibly is cheaper than a second result shape per kind. Adding it later is a new kind version.
- `previous_year` comparison in v1: not in the acceptance workflow; adding a variant later does not break `v: 1` since `compare` is a string enum.

## Consequences

- `crates/protocol/src/exploration.rs` implements this shape and the limits as constants; TypeScript is generated from it.
- The query suite author writes Q01 to Q14 as JSON instances of this state plus resolved bounds and expected hashes. Funnel, journey, attribution, retention, session-list and numeric-summary cases move their settings from `parameters` into `report` or a `PropertyStat` measure.
- Changing any field meaning requires `v: 2` and a migration for saved explorations.

## Affected contracts

[analytics-contract.md](../analytics-contract.md) "Typed exploration"; [performance-validation.md](../performance-validation.md) query suite; protocol crate (milestone 03); workload manifest.
