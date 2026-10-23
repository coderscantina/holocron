# Performance and reliability validation

This contract extends [the technical briefing](technical-briefing.md). Its numbers are requirements and proposed test inputs, not measured results. Publish the workload manifest, fixture generator, query definitions, engine versions, and run configuration before tuning. Keep failed runs in the results.

The reference hardware, retention, burst, concurrency, and four p95 targets are confirmed requirements. Exact fixture distributions, query parameters, sample counts, cold-cache methodology, and soak duration below are proposed reproducible procedures. Milestone 01 froze them in the [workload manifest](../fixtures/workload/manifest.json), [query cases](../fixtures/workload/queries/), and [streams](../fixtures/workload/streams.json); [the workload guide](contracts/workload-and-query-suite.md) lists each choice and what milestone 02 fills in after first generation. They must not be described as additional product choices already approved by Mike.

The initial subject is one Rust process serving collection, dashboard operations, subscriptions, and Vue assets. SQLite with WAL and `synchronous=FULL` owns the durable inbox and control state. DuckDB owns analytical records and an applied-batch ledger. The gate may lead to a different embedded engine arrangement. It cannot reduce feature scope, accuracy, retention, or the agreed machine and latency limits.

## Fixed deployment and targets

| Item                               | Contract                                                        |
| ---------------------------------- | --------------------------------------------------------------- |
| Application host                   | 2 vCPU, 4 GiB RAM, 250 GiB local SSD                            |
| Retained primary events            | Exactly 120,000,000 over 12 calendar months                     |
| Nominal monthly volume             | Exactly 10,000,000 primary events per month                     |
| Burst                              | 500 primary events/second for 60 seconds, exactly 30,000 events |
| Concurrent dashboard users         | Two independent authenticated users with separate query streams |
| Durable collection acknowledgement | p95 at most 250 ms                                              |
| Initial analytical visibility      | p95 at most 2 seconds                                           |
| Common 30-day reports              | Each defined query's p95 at most 2 seconds                      |
| Defined 12-month explorations      | Each defined query's p95 at most 10 seconds                     |
| Retention                          | Events 12 months, replay 14 days, aggregated heatmaps 90 days   |
| Recording                          | 10% capture sampling when enabled, 25 GiB raw replay cap        |

Use a pinned Linux VM image, filesystem, mount options, CPU model, storage class, kernel, and container limits. Record whether vCPUs are shared or dedicated. Run the load generator and browser automation on separate hosts. Keep their clocks synchronized and record measured clock offset; use one monotonic measurement clock where possible. Store the exact host and engine settings with each result.

The application deployment includes its data, WAL files, inbox, analytical files, assets, logs retained locally, and query spill. It must operate inside the 250 GiB volume without borrowing the generator's disk. Backups use a separately declared destination whose cost and capacity are reported. Do not count that destination as free.

Record the software build commit, release profile, enabled features, allocator, SQLx version, SQLite version and compile options, DuckDB version, dependency lockfile digest, schemas, metric versions, and admission/query limits. Do not use debug builds for either capacity or cost results.

## Reproducible retained fixture

Freeze the measurement clock at `2026-10-01T00:00:00Z`. Generate events from `2025-10-01T00:00:00Z` through, but excluding, that clock. A calendar month contains exactly 1,000,000 capture groups of ten events. A capture group is a generation unit, not a claim about the product's derived sessions.

Use a published seed and a specified integer PRNG algorithm, not the language's default random implementation. Assign UUIDs deterministically from the fixture namespace and record ordinal. All data is synthetic. Publish canonical input hashes and a small independently readable sample. Generation must not require retaining the whole dataset in RAM. Frozen: SplitMix64, the master and stream seeds, and UUIDv5 names are in the manifest's `prng` and `uuid` sections.

