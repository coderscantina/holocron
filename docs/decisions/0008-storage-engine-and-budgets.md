# 0008: Storage engine, layout, and budgets

Status: Proposed (milestone 02). The transfer protocol, inbox and budgets below are built and tested. The capacity gate is not passed: it has not run on the reference host, and the local approximation misses the report latency targets for the dominant site. Evidence: [m02 storage gate](../evidence/m02-storage-gate.md). Milestone 04 wired it into collection: [m04 durable modern collection](../evidence/m04-durable-modern-collection.md).

## Problem

Milestone 04 needs a durable inbox and transfer it can wire into collection, and milestones 05 to 09 need to know the analytical layout before they build on it. The [performance contract](../performance-validation.md) asks whether SQLite plus DuckDB in one process fits 2 vCPU, 4 GiB and 250 GiB with 120M retained events, two report users and the 500 events/s burst.

## Constraints

Confirmed: durable SQLite commit (WAL, `synchronous=FULL`) before a stored-event acknowledgement; exact counts without sampling; one analytical writer and bounded readers; stable batch IDs with an applied-batch ledger; the agreed hardware and the four p95 targets. Lowering a target or a feature needs Mike's decision.

## Evidence

All measurements are a local approximation: an OrbStack Linux VM on an Apple M1 Pro laptop, container limited to 2 CPUs and 4 GiB without swap, data on a bind mount. Process restart only; the OS page cache was never cleared. Details and failed runs are in the evidence report.

| Check | Local result | Target |
| --- | --- | --- |
| Transfer interruption, 8 boundaries, 20 seeds each, real SIGKILL | 160 of 160 recover, macOS and Linux | every boundary |
| Golden results, 25 early-gate cases, 120M events | 25 of 25 hashes match the independent oracle | exact |
| Burst acknowledgement (30,000 events, two report users, in-process, no HTTP) | p95 89.5 ms, max 549.5 ms, 0 refused | p95 250 ms |
| Burst visibility, acknowledgement to visible | p95 813 ms, max 1,594 ms | p95 2 s |
| 30-day reports, site D, concurrent | p95 232 to 422 ms | p95 2 s |
| 30-day reports, site A, concurrent | p95 1.3 to 5.6 s, 6 of 9 cases over | p95 2 s |
| 12-month explorations, site D, concurrent | p95 0.8 to 2.3 s | p95 10 s |
| 12-month explorations, site A, concurrent | p95 11.6 to 24 s; `Q02-12m-A-utc` times out at 30 s | p95 10 s |
| 12-month site A, one user, process restart | 4.3 to 8.3 s, except `Q02-12m-A-utc` at 14.6 to 20 s | p95 10 s |
| Memory, cgroup peak with two users and ingestion | 3.75 to 3.84 GB (2.1 to 2.4 GB anonymous, the rest file cache), no OOM | 4 GiB |
| Disk, analytical file after load | 44 GB file, 26 GB in use, 16.4 GB of it replay | 250 GiB volume |

Two earlier burst runs with a 1.5 GiB engine limit and default allocator arenas were killed by the OOM killer (3.84 GB anonymous memory at the last sample). They are kept as failed runs.

Identity-day rollups, measured on 2026-10-04 at 1% and 10% on the laptop, not the reference host ([evidence](../evidence/m06-identity-day-rollups.md)). Two users, p95, overview panels included in Q01:

| Case | 10% raw scan | 10% rollups | Target |
| --- | --- | --- | --- |
| Q01-30d-A-vienna | 2,085 ms | 123 ms | 2 s |
| Q01-12m-A-utc | 15,023 ms | 833 ms | 10 s |
| Q02-30d-A-vienna-page-99 | 209 ms | 68 ms | 2 s |
| Q02-12m-A-utc | 2,578 ms | 225 ms | 10 s |

All nine cases the rollups serve give byte-identical results to the raw scan at both scales. Correction took 627 s at 10% against 667 s before.

## Choice

### Engines and files

