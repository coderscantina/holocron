# Evidence: filtered reports and the first full-scale run

Date: 2026-10-05. Follow-up to [m06 identity-day rollups](m06-identity-day-rollups.md), which left filtered site A reports on raw scans at 60 to 90 s extrapolated for full scale. Decision: [0008 Filtered reports](../decisions/0008-storage-engine-and-budgets.md#filtered-reports).

Result: at 10% every declared case passes, the slowest annual filtered case at 2.7 s p95. At full scale, measured for the first time, unfiltered reports pass on rollups but filtered annual site A reports miss 10 s by up to three times, and property pivots and session-property cases do not fit the 1 GiB engine budget. One of them aborts the process inside DuckDB.

## Run

- Source: commit `0353e44` plus the uncommitted rollup change and this one.
- Host: Apple M1 Pro, 10 cores, 32 GiB, macOS (Darwin 27.0.0), APFS SSD. Not the reference host.
- Engine budget as in 0008: 2 threads, 1 GiB. C and C++ built with `CFLAGS=-g0 CXXFLAGS=-g0`.

```sh
cargo build --release -p holocron-workload
B=target/release/holocron-gate
HOLOCRON_DISK_RESERVE_MIB=256 $B load --data GATE --groups-per-month 100000     # 10%; 1000000 for full scale
HOLOCRON_DISK_RESERVE_MIB=256 $B sessions --data GATE
HOLOCRON_DISK_RESERVE_MIB=256 $B explore --data GATE --out OUT --observations 20
```

The full-scale run set `HOLOCRON_QUERY_TIMEOUT_S=900` (new, gate only): the byte comparison of rollup and raw results runs the raw planner on unfiltered annual cases, which takes longer than the 30 s read budget at full scale.

## What changed

| Change | Where | Effect |
| --- | --- | --- |
| Stored session fields | `sessions` columns `country` to `touch_query`; correction `rebuilt` keeps the touch's `seq` and the insert joins the first and touch events | The planner joins `sessions` instead of computing `arg_min` per session over the whole range |
| Filter in the reading statement | `explore/run.rs` `materialize` | Event and property conditions reach the scans; nothing is materialized before filtering |
| Read columns only | `Plan::read` | A property only the filter reads is not carried past it |
| One write without session measures | `materialize` | The matching events are written once as `<p>_ev`, not copied from `<p>_scope` |
| `asof_loop_join_threshold = 0` | `storage/src/analytics.rs` | Correction's ASOF joins no longer run as nested loops |
| One migration rule | `storage/src/analytics.rs` `open` | A file whose derived tables predate the rollups or the stored fields drops them and marks every day dirty; replaces the rollup-only rule |

Profile that led here, Q03-12m-A-utc at 10%, single user: materializing the year with session fields took 3.6 to 4.8 s, filtering it 0.4 s, every cut afterwards under 10 ms. Hand-written alternatives on the same data: filtering events and the property first, then resolving sessions for the 5,088 candidates, took 0.7 s; stored session fields cut Q04's join from 5 to 7.5 s to 0.8 s; dropping the filter-only property columns cut Q12-broad's statement from 2.5 s to 0.8 s.

## Correctness

- Every derived table after a full rebuild at 10% (`sessions` without the new columns, `session_members`, `day_aggregates`, `hour_aggregates`, `dimension_aggregates`, `visitor_days`) is row-identical (`EXCEPT ALL` both ways) to the copy built before this change.
- The stored fields of all 1.2M sessions at 10% equal `arg_min_null` over their members, the planner's previous definition. The fixture's sessions all start with a pageview, so the touch never differs from the first event there; `rollups.rs` covers sessions that start with a custom event, comparing rollups (own touch computation) with the planner (stored fields).
- The rollup cases serialize to the same bytes as the raw planner at 10% (9 cases) and at full scale (the cases that completed: Q01 and Q02 on sites A and D).
- `storage.rs` `a_file_with_outdated_derived_tables_rebuilds_every_day` replaces the rollup-only migration test.
- 140 tests in analytics, server, protocol and storage pass; clippy with `-D warnings` is clean.

## Latency at 10%

Two users, 20 samples per case, full report-lane read, all 38 cases pass. The suite took 216 s, against 626 s in the m06 run and 291 s before the column and copy changes. Before is the rollup run of [m06 identity-day rollups](m06-identity-day-rollups.md).

| Case | Before p95 | After p50 / p95 | Target p95 |
| --- | --- | --- | --- |
| Q04-12m-A-utc | 6,487 | 1,991 / 2,697 | 10,000 |
| Q04-12m-A-utc-selective | 7,138 | 2,027 / 2,681 | 10,000 |
| Q12-12m-A-utc-pivot | | 2,457 / 2,621 | 10,000 |
| Q12-12m-A-utc-session | 5,753 | 1,487 / 1,568 | 10,000 |
| Q12-12m-A-utc | 7,518 | 1,393 / 1,465 | 10,000 |
| Q12-12m-A-utc-broad | 7,586 | 1,117 / 1,186 | 10,000 |
| Q03-12m-A-utc | 7,233 | 1,006 / 1,152 | 10,000 |
| Q02-12m-A-utc-tail | 727 | 321 / 340 | 10,000 |
| Q03-30d-A-vienna | | 286 / 305 | 2,000 |

## Correction

| Run | Records | Time |
| --- | --- | --- |
| 10%, fresh load, before this change | 12.4M | 640 s |
| 10%, rebuild after the migration, this change | 12.4M | 43 s |
| Full scale, fresh load, this change | 124M | 1,124 s |

Two attempts failed first: computing the stored fields with 17 `arg_min_null` aggregates ran out of the 1 GiB engine memory after 131 s, and selecting the first and touch event through a join inside `rebuilt` made a backfill run over 40 minutes. Sampling showed DuckDB's nested-loop join on `site` and `population` equality, which DuckDB plans for an ASOF join when it estimates fewer than 64 left rows (`plan_asof_join.cpp`). With the threshold at 0 the same rebuild took 43 s. The 10% fresh-load path with this change was not timed separately, and the two 10% runs chunk days differently (one batch ID against many), so 640 s to 43 s is not a like-for-like comparison.

## Full scale

The full fixture loaded for the first time: 124M records, admitted in 1,601 s, drained by 1,889 s, closed (inbox vacuum) at 2,605 s; 23.9M sessions. The analytical file is 13 GB. The disk had 54 to 66 GB free during the run.

575 of 760 samples failed with `Out of Memory` at the 1 GiB limit. Run alone, Q01-30d-D-utc completes (561 and 644 ms) while Q12-12m-A-utc-pivot fails in both users, so the failures come from cases that do not fit, not from state left behind; a failed read rolls back its temp tables. Q12-12m-A-utc-session alone aborts the process: `Assertion failed: (new_remaining_size != 0), function SetRemainingSizeAndUpdateReservation, file temporary_memory_manager.cpp, line 28`.

Cases with samples, p95 in ms. Many of the other user's requests failed fast during the run, so these saw less contention than a clean run would:

| Case | n | p50 / p95 | Target | Passes |
| --- | --- | --- | --- | --- |
| Q04-12m-A-utc | 10 | 31,546 / 33,511 | 10,000 | no |
| Q12-12m-A-utc-broad | 10 | 14,172 / 14,761 | 10,000 | no |
| Q12-12m-A-utc-tail | 10 | 13,288 / 13,648 | 10,000 | no |
| Q03-12m-A-utc | 10 | 10,637 / 11,482 | 10,000 | no |
| Q12-12m-D-vienna | 10 | 9,621 / 10,023 | 10,000 | no |
| Q01-12m-A-utc (rollups) | 11 | 3,314 / 4,059 | 10,000 | yes |
| Q03-30d-A-vienna-drill-at | 10 | 3,019 / 3,087 | 2,000 | no |
| Q04-30d-A-vienna | 10 | 2,260 / 2,298 | 2,000 | no |
| Q04-30d-A-vienna-broad | 10 | 2,238 / 2,265 | 2,000 | no |
| Q02-12m-A-utc-tail | 10 | 2,088 / 2,194 | 10,000 | yes |
| Q12-30d-A-vienna-pivot | 10 | 1,895 / 1,956 | 2,000 | yes |
| Q01-30d-A-vienna (rollups) | 11 | 645 / 977 | 2,000 | yes |

From 10% to full scale the filtered annual site A cases grew 10 to 12 times, so the remaining gap is not a constant overhead. Q04-12m-A-utc is the worst at 3.4 times its target. The Q01-12m-A-utc rollup case grew 8 times (499 ms to 4,059 ms), inside the 4 to 8 s the rollup evidence extrapolated.

## Not verified

- A clean full-scale suite: memory failures removed most samples and changed the contention of the rest.
- The reference host. These numbers are from a laptop.
- Latency with ingestion running alongside, as before.
- The 10% fresh-load correction time with this change.

## Open items

Followed up the same day in [m06 time-ordered members and property partitions](m06-member-order-and-property-partitions.md): members stored in event-time order with their event time, properties in key partitions, 31-day chunks, and a DuckDB build without assertions. Every full-scale sample completes there; five annual and two 30-day site A cases still miss their targets.