| Primary fixture component    | Exact count or distribution                                                                                                                                                                |
| ---------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Primary records              | 120,000,000: 90,000,000 pageviews, 24,000,000 custom events, 6,000,000 performance events                                                                                                  |
| Sites                        | 12: 60,000,000 events at site A, 30,000,000 at site B, 3,000,000 at each remaining site                                                                                                    |
| Daily capture identities     | 12,000,000 distinct site/day pseudonyms, ten primary events each                                                                                                                           |
| Identified capture groups    | 4,000,000; remaining 8,000,000 are anonymous                                                                                                                                               |
| Explicit supplied identities | 400,000 identifiers, ten identified capture groups per identifier; site-scoped person counts are computed by the oracle                                                                    |
| Typed event properties       | 80,000,000 records have three properties, exactly 240,000,000 property values; remaining records have none                                                                                 |
| Property keys                | 48: 16 string, 16 numeric, 16 date/boolean/array keys with a published type assignment                                                                                                     |
| Revenue                      | 1,200,000 custom events carry normalized revenue; 300,000 each in EUR, USD, GBP, and JPY; integer minor units with an exact sum per currency (frozen in the manifest)                                                 |
| Dimensions                   | 100,000 paths, 1,000,000 full query strings, 10,000 referrer domains, 64 countries, 256 region values, 16 browsers, 8 operating systems, 3 devices, 32 custom event names, 1,000 campaigns |
| Session properties           | Two properties on each identified capture group, exactly 8,000,000 initial values; correction fixtures exercise replacement separately                                                     |

Use global capture-group ordinal modulo 40 for site allocation: residues 0 through 19 use A, 20 through 29 use B, and each residue 30 through 39 uses one other site. Within each month, mark the first 333,333 capture groups identified in the first eight months and the first 333,334 in the remaining four. Assign supplied IDs by identified-group ordinal modulo 400,000. This gives four million identified groups and ten groups per supplied ID, with no repeated supplied ID within a month. Consent is explicit on those groups. Generated daily pseudonyms are unique within each site/day; imports do not manufacture them.

Within each month distribute capture groups across its UTC days deterministically. Choose each group's first timestamp inside the interval from local day start through 23:50, then place its ten records thirty seconds apart. Publish site timezones and include UTC, Europe/Vienna, America/New_York, and Asia/Kolkata. Frozen: days are site-local calendar dates; the site list, day placement, and start-time rule are in the manifest. Use event ordinal modulo 20 to assign fifteen pageviews, four custom events, and one performance event. Use a separate seeded permutation for property selection, then choose exactly 80 million records. Avoid making every custom event a mobile signup merely because field selectors share a modulus.

For each dimension, reserve enough deterministic rows to cover every declared value, then apply a published Zipf-like skew to the remainder. The exponent and hash-to-value mapping are frozen in the manifest's `dimension_rules`. Include a dominant path, a dominant referrer, high-cardinality tails, empty values, and missing properties. State observed joint distributions for device, event name, plan, country, and referrer. The query acceptance fixture must contain mobile signup events with `plan=pro` in both comparison periods.

Properties include string plan and campaign values, scaled numeric prices/counts, valid UTC dates, booleans, and bounded arrays. Publish value cardinality, null/missing frequency, array lengths, and UTF-8 byte sizes for every key. Include path and event-property predicates with selective, medium-selectivity, and broad matches. Fix each query's actual matched-record count in the manifest after generation; never let selectivity drift between runs.

Generate performance values from frozen distributions with explicit zero and missing values, and publish independent expected percentiles. Revenue totals use integer scaled units in the oracle. Specify decimal conversion and rounding before comparing engine results. No implicit currency conversion is allowed.

The generator produces an input inventory, expected exact event/type/site/day counts, property counts, consent coverage, revenue sums, and hashes. A separately implemented streaming oracle computes golden results for the defined reports. It must not call the application's query compiler or reuse its session implementation. A small hand-reviewed fixture establishes formulas before the large fixture is trusted.

## Recording and associated data

Recording storage and processing share the same machine budget. They cannot be disabled in the final integrated run to make ordinary reports pass.

The retained recording fixture has exactly 250,000 valid masked replay fragments of 65,536 bytes each, 16,384,000,000 stored payload bytes before engine/container overhead, distributed over the last 14 days. Fragments are valid supported-recorder payloads with realistic dictionary redundancy, not random padding. Publish compressed transport sizes separately. Include capture IDs, chunk order, and final-chunk markers sufficient to replay selected sessions.

The heatmap fixture contains exactly 9,000,000 input interactions distributed over 90 days, with a manifest of resulting aggregate cells. Associated storage counts must distinguish primary events, properties, replay fragments, input interactions, and aggregate rows. Do not call the whole deployment a 120M-row dataset.

For the burst, every ten primary events form one capture group. Exactly 300 of its 3,000 groups are selected for recording, each sends ten 65,536-byte fragments during the minute, adding 3,000 fragments. Exactly 3,000 heatmap interactions arrive during the minute. Performance and revenue records follow the primary fixture's fixed ratios. Do not add replay bytes to the primary event count.

