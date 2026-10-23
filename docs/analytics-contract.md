# Analytics and identity contract

Read this document before implementing identity, sessions, reporting, historical import, or exploration types. Confirmed policy comes from [the technical briefing](technical-briefing.md). Choices within that policy are settled in the decision records under [docs/decisions](decisions/): [0001 identity and consent](decisions/0001-identity-and-consent.md), [0002 sessions and events](decisions/0002-sessions-and-events.md), [0003 metric formulas](decisions/0003-metric-formulas.md), and [0004 exploration state and limits](decisions/0004-exploration-state-and-limits.md), [0005 collection admission policy](decisions/0005-collection-admission-policy.md), [0006 retention, clock and operations](decisions/0006-retention-clock-and-operations.md), and [0007 access and account security](decisions/0007-access-and-account-security.md). Each section below carries a status: Confirmed (briefing), Adopted (milestone 01, team decision), Confirmed (Mike, 2026-10-03), or Needs Mike's decision. Where this file and a decision record differ, the record wins.

## Identity populations

Status: Confirmed policy. Derivation, proxy trust, key retention and tombstones, timezone-change rotation, the daily identity of linked events, keyless events and identity coverage, and the consent transport are Adopted in [0001](decisions/0001-identity-and-consent.md). Revocation keeping history and client-asserted consent are Confirmed (Mike, 2026-10-03).

Anonymous identity is a keyed, cookieless pseudonym scoped to one website and one local calendar day. Daily visitor counts are estimates of identities, not people. Across several days, expose visitor-days as the sum of daily distinct identities. Do not present that sum as a deduplicated monthly audience.

Transient inputs, key derivation (BLAKE3 keyed, random per-site per-day keys destroyed 25 hours after the local day ends), proxy trust, hostname normalization, missing inputs, and local-date calculation are settled in [0001](decisions/0001-identity-and-consent.md). Salt alone does not protect a small input space; use a secret keyed derivation. Keep site secrets out of exported report data and logs. Privacy wording must describe pseudonymization honestly.

Explicit modern `identify(id, data)` is accepted, with its ID hashed inside the daily site scope by default. Ordinary identity calls and event properties do not establish consent. A consented persistent mode needs an explicit client extension or trusted application integration with revocation semantics; the transport is the `consent` payload key defined in [0001](decisions/0001-identity-and-consent.md). Until that signal is present and persistent analytics are enabled for the site, remain in daily mode.

Persistent identity uses a separate key namespace. Reports distinguish anonymous visitor-days, identified profiles, and eligible identified coverage. Do not merge daily anonymous history into persistent profiles implicitly. Consented persistent sessions cross midnight; revocation or disabling the site setting closes open identified sessions at that instant ([0001](decisions/0001-identity-and-consent.md)).

Imported history retains restricted source identifiers as provenance. Source IDs do not reconstruct the new daily identity. Display imported identity coverage and calculation limits beside historical reports. Exact event counts remain possible even when visitor estimates are limited.

## Event contract

Status: Confirmed policy. Accepting any event time is Confirmed (Mike, 2026-10-03). Property types and number classification, the event-time consequences, deduplication, and revenue representation and rounding are Adopted in [0002](decisions/0002-sessions-and-events.md); admission outcomes, body caps and token binding in [0005](decisions/0005-collection-admission-policy.md); the retention cutoff in [0006](decisions/0006-retention-clock-and-operations.md).

Store immutable event identity, website/source, event and receive times, source provenance, event kind, typed dimensions/properties, policy versions, and ingestion sequence. Keep immutable facts separate from recalculated sessions and aggregates.

Use UTC instants for storage and the selected website timezone for calendar reporting. Missing, explicit null, string, boolean, integer, decimal, date, and array/object property support, and how mixed types across events filter and group, are defined in [0002](decisions/0002-sessions-and-events.md). Revenue is stored as integer minor units per ISO 4217 exponent, not floating-point accumulation.

