# Small correctness fixture

This fixture is the hand-audited oracle for metric correctness (milestone 01, work item 5). It has 166 input records for two websites. Every expected number below has a written derivation that names the input ids and the rule that produces it. You can check every number with the input file, a calendar and this page. No SQL or code is needed.

The large benchmark fixture tests scale. It is not an oracle for correctness ([analytics contract](../../docs/analytics-contract.md), Correctness fixtures).

Rules are cited by decision record and section heading:

- 0001: [identity and consent](../../docs/decisions/0001-identity-and-consent.md)
- 0002: [sessions and events](../../docs/decisions/0002-sessions-and-events.md)
- 0003: [metric formulas](../../docs/decisions/0003-metric-formulas.md)
- 0004: [exploration state and limits](../../docs/decisions/0004-exploration-state-and-limits.md)

All results are `metric_version = 1`. Site A uses `session_policy_version` 1 throughout. Site B uses 1 before its timezone change and 2 after it.

## Files

| File | Content |
| --- | --- |
| `input/events.jsonl` | 166 records, one per line: `id`, `site`, `receive_time`, `transient` (IP, User-Agent, geo result), and the `/api/send` request with its `{type, payload}` body |
| `input/sites.json` | Both sites: timezone, session window, persistent identity setting, engagement rule, policy versions, the site B timezone change, and the evaluation instant |
| `input/user-agents.json` | Fixed browser, OS and device for the four user agents |
| `expected/outcomes.json` | Accepted, duplicate, suppressed and keyless records, consent and revenue classification, counters |
| `expected/sessions.json` | All 78 sessions with member event ids, the keyless events, and performance attachment |
| `expected/traffic.json` | Pageviews, events, sessions, visitors and visitor-days with day series, identified people, bounce, observed span |
| `expected/realtime.json` | Active identities and the 30-minute series at the evaluation instant |
| `expected/goals.json` | Session and person goals |
| `expected/funnels.json` | Two daily funnels and one person funnel |
| `expected/journeys.json` | Pageview journeys with lengths 7 and 2 |
| `expected/attribution.json` | Last and first touch per conversion, for goals counted every and once, and revenue |
| `expected/retention.json` | Weekly cohorts with incomplete intervals and weighted averages |
| `expected/revenue.json` | Revenue per currency in minor units |
| `expected/percentiles.json` | LCP and CLS percentiles |
| `expected/properties.json` | Typed property filters, negative operators, group-by with null and not-set cells, stored values, catalog |
| `expected/acceptance-exploration.json` | The 0004 acceptance workflow with the previous-period comparison and the Austria drill-down |

## Conventions

- **Delivery order.** `events.jsonl` is sorted by `receive_time`. Accepted records get the next `ingestion_sequence` in file order (0002 Equal timestamps).
- **Event time.** `payload.timestamp` is integer seconds, so event time is that value times 1000. Without it, event time is the receive time. Any event time is accepted (0002 Event time). Most records have no timestamp, like the unchanged tracker, so their event time has millisecond precision.
- **Identity labels.** Pseudonyms are keyed with random day keys, so the fixture cannot state their values. Results name identities by their inputs instead:
  - `anon <ip> <ua> <local day>` is the anonymous daily pseudonym (0001 Keyed derivation).
  - `daily-id <id> <local day>` is the daily identified pseudonym.
  - `profile <id>` is the persistent profile.
  - Site B labels add `epoch N`, because its timezone changed (0001 Timezone change).
- **Two identities per event.** Every eligible event has a daily pseudonym, also when it is linked to a profile. Daily measures (visitors, visitor-days, realtime, daily funnels, daily attribution) use the daily pseudonym. Person measures use the profile. Sessions use the profile for linked events and the daily pseudonym otherwise (0001 Daily identity of every event).
- **Local day.** The calendar date of the event time in the site timezone that applies at that event time (0001 Local date, 0002 Midnight, DST, timezone change).
- **Ranges.** Ranges are local dates, inclusive, resolved to half-open instants `[start, end)` (0003 Common terms, 0004 Semantics). Sessions are in range when their start is in range (start mode).
- **Keyless events.** An event for local day D received at or after `end(D) + 25h` finds D's day key destroyed. It is stored without a daily pseudonym. It counts in pageviews, events and revenue, but not in visitors, sessions or any other identity-based measure (0001 Key retention and keyless events).
- **Identity coverage.** Every identity-based result carries `identity_coverage`: eligible events in range with a daily pseudonym or profile session, over all eligible events in range (0003 Common terms).
  The keyless events fall only in the traffic cases for site A and for 2026-06-02. Every other identity-based case has coverage 1, and its file gives the exact counts.
- **Retention cutoff.** An event older than the start of local date `today − 12 months` is not stored (`outside_retention`). `today` is the local date of the receive time, or of the evaluation instant for queries (0006 Retention cutoff).
- **Eligible events.** Pageviews and custom events. Identify and performance records never start, extend or count in sessions, visitors, bounce, span or realtime (0002 Eligible event kinds).
- **Device and country.** Device comes from `input/user-agents.json`. Country is `transient.country`, the fixed geo lookup result. In explorations, device, country and referrer domain are session fields (0004 Field scope).
- **Rounding.** Ratios are rounded half to even to 4 places (0004 Encoding conventions), and the exact numerator and denominator are always given next to them. Spans are integer milliseconds, and the mean span is rounded half to even to a whole millisecond (0003 Formulas).

## Input blocks