Run a separate cap test beginning 1 MiB below the configured 25 GiB replay limit. Verify capture admission, recorded outcomes, transfer, cleanup, and UI freshness at and above the cap, and the 5 GiB saved-replay sub-cap inside it ([0006](decisions/0006-retention-clock-and-operations.md)). Ordinary collection must remain available. The retained event fixture and cap test are different manifests; do not merge their disk figures without labeling them.

## Query suite and dashboard behavior

Every query definition includes normalized exploration state (ExplorationState v1 from [0004](decisions/0004-exploration-state-and-limits.md)), authorized sites, resolved absolute dates, timezone, metric/session versions, parameters, expected row count, golden-result hash, sort order, and bounded output size. These definitions are the supported complexity envelope for the latency gate, within the limits set in 0004. The product still exposes the full agreed feature set. The frozen cases are in [`fixtures/workload/queries/`](../fixtures/workload/queries/); expected row counts and golden hashes are fixed after first generation by milestone 02.

| ID  | Query family             | Required cases                                                                                                                 |
| --- | ------------------------ | ------------------------------------------------------------------------------------------------------------------------------ |
| Q01 | Traffic overview         | Hour/day series, pageviews, exact daily identities, visitor-days, observed span, engagement/bounce, previous-period comparison |
| Q02 | Pages and referrers      | Exact totals, top 100 with deterministic ties, high-cardinality tail filter, pagination beyond the first page                  |
| Q03 | Acceptance exploration   | Mobile AND signup AND `plan=pro`, split country/referrer, drill into country, previous-period comparison                       |
| Q04 | Grouped predicates       | Repeated same-field conditions and nested AND/OR on dimensions plus typed properties; missing-value behavior                   |
| Q05 | Sessions                 | A site's paginated sessions and one complete activity sequence; daily rotation and inactivity policy                           |
| Q06 | Funnel and goal          | Three ordered steps within a fixed window, repeated steps, property predicate, exact conversion counts                         |
| Q07 | Journey                  | Three-step paths with a fixed top-100 result, exact counts and explicit handling of loops                                      |
| Q08 | Attribution/campaigns    | Frozen model and lookback, distinct campaign breakdown, goals and revenue kept per currency                                    |
| Q09 | Retention/cohorts        | Only consented persistent identities, defined day offsets, eligibility coverage, grouped cohort predicate                      |
| Q10 | Revenue                  | Currency-separated sum/count and time series, filtered event/property breakdown                                                |
| Q11 | Performance              | Nearest-rank p50/p75/p95 per metric and device, no interpolation, missing values excluded, zeros kept ([0003](decisions/0003-metric-formulas.md)) |
| Q12 | Event/session properties | String/numeric/date/boolean/array filtering, numeric summary, property pivot and cardinality tail                              |
| Q13 | Replay/heatmap           | Recording list, fetch/order one replay, heatmap cells for a page/version/viewport; bounded payloads                            |
| Q14 | Live/control             | Realtime snapshot, freshness/status, annotations, saved exploration, board and access-scoped snapshot                          |

Instantiate Q01 through Q12 for the last 30 days and for all 12 months, except where a feature has a narrower defined retention such as replay. Include dominant site A and a smaller site, UTC and Vienna, and declared selective/broad cases. Q01 through Q12's 30-day report cases each meet 2 seconds; their declared annual exploration cases each meet 10 seconds. Q05's single-session activity and Q13/Q14 get separate latency distributions and correctness assertions. They are not silently excluded from concurrent load.

An exact daily distinct count must not be replaced by an approximate aggregate. Oracle comparison uses exact integers and scaled currency totals, documented percentile tolerance only for representation, and canonical sorted rows. Hidden event sampling is forbidden. Default pagination can bound output, but cannot change reported totals.

Each dashboard user executes one request at a time with a 500 ms think time. User A repeats Q01, Q02, Q03, Q06, Q11; user B repeats Q04, Q07, Q08, Q09, Q12. Alternate 30-day and annual cases at a fixed 3:1 ratio. Include live snapshots every two seconds, one saved-view mutation and board reopen per minute, and Q13 replay/heatmap requests once per minute. Stream order and jitter seed are frozen in [`streams.json`](../fixtures/workload/streams.json). Record queue wait as part of latency. Do not report only analytical execution time.

The final browser test performs the briefing's complete investigation, save, board pin, and reopen workflow with the real Vue application. Measure HTTP result latency and time until the result is painted separately. Cancellation must prevent stale results from replacing the newest state. Performance API targets do not imply a measured browser rendering target; publish both without substituting one for the other.

