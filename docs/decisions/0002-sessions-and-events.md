# 0002: Sessions, events, properties, and versions

Status: Adopted (milestone 01, team decision) unless an item says otherwise. Items marked Confirmed (Mike, 2026-10-03) were decided by Mike on that date. Confirmed items restate [the briefing](../technical-briefing.md). Identity rules come from [0001](0001-identity-and-consent.md).

## Problem

Ingestion, session derivation, correction, and the property catalog need one set of rules for which events count, when a session starts and ends, how late and duplicate events behave, and how typed properties and revenue are stored.

## Constraints

Confirmed:

- Configurable inactivity window per website, default 30 minutes. Anonymous sessions close at identity rotation.
- Confirmed (Mike, 2026-10-03): live payloads may carry any timestamp, matching upstream. Event time and receive time are stored separately.
- Events are counted exactly. Recorder chunks must not create engagement.
- Client event IDs are optional; unchanged trackers may deliver duplicates after a lost response.
- Revenue uses integer minor units or bounded decimals, never float accumulation.
- Policy versions are stored; recalculation is explicit.

## Evidence

- `send/route.ts`: `type` is `event | identify | performance`. `event` becomes pageview (no `name`), custom event (`name`), link event, or pixel event by source. Upstream starts a new visit when `now - iat > 1800` seconds, measured on receive time and only for untimestamped payloads. Performance payloads carry `lcp, inp, cls, fcp, ttfb` (ms, `cls` max 100). `timestamp` is integer seconds.
- `record/route.ts`: `record` and `heatmap` payloads attach to the session via the cache token; they carry no event name.
- `tracker/index.ts`: `pushState` and `replaceState` with a changed URL send a pageview after 300 ms; `identify` resends the base payload with `type: "identify"`; performance flushes once per page on hide, pagehide, 10 s timer, or navigation.
- `lib/data.ts`: event data is flattened with dot keys; types are string, number (`toFixed(4)`), boolean, date (RFC 3339 regex match), array (JSON string, dropped when over 500 chars). Strings truncate at 500 chars, keys at 500. Revenue is `eventData.revenue > 0 && eventData.currency` (`saveEvent.ts`), currency truncated to 10 chars.

## Choice

### Eligible event kinds

| Payload | Stored kind | Session eligible | Notes |
| --- | --- | --- | --- |
| `event` without `name`, website source | `pageview` | yes | SPA `pushState`/`replaceState` navigations are pageviews, matching the tracker |
| `event` with `name`, website source | `custom_event` | yes | engagement by definition |
| `event`, link source | `link_event` | no | counted per link; no session derivation |
| `event`, pixel source | `pixel_event` | no | counted per pixel; no session derivation |
| `identify` | `identify` | no | updates identity and session properties only |
| `performance` | `performance` | no | attached to the session active at its event time, else unattached |
| `record` fragments, `heatmap` interactions | not events | no | attached by capture ID; never create or extend sessions |

Only eligible events start, extend, or close sessions and only they count toward duration, bounce, and realtime.

### Timeout rule

Adopted as recommended: within one identity, order eligible events by `(event_time, ingestion_sequence)`. A gap `>= window` between consecutive eligible events starts a new session. A gap `< window` extends the current one. Default window 30 minutes, configurable 1 to 1440 minutes per site. A one-event session has span zero.

Gaps are measured on event time, not receive time. Upstream's receive-time visit expiry is not adopted because it mislabels late events.

### Identify, performance, and recorder messages

- `identify` sets session properties on the session open at its event time (latest value wins by the same ordering). With no open session, the properties attach to the next session of that identity on that day, or are kept as identity-day properties if none follows. It never changes session boundaries.
- `performance` joins the session of the same identity whose interval `[first eligible event time, last eligible event time + window)` contains its event time (pageview path match is not required). The tail matters because the tracker flushes performance on page hide, after the last pageview. Sessions of one identity never overlap under this interval, since the next session starts at least one window after the last event. For anonymous and daily identities the tail cannot cross midnight, because the next day has another pseudonym. Unattached performance rows remain queryable by page and device.
- Recorder fragments and heatmap interactions reference `capture_id` only. Session rederivation does not move, split, or lose them.

### Equal timestamps