| Block | Ids | Site, local dates | What it exercises |
| --- | --- | --- | --- |
| DST fall-back | fal-01 to fal-04 | A, 2025-10-26 (25 h) and 10-27 | Instant gaps across the transition, midnight on a 25-hour day |
| DST spring-forward | spr-01 to spr-04 | A, 2026-03-29 (23 h) and 03-30 | Instant gaps across the transition, midnight on a 23-hour day |
| Inactivity | gap-01 to gap-06, perf-07 | A, 2026-06-02 | Gap of exactly 30 min, gap of 30 min minus 1 ms, one-event sessions, custom-only session, performance attached in a session tail |
| Duplicates | dup-01 to dup-06, b-01 | A 2026-06-02 and 06-04, B 2026-06-02 | Same eventId and payload, same eventId and different payload, retry without eventId, another source, window expiry |
| Late and early | late-01 to late-04 | A, 2026-06-03 | Event time 5 min ahead and 1 s beyond, 24 h behind and 1 s beyond, all accepted |
| Keyless | key-01, key-02 | A, 2026-06-02, received 06-05 | Events after their day key was destroyed: counted in views, events and revenue only |
| Retention and far future | ret-01, ret-02, fut-01 | A, received 2026-10-03 | One second before the retention cutoff, exactly at it, and an event time in 2100 |
| Properties | prop-01 to prop-06 | A, 2026-06-10 | String, integer, decimal, bool, date, array, null, missing, nested keys, mixed types for `qty`, `1.0` as integer, exact decimal rounding |
| Revenue | rev-01 to rev-11 | A, 2026-06-11 | EUR, USD, GBP, JPY, refund, rounding from decimal text, string amount, unknown currency, zero |
| Funnels | fun-01 to fun-21 | A, 2026-06-16 and 06-17 | Order, re-entry, half-open window, simultaneous events, midnight, repeated steps |
| Journeys | jou-01 to jou-15 | A, 2026-06-18 | Loop compression, step limit, one-step session, non-consecutive repeats, ordering |
| Attribution and goals | att-01 to att-14 | A, 2026-06-23 and 06-24 | Referrer, UTM and click-ID touches, internal and direct traffic, midnight, campaign case, goals once and every |
| Performance | perf-01 to perf-06 | A, 2026-06-25 | Percentiles, explicit zero, missing metric |
| Acceptance | acc-01 to acc-25 | A, 2026-08-04 to 10-03 | Mobile, `signup`, `plan = pro`, country, referrer, previous period and its boundaries |
| Consent | con-01 to con-19 | A, 2026-08-11 to 10-01 | Grant, same-day revocation, midnight-crossing persistent session, retention cohorts |
| Realtime | rt-01 to rt-07 | A, 2026-10-03 | Window `[now − 5 min, now]` at 2026-10-03T10:00:00.000Z, its exact start, a late event, a fast client clock |
| Site B | b-01 to b-12 | B, 2026-06-02 to 09-20 | New York midnight, timezone change to UTC as an identity rotation, series over two timezones, consent ignored, identified user on two devices |

## Outcomes (`outcomes.json`)

166 records: 163 accepted, 2 duplicates, 1 suppressed. No record is refused for its event time (0002 Event time, 0005 Event time outcomes).

| Record | Outcome | Why |
| --- | --- | --- |
| dup-01 | accepted | First use of `evt-0001` on site A (0002 Client event-ID deduplication) |
| dup-02 | duplicate, HTTP 200 | Same `(source, eventId)` within 48 h, same payload. Not stored. The response body is the normal stored-event body |
| dup-03 | duplicate, HTTP 200, conflict | Same `(source, eventId)`, payload differs (`sku` B2). Not stored, not merged. `dedup_conflict` becomes 1 |
| b-01 | accepted | `evt-0001` on site B. Scope is per source, so it does not collide with dup-01 |
| dup-05 | accepted | Retry of dup-04 without `eventId` is a separate stored event |
| dup-06 | accepted | `evt-0001` again 49 h after dup-01's receive time, outside the 48 h window |
| late-01 | accepted, counted in `event_time_ahead` | Event time is receive time + 301 s, more than 5 min ahead |
| late-02 | accepted | Event time is receive time + 300 s, not more than 5 min ahead |
| late-03 | accepted | Event time is receive time − 86,400 s. The 2026-06-03 key lives until 2026-06-04T23:00Z |
| late-04 | accepted | Event time is receive time − 86,401 s, received 2026-06-04T10:25Z, so the key still exists |
| key-01, key-02 | accepted, keyless | Event time on 2026-06-02, received 2026-06-05T10:00Z. That is after `end(2026-06-02) + 25h` = 2026-06-03T23:00Z, so the day key is a tombstone (0001 Key retention and keyless events) |
| ret-01 | suppressed `outside_retention`, HTTP 200 | Received 2026-10-03T09:00Z, local date 10-03, so the cutoff is the start of 2025-10-03 local = 2025-10-02T22:00:00.000Z. Its event time is 1 s earlier. Not stored (0006 Retention cutoff) |
| ret-02 | accepted, keyless | Event time exactly at the cutoff, which is kept. Its day ended long ago, so it has no key |
| fut-01 | accepted, counted in `event_time_ahead` | Event time 2100-01-01T00:00Z. Its key is created on demand, so it has a pseudonym. It falls in no report range |
| rt-07 | accepted | Event time is receive time + 240 s |

Consent (0001 Persistent consent transport): con-01, con-06, con-09 and con-14 grant consent for dora, anna, ben and cleo on site A. Site A has `persistent_identity_enabled = true` and each payload has an `id`. con-03 revokes dora at 2026-08-11T09:05:00.000Z (0001 Revocation). b-06 is ignored because site B has persistent identity disabled, so `consent_ignored` is 1.

Counters: `event_time_ahead` is 2 (late-01, fut-01), `outside_retention` is 1 (ret-01). `revenue_invalid` is 2: rev-04 has a string amount and rev-05 an unknown currency. rev-06 is a zero amount, which is valid and not counted (0002 Currencies and revenue).

Stored kinds: site A has 85 pageviews, 54 custom events, 5 identify (con-01, con-03, con-06, con-09, con-14) and 7 performance (perf-01 to perf-07). That is 151, plus the 3 records that were not stored, makes 154 site A records. Site B has 9 pageviews, 2 custom events (b-01, b-08) and 1 identify (b-06).

