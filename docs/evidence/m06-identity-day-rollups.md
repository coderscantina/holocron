# Evidence: identity-day rollups for site A reports

Date: 2026-10-04. Follow-up to [m06](m06-traffic-and-exploration.md), which found that site A misses its 30-day and annual targets at full scale when every report scans raw events. Decision: [0008 Rollups](../decisions/0008-storage-engine-and-budgets.md#rollups).

## Run

- Source: commit `0353e44` plus this change, uncommitted.
- Host: Apple M1 Pro, 10 cores, 32 GiB, macOS (Darwin 27.0.0), APFS SSD. Not the reference host.
- C and C++ built with `CFLAGS=-g0 CXXFLAGS=-g0`.

```sh
cargo test -p holocron-analytics -p holocron-server -p holocron-protocol -p holocron-storage
cargo clippy -p holocron-server -p holocron-analytics -p holocron-protocol -p holocron-storage -p holocron-workload --all-targets -- -D warnings

cargo build --release -p holocron-workload
B=target/release/holocron-gate
HOLOCRON_DISK_RESERVE_MIB=256 $B load --data GATE --groups-per-month 10000      # 1%; 100000 for 10%
HOLOCRON_DISK_RESERVE_MIB=256 $B sessions --data GATE
HOLOCRON_DISK_RESERVE_MIB=256 $B explore --data GATE --out OUT --observations 20 --raw    # before
HOLOCRON_DISK_RESERVE_MIB=256 $B explore --data GATE --out OUT --observations 20 --plans  # after
```

Before and after ran on the same corrected store with the same binary: `--raw` forces the raw planner for every case. Correction was timed on a fresh load with the binary from commit `0353e44` (before) and with this change (after). The before binary records no site policies; that changes no session, since every site uses the default 30-minute window.

## What was built

| Part | Where | Behaviour |
| --- | --- | --- |
| Tables | `crates/storage/src/analytics.rs` | `hour_aggregates` (day, UTC hour: pageviews, custom events, distinct daily pseudonyms, first and last event time), `dimension_aggregates` (day, field, value: pageviews, custom events, distinct daily pseudonyms) for `path`, `referrer_domain`, `browser`, `os`, `device`, and `visitor_days` (day, daily pseudonym, first and last event time). `day_aggregates` serves day totals and marks a day as rolled up |
| Maintenance | `crates/analytics/src/correction.rs`, `explore/rollup.rs` `refresh_sql` | Rebuilt in the correction transaction for work days and for every day of a session, old or new, that has an event on a work day. Session-scoped fields take the session's value: the first event's (its sequence is the session ID), the first pageview's for referrer domain |
| Reads | `crates/analytics/src/explore/rollup.rs`, `run.rs` | One rule picks the source per request (below). Served days come from rollups, every other instant from events through the existing materialization, session measures from `sessions` |
| Old files | `crates/storage/src/analytics.rs` `open` | A file with `day_aggregates` but no `visitor_days` gets its day aggregates deleted and every day with events marked dirty, so correction rebuilds them |
| Raw planner | `explore/run.rs` `materialize` | Reads `session_members` only for the window's events. Before, every request hashed all members: 124 ms for an empty 2-hour window at 10%. Same rows, checked by hash on a 22-hour window |
| Gate | `crates/workload/src/gate.rs` | `sessions` records each site's policy first, as the server does at startup. Without it analytics assumed UTC for site A while its local days were stamped in Vienna |
| Bench | `crates/workload/src/explore_bench.rs` | Q01 cases carry the overview's five panels; `--raw`; every case the rollups serve also runs once through the raw planner and both results are compared byte for byte (`rollup`, `matches_raw` in the summary, differing documents in `OUT/mismatch/`) |

### The rule

A request uses rollups when it has no filter and every cut has either no dimension (any measure except identified people), or exactly one of the five rollup dimensions with pageviews, events and visitors only. Anything else, including a single panel outside that, sends the whole request to the raw planner.

Within a rollup request, a day is served when all of these hold:

- it lies whole inside the retention-clipped range, in one timezone epoch (a date two epochs share is never served);
- it has no dirty row;
- its `day_aggregates` row has the current metric version and its events lie inside the day's instants, or it has no rollups at all;
- for the series, its UTC-hour cells each fall in one bucket, an hourly bucket holds one cell of the day, and a coarser bucket holds the whole day or two neighbouring buckets.

A count of the range's eligible events must then equal the served days' count plus the event-counted part. If it does not (a day with events but no rollups, or local days stamped under a timezone the analytics policy copy does not have), the request runs on the raw planner.

### Deviation from the agreed plan

Q01 asks for `span_median`, which the plan listed as a fallback. That would keep Q01 on raw scans. Session measures instead read `sessions` directly for the range, which is what the raw planner counts too, so `sessions`, `bounce_rate`, `span_mean` and `span_median` need no rollup and stay exact. Hour cells are keyed by UTC hour rather than local hour, so day buckets in another timezone with whole-hour offsets are served too; hourly buckets in another timezone are served when the cells align (UTC against Vienna) and counted from events otherwise.

## Tests

`crates/analytics/tests/rollups.rs`, each comparing the serialized `ExploreResult` of rollups and raw planner and counting the days served (by bumping `day_aggregates` pageviews):

| Test | Shows |
| --- | --- |
| `rollups_match_events_in_the_site_timezone` | hourly across the repeated hour of 2026-10-25 (that day counted from events), day series with previous period and five panels, weeks, paged and sorted path rows |
| `another_timezone_counts_boundary_days_from_events_and_visitor_days_in_each_bucket` | UTC day and hour series on a Vienna site, a visitor-day across UTC midnight, Kolkata (half-hour offset) series counted from events |
| `dirty_days_are_counted_from_events_until_corrected` | a late event's day is not served until correction |
| `a_day_with_events_but_no_rollups_sends_the_request_to_events` | the count check |
| `a_profile_session_crossing_midnight_keeps_the_next_days_session_fields` | fails if correction refreshes only work days |
| `a_timezone_change_day_is_counted_from_events` | Vienna to UTC: the two shared dates are not served |
| `local_days_stamped_under_another_timezone_are_counted_from_events` | Vienna-stamped data with no policy copy |
| `the_retention_cutoff_drops_rollup_days_before_it` | three of six days retained |
| `states_outside_the_rollups_use_the_raw_planner` | filter, identified people, session measure by path, two dimensions, country, property dimension and statistic, one foreign panel |

`sessions.rs` `correction_is_idempotent_and_replay_is_deterministic` now also compares the three rollup tables between correction in waves and in one pass. `storage.rs` `a_file_from_before_the_rollups_rebuilds_every_day` covers the old-file path. Unit tests cover the day rule and the series fit.

All pass, and clippy with `-D warnings` is clean.

## Latency

Two users, 20 samples per case, full report-lane read. p95 is nearest rank over 20 samples. Q01 cases include the five overview panels in both columns, which the m06 run did not send. Every resolved range equals the declared one; nothing was refused.

| Case | 1% raw p50 / p95 | 1% rollup p50 / p95 | 10% raw p50 / p95 | 10% rollup p50 / p95 | Target p95 |
| --- | --- | --- | --- | --- | --- |
| Q01-30d-A-vienna | 134 / 136 | 41 / 44 | 1,317 / 2,085 | 94 / 123 | 2,000 |
| Q01-12m-A-utc | 720 / 782 | 174 / 181 | 10,455 / 15,023 | 510 / 833 | 10,000 |
| Q02-30d-A-vienna | 26 / 30 | 13 / 14 | 183 / 203 | 54 / 60 | 2,000 |
| Q02-30d-A-vienna-page-99 | 29 / 34 | 15 / 17 | 180 / 209 | 61 / 68 | 2,000 |
| Q02-12m-A-utc | 149 / 165 | 58 / 60 | 2,244 / 2,578 | 162 / 225 | 10,000 |

Value check: all 9 cases the rollups serve (these 5 and the site D Q01 and Q02 cases) serialize to the same bytes as the raw planner at both scales.

From 1% to 10%, Q01-12m-A-utc grows 4.6 times with rollups and 19 times without. If the next tenfold step grows it 5 to 10 times, full scale lands at 4 to 8 s against 10 s; Q01-30d-A-vienna at 0.4 to 1.2 s against 2 s. That is an extrapolation, not a measurement. With the five panels, raw Q01 cases now miss their targets at 10% already.

Where the annual time went, single user at 10%, before the `session_members` fix: 410 ms for the two boundary days and 127 ms for the compare window, 82 ms for the count of the year's events, about 200 ms for the six cuts, the largest the day series from `visitor_days` (1.2M rows). Run alone, the fixed boundary statement takes 104 ms instead of 276 ms for 22 hours, and 1 ms instead of 204 ms for an empty window.

Filtered families stay on raw scans as agreed. At 10%, rollup run p95: Q03-12m-A-utc 7,233 ms, Q04-12m-A-utc 6,487 ms, Q04-12m-A-utc-selective 7,138 ms, Q12-12m-A-utc 7,518 ms, Q12-12m-A-utc-broad 7,586 ms, Q12-12m-A-utc-session 5,753 ms, Q02-12m-A-utc-tail 727 ms (filtered by path). At full scale these remain the 60 to 90 s cases of m06.

## Correction and storage

| Scale | Before | After |
| --- | --- | --- |
| 1% (127k sessions) | 16.4 s | 17.9 s |
| 10% (1.42M sessions) | 667 s | 627 s |

The 10% difference is within run-to-run noise; the rollup statements cost about 1 s per 64-day step at 10%. Most correction time remains the identify membership insert (25 to 60 s per step), which predates this change.

Two earlier versions of the refresh failed at 10% and are kept as failed runs: refreshing every day of every rebuilt profile ran out of the 1 GiB engine memory after 139 s, and resolving session fields with a join over all members and events spilled 69 GB before failing.

At 10%, the rollups hold 12,385 hour rows, 2.57M dimension rows and 1.2M visitor-day rows for 12M events; the analytical file grew from 1.3 GB to 1.4 GB.

## Not verified

- Full scale and the reference host: the extrapolation above is from 1% and 10% on a laptop.
- Correction time with live ingestion: only the backfill (`sessions`) was timed, not `holocron-gate correct`.
- The policy-copy check covers events in the range. It does not prove the copy is right for days outside it.

## Open items

- Filtered reports (Q03, Q04, Q12 and the `-tail` cases) still scan raw events. They are the remaining site A risk.
- The count check reads every eligible event time of the range, 82 ms per year at 10%. It is the price of not trusting the policy copy; storing per-day event counts by instant would remove it.
- The identify membership insert dominates correction at 10% and grows faster than linearly (m06 open item).