Live payloads may carry any event time, as upstream; none is refused for its timestamp ([0002](decisions/0002-sessions-and-events.md)). An event older than the 12-month retention cutoff on arrival is not stored and is counted as `outside_retention`. A late event whose day key is already destroyed is stored without a daily pseudonym: it counts exactly in pageviews and events, is excluded from identity-based measures, and shows in identity coverage. Events more than five minutes ahead are counted in `event_time_ahead`; far-future rows are never reached by retention. Imports enter through their own authenticated offline path. Record receive time independently so visibility latency cannot be inferred from an old client timestamp.

Client event-ID deduplication is optional: payload `eventId`, scoped to the source, 48-hour window, first accepted wins ([0002](decisions/0002-sessions-and-events.md)). Scope request idempotency separately from stable transfer batch IDs. Unchanged trackers can deliver duplicate events after a lost response.

## Session derivation

Status: Confirmed policy; eligible kinds, timeout boundary, ordering, performance attachment, and midnight/DST/timezone rules are Adopted in [0002](decisions/0002-sessions-and-events.md).

Use a configurable sliding inactivity window per website, default 30 minutes. Accepted eligible events extend the session in event-time order. Close an anonymous session at local identity rotation. Delayed events can join or split derived sessions, so session IDs and report aggregates need deterministic correction behavior.

Adopted timeout rule: a gap greater than or equal to the configured window, measured on event time, starts a new session. Pageviews and custom events are the eligible kinds; identify, performance, link, pixel, and recorder messages never start or extend a session ([0002](decisions/0002-sessions-and-events.md)).

Equal timestamps are ordered by ingestion sequence, so there are no ties ([0002](decisions/0002-sessions-and-events.md)). Capture IDs remain stable even when a session is rederived. Session records carry a policy version; a site configuration change creates a new version rather than silently reinterpreting old saved reports.

Test local midnight, both DST transitions, clock offsets, one-event sessions, boundary gaps, multi-day windows, and timezone changes. A timezone change takes effect at an explicit instant and does not recalculate history ([0002](decisions/0002-sessions-and-events.md)).

Sessions, their members and day aggregates are derived data, rebuilt from immutable events by bounded, idempotent correction steps driven by dirty identity days. Every result carries its freshness: ingestion watermark, corrected-through batch, whether a dirty day can still change it, `metric_version`, and the session policy versions in range. The query `analytics.freshness` returns it for a site and range. Window-change semantics, recalculation (`recalculations.start`, `recalculations.status`) and policy ownership are Adopted in [0002](decisions/0002-sessions-and-events.md) Calculation and policy versions and Policy ownership; evidence in [milestone 05](evidence/m05-identity-sessions-and-corrections.md).

## Formula decisions

Status: Adopted in [0003](decisions/0003-metric-formulas.md) as `metric_version = 1`, including realtime buckets, channels, link and pixel measures, and property statistics. The table below is the original recommendation and is kept for orientation; 0003 holds each formula's population, time bounds, exclusions, grouping, denominator, and the resolution of every "required decision" column.

| Measure           | Recommended initial definition                                                                        | Required decision or limit                                                                     |
| ----------------- | ----------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| Views             | Count accepted pageview events in the selected interval                                               | Define event-kind mapping and SPA navigation behavior from modern tracker fixtures             |
| Events            | Count accepted custom events matching predicates                                                      | Keep event-name and typed-property filters precise                                             |
| Visitor-days      | Sum of distinct daily pseudonyms per site identity day                                                | Show partial-day behavior and imported limits; no cross-day anonymous deduplication            |
| Sessions          | Sessions whose start falls in the selected interval                                                   | Document alternate event-overlap selection as a separate mode, especially for funnels          |
| Bounce rate       | Sessions with one pageview and no configured engagement event divided by eligible pageview sessions   | Confirm whether engagement events exist and their site configuration; do not infer active time |
| Observed duration | Last minus first eligible event time, with single-event span zero                                     | State mean/median denominator; label recorded span rather than time actively spent             |
| Realtime          | Identities with an eligible event in a defined recent window                                          | Define the window and late-event exclusion; received-now does not mean active-now              |
| Conversion        | Qualifying session or identified-profile goal completions divided by its declared eligible population | State counting once versus repeatedly and session versus person scope                          |
| Revenue           | Sum valid accepted revenue amounts by currency after defined refund/deduplication rules               | Define event schema, orders, rounding, invalid values, and refund behavior                     |
| Percentiles       | Exact defined order statistic over stored observations initially                                      | If an approximate statistical method is later used, name it and expose its error contract      |