One data directory: `control.sqlite` for accounts, sites and settings (the server, since milestone 03), `inbox.sqlite` (both SQLite 3.51.3 through SQLx 0.9.0), `analytics.duckdb` (DuckDB 1.5.6, duckdb-rs 1.10506.0), and `spill/` for DuckDB temporary files on the same volume.

### Durable transfer

Implemented in `crates/storage` as one interface: `admit`, `watermark`, `read`, `write`, `status`, `drain`, `close`.

1. Admission encodes records (postcard, `inbox_format` 2 since milestone 04) and commits them in a group commit: the writer takes every request queued when it is free, no added delay. Multi-row inserts get consecutive `AUTOINCREMENT` sequences. Outcomes return after `COMMIT`. On macOS `fullfsync` is on, because plain `fsync` does not flush the drive there.
2. Transfer assigns the oldest pending records to a stable batch (`batches`, `AUTOINCREMENT` ID, first and last sequence, record and byte counts) in one SQLite transaction, or resumes the oldest assigned batch.
3. DuckDB applies the batch in one transaction: canonical rows, `dirty_days` for every identity day touched, and the `applied_batches` ledger row with `analytics_schema` and `inbox_format`. A batch already in the ledger is not reinserted; a ledger row whose membership differs is refused as an incompatible replay.
4. The watermark (last batch ID, last sequence) is published after the DuckDB commit, then the inbox rows are retired. Batches apply in ID order, so the watermark is the highest contiguous applied position. On startup the ledger is read back, which restores missed notifications.

Client `eventId` deduplication lives in SQLite next to the inbox (48-hour window from receive time). Since `inbox_format` 2 each record carries `ClientEventId {id, fingerprint}`; the first claim stores the payload fingerprint, and a later copy returns `Outcome::Duplicate` with a flag when the fingerprints differ (`dedup_conflict`). Transfer refuses records of any other format as incompatible, so an inbox must drain before an upgrade that changes the format. No release has shipped format 1. Pruning that table is maintenance work for milestone 12.

### Analytical layout

- `events`: one row per primary event, typed columns for every 0004 dimension, identity as `UHUGEINT` (daily pseudonym, profile), `local_day` of the identity day, performance and revenue columns. 50 bytes per event on disk.
- Event properties: one row per property value, type tag plus one typed column per type (decimals as integers of 0.0001), `site` and `event_time` repeated for zone-map pruning. 16 bytes per value. Arrays are stored as JSON text. Since 2026-10-05 the rows live in 64 key partitions, `event_properties_0` to `event_properties_63`: `property_keys` assigns a key the next partition round robin when it is first stored, and the view `event_properties` unions them. Keys per site are not bounded, so a table per key is not an option; up to 64 keys each have a partition of their own.
- `session_properties`, `replay_fragments` (payload as `BLOB`), `heatmap_interactions` (raw), `dirty_days`, `applied_batches`.
- Rows are clustered by month, then by (site, event_time). Live ingestion appends in arrival order, which interleaves sites in every row group; then a one-site annual query scans every site's year. Clustering cut annual site D cases from 11.5 s to 0.45 s. This needs a compaction job that rewrites closed periods; the experiment took 342 s for 368M rows on the host. Milestone 12 schedules it with retention and backup.
- Sessions in the gate were a provisional table rebuilt by SQL (45 s for 12M sessions). Milestone 05 replaced it with derived `sessions`, `session_members` and `day_aggregates`, rebuilt incrementally from `dirty_days` ([evidence](../evidence/m05-identity-sessions-and-corrections.md)). Each member row carries its event time. Correction appends changed `sessions` rows in (site, start) order and changed `session_members` rows in (site, event time) order, and leaves unchanged rows in place, so both stay in time order under live ingestion.

### Rollups

Exact identity-day rollups serve unfiltered reports. A daily pseudonym exists for one identity day only, so distinct visitor-days over a range are the sum of per-day distinct counts.

