# Workload and query suite

Milestone 01, work item 5, large-workload part. This page is the reading guide for the frozen generator contract and query suite that milestone 02 implements. The JSON files are the authority:

| File | Content |
| --- | --- |
| [`fixtures/workload/manifest.json`](../../fixtures/workload/manifest.json) | Generator contract: clock, PRNG, seeds, UUIDs, sites, every count and distribution, burst, soak, correction, retention-advance, cap test, measurement method |
| [`fixtures/workload/queries/`](../../fixtures/workload/queries/) | 80 query cases, one file each, Q01 to Q14 |
| [`fixtures/workload/streams.json`](../../fixtures/workload/streams.json) | Two dashboard user streams, the early-gate subset and its streams |

Status: frozen before storage tuning. The numbers in [performance validation](../performance-validation.md) marked as proposed procedures are frozen here. The confirmed targets (hardware, retention, burst, concurrency, the four p95 limits) are restated, not changed. Formulas come from [0003](../decisions/0003-metric-formulas.md), state shape and limits from [0004](../decisions/0004-exploration-state-and-limits.md), event and session rules from [0002](../decisions/0002-sessions-and-events.md), identity from [0001](../decisions/0001-identity-and-consent.md), retention cutoff and application clock from [0006](../decisions/0006-retention-clock-and-operations.md).

## What is frozen

### Clock and randomness

- Measurement clock `2026-10-01T00:00:00Z`. Events from `2025-10-01T00:00:00Z` up to, not including, the clock.
- The application under test uses the `Clock` from [0006](../decisions/0006-retention-clock-and-operations.md). Load, soak, and correction runs use mode `run` starting at the measurement clock. Exact boundary cases (24 h and 5 min edges) use mode `frozen` and move it forward with `testing.setClock`, because HTTP delay moves an exact boundary.
- PRNG: SplitMix64 with Vigna's constants. Draws are random access, `draw(seed, i) = mix64(seed + (i + 1) * gamma)`, so generation can be split across workers and never needs the dataset in RAM.
- Master seed `0x484F4C4F43524F4E` ("HOLOCRON"). Each stream seed is `mix64(master XOR fnv1a64(name))`. All 33 stream seeds and test vectors are in the manifest.
- Exact-count selections use an affine bijection `(a * i + b) mod N`. It gives exact totals with closed-form indexing and spreads every residue class evenly, so no selector shares a modulus with the site rule.
- UUIDs are version 5 under the fixture namespace `63fbcd3b-48d6-5d90-aa04-53ffd8a4c059`, with names like `site:A` and `event:{ordinal}`.

### Sites

| Site | Timezone | Primary events | Group residues mod 40 |
| --- | --- | --- | --- |
| A | Europe/Vienna | 60,000,000 | 0 to 19 |
| B | America/New_York | 30,000,000 | 20 to 29 |
| C | Asia/Kolkata | 3,000,000 | 30 |
| D | UTC | 3,000,000 | 31 |
| E | Europe/London | 3,000,000 | 32 |
| F | America/Los_Angeles | 3,000,000 | 33 |
| G | Asia/Tokyo | 3,000,000 | 34 |
| H | Australia/Sydney | 3,000,000 | 35 |
| I | America/Sao_Paulo | 3,000,000 | 36 |
| J | Europe/Berlin | 3,000,000 | 37 |
| K | Asia/Kathmandu | 3,000,000 | 38 |
| L | UTC (changes to America/Chicago in the correction run) | 3,000,000 | 39 |

Site D is the smaller site in the query suite.

### Retained fixture

- 12 months of 1,000,000 capture groups, ten events each, 30 seconds apart. Months and days are site-local calendar dates, so each site gets exactly its share per local month. UTC-month totals differ slightly and are published by the generator.
- Groups land on local days by the affine rule, about 33,333 per day, and start uniformly between local midnight and 23:50. No group crosses midnight.
- Event kind by `ordinal mod 20`: 15 pageviews, 4 custom events, 1 performance event.
- 4,000,000 identified groups with consent, 400,000 supplied IDs, ten groups each, never twice in a month. The generator emits client inputs (IP from `2001:db8::/32`, user agent), not pseudonyms. The loader derives identities with the production derivation, so late events sent later reach the same identity.
- Dimensions cover every declared value through deterministic coverage rows, then follow integer Zipf weights `floor(2^40 / (k + 1))` with a per-dimension head share and empty share. Path `/` and referrer `google.com` dominate.
- 80,000,000 records carry exactly three properties (240,000,000 values): every custom event plus 56,000,000 selected pageviews. One key per slot: string, numeric, date/bool/array. The 48 keys, their types, weights, value rules, null rates, and byte sizes are listed in the manifest.
- 8,000,000 session property values on identified groups, carried by 4,000,000 identify messages. These messages are associated records, not primary events.
- Acceptance plants: 40 groups per local day become mobile with a `signup` event and `plan=pro`, spread over all sites. Generation fails if a Q03 case has no match in either comparison period.

### Revenue