## Sessions (`sessions.json`)

The timeout rule (0002 Timeout rule): within one identity, order eligible events by `(event_time, ingestion_sequence)`. A gap of 30 min or more starts a new session. Anonymous and daily-identified sessions also end at local midnight and at a timezone change, where the next event gets a new pseudonym (0002 Midnight, DST, timezone change). Persistent profile sessions ignore midnight (0001 Identified sessions and midnight). Span is last minus first event time.

A session is a bounce when it has exactly one pageview and no custom event (0003 Formulas). Sessions without a pageview are outside the bounce and span denominators.

The sessions that carry a specific rule:

| Session | Span ms | Derivation |
| --- | --- | --- |
| fal-01, fal-02 | 1,500,000 | 00:40Z and 01:05Z on 2025-10-26. Wall clock reads 02:40 CEST then 02:05 CET, but the instants are 25 min apart, under the window |
| fal-03 / fal-04 | 0 / 0 | 22:55Z is 23:55 CET, still 2025-10-26 because that day has 25 hours. 23:05Z is 00:05 on 10-27: new day, new pseudonym, new session, although only 10 min passed |
| spr-01, spr-02 | 1,200,000 | 00:50Z (01:50 CET) and 01:10Z (03:10 CEST). Wall clock moved 80 min, instants 20 min |
| spr-03 / spr-04 | 0 / 0 | 21:50Z is 23:50 CEST on 2026-03-29, a 23-hour day. 22:05Z is 00:05 on 03-30: split at midnight |
| gap-01 / gap-02, gap-03 | 0 / 1,799,999 | gap-01 to gap-02 is exactly 1,800,000 ms, so it splits. gap-02 to gap-03 is 1,799,999 ms, so it extends |
| gap-04 | 0 | Custom event only, no pageview |
| gap-05, gap-06 | 5,000 | One pageview plus one custom event: not a bounce, because any custom event is engagement |
| dup-01, dup-04, dup-05 | 11,000 | dup-02 and dup-03 were not stored. dup-05 is stored, so the session has 2 pageviews |
| late-01 to late-04 | 1,499,000 | late-01 and late-02 share event time 10:00:00Z, and late-01 has the lower ingestion sequence. late-03 (10:20Z) and late-04 (10:24:59Z) arrived a day late, but the 2026-06-03 key still existed, so they get V7's pseudonym and join. The span runs from 10:00:00 to 10:24:59 |
| prop-01 to prop-06 | 300,000 | 09:00 to 09:05 |
| fun-12, fun-13 / fun-14 | 1,200,000 / 0 | 20 min gap extends, then a 40 min gap splits |
| fun-15 / fun-16 | 0 / 0 | 21:50Z and 22:05Z on 2026-06-16 are 23:50 and 00:05 local. Split at midnight |
| fun-09, fun-10, fun-11 | 60,000 | fun-09 and fun-10 share event time 11:00:00Z. fun-09 has the lower ingestion sequence |
| att-09 / att-10 | 0 / 0 | 23:50 and 00:10 local: split at midnight. att-10 has no pageview |
| con-02 / con-04 | 0 / 0 | con-02 is dora's profile session, after the grant con-01. Revocation con-03 at 09:05 closes it. con-04 at 09:06 still carries `id: dora` but is no longer linked, so its session identity is `daily-id dora 2026-08-11` (0001 Revocation) |
| con-05 / con-07, con-08 | 0 / 480,000 | con-05 comes before anna's consent (con-06) and has no id: anonymous. con-07 and con-08 carry `id: anna` after the grant: profile. No retroactive merge (0001 identify, 0001 Persistent consent transport) |
| con-11, con-12 | 900,000 | ben's profile at 23:50 and 00:05 local: one session across midnight |
| acc-05, acc-06 | 119,999 | 21:58:00.000Z to 21:59:59.999Z |
| acc-07, acc-08 | 30,000 | Starts at 2026-09-03T22:00:00.000Z, which is 00:00 local on 09-04 |
| acc-09, acc-10 / acc-11, acc-12 | 120,000 / 120,000 | Same IP and UA on 09-05 and 09-06: two pseudonyms |
| rt-07 | 0 | Starts at its event time 10:02, four minutes after it was received |
| fut-01 | 0 | A session on local day 2100-01-01 |
| b-02 / b-03 / b-12 | 0 / 0 / 0 | Site B before the change, New York (EDT). 03:50Z is 23:50 on 09-13 and 04:05Z is 00:05 on 09-14, so they split at New York midnight. b-12 at 2026-09-15T03:00Z is 23:00 New York on 09-14, the same identity day as b-03, but 23 h later. Policy 1 |
| b-10 / b-11 | 0 / 0 | 11:50Z (07:50 New York, epoch 1) and 12:10Z (12:10 UTC, epoch 2), same IP and UA, 20 min apart. Both read local date 09-15, but the change at 12:00Z rotates identity, so these are two pseudonyms and two sessions (0001 Timezone change, 0002 Midnight, DST, timezone change). Policies 1 and 2 |
| b-04 / b-05 | 0 / 0 | After the change, UTC applies. 23:50Z on 09-15 and 00:05Z on 09-16 split at UTC midnight. Under New York they would be one session at 19:50 and 20:05. Policy 2 |
| b-07, b-08, b-09 | 540,000 | All carry `id: eve`. Site B does not link profiles, so this is `daily-id eve 2026-09-20`. b-09 comes from a second device (other IP and UA) but has the same daily identified pseudonym, which does not include IP or UA (0001 Keyed derivation) |

All other sessions are one visit by one anonymous visitor within a few minutes, and their spans are the first-to-last differences of the listed times:

- journeys: 180,000, 180,000, 120,000, 120,000, and 0 for jou-15
- the funnel and attribution sessions
- the acceptance pairs: 60,000 for a pageview plus a signup one minute later
- the realtime pair rt-02, rt-03: 120,000