## Measurement rules

Record offered, accepted, policy-suppressed, rejected, timed-out, and successfully visible records separately. An intentionally filtered bot request is not an accepted stored event. Count failures in the latency report; never discard timed-out requests or only summarize successful fast responses. Use an open-loop collection generator at the specified arrival rate, so server delay does not reduce offered load.

Collection latency starts before sending a request and ends after the full response is received. A successful stored result corresponds to a durable SQLite commit. Instrument queue admission, group-commit wait, transaction, synchronization, response encoding, and network phases for diagnosis. The end-to-end number is the gate.

Visibility starts at the accepted response and ends when an authorized analytical query can see the exact record and the committed-ingestion watermark covering it. Also publish send-to-visible latency. Probe every burst event through batched ID lookups at a fixed 100 ms cadence, outside the timed report pool, and state the probe overhead. A notification alone does not establish visibility. Record each probe's consistent result/watermark snapshot and clock uncertainty.

Collect at least 200 observations per timed query case and 100,000 acknowledged primary events per warm latency phase. Use nearest-rank p95 and publish sample count, min, median, p95, p99, maximum, histogram, errors, and run-to-run variation. Complete three independent runs from the same fixture. Each run must pass; an average of one passing and two failing runs does not pass. Do not average all query families together to hide one slow family.

Cold and warm results are separate:

1. A cold case starts from a restored identical fixture, an application/engine restart, empty application result cache, and a rebooted dedicated VM to clear host page cache. Run at least 40 independent cold starts per cold query case. Include startup-to-ready time separately.
2. A warm uncached case keeps the engines and host cache warm but bypasses the application result cache. Run the fixed suite at least 200 times per query case.
3. A warm cached case enables the production cache and reopens the same saved state. Label cache hits and misses; it cannot replace the warm uncached gate.

The confirmed latency numbers apply to both cold and warm uncached report cases. Milestone 01 froze that qualification in the manifest's `measurement` section alongside the query suite. Publish cold/warm results independently; no blended p95. Never clear shared-host caches or claim that restarting the process cleared the operating system cache. If genuine cold execution cannot be reproduced, mark that part of validation unverified.

Memory accounting includes the entire Rust process, all engine threads, allocator arenas, recorder processing, subscriptions, request buffers, query results, and pending work. Report process RSS/PSS, cgroup current/peak anonymous memory, file cache, kernel memory, swap, and OOM events. The deployment's 4 GiB limit includes its charged file cache and kernel usage; process RSS below 4 GiB is insufficient. Disable host swap for the fixed gate and record the setting.

Engine spill is disk usage, not additional RAM. Measure peak temporary bytes, sustained/peak I/O, query-spill lifetime, inbox bytes, both WALs, replay bytes, and total data-volume occupancy. Report host page cache separately from engine buffer settings. No spill directory may live on an unreported volume. Query concurrency, thread count, queue limits, and timeouts are frozen before runs.

## Handoff, corrections, and failure checks

Create deterministic failpoints in the prototype around each transfer boundary. They exist to verify the protocol; remove ad hoc scratch tools and retain only focused test support justified by this contract.

| Interruption point                            | Required recovery                                                                                                      |
| --------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| Before inbox commit                           | No successful stored acknowledgement; retry can be admitted                                                            |
| After SQLite commit, before response          | Record remains recoverable; without optional client ID a request retry may be a separate event, and this is documented |
| After acknowledgement, before assignment      | Every accepted record remains pending and transfers after restart                                                      |
| After stable assignment commit                | Resume the same batch ID and exact membership                                                                          |
| During DuckDB writes before commit            | No partial batch/ledger becomes visible; replay the assigned batch                                                     |
| After DuckDB commit, before SQLite retirement | Ledger prevents reinsertion; retire matching records after reconciliation                                              |
| During SQLite retirement                      | No accepted record is lost and no internally replayed event is duplicated                                              |
| After analytical commit, before notification  | Queries see the committed watermark; restart reconciliation restores notifications                                     |
| During correction application                 | Result is either the old labeled calculation or the complete new version, never mixed rows claiming complete freshness |

Inject process termination at every boundary at least twenty times with fixed seeds, including nonempty batches of different sizes, recording fragments, properties, and late events. The oracle checks every accepted ID, property, ledger assignment, exact totals, and required correction state after recovery. Test request idempotency separately with optional client IDs and dashboard command keys; internal transfer deduplication is not proof of client retry deduplication.

