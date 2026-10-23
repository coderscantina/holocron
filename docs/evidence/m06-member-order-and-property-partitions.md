# Evidence: time-ordered members, property partitions and the DuckDB abort

Date: 2026-10-05. Follow-up to [m06 filtered reports](m06-filtered-reports.md), whose first full-scale run left filtered annual site A reports at up to 33.5 s against 10 s, property reports out of memory, and one case aborting the process. Decision: [0008 Filtered reports](../decisions/0008-storage-engine-and-budgets.md#filtered-reports).

Result: at full scale every one of the 760 samples completes. Before, 575 failed with out-of-memory errors and Q12-12m-A-utc-session aborted the process. 31 of 38 cases pass. Seven still miss: five annual site A cases (Q12 pivot 24.5 s, Q12 session 19.0 s, Q12 broad 15.2 s, Q04 12.5 s, Q12 tail 10.5 s against 10 s) and two 30-day site A Q04 cases (2.2 s against 2 s). At 10% all 38 pass, and the suite takes half as long.

## Run

- Source: commit `0353e44` plus the uncommitted rollup and filtered-report changes, plus this one.
- Host: Apple M1 Pro, 10 cores, 32 GiB, macOS (Darwin 27.0.0), APFS SSD. Not the reference host.
- Engine budget as in 0008: 2 threads, 1 GiB. Release build; libduckdb-sys now builds without debug info (see [The abort](#the-abort)).

```sh
cargo build --release -p holocron-workload
B=target/release/holocron-gate
HOLOCRON_DISK_RESERVE_MIB=256 $B sessions --data GATE        # opens the file: migrates it
HOLOCRON_QUERY_TIMEOUT_S=900 HOLOCRON_DISK_RESERVE_MIB=256 $B explore --data GATE --out OUT --observations 20
```

Both scales reuse the data of the [filtered reports](m06-filtered-reports.md) run, copied, so the event and property rows are the same.

## What changed

| Change | Where | Effect |
| --- | --- | --- |
| Member event times | `session_members.event_time`; correction writes it | The planner selects a range's members by site, role and time. The semi-join of all members against the range's events is gone |
| Time order kept | correction diffs old and new `sessions` and `session_members` rows, deletes the changed ones and appends the new ones in (site, time) order | A profile's earlier sessions are no longer rewritten each step, so row groups stay in time order and a range prunes |
| Joins on sequences | `explore/run.rs` `select_rows` | Members and sessions join on `seq` and `session_id` alone once restricted to the site; both are unique across sites |
| Sessions bounded | the session join reads sessions overlapping the chunk | Pruned by start time |
| 31-day chunks | `explore/run.rs` `materialize` | A 12-month range is 12 statements appended to one table; each join fits the budget instead of spilling |
| Property partitions | `event_properties_0` to `_63`, `property_keys`, view `event_properties` | A property join scans its key's partition instead of every property row of the range |
| Pre-filtered key scans | `property_joins` | A property only the filter reads keeps only rows one of its conditions matches |
| Session properties once per range | `<p>_sp<i>` | The latest identify per session is computed once, from the identifies that can attach to a session overlapping the range, not per statement over all time |
| Schema version 3 | `ANALYTICS_SCHEMA` | Ledger entries record the new layout (0006) |
| DuckDB without debug info | `Cargo.toml` `[profile.release.package.libduckdb-sys]` | `NDEBUG` is set, as in DuckDB's own releases |

Keys per site are not bounded (the 500-key cap only limits the catalog listing), so a table per key would grow without limit. `property_keys` assigns each key the next of 64 partitions round robin when it is first stored. The fixture's 48 keys each have their own. Hashing keys into partitions was rejected: with 64 partitions the worst fixture key would scan 6.9 times its own rows.

Prototype on the full-scale data, Q04-12m-A-utc's statement alone, single user, count of matching events and visitors (results equal in every variant):

| Variant | Time |
| --- | --- |
| Before | 27.1 s |
| Members by time, joins on sequences, key tables | 14.4 s |
| Plus pre-filtered key scans | 11.4 s |
| Plus 12 chunks with bounded sessions | 6.9 s |

Before, a profile split CPU time this way: about 21 s on members (the 57M-row semi-join plus the 57M × 41M join on `(site, seq)`), 8 s on sessions and 10 s on two property scans and joins. The remaining cost after the change is the event scan itself (about 5.7 s of CPU for 57M site A events) and the member join.

## Correctness

- At 10%, every derived table after a migration and full rebuild by correction with this change (`sessions`, `session_members` without the new column, `day_aggregates` without `computed_through`, `hour_aggregates`, `dimension_aggregates`, `visitor_days`) and all 24M event property rows are row-identical (`EXCEPT ALL` both ways) to the file before.
- Every member's `event_time` equals its event's time (roles 1 and 2) or its identify's time (role 3). Every partition row belongs to a key assigned to that partition.
- All 38 cases serialize to the same bytes before and after the change at 10%: the pre-change planner was rebuilt from the same tree without this change, and both dumped every case's result.
- At full scale, 35 of 38 cases serialize to the same bytes before and after; the other three failed before ([Full scale](#full-scale)).
- The rollup cases still serialize to the same bytes as the raw planner, at 10% and full scale.
- `storage.rs` has a test for the property migration (`a_file_with_one_property_table_moves_values_to_key_partitions`); the outdated-derived-tables test now starts from a file without member event times.

## Latency at 10%

Two users, 20 samples per case, all 38 cases pass. The suite took 103 s against 202 s before.

| Case | Before p95 | After p50 / p95 | Target p95 |
| --- | --- | --- | --- |
| Q12-12m-A-utc-pivot | 2,621 | 1,444 / 1,526 | 10,000 |
| Q12-12m-A-utc-session | 1,568 | 1,227 / 1,301 | 10,000 |
| Q04-12m-A-utc | 2,697 | 1,062 / 1,137 | 10,000 |
| Q12-12m-A-utc-broad | 1,186 | 865 / 905 | 10,000 |
| Q12-12m-A-utc-tail | 1,129 | 616 / 633 | 10,000 |
| Q03-12m-A-utc | 1,152 | 497 / 560 | 10,000 |
| Q04-12m-A-utc-selective | 2,681 | 330 / 407 | 10,000 |
| Q12-12m-A-utc | 1,466 | 408 / 417 | 10,000 |

## Full scale

124M records, 12M sessions. Two users, 20 samples per case, 760 samples, no failures. The suite took 1,274 s. Before is the [filtered reports](m06-filtered-reports.md) run, where most samples failed with out-of-memory errors, so its few successes saw less contention than this run.

| Case | Before n, p95 | After p50 / p95 | Target | Passes |
| --- | --- | --- | --- | --- |
| Q12-12m-A-utc-pivot | 0, out of memory | 21,387 / 24,493 | 10,000 | no |
| Q12-12m-A-utc-session | 0, abort | 17,511 / 18,973 | 10,000 | no |
| Q12-12m-A-utc-broad | 10, 14,761 | 12,842 / 15,218 | 10,000 | no |
| Q04-12m-A-utc | 10, 33,511 | 10,538 / 12,505 | 10,000 | no |
| Q12-12m-A-utc-tail | 10, 13,648 | 9,500 / 10,492 | 10,000 | no |
| Q04-12m-A-utc-selective | | 5,848 / 6,901 | 10,000 | yes |
| Q03-12m-A-utc | 10, 11,482 | 4,492 / 5,824 | 10,000 | yes |
| Q04-12m-D-vienna | | 4,479 / 5,721 | 10,000 | yes |
| Q12-12m-A-utc | | 3,633 / 4,045 | 10,000 | yes |
| Q01-12m-A-utc (rollups) | 11, 4,059 | 2,857 / 3,181 | 10,000 | yes |
| Q12-12m-D-vienna | 10, 10,023 | 2,114 / 2,348 | 10,000 | yes |
| Q04-30d-A-vienna-broad | 10, 2,265 | 1,631 / 2,221 | 2,000 | no |
| Q04-30d-A-vienna | 10, 2,298 | 1,350 / 2,220 | 2,000 | no |
| Q03-30d-A-vienna-drill-at | 10, 3,087 | 1,211 / 1,734 | 2,000 | yes |
| Q12-30d-A-vienna-pivot | 10, 1,956 | 1,488 / 1,628 | 2,000 | yes |

Results before and after at full scale: 35 of 38 cases serialize to the same bytes. The other three (Q12-12m-A-utc-pivot, Q12-12m-A-utc-session, Q12-30d-A-vienna-session) ran out of memory with the pre-change planner, so there is nothing to compare; the old planner, built with `NDEBUG`, returned the error instead of aborting.

What the misses have in common: they read most of site A's 57M annual events. The pivot, tail and broad cases keep nearly every event, so the materialized table holds tens of millions of rows before any aggregation. Q12-session joins every event to its session's identify values. With 2 threads that is more work than 10 s allows, whatever the joins cost.

## The abort

`[profile.release] debug = "line-tables-only"` makes Cargo pass `DEBUG=true` to build scripts. libduckdb-sys defines `NDEBUG` only when `DEBUG` is false, so the release binary kept DuckDB's internal assertions, which DuckDB's own release builds compile out. The failed one (`new_remaining_size != 0` in `temporary_memory_manager.cpp`) sits on an out-of-memory path. The same statement in DuckDB 1.5.6's Python release build, with the server's settings, returns `Out of Memory Error` instead of aborting.

The package override builds libduckdb-sys without debug info, so `NDEBUG` is set. The assertion's text is in the old gate binary and absent from the new one (`strings`). Q12-12m-A-utc-session now completes at full scale.

## Migration

Opening a file from before this change does two things. Event property rows move to their key partitions in one transaction, keys in order of first appearance: 240M rows in 262 s at full scale. Derived tables without member event times are dropped and every day is marked dirty, as the previous rule did for files without the stored session fields.

At 10% the migration and the full rebuild took 2 min. At full scale the rebuild runs out of the 1 GiB budget in correction's first step: 64 days of site A touch nearly all of its 400k profiles, and correction rebuilds each profile's whole year, so `scope` holds 22M rows and the session windows over it do not fit next to the step's other tables. The same statements pass alone in 27 s. This is not new with this change: the previous rule rebuilds the same way and was only run at 10%. The fresh-load path passes at full scale because there histories grow one day at a time. For the measurement, the full-scale copy got its member event times in place (one sorted copy of `session_members` and `sessions`, 20 s on 8 threads) instead of the rebuild. That layout is sorted globally rather than per correction step. At 10%, where correction wrote the layout, a 31-day chunk read row groups holding 0.86M members to find the 0.52M it needed, out of 12.4M.

## Not verified

- A full rebuild of derived data at full scale, by the migration or by a recalculation of many days: it runs out of memory (above).
- Correction under live ingestion at full scale with the diffed writes. Correction's full rebuild at 10% wrote identical tables; the full-scale fresh-load path was not rerun.
- The reference host. These numbers are from a laptop.
- Latency with ingestion running alongside.

## Open items

- Deferred by Mike on 2026-10-06 to milestone 13 on the reference host: the site A misses and the planner designs below. The installation tiers stay proposed.

- For Mike: five annual and two 30-day site A cases still miss their targets at full scale (above). Every request starts from the site's events (57M a year for site A) and joins members, sessions and properties onto them. The misses keep most of those events. Designs to test, most valuable first; their effect is estimated, not measured:
  1. Property-driven plans: store the event's `kind`, `daily_id` and `profile_id` on its property rows (written once, never changed). A state whose filter and dimensions are event properties only then reads the key partitions it names: Q12-broad is a union of four partitions, the tail a group-by over one, the pivot a join of two on `seq`. The not-set cell of a count is the range total minus the matched rows; visitors in a not-set cell still need the event scan.
  2. Session-first plans: apply top-level conjuncts that read only session fields to `sessions` first, and join members of the matching sessions only. Correction stores each session's latest property values, so a session-property condition is a scan, not a per-request aggregate. Covers Q04, Q12-session and the 30-day Q04 cases. Events without a session still test their own values.
  3. No temp table for a single cut without comparison: aggregate in the reading statement. Saves writing up to 57M rows; not enough alone for the pivot.
- Done 2026-10-06: bounded rebuild and the half-days retry, see [0008](../decisions/0008-storage-engine-and-budgets.md#unresolved-risks). The two items below are the original findings.
- Correction cannot rebuild a full-scale file from scratch in 1 GiB: a step's work is bounded by days, not by the profile histories they pull in. Stopgap: on an out-of-memory error, which now rolls the step back cleanly, retry with half the days. Fix: below.
- Correction rebuilds every touched profile's whole history. With a year of data, each live step reruns the session rules over the full year of every profile active that day. The fresh-load run hid this because histories grew one day at a time. A change on day D can only merge or split sessions within one window of D, so the rebuild can be limited to the profile's sessions overlapping D plus and minus the window, widened to their full extent, as revocations already are. Measure before milestone 13's soak.
- Capacity per installation: the standard budget's limit for the largest site, from this run and the 10% run, and proposed installation tiers are in [0008 Installation tiers](../decisions/0008-storage-engine-and-budgets.md#installation-tiers-sizing-guidance).