Keyless events (key-01, key-02, ret-02) have no daily pseudonym and belong to no session. key-01 at 12:05Z would otherwise have joined V6's session {dup-01, dup-04, dup-05}, which ended at 12:00:11.

Performance attachment (0002 Identify, performance, and recorder messages): a performance row joins the session of its identity whose interval `[first, last + window)` contains its event time. perf-07 at 09:20 belongs to V3's session {gap-02, gap-03}. That session ends at 08:59:59.999, so its interval runs to 09:29:59.999. perf-07 does not extend the session. perf-01 to perf-06 come from identities without sessions and stay unattached.

## Traffic (`traffic.json`)

Formulas from 0003 Formulas:

- Pageviews and events are counts of stored events in range. Sessions count by start.
- Visitor-days are the distinct daily pseudonyms over the range. Since a pseudonym lives for one identity day, that equals the sum over identity days. Visitors use the same count when the range lies inside one identity day (0003 Visitors versus visitor-days).
- The day series counts distinct pseudonyms per day bucket of the exploration timezone. It can sum to more than the headline when the timezones differ, and then `series_sum_differs` is true.
- Bounce rate is bounces over sessions with a pageview.
- Observed span sums spans over sessions with a pageview. The mean is that sum over the count, rounded half to even. The median is the nearest-rank p50, rank `ceil(0.5 × n)`.

### Site A, all data (`site-a-all`)

Range 2025-10-01 to 2026-10-03 in Europe/Vienna, `[2025-09-30T22:00Z, 2026-10-03T22:00Z)`. The range starts before the query's retention cutoff, so the result returns `retention_start` 2025-10-02T22:00:00.000Z and counts from there (0006 Retention cutoff). It contains every stored site A record except fut-01. Subtotals by block:

| Block | Pageviews | Custom events | Sessions | With pageview | Bounces | Span sum ms | Visitor-days |
| --- | --- | --- | --- | --- | --- | --- | --- |
| fal | 4 | 0 | 3 | 3 | 2 | 1,500,000 | 2 |
| spr | 4 | 0 | 3 | 3 | 2 | 1,200,000 | 2 |
| ret-02 on 2025-10-03 (keyless) | 1 | 0 | 0 | 0 | 0 | 0 | 0 |
| gap, dup, key on 06-02 | 7 | 4 | 5 | 4 | 1 | 1,815,999 | 4 |
| late | 4 | 0 | 1 | 1 | 0 | 1,499,000 | 1 |
| dup-06 on 06-04 | 0 | 1 | 1 | 0 | 0 | 0 | 1 |
| prop | 1 | 5 | 1 | 1 | 0 | 300,000 | 1 |
| rev | 0 | 11 | 2 | 0 | 0 | 0 | 2 |
| fun | 9 | 12 | 9 | 7 | 1 | 2,820,000 | 8 |
| jou | 15 | 0 | 5 | 5 | 1 | 600,000 | 5 |
| att | 7 | 7 | 8 | 7 | 3 | 840,000 | 6 |
| perf | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| acc | 12 | 13 | 12 | 12 | 0 | 1,229,999 | 12 |
| con | 14 | 0 | 12 | 12 | 10 | 1,380,000 | 11 |
| rt | 6 | 1 | 6 | 5 | 4 | 120,000 | 6 |
| **Total** | **84** | **54** | **68** | **60** | **24** | **13,304,998** | **61** |

Notes on the subtotals:

- The fun span sum is F1 1,200,000 + F2 180,000 + F3 60,000 + F4 1,200,000 + F5 0 + F6 120,000 + F7 60,000. Its visitor-days are F1 to F7 on 06-16 plus F5 again on 06-17.
- The att span sum is 300,000 + 120,000 + 60,000 + 360,000 for the four multi-event sessions with a pageview. Its visitor-days are T1 to T5 on 06-23 plus T4 on 06-24.
- The acc span sum is 300,000 + 60,000 + 119,999 + 30,000 + 120,000 + 120,000 + 180,000 + 5 × 60,000. Its 12 visitor-days are one per session.
- The con visitor-days, 11 in total:
  - dora on 08-11 is one daily pseudonym, `daily-id dora 2026-08-11`, for both the linked con-02 and the unlinked con-04. So the same-day revocation does not count her twice.
  - anna has two on 09-15: anonymous con-05, and `daily-id anna` for her linked events.
  - The rest are ben on 09-16 and 09-17, anna on 09-22, cleo on 09-23, the anonymous visitor on 09-24, ben and cleo on 09-29, and anna on 10-01.
- The rt visitor-days are six anonymous identities on 10-03 (rt-01, rt-02 with rt-03, rt-04, rt-05, rt-06, rt-07). Together with AC6 that makes 7 on 10-03.

The day series is the per-day count in `visitor_days_series_by_local_day`. The exploration timezone is the site timezone, so it sums to 61 and `series_sum_differs` is false.

`events_by_name`: signup 25 (6 in fun, 6 in att, 13 in acc), purchase 12 (10 in rev, att-13 and the keyless key-02), signup_start 6 (fun-02, fun-05, fun-08, fun-09, fun-13, fun-16), configure 5, add_to_cart 2 (dup-01, dup-06), ping 2 (gap-04, rt-04), cta_click 1, refund 1.

Bounce rate: 24 / 60 = 0.4.

Observed span:

- The mean is 13,304,998 / 60 = 221,749.97, which rounds to 221,750.
- For the median, the 60 spans sorted ascending begin with 24 zeros (every single-pageview session), then 5,000, 11,000, 30,000 and nine sessions of 60,000. Rank `ceil(30) = 30` falls in the 60,000 group, so the median is 60,000.

Identified people: 4 profiles (dora, anna, ben, cleo). Coverage is linked eligible events over all eligible events: 11 linked (con-02, con-07, con-08, con-10, con-11, con-12, con-13, con-15, con-17, con-18, con-19) over 138 (84 + 54), so 0.0797. con-04 is not linked (0003 Formulas, Identified people).