1,200,000 `purchase` events, 300,000 each in EUR, USD, GBP, and JPY. Amounts cycle through ten fixed tiers, and 2% are refunds, so the sums have a closed form: gross is 29,400 times the tier sum and refunds are -600 times it.

| Currency | Exponent | Gross (minor units) | Refunds | Net |
| --- | --- | --- | --- | --- |
| EUR | 2 | 2,969,106,000 | -60,594,000 | 2,908,512,000 |
| USD | 2 | 2,998,506,000 | -61,194,000 | 2,937,312,000 |
| GBP | 2 | 2,954,406,000 | -60,294,000 | 2,894,112,000 |
| JPY | 0 | 2,969,400,000 | -60,600,000 | 2,908,800,000 |

The `revenue` property stores as decimal for EUR, USD, and GBP and as integer for JPY. That gives one realistic mixed-type key on purpose. The four-currency split is adopted in [0002](../decisions/0002-sessions-and-events.md).

### Performance

6,000,000 events, one per even group. Each metric has a fixed missing rate, an explicit zero rate, and an integer knot table, scaled by device (desktop 1.0, tablet 1.2, mobile 1.4). Percentiles are nearest-rank from 0003.

### Associated data and run manifests

- Replay: 50,000 captures with 5 fragments of 65,536 bytes in the last 14 local days, 16,384,000,000 payload bytes.
- Heatmap: 100,000 interactions per local day for 90 days, 9,000,000 total. The aggregate cell key is provisional until milestone 09.
- Burst: 3,000 groups at 500 events/s for 60 s. 300 recorded groups send 3,000 fragments, plus 3,000 heatmap interactions.
- Soak: 345,600 baseline events at 4/s plus 24 hourly bursts, 1,065,600 offered events. Soak replay plus the retained replay is 23.4 GB, under the 25 GiB cap.
- Correction: 1,000 late events in five kinds aimed at existing groups from 2026-09-30, 200 keyless late events aimed at October 2025 to August 2026 groups (anonymous pageviews and purchases that must not touch identity measures, plus consented pageviews that link to the profile by event time, [0001](../decisions/0001-identity-and-consent.md)), and 100 out-of-order identify changes (50 must not win, 50 must). Boundary cases: 30-minute gap at 1800 s and 1799 s, local midnight for Vienna, Kolkata, and UTC (anonymous and consented), 24 h and 24 h + 1 s late (both stored, since no event time is refused, [0002](../decisions/0002-sessions-and-events.md)), 5 min and 5 min + 1 s ahead (only the second counts in `event_time_ahead`), 400 days ahead, the retention cutoff and one second before it (`outside_retention`), and a live timezone change on site L. DST cases are planted in the retained fixture on both Vienna and New York transitions.
- Retention advance: `testing.setClock` to `2026-11-01T00:00:00Z`, 10,000,000 October events, removal by the [0006](../decisions/0006-retention-clock-and-operations.md) cutoffs (events by site-local date minus 12 months, replay captures whole after 14 days, heatmap cells after 90 days). 100 saved captures must survive the 14-day removal.
- Cap test: separate deployment prefilled 1 MiB below 25 GiB (409,584 fragments). Of 64 offered fragments, 16 are admitted and 48 are suppressed with `replay_budget_exceeded`, while ordinary collection keeps accepting. A saved-replay case prefills saved captures 1 MiB below the 5 GiB sub-cap: one 10-fragment save succeeds, the next is refused with `saved_replay_budget_exceeded` ([0006](../decisions/0006-retention-clock-and-operations.md)).

### Measurement

- At least 200 observations per timed case, 100,000 acknowledged events per warm phase, three independent runs that each pass.
- Cold: 40 rebooted-VM starts per cold case. The 24 cold cases are the 30-day site A Vienna and 12-month site A UTC case of Q01 to Q12.
- Warm uncached: 200 runs per case. Warm cached is labeled and never replaces warm uncached.
- Frozen qualification: the 2 s and 10 s p95 targets apply to both cold and warm uncached cases. No blended p95.
- Queries apply the [0006](../decisions/0006-retention-clock-and-operations.md) cutoff as a predicate. Golden results assume the application clock's local date is 2026-10-01 in the queried site's timezone. The 24-hour soak crosses local midnight, so its correctness checks use oracle results for the clock's current local date.
- Selectivity classes: selective is at most 0.1% of the case's in-range population, medium is 1% to 10%, broad is at least 25%.

## What milestone 02 fills in after first generation

Each item is measured once from the first generation and then fixed:

- canonical input hashes and the readable sample (first 1,000 groups of September 2026)
- exact counts per kind, site, local day, and UTC day, plus site-scoped person counts
- per-key observed cardinality, null and missing counts, byte sizes, and observed joint distributions
- planted group counts, expected percentiles, replay template set with compression ratio, heatmap aggregate rows
- each query case's `row_count`, `matched_records`, and `golden_result_hash`, plus the per-family canonical result document the hash covers
- oracle deltas for the correction workload, the retention removal manifest, and the correction completion target (approved by Mike before the final gate)

A case that misses its selectivity class gets a different literal before hashes are fixed. The data never changes for a query. After that, a changed hash is a failure, not an update.