Process termination does not test power-loss durability. Use controlled abrupt VM resets after confirmed acknowledgements and during group commit/transfer, at least twenty resets per selected boundary. After reboot verify every confirmed accepted record and engine integrity. Describe the virtual storage's flush guarantees and limitations. An environment that cannot exercise power loss or guarantee flush behavior cannot certify that part of the contract.

Keep initial visibility and correction freshness separate. Inject exactly 1,000 late events from the last 24 hours, 200 older late events whose day keys are destroyed (stored keyless, [0001](decisions/0001-identity-and-consent.md)), 100 out-of-order identify/property changes, and boundary cases at the 30-minute timeout, local midnight, both DST transitions, and a site timezone change. No event time is refused ([0002](decisions/0002-sessions-and-events.md), Mike 2026-10-03). Include events 24 hours and 24 hours plus one second late (both stored), 5 minutes and 5 minutes plus one second ahead (only the second counts in `event_time_ahead`), one far in the future, and events just before and at the retention cutoff (the earlier is suppressed as `outside_retention`, [0006](decisions/0006-retention-clock-and-operations.md)). Frozen in the manifest's `correction` section. Publish oracle session, funnel, attribution, revenue, cohort, and duration changes affected by these records. Dirty ranges and calculation versions survive interruption.

After a correction workload drains, require all affected golden results to match and no outstanding dirty range to remain. Record p95/p99/worst correction completion latency from accepted response, time until each report stops being labeled stale, and results observed during correction. The 2-second initial visibility target must not be presented as a 2-second corrected-report guarantee. Freeze a correction completion target in the early-gate manifest after the first measurement and approve it before the final integrated gate; no agreed numerical correction target exists yet.

Deletion/reset tests cover a site with pending assigned batches, an analytical transaction in progress, and a completed batch awaiting cleanup. After successful deletion and restart, neither transfer nor restore reconciliation may resurrect the site's records. Verify query caches, recording chunks, heatmaps, and restricted provenance as well as primary events.

## Sustained load, backpressure, maintenance, and restore

Run a 24-hour soak on the retained fixture at a fixed baseline of four primary events/second. Add the 30,000-event burst once per hour while both dashboard streams run, producing exactly 1,065,600 offered primary events over the day. Record baseline recording/heatmap ratios separately in its manifest. The final successful soak has no unhandled crashes, OOM events, silently dropped accepted records, or duplicate internal transfers.

Measure time for the inbox to return to its pre-burst depth and oldest-record age after every burst. It must not increase from hour to hour. Maintain the p95 acknowledgement, visibility, and report gates throughout the soak. Report tail delays and any rejected load. Rejection during the agreed healthy burst is a failure of that capacity gate, even if backpressure behaves correctly.

Separately pause transfer for ten minutes while offering 500 events/second, then resume it. Set bounded inbox, request, executor, subscription, and spill budgets in advance. When those budgets are exhausted, requests receive explicit rejection before any stored success is returned. Verify recovery, Retry-After behavior where supplied, slow subscriber resync, and recording-cap separation. A stalled report must not stall durable admission indefinitely.

Use a disposable data volume to inject ENOSPC during SQLite commit, DuckDB transfer, query spill, replay admission, backup, and retention/compaction. Preserve reserve space for controlled shutdown and diagnostics through a declared budget. Test read-only storage and I/O errors separately. No failure path may acknowledge a record that only exists in memory. Publish which failures leave collection unavailable and how health reports distinguish that from stale analytics.

Retention tests begin with the full 12-month fixture, then advance the measurement clock by one calendar month and add exactly 10 million new events. Verify the exact eligible-event manifest removed, current retained count, associated property/replay/heatmap deletion, and correction/ledger safety. Do not require 10 million removals merely because months contain that many fixture rows when a site's local retention boundary differs. Use the oracle's timezone-aware cutoff. Measure elapsed maintenance time, query impact, peak disk, and post-checkpoint occupancy. File size need not shrink immediately.

Backup first uses the planned maintenance pause. Drain or freeze the transfer boundary, record both stores' positions, close/checkpoint safely, and capture control state, inbox assignments, batch ledgers, analytical records, schema/metric versions, recording data, and required secrets in one versioned manifest. Do not copy live database files independently and call them a coherent backup.

