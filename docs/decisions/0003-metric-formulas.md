# 0003: Metric formulas

Status: Adopted (milestone 01, team decision). This record settles every row of the "Formula decisions" table in [analytics-contract.md](../analytics-contract.md) plus the behavioral and identified reports. All formulas are `metric_version = 1`. Clarifications made during milestone 01, before anything computes results, stay at version 1. Identity terms come from [0001](0001-identity-and-consent.md), event and session terms from [0002](0002-sessions-and-events.md).

## Problem

Query compilation, the fixture oracle, and the dashboard need formulas with a fixed population, time bound, exclusion, grouping, and denominator, so results from different code paths can be compared exactly.

## Constraints

Confirmed: exact counts over stored events, no hidden sampling; visitor-days across days rather than a fake deduplicated audience; duration labeled as observed span; person-level retention and cohorts only on the consented population with coverage shown; currencies kept apart; zero denominators displayed as unavailable; previous-period comparison in the acceptance workflow.

## Evidence

Upstream Umami 3.4.0 counts "visitors" as distinct monthly-salted session IDs and "visits" as 30-minute receive-time visits; bounce is visits with one pageview; time on site is the span of the visit. Those are the formulas we replace; they are listed to keep compatibility tests separate from metric tests. The tracker fires a pageview on every SPA navigation, so pageview counting needs no special SPA rule.

## Common terms

- Range `[start, end)`: resolved instants from the exploration state (see [0004](0004-exploration-state-and-limits.md)), half-open.
- "In range" for an event: `start <= event_time < end`. For a session: `start <= session_start < end` (start mode). Funnels, journeys, and goals additionally use event mode where noted.
- Exclusions everywhere: suppressed bot traffic is never stored; refused payloads are never stored. Link and pixel events are excluded from website reports unless the source is the link or pixel itself.
- Grouping: any dimension or typed property in the state. A visitor or session that has several values of a split dimension appears in each cell, so shares do not sum to 100%.
- Not set: rows where a grouped field has no value (missing property key, no referrer, no UTM source) form one "not set" cell, encoded `{"t":"not_set"}` ([0004](0004-exploration-state-and-limits.md)). A property that is present with JSON `null` forms its own `(null)` cell. Both cells sort and page like any other cell, so for a single-valued field the cells add up to the ungrouped total, and include and exclude drill-downs on a cell partition it.
- Per-event and per-session fields: [0004](0004-exploration-state-and-limits.md) Field scope says which dimensions come from the event and which from its session. Grouping events by a session field uses the value of the event's session.
- Denominator for a share: the ungrouped total of the same measure in the same snapshot.
- Identity coverage: keyless events ([0001](0001-identity-and-consent.md) Key retention and keyless events) have no daily pseudonym and no anonymous session. Every measure that needs a daily pseudonym or a session (visitors, visitor-days, sessions, bounce, span, realtime, daily funnels, session journeys, daily attribution, daily goals) uses only events that have one, and its result carries `identity_coverage` = eligible events in range with a daily pseudonym or profile session / all eligible events in range, with both counts. Pageviews, events, revenue, percentiles, property statistics and hits count every stored event. With coverage below 1 the UI says how many events had no identity. Person measures keep their existing consented coverage.

## Formulas