## Query cases

Every case file carries an exploration state v1 exactly per 0004 (or `null` for operations that have no state). Q05 session lists and Q06 to Q09 use the typed `report` part (`session_list`, `funnel`, `journey`, `attribution`, `retention`), and Q12 uses `PropertyStat` measures for its numeric summary. Each file also carries authorized sites, resolved half-open bounds (with `retention_start` and `compare_retention_start` where a bound starts before the cutoff), timezone, versions, parameters, sort order with tie rule, output bound, selectivity, latency class, and placeholders for the expected values. Names follow `Q{nn}-{window}-{site}-{timezone}[-{variant}]`.

| Family | Cases | Notes |
| --- | --- | --- |
| Q01 traffic | 4 | hourly 30-day series on site A (720 points), previous period |
| Q02 pages and referrers | 7 | top 100, path tail filter, page index 99 |
| Q03 acceptance | 6 | the 0004 example with and without the `f4` drill-down |
| Q04 grouped predicates | 7 | nesting depth 3, repeated fields, `is_not_set`, selective and broad |
| Q05 sessions | 6 | `session_list` reports with `span` in integer ms, plus two single-session activity cases |
| Q06 funnel | 4 | `funnel` report: three steps, a repeated step, property predicate, 10-minute window |
| Q07 journey | 4 | three-step paths, top 100, loop compression |
| Q08 attribution | 4 | last touch, 30-day lookback, revenue per currency |
| Q09 retention | 4 | `retention` report: consented profiles, 30 daily or 13 weekly intervals, grouped predicate |
| Q10 revenue | 6 | currency series and filtered breakdown |
| Q11 performance | 4 | p50/p75/p95 per metric and device |
| Q12 properties | 14 | all five types, `PropertyStat` count/min/max/sum of `price`, pivot, sku tail, session properties, selective and broad |
| Q13 replay and heatmap | 4 | recording list, one replay fetch, heatmap cells for sites A and D |
| Q14 live and control | 6 | realtime, status, annotations, saved exploration, board, restricted share |

Q01 to Q12 each have 30-day and 12-month cases on site A and site D, in UTC and Europe/Vienna. 30-day cases cover local 2026-09-01 to 2026-09-30, annual cases 2025-10-01 to 2026-09-30. The annual previous period lies before the fixture, so its comparison is all `zero_baseline` by construction. Q05 activity, Q13, and Q14 have their own latency distributions and run inside concurrent load.

The early-gate subset is every Q01 to Q04 case plus `Q06-30d-A-vienna`, 25 cases in total.

## Streams

User A repeats Q01, Q02, Q03, Q06, Q11. User B repeats Q04, Q07, Q08, Q09, Q12. Each cycle has three 30-day rounds and one annual round, and rotates through each family's cases by a fixed formula. Think time is 500 ms with seeded jitter of plus or minus 50 ms. One request is in flight per user, and queue wait counts as latency.

Periodic work: each user refreshes a live snapshot every 2 s on its own subscription connection. Once a minute, user A saves and reopens an exploration and reopens a board, and user B issues one Q13 request in rotation. Minute tasks queue as the user's next request.

## Supported query complexity envelope

The gate promises the latency targets for the cases in this suite, not for arbitrary queries. The limits are 0004's and are not redefined here: nesting depth 3, 32 conditions, 50 list items, 3 dimensions, 8 measures, page size 100, 10,000 addressable rows, 744 series points, 366-day relative range, 30 s cancellation, 8 funnel steps, 7 journey steps. The suite reaches these points:

- nesting depth 3 and repeated same-field conditions (Q04)
- two dimensions with a drill-down condition (Q03), two property dimensions (Q12 pivot)
- the last addressable page, rows 9,900 to 9,999 (Q02)
- 720 hourly points (Q01)
- 12-month ranges on the dominant site

Queries outside these cases still run and stay exact. The product exposes the full feature set, and the 30 s cancellation still applies.

The report-kind limits of 0004 apply as well: funnel window 60 s to 30 days, 4 attribution conversions, 32 day or 53 week retention intervals, 3,000 retention cells, 16 session list columns, and the 32-condition limit counted across `filter` and every report predicate. The largest retention case is 30 daily cohorts by 30 intervals, 900 cells.

## Choices made here

These were proposed procedures in performance validation. They are now frozen, and the ones a decision record adopted link to it:

- Revenue is split 300,000 each across EUR, USD, GBP, and JPY, keeping the 1,200,000 total. Adopted in [0002](../decisions/0002-sessions-and-events.md).
- Month and day placement use site-local dates, the reading the "local day start" sentence supports.
- The retention cutoff removes events whose site-local date is before the clock's local date minus 12 calendar months. Adopted in [0006](../decisions/0006-retention-clock-and-operations.md), together with the test clock.
- DST boundary cases are planted in the retained fixture. A live late event can reach those days, but it is keyless there and cannot test anonymous session boundaries.
- Cold cases are limited to two per family (960 cold starts for the final gate), because a rebooted VM per observation is expensive.