Identity coverage: 135 of the 138 eligible events have a daily pseudonym. The keyless key-01, key-02 and ret-02 do not, so the rate is 0.9783. Visitors, sessions, bounce and span above use only the 135.

### Site B, all data (`site-b-all`)

Range 2026-06-01 to 2026-09-30 in UTC. Nine pageviews (b-02, b-03, b-04, b-05, b-07, b-09, b-10, b-11, b-12), two custom events (b-01, b-08), nine sessions. Policies 1 and 2 both appear.

- Eight sessions have a pageview. Seven of them are single-pageview bounces, so 7 / 8 = 0.875. The only non-bounce is the eve session.
- Spans are seven zeros and 540,000. The sum is 540,000 and the mean is 67,500. The median is rank 4, which is 0.
- Visitor-days are 8 distinct pseudonyms:
  - 198.51.100.80 on 06-02.
  - 198.51.100.81 on New York 09-13, New York 09-14 (b-03 and b-12), UTC 09-15 and UTC 09-16.
  - 198.51.100.82 on New York 09-15 (b-10) and UTC 09-15 (b-11).
  - eve on 09-20.
- The UTC day series is 06-02: 1, 09-14: 2 (b-02, b-03), 09-15: 4 (b-12, b-04, b-10, b-11), 09-16: 1, 09-20: 1. It sums to 9, because the New York 09-14 identity has b-03 in the UTC 09-14 bucket and b-12 in the UTC 09-15 bucket. So `series_sum_differs` is true and the headline stays 8 (0003 Visitors versus visitor-days).
- Identified people are 0, because consent was ignored (b-06). Coverage is 0 / 11. Identity coverage is 11 / 11.

The site summaries put the two sites side by side: A has 80 pageviews and 61 visitor-days, B has 9 and 8.

### Focused cases

- The other focused cases have identity coverage 1: 4 / 4 for both DST cases, 9 / 9 for the identified case.
- **`dst-fall-2025`.** Local 2025-10-26 is `[2025-10-25T22:00Z, 2025-10-26T23:00Z)`, 25 hours, and 10-27 is 24 hours. 4 pageviews, 3 sessions, and 2 visitor-days, one per day for V2. Bounce rate is 2 / 3 = 0.6667. The span sum is 1,500,000, the mean is 500,000 and the median of [0, 0, 1,500,000] is 0.
- **`dst-spring-2026`.** Local 2026-03-29 is `[2026-03-28T23:00Z, 2026-03-29T22:00Z)`, 23 hours. Same shape: 4 pageviews, 3 sessions, 2 visitor-days, bounce 2 / 3, span sum 1,200,000, mean 400,000, median 0.
- **`day-2026-06-02`.** One identity day, so the label is "visitors".
  - 7 pageviews (gap-01, gap-02, gap-03, gap-05, dup-04, dup-05, and keyless key-01), 4 events (gap-04, gap-06, dup-01, and keyless key-02) and 5 sessions.
  - Visitors are 4 (V3, V4, V5, V6). key-01 and key-02 have no pseudonym, and perf-07 is not eligible.
  - Identity coverage is 9 / 11 = 0.8182.
  - Bounce rate is 1 / 4 = 0.25. Only {gap-01} is a bounce.
  - Spans sorted are 0, 5,000, 11,000 and 1,799,999. The sum is 1,815,999, the mean 453,999.75 rounds to 454,000, and the median at rank 2 is 5,000.
- **`day-2026-06-03`.** late-01 to late-04 are 4 pageviews in 1 session with 1 visitor. The span, mean and median are 1,499,000 and the bounce rate is 0 / 1. Identity coverage is 4 / 4.
- **`identified-2026-09-14-to-27`.** Eligible events in range are con-05, con-07, con-08, con-10, con-11, con-12, con-13, con-15 and con-16, which is 9. Seven are linked (all but con-05 and con-16), so coverage is 0.7778. Three profiles appear: anna, ben and cleo. Visitor-days are 7: 2 on 09-15, then 1 each on 09-16, 09-17, 09-22, 09-23 and 09-24.

## Realtime (`realtime.json`)

Server time 2026-10-03T10:00:00.000Z. An event's activity time is `min(event_time, receive_time)`. It is active when the activity time lies in `[09:55:00.000Z, 10:00:00.000Z]`, both ends inclusive, counted as distinct daily pseudonyms (0003 Realtime).

| Record | Activity time | Active | Why |
| --- | --- | --- | --- |
| rt-05 | 09:40 | no | Received at 09:59:30, but its event time is 09:40. This is the late-event exclusion |
| rt-01 | 09:50 | no | Too old |
| rt-06 | 09:55 | yes | Exactly at the window start, which is inclusive |
| rt-02, rt-03 | 09:56, 09:58 | yes | One identity |
| rt-07 | 09:58 | yes | Event time 10:02 is ahead of the server, so the receive time counts |
| rt-04 | 09:59 | yes | Custom event |

That makes 4 active identities, with identity coverage 5 / 5 over the events in the window. fut-01 has activity time 09:30 (its receive time) and is outside the window and the series. ret-01 and ret-02 have activity times in 2025.

The series has 30 one-minute UTC buckets. The current minute 10:00 is the last bucket, `[10:00:00.000, 10:00:00.000]`, and the first bucket starts at 09:31. These buckets are non-empty:

| Bucket | Pageviews | Visitors |
| --- | --- | --- |
| 09:40 | 1 | 1 (rt-05) |
| 09:50 | 1 | 1 |
| 09:55 | 1 | 1 |
| 09:56 | 1 | 1 |
| 09:58 | 2 | 2 (rt-03, rt-07) |
| 09:59 | 0 | 1 (rt-04) |

## Goals (`goals.json`)

0003 Goals.