Total order is `(event_time, ingestion_sequence)` where `ingestion_sequence` is the SQLite inbox acceptance position, immutable once assigned. Imported events use the importer's assigned sequence. Equal event times therefore never tie; funnel step order and "first event" are deterministic.

### Midnight, DST, timezone change

- Anonymous and daily-identified sessions close at the end of the local day in the site timezone. The first eligible event on the next local day starts a new session with the new pseudonym. Consented persistent sessions ignore midnight (0001).
- Day boundaries are computed from event time with IANA rules. On a spring-forward day the local day is 23 hours long; on a fall-back day 25 hours. A gap measured across a transition uses instants, so the inactivity window is unaffected.
- A site timezone change has an effective instant (UTC, set to the change time). Events with `event_time < effective` keep the old timezone for day boundaries; later events use the new one. History is not recalculated: old day keys may already be destroyed, so old pseudonyms cannot be rederived. The change bumps the site session policy version. A report spanning the instant shows both versions in its snapshot metadata.
- The effective instant is an identity rotation ([0001](0001-identity-and-consent.md) Timezone change). Anonymous and daily-identified sessions open at that instant close there, and the next eligible event starts a new session under the new pseudonym, even when the local date did not change. Consented persistent sessions continue across it, as they do across midnight.

### Event time

Confirmed (Mike, 2026-10-03): no payload is refused because of its event time. This reverses the earlier 24-hour and 5-minute window. The consequences below are Adopted.