| Measure | Population | Formula | Denominator | Notes |
| --- | --- | --- | --- | --- |
| Pageviews | `pageview` events in range | count | n/a | hash-only navigations are not pageviews unless the tracker sent one |
| Events | `custom_event` events in range matching predicates | count | n/a | event name and property filters are exact typed matches |
| Visitors (single identity day or shorter) | eligible events in range | count distinct daily pseudonym | n/a | shown as "visitors" only when the range lies inside one identity day (see below) |
| Visitor-days | eligible events in range | count distinct daily pseudonym | n/a | a pseudonym exists for one identity day only, so this equals the sum over identity days of distinct pseudonyms per day; shown as "visitor-days" for any range not inside one identity day; a partial day counts the identities seen so far |
| Identified people | eligible events in range linked to a persistent profile | count distinct profile ID | n/a | shown with coverage: linked eligible events / all eligible events in range |
| Sessions | sessions with start in range | count | n/a | pixel and link hits never form sessions |
| Bounce rate | sessions in range with at least one pageview | sessions with exactly one pageview and zero custom events / sessions with at least one pageview | as stated | any custom event is engagement; no per-site engagement configuration in v1 |
| Observed span, mean | sessions in range with at least one pageview | sum of (last − first eligible event time) / session count | sessions with at least one pageview | single-event span is 0 and is included; spans are integer milliseconds and the mean is rounded half to even to a whole millisecond, rendered as a JSON integer |
| Observed span, median | same | exact nearest-rank p50 of spans | same | |
| Realtime active | eligible events whose activity time lies in `[now − 5 min, now]` | count distinct daily pseudonym | n/a | see Realtime below |
| Conversion (goal) | see Goals | completions / eligible population | stated per goal | |
| Revenue | custom events in range with a valid revenue record | per currency: gross = Σ positive minor units, refunds = Σ negative, net = Σ all, count = records with amount ≠ 0 | n/a | no currency conversion; amounts rendered as decimal strings |
| Percentiles | stored observations in range | nearest-rank order statistic, see below | n/a | |
| Property statistic | `integer` and `decimal` rows of one property key on events in range (event scope) or on sessions in range (session scope), matching the filter | `count` = numeric rows; `sum`, `min`, `max` exact; `mean` = sum / count rounded half to even to 4 places | `count` for `mean` | other types of the same key are not rows of this measure; with `count = 0`, `min`, `max` and `mean` are `null` with `reason: "no_values"` and `sum` is `"0.0000"`; `sum`, `min`, `max`, `mean` are decimal strings with 4 places |
| Hits (link or pixel source) | `link_event` or `pixel_event` rows of the source in range | count | n/a | see Links and pixels |

### Visitors versus visitor-days

An identity day is one local date of the site timezone within one timezone epoch ([0001](0001-identity-and-consent.md)). The headline label depends on the resolved range: when it lies inside one identity day, "visitors"; otherwise "visitor-days". The number is the same count of distinct daily pseudonyms either way; only the label changes.

Series count distinct daily pseudonyms per bucket of the exploration timezone, at any granularity. The headline is the distinct count over the whole range, never a re-sum of the series.

- When the exploration timezone is the site timezone in effect for every day in range, every identity day falls in exactly one day bucket, so the day series sums to the headline.
- Otherwise (another exploration timezone, or a range spanning a site timezone change) one identity day can touch two day buckets and count in both. The headline still counts it once, so the series can sum to more than the headline, and the result carries `series_sum_differs: true` for the UI to say so. The flag is set only when the returned series actually sums to more than the headline.

### Percentiles

Observations sorted ascending, `n >= 1`. Rank `r = ceil(p × n)` (1-based), value is the r-th observation. No interpolation; every reported percentile is a real stored value. `n = 0` is unavailable. Missing metric values (performance payload without `lcp`) are excluded; explicit zeros are observations. Performance metrics `lcp, fcp, ttfb, inp` are integer milliseconds; `cls` is a decimal with 4 places. Reported percentiles: p50, p75, p95. Grouping by device, browser, page, country follows the common rule. The oracle uses the same rank definition; tolerance applies only to decimal rendering.

### Realtime

- Activity time of an event = `min(event_time, receive_time)`. Requiring activity time inside a window is the same as requiring both times inside it, and it places an event from a fast client clock (any event time ahead of receive time is accepted, [0002](0002-sessions-and-events.md)) at its receive time.
- Active: eligible events with activity time in `[now − 5 min, now]`, both ends inclusive, where `now` is the application clock ([0006](0006-retention-clock-and-operations.md)). Count distinct daily pseudonyms. Upstream counts `created_at >= now − 5 min`.
- Series: 30 one-minute buckets aligned to UTC minute boundaries. The last bucket is the current partial minute `[floor_minute(now), now]`; bucket `i` (0 to 29) starts at `floor_minute(now) − (29 − i)` minutes. Each bucket reports pageviews and distinct daily pseudonyms of eligible events whose activity time falls in it. Every timezone offset in current use is a whole number of minutes, so UTC minute buckets are also local minute buckets. Upstream's realtime series also starts 30 minutes before the start of the current minute.
- A late event received now with an event time 20 minutes ago lands in its past bucket, so past buckets can grow. It is not active, because its activity time is older than 5 minutes.