- **`signup-session-once`.** Range 2026-06-23. Seven sessions start that day: {att-01}, {att-02, att-03}, {att-04, att-05}, {att-06}, {att-07, att-08}, {att-09} and {att-11 to att-14}. {att-10} starts on 06-24 and is not in range. Four of them contain a `signup`, so 4 / 7 = 0.5714.
- **`signup-session-every`.** Same denominator. Matching events are att-03, att-05, att-08, att-12 and att-14 (two signups in the T5 session), so 5 / 7 = 0.7143.
- **`docs-person-once`.** Range 2026-09-14 to 09-27, consented population only. Anna, ben and cleo each have an eligible event in range, so the denominator is 3. Anna (con-08) and ben (con-11) viewed `/docs`. Cleo's `/docs` view (con-18) is on 09-29, outside the range. 2 / 3 = 0.6667. Coverage is 7 / 9 as in the identified case above.

## Funnels (`funnels.json`)

0003 Funnels: an identity enters at its first step-1 event in range. Each next step is the first matching event strictly after the previous step in `(event_time, ingestion_sequence)` order. It must also lie inside the half-open window `t1 <= event_time < t1 + window`.

**`pricing-signup-daily`.** Steps are pageview `/pricing`, then `signup_start`, then `signup`. Window 60 min, daily scope, range 2026-06-16.

| Identity | Path | Reached | Why |
| --- | --- | --- | --- |
| F1 | fun-01, fun-02, fun-03 | 3 | In order. fun-04 is a second `/pricing` and does not re-enter |
| F2 | fun-06, fun-08 | 2 | fun-05 comes before step 1. fun-07 `signup` comes before the step-2 event fun-08, and no `signup` follows fun-08 |
| F3 | fun-10 | 1 | fun-09 `signup_start` has the same event time as fun-10 but a lower ingestion sequence, so it is not strictly after step 1 |
| F4 | fun-12, fun-13 | 2 | fun-14 at 13:00:00.000 is exactly one window after fun-12 at 12:00:00.000, so the half-open window excludes it |
| F5 | fun-15 | 1 | fun-16 is after local midnight and belongs to a different daily pseudonym |

Steps reached are 5, 3 and 1. Drops are 2 and 2. Conversion is 1 / 5 = 0.2.

**`docs-docs-signup-daily`.** Steps are `/docs`, `/docs`, `signup` (a repeated predicate), same window and range. F6 uses fun-17 then fun-18, two distinct events, then fun-19. F7 has only one `/docs` (fun-20), so it stops at step 1. Steps reached are 2, 1 and 1. Conversion is 0.5.

**`docs-pricing-person`.** Steps are `/docs` then `/pricing`, person scope, range 2026-09-14 to 09-27. Anna enters at con-08, but her only `/pricing` (con-07) is earlier, so she stops at 1. Ben enters at con-11 (23:50 local on 09-16) and reaches con-12 15 minutes later, after midnight. Person scope spans days. Cleo has no `/docs` in range. Steps reached are 2 and 1. Conversion is 0.5.

## Journeys (`journeys.json`)

0003 Journeys. These are pageview journeys for the five sessions starting on 2026-06-18. Loop compression runs on the whole session before the step limit. One-step sessions are reported. Ties compare sequences element by element, and a prefix sorts first.

| Session | Raw paths | Compressed |
| --- | --- | --- |
| J1, jou-01 to jou-04 | `/ / /pricing /docs` | `/ /pricing /docs` |
| J2, jou-05 to jou-08 | `/ /pricing / /pricing` | unchanged, because the repeats are not consecutive |
| J3, jou-09 to jou-11 | `/ /pricing /pricing` | `/ /pricing` |
| J4, jou-12 to jou-14 | `/ /pricing /docs` | unchanged |
| J5, jou-15 | `/` | `/` |

With length 7, no sequence is cut, which gives 4 distinct sequences:

- `/ /pricing /docs` has 2 sessions (J1, J4).
- Then come three sequences with one session each, in element order: `/`, then `/ /pricing`, then `/ /pricing / /pricing`. Each is a prefix of the next.

With length 2, J1, J2, J3 and J4 all become `/ /pricing`. J1 is `/ /pricing` only because compression comes first; cutting first would give `/ /`. So there are 2 distinct sequences: `/ /pricing` with 4 sessions, and `/` with 1. In both cases the counts add up to the 5 entry sessions.

## Attribution (`attribution.json`)

0003 Attribution, daily scope. Touches are the first pageviews of the sessions of the conversion's daily pseudonym that come before the conversion event. Internal referrers (`www.example.com` after stripping `www.`) are ignored. A session with no referrer, UTM source or click ID is direct. The range is 2026-06-23 to 06-24.

The conversion events depend on the counting:

- With `every`, each matching event converts: att-03, att-05, att-08, att-10, att-12 and att-14.
- With `once`, only the first matching event per session converts. So att-12 converts for the T5 session and att-14 does not.
- For revenue, each event with a revenue record converts: att-13.

| Conversion | Last touch | First touch | Why |
| --- | --- | --- | --- |
| att-03 | att-02 (newsletter, email, `june`) | att-01 (news.example.net) | T1 had two sessions that day, both with touches |
| att-05 | direct | direct | The only touch att-04 is internal and ignored |
| att-08 | att-06 (search.example.net, gclid) | att-06 | The later session att-07 is direct and does not replace the last non-direct touch |
| att-10 | direct | direct | 00:10 local on 06-24: a new pseudonym. Its only session is att-10 itself, which is not before the conversion. The news touch att-09 belongs to the 06-23 pseudonym |
| att-12, att-14 | att-11 (`June`) | att-11 | |
| att-13 (EUR 3000) | att-11 | att-11 | Revenue keeps its currency |

Campaign values are not case-folded:

- With `every` and last touch, `June` has 2 conversions and `june` has 1. With first touch, `June` has 2, because att-03's first touch has no campaign.
- With `once`, `June` drops to 1 under both models.
- Direct is 2 in every variant.

## Retention (`retention.json`)

0003 Retention and cohorts:

- Consented profiles only.
- Weekly cohorts start on Monday in Europe/Vienna, and a profile's cohort is the week of its first eligible event. Activity is any eligible event in the interval. Intervals are calendar weeks.
- The report asks for 3 intervals, so every cohort row shows offsets 0 to 2.
- The watermark is 2026-10-03T10:00:00.000Z. An interval ending after it is `complete: false`, and one that has not started shows 0.

| Cohort | Members (first event) | Week 0 | Week 1 | Week 2 |
| --- | --- | --- | --- | --- |
| 2026-09-14 | anna (con-07), ben (con-10) | 2 / 2, complete | 1 / 2 = 0.5 (anna con-13), complete | 2 / 2 (ben con-17, anna con-19), incomplete |
| 2026-09-21 | cleo (con-15) | 1 / 1, complete | 1 / 1 (con-18), incomplete | 0 / 1, not started, incomplete |

- The week of 2026-09-28 ends at 2026-10-04T22:00Z, after the watermark, so it is incomplete. The week of 2026-10-05 has not started.
- Anna's cohort starts at con-07, not con-05. con-05 is anonymous and is never merged into the profile.
- Dora's first event is in August, outside the cohort range.
- Averages are weighted by cohort size over complete intervals only:
  - Week 0 is (2 + 1) / (2 + 1) = 1.0.
  - Week 1 is 1 / 2 = 0.5, because only the 09-14 cohort is complete.
  - Week 2 has no complete cohort, so it is `null` with `no_denominator`.
- Coverage for the 09-14 week: eligible events con-05, con-07, con-08, con-10, con-11 and con-12 are 6, of which 5 are consented, so 0.8333.
- Coverage for the 09-21 week: con-13, con-15 and con-16 are 3, of which 2 are consented, so 0.6667.

## Revenue (`revenue.json`)

0002 Currencies and revenue:

- Amounts are integer minor units by ISO 4217 exponent, rounded half to even from the JSON number text as an exact decimal.
- Negative amounts are refunds. Zero is valid but is no record. Currencies are kept apart.
- From 0003 Formulas, Revenue: gross is the sum of positives, refunds the sum of negatives, net the sum of all, and count the records with a non-zero amount.

| Record | Data | Result |
| --- | --- | --- |
| rev-01 | 49.9 EUR | 4990 |
| rev-02 | 19.995 EUR | 1999.5 cents rounds half to even to 2000 (a binary float would give 1999) |
| rev-03 | −19.99 EUR (refund) | −1999 |
| rev-04 | `"12.00"` EUR | invalid: the amount is a string |
| rev-05 | 10 ZZZ | invalid: unknown currency |
| rev-06 | 0 EUR | valid, no revenue record, not counted as invalid |
| rev-07 | 12.5 GBP | 1250 |
| rev-08 | 25 `usd` | USD 2500, uppercased |
| rev-09 | 0.125 USD | 12.5 cents rounds half to even to 12 |
| rev-10 | 1500.5 JPY | exponent 0, 1500.5 rounds half to even to 1500 |
| rev-11 | 3000 JPY | 3000 |

For 2026-06-11:

- EUR: gross 4990 + 2000 = 6990, refunds −1999, net 4991 ("49.91"), count 3.
- USD: 2500 + 12 = 2512, count 2.
- JPY: 1500 + 3000 = 4500, count 2.
- GBP: 1250, count 1.
- `revenue_invalid` is 2.

For all site A data, att-13 adds EUR 3000 and the keyless key-02 adds EUR 1000. Keyless events still count in revenue. That gives gross 10990, refunds −1999, net 8991, count 5. The other currencies are unchanged.

## Percentiles (`percentiles.json`)

0003 Percentiles: nearest rank `r = ceil(p × n)`. A missing metric is excluded and an explicit zero is an observation. The input is perf-01 to perf-06 on 2026-06-25. perf-07 is on 06-02 and outside this range.

- **LCP, all.** perf-06 has no `lcp`, which leaves 5 values: sorted 0 (perf-05), 800, 1200, 2500, 4000. p50 is rank 3, 1200. p75 is rank 4, 2500. p95 is rank 5, 4000.
- **CLS, all.** perf-04 has no `cls`, which leaves 5 values: sorted 0.0100, 0.0412, 0.0500, 0.1000, 0.2500. p50 is 0.0500, p75 is 0.1000 and p95 is 0.2500.
- **LCP, mobile.** perf-01 and perf-02, n = 2, values 1200 and 2500. p50 is rank 1, 1200. p75 and p95 are rank 2, 2500.
- **LCP, desktop.** perf-03, perf-04 and perf-05, n = 3, sorted 0, 800, 4000. p50 is rank 2, 800. p75 and p95 are rank 3, 4000.

## Properties (`properties.json`)

0002 Property type system, 0004 Negative operators and missing values. The base is the five `configure` events on 2026-06-10 (prop-02 to prop-06).

Stored types:

- `qty` is integer 1 (prop-02), string `"1"` (prop-03), decimal `2.5000` (prop-04), integer 3 (prop-05) and integer 1 (prop-06). prop-06 sends `1.0`, and numbers are typed by value, so it is the same integer as prop-02.
- `ratio` 2.71828 is stored as `2.7183`. `0.12345` is stored as `0.1234`, rounded half to even from the exact decimal.
- `delivery` matches the date pattern and is stored as an instant.
- `coupon` is null in prop-02, `"SUMMER"` in prop-03, and missing in prop-04, prop-05 and prop-06.
- `nested.level` comes from flattening the object `{nested: {level: "x"}}`.

Filters, counted in events:

- `qty eq "1"` (string) matches prop-03 only: 1. `qty eq 1` (integer) matches prop-02 and prop-06: 2. A typed filter matches only rows of its type.
- `qty neq 1` (integer) is the exact complement of `eq`: prop-03, prop-04 and prop-05, which is 3.
- `qty gt 1` matches decimal 2.5 and integer 3: 2. `qty gte 1` also matches the two integer 1 rows: 4. Numeric comparisons cover integer and decimal rows, never string rows, and have no complement.
- `color eq red` matches prop-02 and prop-05: 2. `color neq red` matches the other three, including prop-04 and prop-06 where `color` is missing: 3.
- `gift eq true` matches prop-02: 1. `ratio gt 1.0000` matches prop-02: 1. `delivery gt 2026-06-15` matches prop-02: 1.
- `tags contains "a"` matches prop-02: 1. `tags contains "b"` matches prop-02 and prop-03: 2. `tags not_contains "b"` matches the other three: 3.
- `coupon is_null` matches prop-02: 1. `is_set` matches prop-02 and prop-03: 2, because a null row is present. `is_not_set` matches prop-04, prop-05 and prop-06: 3.
- `nested.level eq x` matches prop-05: 1.

Group-by uses `(type, value)` cells. A present null is its own `(null)` cell and a missing key is the not-set cell (0003 Common terms). Each group-by adds up to the 5 base events:

- `qty`: (integer, 1) has 2 events, and (string, "1"), (decimal, 2.5000) and (integer, 3) have 1 each.
- `color`: red has 2, blue has 1, not set has 2.
- `coupon`: SUMMER has 1, null has 1, not set has 3.

The catalog reports observed types per key with counts: `qty` has integer 3, string 1, decimal 1. `ratio` has decimal 3. `coupon` has null 1, string 1. The other keys have a single type.

## Acceptance exploration (`acceptance-exploration.json`)

This is the 0004 Complete example on site A: measures `events` and `visitor_days`, split by country and referrer domain, with filters device mobile, event `signup` and `plan = "pro"`. It is evaluated on 2026-10-03.

The range is the last 30 local days, 2026-09-04 to 10-03, which resolves to `[2026-09-03T22:00Z, 2026-10-03T22:00Z)`. The previous period is the 30 days before it, `[2026-08-04T22:00Z, 2026-09-03T22:00Z)`, which is local 08-05 to 09-03 (0003 Comparison and zero baselines). The range contains now, 12:00 local on 10-03, so the previous totals stop at the same offset into their span: `[2026-08-04T22:00Z, 2026-09-03T10:00Z)`, until 12:00 local on 09-03.

Device, country and referrer domain are session fields (0004 Field scope). Every signup here shares its session with the landing pageview that carries the referrer, so each event takes its session's values.

| Event | Local day | Device, country, referrer, plan | Current | Previous |
| --- | --- | --- | --- | --- |
| acc-02 | 08-04 | mobile, AT, news, pro | | out: 23:55 local 08-04 is before compare_start |
| acc-04 | 08-20 | mobile, DE, search, pro | | yes |
| acc-06 | 09-03 | mobile, AT, news, pro | | out: 23:59:59.999 local is after the 12:00 offset of now |
| acc-08 | 09-04 | mobile, AT, news, pro | yes: the session starts at exactly 22:00:00.000Z | |
| acc-10 | 09-05 | mobile, AT, news, pro | yes | |
| acc-12 | 09-06 | mobile, AT, news, pro | yes: same IP and UA as acc-10, new day | |
| acc-14, acc-15 | 09-10 | mobile, AT, news, pro | yes, 2 events, 1 pseudonym | |
| acc-17 | 09-12 | mobile, AT, news, `Pro` | out: exact string match | |
| acc-19 | 09-30 | mobile, DE, search, pro | yes | |
| acc-21 | 09-30 | desktop | out | |
| acc-23 | 10-02 | plan `free` | out | |
| acc-25 | 10-03 | mobile, AT, social, pro | yes: today, a partial day | |

Totals:

- Events: current 7, previous 1. The change is 6 and the relative change 6 / 1 = 6.0.
- Visitor-days: current 6 distinct pseudonyms (AC10 09-04, AC1 09-05, AC1 09-06, AC2 09-10, AC3 09-30, AC6 10-03), previous 1. The change is 5 and the relative change 5.0.

Rows, sorted by events descending, with ties broken by dimension values ascending (0004 Semantics):

| Country, referrer | Events now / before | Change, relative | Visitor-days now / before | Change, relative |
| --- | --- | --- | --- | --- |
| AT, news.example.net | 5 / 0 | 5, null `zero_baseline` | 4 / 0 | 4, null `zero_baseline` |
| AT, social.example.net | 1 / 0 | 1, null `zero_baseline` | 1 / 0 | 1, null `zero_baseline` |
| DE, search.example.net | 1 / 1 | 0, 0.0 | 1 / 1 | 0, 0.0 |

The Austria drill-down (condition `f4`) drops the DE row:

- Events: 6 now and 0 before, a change of 6 and relative change null with `zero_baseline`.
- Visitor-days: 5 now and 0 before, a change of 5 and relative change null with `zero_baseline`.

## Resolved questions

Three questions from earlier rounds are settled in [0003](../../docs/decisions/0003-metric-formulas.md). None of them changes an expected value here.

1. **Touch of a session without a pageview.** The touch is the first pageview, or the first eligible event when the session has no pageview, matching 0004. att-10 stays direct, because its only touch is not before the conversion.
2. **Pageview journeys and sessions without pageviews.** A pageview journey only includes sessions with at least one pageview. All sessions on 2026-06-18 have pageviews.
3. **When `series_sum_differs` is set.** Only when the returned series actually sums to more than the headline. Site B sets it and site A doesn't.

Consent without a signed token, revocation keeping history, and accepting any event time are confirmed by Mike (2026-10-03, 0001 and 0002). Dora's revocation history is not exercised: she sits outside every retention and goal range.

## Not covered here

Collection-surface behaviour has its own fixtures under `fixtures/collection/`: bots, oversize bodies, property limits, malformed consent values, link and pixel totals, and recorder fragments. Imported history is not part of this fixture. Running it through the importers is milestone 11 work.