- Tables: `day_aggregates` (day totals), `hour_aggregates` (day and UTC hour), `dimension_aggregates` (day and one value of `path`, `referrer_domain`, `browser`, `os` or `device`) and `visitor_days` (first and last event of each daily pseudonym, for day buckets in another timezone). Distinct visitors are summed only per day or per day and value.
- Maintenance: inside the correction transaction, for work days and every day of a session that has an event on a work day, because session-scoped fields of a profile session crossing midnight come from the day before. Metric version, policy changes and recalculation invalidate through `dirty_days` and the stored metric version.
- Source rule: a request with no filter, whose every cut has no dimension (any measure but identified people) or one rollup dimension with pageviews, events and visitors, reads rollups; anything else runs the raw scan entirely. Session measures, `span_median` included, read `sessions`, the rows the raw scan counts.
- Served days: whole days inside the retention-clipped range in one timezone epoch, without a dirty row, with current rollups whose events lie inside the day or no rollups at all, and whose cells fit the series buckets. Boundary, dirty, timezone-change and the remaining days are counted from events in the same snapshot, so results never lag ingestion. A count of the range's eligible events confirms the split; if it differs, the request runs the raw scan.

### Filtered reports

Filtered and multi-dimension reports scan events. These rules keep the scan proportional to what the state reads, and one engine setting keeps correction fast enough to maintain the stored fields (added 2026-10-05, [evidence](../evidence/m06-filtered-reports.md); time order, chunks and partitions the same day, [evidence](../evidence/m06-member-order-and-property-partitions.md)):

- Stored session fields: correction writes each session's scoped values on `sessions` (the first event's `country` to `screen`, the touch's `referrer_domain` to `click_id` and page query), in the transaction that writes its members. The planner joins them instead of recomputing the first event and touch of every session in the range. An event without a session uses its own values, as before.
- Filter first: the filter applies inside the statement that reads events, so the engine pushes event and property conditions into the scans. Only columns a cut reads are kept; a property used only by the filter is not carried past it.
- No copies: without session measures the matching events are written once.
- Time-ordered members: the planner reads `session_members` by site, role and event time and `sessions` by site and the instants a session overlaps, and joins both on sequence numbers alone, which are unique across sites. The semi-join of members against the range's events is gone.
- Chunks: a range longer than 31 days runs as one statement per 31 days, appended to the same table. Each statement's joins fit the 1 GiB budget instead of spilling, and the time order lets it read its own row groups.
- Property partitions: a property condition reads its key's partition. A property only the filter reads keeps the rows one of its conditions matches, so the join holds matches instead of the key's whole range. Session properties are resolved once per range, latest identify per session, from the identifies that can attach to a session overlapping it.
- Correction runs with `asof_loop_join_threshold = 0`. DuckDB plans an ASOF join as a nested loop when it estimates fewer than 64 left rows, and correction's ASOF joins ran that way for minutes per step at 10% of the fixture.

### Budgets

| Budget | Value | Basis |
| --- | --- | --- |
| DuckDB threads, shared by writer and readers | 2 | two vCPUs |
| DuckDB memory limit | 1 GiB | 1.5 GiB was OOM-killed with the rest of the process |
| Allocator | glibc with `MALLOC_ARENA_MAX=2` | default arenas were part of the OOM runs; jemalloc untested |
| DuckDB build | release without debug info, so `NDEBUG` is set | with debug info libduckdb-sys keeps DuckDB's internal assertions, and one aborted the server on an out-of-memory path where a DuckDB release build returns an error |
| Tokio worker threads | 2 | two vCPUs |
| Report readers / queue | 2 workers, 8 waiting | two dashboard users |
| Live reads (realtime, status, probes) | 1 worker, 8 waiting | must not queue behind reports |
| Query timeout, queue wait included | 30 s | 0004 |
| Spill | `<data>/spill`, at most 40 GiB | peak 3.2 GB during the burst |
| Inbox backlog | 1 GiB of encoded records | burst peak was 4.6 MB |
| Admission queue | 64 MiB | |
| Disk reserve | 4 GiB free, else admission answers unavailable; `--disk-reserve-mib` / `HOLOCRON_DISK_RESERVE_MIB` | controlled shutdown |
| Batch | 20,000 records or 32 MiB, 250 ms after a commit wakeup | visibility p95 under 1 s |