Rate comparisons need an explicit zero-baseline representation. Display unavailable instead of dividing by zero or inventing an infinite percentage. Totals and breakdown shares use the same denominator and result snapshot. A visitor may occur in several categories, so percentages do not necessarily sum to 100%.

## Behavioral and identified reports

Status: Confirmed policy; funnel, journey, attribution, goal, and retention formulas are Adopted in [0003](decisions/0003-metric-formulas.md), and their saved definitions in [0004](decisions/0004-exploration-state-and-limits.md) Report definitions. Saved replays being exempt from the 14-day removal within a 5 GiB sub-cap is Confirmed (Mike, 2026-10-03) in [0006](decisions/0006-retention-clock-and-operations.md).

Funnels define ordered steps, matching predicates, scope, conversion window, repeated steps, and handling of simultaneous events. Use a bounded ordered scan or measured query plan rather than unbounded self-joins. Daily anonymity limits a cross-day anonymous funnel; show that limitation instead of secretly linking identities.

Journeys define entry population, sequence length, loop compression, terminal state, and time window. Attribution defines first/last touch or another named rule, direct/internal traffic handling, lookback, and eligible identity scope. Campaign filters preserve their source values and documented normalization.

Retention and long-term cohorts require consented persistent identification. Specify cohort entry, activity event, elapsed versus calendar intervals, timezone, denominator, incomplete intervals, and eligible coverage. Daily anonymous data can support within-day activity analysis, but cannot prove cross-day person retention. Imported source identity is a separate population with explicit limitations.

Goals, annotations, boards, segments, and saved reports refer to versioned definitions. Editing a goal does not silently mutate prior metric versions. A saved exploration pins its metric version and is revalidated against the field catalog and permissions on open; stale versions and invalid fields are shown, never silently dropped ([0002](decisions/0002-sessions-and-events.md), [0004](decisions/0004-exploration-state-and-limits.md)).

## Typed exploration

Status: Confirmed policy; the v1 state shape, field scope, operator semantics, sources, report definitions, limits, timestamp and encoding conventions are Adopted in [0004](decisions/0004-exploration-state-and-limits.md).

One normalized state drives URL navigation, saved explorations, board cards, exports, and query keys. It contains schema version, site, measures, dimensions, grouped predicates, date range, timezone, granularity, comparison, metric/session policy, sorting, pagination, and visualization.

Use tagged expressions for AND/OR groups and typed field/operator/value combinations. Preserve repeated conditions. Validate maximum nesting, condition count, dimensions, result rows, query runtime, and encoded URL size against the limits in [0004](decisions/0004-exploration-state-and-limits.md), declared in the generated protocol and workload manifest. Deep expressions should fail visibly rather than lose predicates during serialization.

Dynamic property names resolve through an authorized field catalog and bound values. The planner maps approved dimensions/operators to SQL. User SQL, interpolated identifiers, and arbitrary database access are outside this interface.

Resolve relative dates once per query. Return resolved bounds and a consistent dataset/correction snapshot so chart, headline totals, and tables can describe the same dataset. SQLite control revision, DuckDB ingestion watermark, and correction state are distinct. A high ingestion watermark does not prove every derived report is corrected.

When navigating to another site, revalidate fields and versions. Do not carry invalid site-specific filters silently. A revoked permission removes cached results and prevents saved-view/export access. Server authorization is authoritative even if the browser hid an action.

## Correctness fixtures

Milestone 01 produces a tiny hand-auditable fixture with two websites, anonymous and consented identified populations, events spanning midnight/DST, inactivity boundaries, duplicate retries, late events, typed properties, revenue currencies/refunds, ordered and repeated funnel steps, and incomplete retention periods.

Maintain expected results independently of the production SQL. A fixture manifest explains each expected total and denominator. Run the same fixture through direct ingestion and each supported importer where source history permits the same facts. Record differences caused by unavailable history rather than rewriting expectations to match the implementation.

The large benchmark fixture tests scale. It is not the oracle for metric correctness.