### Links and pixels

A report whose source is a link or pixel ([0004](0004-exploration-state-and-limits.md) `source`) counts that source's `link_event` or `pixel_event` rows.

- Measures: `hits` (count), `visitors` and `visitor_days` (distinct daily pseudonyms; identity days are UTC days for these sources, [0001](0001-identity-and-consent.md)).
- Dimensions: `referrer_domain`, `referrer_path`, `country`, `region`, `city`, `browser`, `os`, `device`, `language`, taken from the hit's own request, since hits have no sessions.
- Bot hits are suppressed, not stored. Hits that could not be stored during a storage outage are counted per source ([0005](0005-collection-admission-policy.md)); the report shows that `lost_hits` count for the range next to the total, so an outage is visible instead of looking like a traffic drop.

### Comparison and zero baselines

Previous period = the range of equal length ending at `start`, `[start − (end − start), start)`, resolved in the exploration timezone so a 30-local-day range compares against the 30 local days before it. When the range contains the application clock's now, totals compare only up to the same offset into each span: today until 15:20 against yesterday until 15:20 (adopted by Mike on 2026-10-15). Series points still cover the whole previous span. Change = `current − previous`; relative change = `(current − previous) / previous`.

Typical weekday (`typical_weekday`) compares with the median of the same local weekday over 8 weeks, as [0015](0015-baselines-projections-and-noticed-changes.md) defines. The zero-baseline rules below apply to it unchanged.

- `previous = 0` and `current > 0`: relative change is `null` with `reason: "zero_baseline"`; the UI shows "new".
- both 0: change 0, relative change `null` with `reason: "zero_baseline"`.
- Any rate whose denominator is 0: value `null`, `reason: "no_denominator"`.

Never emit infinity, NaN, or an invented 100%.

### Goals

A goal is a versioned definition: predicate (event name, event property, or path match), scope `session` or `person`, counting `once` or `every`. Default scope session, counting once.

- Scope session: completions = sessions in range with at least one matching event (once) or matching events in sessions in range (every). Denominator = sessions in range.
- Scope person: consented population only. Completions = profiles with a matching event in range (once) or matching events (every). Denominator = profiles with an eligible event in range. Coverage is shown.
- Editing a goal creates a new goal version; saved reports keep the version they reference.

### Funnels