The SQLite inbox file keeps its peak size after a drain (5.2 GB after the bulk load, 37 KB after `VACUUM`). Since milestone 04 the inbox uses `auto_vacuum=INCREMENTAL` and runs `PRAGMA incremental_vacuum` whenever transfer drains it; a file from before is converted with one full `VACUUM` at open.

Adopted by Mike on 2026-10-04 (milestone 04): the disk reserve is an operator flag, `--disk-reserve-mib` (env `HOLOCRON_DISK_RESERVE_MIB`, default 4096). Development disks and the crash test need a smaller reserve, and an operator with a large volume may want a larger one. Below the reserve, send and batch answer 503 and `collection.status` reports `unavailable` with free and reserved bytes. Setting it to 0 removes the protection; the flag does not stop that.

### Installation tiers (sizing guidance)

Status: decided by Mike on 2026-10-15 as sizing guidance only. The table below is published as estimates for choosing a host. The performance contract still promises only the standard reference host; small and large are not promises until measured. The shaping steps below are not adopted.

The engine's thread and memory limits apply to the whole DuckDB instance: writer, correction and every reader share them, and DuckDB has no per-query memory limit. Budgets are therefore per installation, never per site or per request. One installation can only divide its budget between lanes (report workers and queue, live lane, correction step size).