Restore on a clean VM with no source data directory. Verify accepted IDs, totals, hashes, permissions, saved explorations, replay order, and pending work, then resume collection and transfer. Test a backup containing a nonempty inbox and a DuckDB-committed batch not yet retired. Ledger lifetime and restore validation must prevent replay of forgotten batches. Reject inconsistent paired snapshots with guidance rather than best-effort guessing. Record backup pause, duration, size, restore time, and time to fresh reports. Publish measured recovery-point and recovery-time outcomes; no zero-time recovery claim follows from durable admission.

## Whole-deployment cost comparison

Compare against Umami 3.4.0 at `ec0ff50388c264ed8ce46f00967e92f7e71476ae`, using pinned deployment images, dependencies, schema, and database settings. The comparison is proposed work. There is no measured cost saving or agreed percentage improvement.

Define two upstream baselines separately: the self-hosted application plus PostgreSQL, and, only where relevant to this workload, an application/PostgreSQL/ClickHouse arrangement. Include every process, volume, database, object store, queue if actually used, and required runtime. Do not compare Holocron's process RSS against Umami's Node process while omitting PostgreSQL or ClickHouse.

Measure the baseline on the same machine/storage class and input dimensions. Map only common analytical questions to each API and document differing identity, session, distinct-count, and percentile formulas. Use baseline-native expected results to verify its run; do not fail it merely because Holocron's new metrics differ. Publish unsupported query cases rather than inventing equivalents or removing Holocron scope. Run unchanged supported tracker/recorder scripts where their contracts permit.

Report CPU seconds per million accepted events and per complete query stream, total deployment memory peak, persistent/temporary/backup bytes, volume IOPS and throughput, acknowledgement/report distributions, operator backup/restore steps, and recovery outcomes. State whether each deployment meets the fixed workload. A cheaper configuration that fails it is not a qualifying alternative.

For a monetary comparison, freeze the hosting provider, region, price date, currency, VM/storage/backup/egress rates, and monthly usage model before calculating. Show the calculation and quote sources. Include unavoidable reserved capacity and external services. Keep measured resource results distinct from modeled bills. Report any gain as a measured difference with run variation, not a promised percentage inferred from Rust or embedded storage.

## Capacity sweep

The gate answers whether the reference host meets every target with the full fixture. A capacity sweep answers how much data a budget can serve, for sizing guidance and the installation tiers ([0008](decisions/0008-storage-engine-and-budgets.md#installation-tiers-sizing-guidance)). It supplements the gate and never replaces it.

Load the fixture at three or more scales that bracket the expected limit (for example 20%, 40% and 60% of the retained volume), each into a fresh data directory through admission, inbox and transfer, then let correction finish. Run the full declared exploration suite with two users and at least 20 samples per case at each scale, under the budget being measured. The capacity is the largest scale where every declared case meets its p95 target, stated as the largest site's annual eligible events and the installation's retained records. Report the failing cases of the next scale. Interpolating between measured scales is allowed when labeled; extrapolating beyond the largest measured scale is an estimate.

## Gates and evidence

The early gate uses a release-mode vertical slice with real collection validation, FULL SQLite commits, assigned-batch recovery, canonical DuckDB writes, representative typed queries, and watermarks. It can use a load harness before the full dashboard exists. It loads the full 120M fixture and associated storage, then runs Q01 through Q04, a representative exact ordered Q06 query, burst, memory/disk checks, and transfer failpoints on the agreed host. Include bounded correction and recording processing as workload probes; label any provisional implementation. Freeze this early subset before tuning. Frozen as `early_gate` in [`streams.json`](../fixtures/workload/streams.json). The remaining query families close in their feature milestones and all run in the final gate. A 1M-row smoke run is useful setup verification, not the capacity gate.

The early decision records measured bottlenecks, corrections still requiring a target, disk budget, and whether the initial engine arrangement can satisfy the same contract. Keep a failed gate reproducible before revising the design. Changing engines requires rerunning the relevant gate with the same inputs; it does not authorize narrowing features or replacing exact counts with estimates.

The final integrated gate repeats the performance and reliability tests through the shipped application, unchanged supported modern tracker/recorder scripts, generated dashboard API, real Vue workflows, permissions, subscriptions, all analytical feature families, replay/heatmap load, retention, import snapshots, backup, and clean-machine restore. Run the 24-hour soak and paired-store failure tests on the release candidate. Results from the early prototype cannot certify final integrations.

Publish machine-readable samples and a concise report with fixture hashes, builds, commands, phase logs, exact reconciliation results, latency distributions, resource peaks, spill/backlog graphs, cost methodology, failures, and verification limitations. A check that failed to start is not evidence. Untested power loss, cold execution, or restore is marked unverified and leaves that gate open.