- Definition: 2 to 8 ordered steps, each a predicate; conversion window 1 minute to 30 days from the step-1 event; scope `daily` (default) or `person`.
- Identity: daily pseudonym for `daily`, persistent profile for `person`.
- Entry: an identity enters once per range, at its first step-1 event in range (event mode: step-1 event in range; later steps may fall after `end` but within the window).
- Progress: step k+1 is the first event matching its predicate strictly after the step-k event in `(event_time, ingestion_sequence)` order and within the window from step 1. The window is half-open: `t1 <= event_time < t1 + window`, so an event exactly one window after step 1 misses. Every Holocron interval is half-open, so a 10-minute funnel window and a 30-minute inactivity gap read the same way (upstream's funnel uses an inclusive `between`). Repeated predicates need distinct events. Simultaneous events are ordered by ingestion sequence; there are no ties.
- Results: count of identities reaching each step; conversion = step n / step 1; step drop = step k − step k+1.
- Daily scope cannot cross local midnight because the pseudonym changes; the result states this and the UI shows the limitation. Person scope spans days.
- Execution: one ordered pass per identity with bounded per-identity event lists; no unbounded self-join.

### Journeys

- Entry population: sessions with start in range, or sessions of consented profiles when scope is `person`. A pageview journey only includes sessions with at least one pageview, and an event journey only sessions with at least one custom event.
- Step value: page path for pageview journeys, event name for event journeys. Sequence length 2 to 7.
- Loop compression: consecutive equal values collapse (`A A B` is `A B`). Non-consecutive repeats stay. Compression runs on the whole session before the step limit, so the limit counts compressed steps: `A A B C` with length 2 is `A B`.
- Terminal: session end or step limit, whichever comes first; a sequence shorter than the limit is reported with its actual length. One-step sessions are reported as one-step sequences, so the counts of all sequences add up to the entry population.
- Output: top 100 sequences by session count, plus the total number of distinct sequences. Ties compare sequences element by element, each value in UTF-8 byte order, and a sequence that is a prefix of another sorts first. No separator character is involved.

### Attribution

- Touch: a session's first pageview, or its first eligible event when it has no pageview ([0004](0004-exploration-state-and-limits.md) session fields), carries referrer domain, UTM fields, and click IDs. Direct = no referrer domain, no UTM source, no click ID. Internal = referrer domain equals a site hostname after `www.` stripping; internal touches are ignored.
- Models: `last_touch` (default) and `first_touch`. Lookback 30 days, fixed in v1.
- Anonymous scope: the only sessions an identity can reach are within one local day, so anonymous attribution is effectively per identity-day: the last or first non-direct touch among that day's sessions before the conversion; otherwise direct.
- Person scope (consented): touches across sessions within 30 days before the conversion event.
- Conversion: a goal completion or a revenue event. Revenue attribution keeps currencies apart. Campaign breakdowns group by `utm_campaign` exactly as sent (trimmed, no case folding).
- UTM values: taken from the page URL after percent-decoding and `+` as space (as upstream's `searchParams.get`), then trimmed. An empty value after trimming (`utm_campaign=`) is the same as missing, the "not set" cell. No Unicode normalization and no case folding, so `June`, `june` and a decomposed `é` stay different values, exactly as sent. Internal whitespace is kept. Values are truncated at 255 Unicode scalar values, upstream's column length.
- Conversion event: for a goal counted `every`, each matching event. For a goal counted `once`, the first matching event in `(event_time, ingestion_sequence)` order within the counted unit: the session for scope `session`, the profile's range for scope `person`. For revenue, each event with a revenue record. Touches are looked up before that event's time.

### Channels

`channel` is a session field computed from the session's touch (its first pageview, or its first eligible event when it has no pageview: referrer domain, page query, `utm_medium`, `utm_source`) and the site's hostnames. Holocron keeps upstream's taxonomy so Umami operators see familiar groups. Rules apply in order and the first match wins. Matching is case-insensitive substring, as upstream.

| Order | Channel | Rule |
| --- | --- | --- |
| 1 | `paidAds` | page query contains a `PAID_AD_PARAMS` entry |
| 2 | `referral` | `utm_medium` contains `referral`, `app`, or `link` |
| 3 | `affiliate` | `utm_medium` contains `affiliate` |
| 4 | `sms` | `utm_medium` or `utm_source` contains `sms` |
| 5 | `llm` | referrer domain contains an `LLM_DOMAINS` entry |
| 6 | `paidSearch`, `organicSearch` | referrer domain contains a `SEARCH_DOMAINS` entry, or `utm_medium` contains `organic` |
| 7 | `paidSocial`, `organicSocial` | referrer domain contains a `SOCIAL_DOMAINS` entry |
| 8 | `email` | referrer domain contains an `EMAIL_DOMAINS` entry, or `utm_medium` contains `mail` |
| 9 | `paidShopping`, `organicShopping` | referrer domain contains a `SHOPPING_DOMAINS` entry, or `utm_medium` contains `shop` |
| 10 | `paidVideo`, `organicVideo` | referrer domain contains a `VIDEO_DOMAINS` entry, or `utm_medium` contains `video` |
| 11 | `referral` | any other referrer domain that is not a site hostname after `www.` stripping |
| 12 | `direct` | everything else, including no referrer and internal referrers |

- Paid or organic: paid when `utm_medium` contains `cp`, `ppc`, `retargeting`, or `paid`. This is upstream's ClickHouse rule; its PostgreSQL path tests `LIKE 'p%'`, which would mark `partner` as paid, and is not adopted.
- Domain and parameter lists are copied verbatim from `src/lib/constants.ts` at the pinned commit. Changing a list changes results and bumps `metric_version`.
- Upstream picks the first non-empty channel among a visit's events. Holocron classifies the session's touch, the same touch attribution uses, so a session has exactly one channel. The results differ only when a session's first pageview has no touch but a later event has one.

### Retention and cohorts

- Population: consented persistent profiles only.
- Cohort entry: a profile's first eligible event on the site (any time) falls in the cohort period (local day or local week starting Monday, in the exploration timezone).
- Activity: at least one eligible event in the interval.
- Intervals: calendar offsets from the cohort period (day 0, day 1, …, or week 0, week 1, …), not elapsed time. Day 0 is the cohort period itself and is always 100%.
- Rate = active profiles in interval / cohort size. Cohort size is the denominator shown.
- Incomplete interval: an interval whose end is after the result watermark (the request's application clock; ingestion lag is reported separately by freshness) is reported with `complete: false` and excluded from averages. It still shows its current value, which is 0 for an interval that has not started.
- Rows: every cohort period in range is a row, also with cohort size 0. Every requested offset is a column. A size-0 cohort has `null` rates with `reason: "no_denominator"`.
- Average per offset: weighted by cohort size, `Σ active / Σ cohort size` over cohorts whose interval at that offset is complete. With no complete cohort the average is `null` with `reason: "no_denominator"`. Weighting by size keeps the average equal to the rate of the pooled population, and small cohorts do not swing it.
- Coverage: consented eligible events / all eligible events in the cohort period, shown beside the table.
- Cohort predicates (grouped AND/OR) filter cohort entry on the profile's events in the cohort period.
- Imported source identities are a separate population shown with their own limitation note; they never join this table.

### Reporting rules confirmed by Mike (2026-10-06)

Milestones 07 and 08 built these where the formulas above were silent:

- A pageview step or goal is `path` plus `event_name` not set, because custom events carry their page's path.
- An internal referrer counts as no referrer. A touch with an internal referrer but a UTM source or click ID is still a touch.
- Session goals and session-scope attribution conversions use sessions starting in range (start mode), so attribution totals equal goal completions. Conversions without the scope's identity are left out and shown in coverage.
- A weekly cohort period is the whole Monday week the range touches, also when the range starts mid-week. Cohort predicates do not narrow coverage.
- Sorting by a revenue amount orders by currency first ([0004](0004-exploration-state-and-limits.md)), and goal conversion is the `goals` report kind on the exploration path.

## Rejected alternatives

- Linear interpolation (type 7) percentiles: smoother but every value would be synthetic; nearest-rank matches the validation procedure and the oracle.
- "Visits" as a 30-minute receive-time window (upstream): mislabels late events.
- Configurable engagement events for bounce in v1: more state and another policy version for little gain; custom events already signal engagement.
- Monthly deduplicated "unique visitors" for anonymous traffic: impossible under daily keys; visitor-days is the honest number.
- Elapsed-time retention intervals: harder to read next to calendar reports and not what operators expect from Umami.
- Visitor-days as a sum of exploration-timezone day buckets: a visitor whose identity day touches two buckets would count twice in the headline. The distinct count over the range is the same number when timezones agree and stays honest when they differ.
- Negative operators that skip missing keys: an exclude drill-down would then drop rows that were never in the excluded cell, and include plus exclude would not add up to the total.
- Unweighted retention averages: one tiny cohort would move the average as much as a large one.

## Consequences

- The oracle implements these formulas independently; the small fixture explanation cites this record by section.
- The field catalog must mark whether a dimension is per event or per session, since grouping sessions by an event dimension multiplies rows.
- `metric_version` starts at 1; any edit to this file that changes a result bumps it and is recorded here.

## Affected contracts

[analytics-contract.md](../analytics-contract.md) "Formula decisions" and "Behavioral and identified reports"; fixture manifest; query suite Q01 to Q12.