A report reads one site, so read latency follows the largest site's annual volume; disk, correction and ingestion follow the installation's total. On the standard budget, the slowest annual case (a property pivot keeping every event) took 1.5 s with site A at 5.7M events a year (10%) and 24.5 s at 57M (full scale), growing about in proportion to the data. The 10 s target then holds up to about 23M events a year for the largest site; 30-day targets break later, near 50M. Tiers, before the planner work in the [filtered reports evidence](../evidence/m06-member-order-and-property-partitions.md#open-items):

| Tier | Host | Engine | Tokio / report workers / queue | Largest site, events a year | All sites, retained records | Disk |
| --- | --- | --- | --- | --- | --- | --- |
| small | 1 vCPU, 2 GiB | 1 thread, 512 MiB | 1 / 1 / 4 | about 8M, estimated | 40M | 25 GiB |
| standard | 2 vCPU, 4 GiB | 2 threads, 1 GiB | 2 / 2 / 8 | about 20M, from two measured scales | 125M, measured | 50 GiB |
| large | 4 vCPU, 8 GiB | 4 threads, 2 GiB | 4 / 4 / 8 | about 35M, estimated | 250M | 100 GiB |

- The engine gets about a quarter of host memory, as on the reference host: the rest goes to the process, allocator arenas and file cache, and 1.5 GiB of 4 GiB was OOM-killed.
- Small assumes 2.5 times the standard latency (half the threads shared by two users, more spill at 512 MiB); large 1.7 times faster (two users with two threads each; the event scan does not scale perfectly). Standard keeps a margin below the 23M estimate.
- Disk: 105 bytes per retained record without replay (13 GB for 124M records), plus replay (16.4 GB for the milestone 02 fixture), the 4 GiB reserve and spill.
- Small must rerun the 500 events/s burst: with one engine thread, transfer, correction and reports queue behind each other. Its correction steps need a memory bound (see the risks below) before anything else.

Shaping, not adopted (it would turn the table into a promise per host class):

1. Proposed without the automatic choice in [0019](0019-budget-presets.md): `--budget` is explicit and the server never detects the host. As first written: The server picks the preset from the host's CPU count and cgroup memory limit; `--budget small|standard|large` (env `HOLOCRON_BUDGET`) overrides it, and explicit flags override single values. The preset sets the engine, Tokio, report lanes, spill and inbox budgets together, and every gate result records it.
2. `collection.status` reports the largest site's trailing 12 months of eligible events from `day_aggregates` against the tier's limit and warns above 80%, so an operator moves up a tier before reports slow down.
3. `holocron init` asks for the expected monthly events of the largest site and recommends a tier.
4. Correction derives its step size from the memory budget and retries a step with half the days when it runs out of memory.

Measuring a tier: load the fixture at several scales (`holocron-gate load --groups-per-month`), run the full exploration suite with two users at each, and take the largest scale where every declared case passes ([performance validation, Capacity sweep](../performance-validation.md#capacity-sweep)). Standard first, since the other tiers scale from it; the reference host then repeats each tier in milestone 13. Milestone 12 would own the presets, status and init guidance.

## Rejected alternatives

- Bulk-loading the retained fixture straight into DuckDB: it would skip the path the gate is about. All 124M records went through admission, the inbox and transfer (47,000 records/s on the host).
- Materializing every match set for table queries: reading back 57M kept rows cost 4.7 to 6.4 s per pass.
- Approximate distinct counts: forbidden by the contract.

## Unresolved risks

- **Dominant-site reports that keep most events miss their targets at full scale.** Measured on the laptop with the full fixture on 2026-10-05 ([evidence](../evidence/m06-member-order-and-property-partitions.md)), two users, every sample completing: Q12-12m-A-utc-pivot 24.5 s, Q12-12m-A-utc-session 19.0 s, Q12-12m-A-utc-broad 15.2 s, Q04-12m-A-utc 12.5 s and Q12-12m-A-utc-tail 10.5 s p95 against 10 s; Q04-30d-A-vienna and its broad variant 2.2 s against 2 s. The other 31 cases pass. A different engine has not been tested.
- **Correction in 1 GiB at full scale** is unmeasured since the fix below. A step that runs out of engine memory rolls back and retries with half the days, down to one; a single day that still fails blocks correction behind it and is logged.
- Resolved 2026-10-06, **correction cost growing with history.** A step rebuilds, per profile with a record on a dirty day, a span that grows until no eligible event of the profile lies within the site's largest window before or after it; identifies up to 3 days before the span and performance rows one window past it are re-attached. Both edges are then session boundaries in any derivation, so the result equals a full rebuild (`correction_around_a_middle_day_equals_a_full_rebuild`). On a synthetic year of 3,000 daily-active profiles (release, 2 threads, 1 GiB) a live step fell from 5.8 to 8.0 s (1.1M sessions rebuilt) to 0.16 to 0.26 s (3,000), and a backfill from 40 s to 9.5 s. At 1% the six derived tables are identical to the previous rule. A profile active around the clock still widens to its whole history.
- **Memory headroom is thin**: up to 3.84 GB cgroup peak in a 4 GiB budget with file cache counted.
- **Disk amplification**: the DuckDB file is 44 GB for 26 GB of live blocks after loading and clustering. Free blocks are reused but the file does not shrink.
- **Replay as uncompressed BLOBs** takes 16.4 GB; the template stream compresses 11.75 times with gzip. Milestone 09 should store compressed chunks.
- Power-loss durability, cold cache, the reference host, the 24-hour soak, corrections and the Umami comparison are unverified (evidence report).

## Consequences

- Milestone 04 wired `Storage::admit` into collection: `Refusal::Busy` answers 429 and `Refusal::Unavailable` 503, both with `Retry-After`. The server runs two Tokio workers (`--worker-threads`) and calls `mallopt(M_ARENA_MAX, 2)` on glibc. `Storage::inbox_age()` and `disk_headroom()` feed `collection.status`. Release failpoints pass 160 of 160 on format 2.
- Milestones 05 to 08 build on `events` and `event_properties` and must keep results exact through `Storage::read`. Unfiltered reports read identity-day rollups ([Rollups](#rollups)); filtered reports scan events under [Filtered reports](#filtered-reports) and miss the full-scale targets.
- Milestone 12 owns compaction (site clustering), `client_event_ids` pruning, ENOSPC injection, and restore coordination of batch IDs with the ledger.
- Milestone 13 reruns this gate on the reference host.

## Affected contracts

[Performance validation](../performance-validation.md) early gate; [workload manifest and query cases](../../fixtures/workload/), whose expected values milestone 02 filled in; the [development workflow](../development-workflow.md) module map (proposed `crates/storage`).