- Payloads without `timestamp` take `event_time = receive_time`. `timestamp` is integer seconds, as upstream; a value whose millisecond form does not fit a signed 64-bit integer is a schema error (400), because it cannot be stored, not because of a time cap.
  - Confirmed (Mike, 2026-10-04, milestone 04): the same 400 also covers a timestamp outside the range both engines store as a timestamp (chrono's range, about ±262,000 years, less one day at each end so the next local day still exists). Such a value fits an i64 but cannot be stored either.
- An event older than the 12-month retention cutoff when it arrives is not stored: outcome `suppressed`, reason `outside_retention`, acknowledged with the normal stored-event body and counted ([0006](0006-retention-clock-and-operations.md)). This is the import rule, and storing a row that queries already filter out and the next retention run deletes would only cost writes.
- A late event whose identity day key is destroyed is stored keyless: counted exactly in pageviews and events, absent from identity-based measures, and visible in identity coverage ([0001](0001-identity-and-consent.md) Key retention and keyless events).
- Corrections are unbounded in the past within retained data. Any accepted event can mark any identity day and session range of the last 12 months (or any future day) dirty for deterministic recomputation; the late event cannot change an earlier session's membership except by the ordering rule above. Freshness and correction state are reported per result, so a late event never makes a result claim more than it knows.
- Events more than 5 minutes ahead of receive time are stored as sent and counted in the diagnostics counter `event_time_ahead`. Known risk: a far-future row (upstream read a millisecond value as seconds and stored year 58692) is never reached by the 12-month retention cutoff and stays until site deletion or an explicit erasure. No cap is set; operations status shows the counter so an operator can find a misconfigured client.

### Client event-ID deduplication

- Field: payload `eventId`, string, 1 to 64 printable ASCII characters. Optional. Not sent by unchanged trackers.
- Scope: `(source_id, eventId)` where source is the website, link, or pixel.
- Window: 48 hours from the first receive time. The dedup index lives in SQLite alongside the inbox and is pruned with the same job as day keys.
- Collision: the first accepted event wins. A later payload with the same ID is acknowledged with outcome `duplicate` and not stored, even when its payload differs. The wire response is the unchanged stored-event body ([0005](0005-collection-admission-policy.md)). Differing payloads increment a `dedup_conflict` counter; they are not merged and not errors, because a retrying client cannot act on an error.
- Without `eventId` a retried request after a lost response is a separate stored event. Documentation says so.

### Request idempotency and batch IDs

Three mechanisms, kept apart:

| Mechanism | Scope | Protects against |
| --- | --- | --- |
| `eventId` (above) | client to inbox | browser or SDK retries |
| Dashboard idempotency key | principal + command + argument fingerprint | dashboard command retries |
| Stable batch ID and applied-batch ledger | inbox to DuckDB | internal transfer replay |

Dashboard idempotency stores only committed results. A command that fails (validation, `stale_revision`, authorization, internal error) commits nothing and stores nothing under its key, so a retry with the same key runs again and may now succeed. Storing failures would pin a transient error such as a stale revision to the key for its whole lifetime. A key reused with different arguments after a stored success is still an explicit conflict.

`/api/send` and `/api/batch` have no request-level idempotency key. A batch of 500 items is admitted item by item with per-item outcomes, matching upstream's result shape; the accepted items of one request share one durable commit ([0005](0005-collection-admission-policy.md)).

### Property type system

Properties come from custom event `data` and `identify` `data`. Objects are flattened with dot keys as upstream. Limits: at most 50 leaf properties per payload, nesting depth 5, key 500 bytes, string 500 characters (truncated). Beyond a limit the property is dropped and `property_dropped` counted; the event itself is still accepted.

Each stored property value has exactly one type:

| Type | Source JSON | Stored as | Rule |
| --- | --- | --- | --- |
| `null` | `null` | type marker only | distinct from missing |
| `bool` | `true`/`false` | boolean | |
| `integer` | number whose exact value is a whole number, within ±(2^63 − 1) | i64 | `1`, `1.0` and `1e3` are all integers; a whole number outside the i64 range is stored as `string` and counted in `property_coerced` |
| `decimal` | number whose exact value has a fractional part | DECIMAL(18,4) | round half to even to 4 places; magnitude over 10^14 is stored as `string` and counted in `property_coerced` |
| `date` | string matching upstream `DATETIME_REGEX` | UTC timestamp, ms | |
| `string` | any other string | text | |
| `array` | JSON array of scalars | array of the above scalar types | nested arrays or objects inside arrays are serialized to a `string` |

Numbers are classified and rounded from the JSON number text as an exact decimal, never through a binary float. So `0.12345` rounds to `0.1234` and `2.00001` is a decimal `2.0000` (it had a fraction before rounding). Classification is by value rather than by spelling because JavaScript serializes `1.0` as `1` while Python sends `1.0`; both should land in one group-by cell.

Missing means no row for that key on the event. Operators `is set` and `is not set` test row presence; `is null` tests the `null` type.

Mixed types across events for one key are allowed. Filters are typed: `equals "1"` matches string rows only, `> 1` matches integer and decimal rows only, `contains` on an array matches element equality with the typed value. Group-by groups on `(type, value)`, so `"1"` and `1` are different cells and the cell label shows the type when a key has more than one. The field catalog reports the types observed per key and their counts.

### Currencies and revenue

- Schema (unchanged from upstream): custom event `data.revenue` (JSON number) and `data.currency` (string). Both are also stored as ordinary properties.
- `currency` must be an ISO 4217 alphabetic code, uppercased; unknown codes mean the event carries no revenue and `revenue_invalid` is counted.
- Amount is stored as an integer in minor units using the ISO 4217 exponent (EUR 2, JPY 0, KWD 3). Values with more precision are rounded half to even to the minor unit, computed from the JSON number text as an exact decimal (so `19.995` EUR is 2000, not the 1999 a binary float gives). An amount whose minor units exceed ±(2^63 − 1) is invalid.
- Negative amounts are refunds and are stored. Zero is valid and is not a revenue record: it adds nothing to revenue and does not increment `revenue_invalid` (a free order is not an error). An amount that rounds to zero minor units (`0.004` EUR) is treated as zero. Non-numeric `revenue` is invalid and counted in `revenue_invalid`.
- Reports keep currencies apart. Gross = sum of positives, refunds = sum of negatives, net = sum of all. No conversion unless a later explicit conversion model is configured.

### Calculation and policy versions

| Version | Scope | Changes when |
| --- | --- | --- |
| `identity_policy_version` | per site, integer | persistent identity toggled, trusted proxy change, derivation construction change; stored on each event |
| `session_policy_version` | per site, integer | inactivity window or timezone change; stored on each session row with its effective instant |
| `metric_version` | global, integer, starts at 1 | any formula in [0003](0003-metric-formulas.md) changes; stored on query results, saved explorations, and board cards |

A saved exploration pins its `metric_version`. Opening it after a change shows both versions and offers to upgrade; it never silently recalculates.

Adopted (milestone 05), where the table above is silent:

- A gap is judged by the window in effect at its earlier event, and a session's attachment tail `[last, last + window)` by the window in effect at its last event. A session carries the `session_policy_version` in effect at its first event. A policy change does not split a session by itself; a timezone change does for anonymous and daily-identified sessions, because it rotates identity.
- A timezone change takes effect at an instant not before the time of the change. A window change may take effect in the past: that is the explicit recalculation, and it rebuilds every identity day with events from one maximum window (24 h) before the instant.
- Version 1 of a site applies to every event time. A site without a recorded policy is derived with the default 30-minute window as version 1 until its policy is recorded, which rebuilds its days.
- Recalculation is a job over a date range. It marks those identity days dirty, and correction rebuilds them under the current versions.
- A policy change takes effect within the retained 12 months and at most a year ahead. An unchanged policy records no version.
- `identity_policy_version` is kept per site in control storage. Storing it on each event, as the table says, waits until a derivation reads it; it needs an inbox format change ([milestone 05 evidence](../evidence/m05-identity-sessions-and-corrections.md), Open items).

### Policy ownership

Adopted (milestone 05). [0008](0008-storage-engine-and-budgets.md) gives sites and settings to `control.sqlite`, so control storage owns every policy fact: the session policy versions (`session_policies`: version, timezone epoch, effective instant, timezone, window), the persistent identity switch with its identity policy version, and consent intervals. `sites` has no timezone column of its own; the current session policy version is the site's timezone.

Analytics keeps a copy of the session policy versions, because correction joins every event against them. One function writes the copy: it compares the owner's versions with the copy, replaces the copy from the first difference, and marks dirty the identity days that difference can affect. A policy command runs it before its commit, so a report read right after the command sees the new version. Startup runs it for every site, which repairs a copy left behind by a crash or a failed commit between the two stores. Until that restart a failed commit leaves the copy one version ahead; only derived rows depend on it, and the repair rebuilds them.

Identity days, the end of a day, the key destruction instant and the retention cutoff come from one implementation (`SitePolicies` in `holocron-analytics`). Collection uses it to stamp records and decide keyless events, analytics to derive sessions.

Rejected: sending policy changes through the inbox as records. They would be ordered with events, but still not atomic with the control commit, and transfer would have to validate settings. Making the analytical copy the owner would make collection read DuckDB on every request.

## Rejected alternatives

- Receive-time timeouts (upstream): late events would extend the wrong session.
- Clamping a far-future or ancient event time to receive time: it would store a time the client never sent.
- Gap `> window` instead of `>= window`: equivalent for real traffic, but `>=` keeps the "exactly 30 minutes" fixture case unambiguous.
- Rejecting duplicate `eventId` with an error: retrying clients would log failures for successful stores.
- Treating link and pixel hits as sessions: a pixel has no navigation to measure.
- Floating-point or `DECIMAL` revenue: integer minor units match the oracle and avoid rounding drift.
- Recalculating history on timezone change: impossible for anonymous identity once day keys are destroyed, and would change saved reports silently.

## Consequences

- Storage needs typed property columns (one row per leaf, type-tagged), an `eventId` dedup index with 48-hour pruning, and per-site policy rows with effective instants.
- The small fixture must include: a gap of exactly 30 minutes, midnight for anonymous and consented identities, both DST transitions in Europe/Vienna, a timezone change, a late event whose day key still exists, a late event whose day key is destroyed (keyless), an event older than the retention cutoff (`outside_retention`), an event more than 5 minutes ahead (`event_time_ahead`), a duplicate `eventId` with a changed payload, mixed-type properties, and EUR, USD, GBP, JPY revenue with one refund.
- Collection compatibility matrix must record `eventId`, that timestamps are accepted as upstream, and the `outside_retention` suppression.
- Dirty-range tracking must cover any day of the retained 12 months, not only the last 24 hours.
- The large workload's split of 1,200,000 `purchase` events into 300,000 each of EUR, USD, GBP and JPY is adopted. It keeps the earlier 1,200,000 total, and JPY covers the zero-exponent case this record requires.
- The payload parser must keep JSON number text (for example `serde_json` with `arbitrary_precision`, or a raw-value pass for `data`) so typing and revenue rounding work on exact decimals.

## Affected contracts

[analytics-contract.md](../analytics-contract.md) "Event contract" and "Session derivation"; collection fixtures; storage schema (milestone 02); query suite Q04, Q05, Q10, Q12.
