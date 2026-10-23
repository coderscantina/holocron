# Milestone 02 evidence: storage performance gate

Status on 2026-10-03: in progress. The durable transfer protocol is built and passes every process-kill boundary. Golden results for the 25 early-gate cases are fixed and the DuckDB probes reproduce all of them exactly at 120M events. The capacity gate is not passed: it has not run on the reference host, and the local approximation misses the report latency targets for the dominant site. The design decision is [0008](../decisions/0008-storage-engine-and-budgets.md).

Every number below is a local approximation, not the reference host. Nothing here is a capacity claim.

## Run

- Source: commit `a02861b` plus the uncommitted milestone 02 changes in this worktree. `Cargo.lock` sha256 `ccb4b1945a2029867072e38b97ac254475318eae3a00496798413df5f998318a`.
- Engines: DuckDB 1.5.6 (duckdb-rs 1.10506.0, bundled), SQLite 3.51.3 (libsqlite3-sys 0.37.0 through SQLx 0.9.0), Tokio 1.53.2. Release profile with line tables, glibc allocator.
- Host: Apple M1 Pro, 10 cores, 32 GiB, macOS (Darwin 27.0.0), APFS SSD. The fixture was generated, loaded and clustered natively on the host.
- Measured runs: OrbStack Linux VM (kernel 7.0.14-orbstack, cgroup v2, 4.18 GB VM memory), `debian:bookworm-slim` container with `--cpus=2 --memory=4g --memory-swap=4g`. Docker reported an effective limit of 3.893 GiB. The CPU limit is a quota: the process sees 10 CPUs. Data on a bind mount of `.bench/full/data`.
- Budgets: [0008](../decisions/0008-storage-engine-and-budgets.md). Runs 3 onward used `HOLOCRON_ENGINE_MEMORY_MIB=1024` and `MALLOC_ARENA_MAX=2`.
- The load generator runs inside the measured process, because no HTTP server exists before milestone 04. Collection latency is admission latency with no network, and process memory includes the harness.

Commands, from the repository root:

```sh
cargo test -p holocron-workload                      # PRNG vectors, affine instances, rounding, JCS
cargo test --release -p holocron-storage --features failpoints
cargo build --release -p holocron-workload
B=target/release/holocron-gate
$B oracle --threads 8                                # inventory, sample, golden documents, case expectations
$B load --data DATA                                  # 120M events and 4M identify messages through the inbox
$B load-associated --data DATA                       # 9M heatmap interactions, 250,000 replay fragments
$B sessions --data DATA                              # provisional sessions
$B recluster --data DATA                             # layout experiment, see 0008
$B verify --data DATA                                # 25 early-gate cases against the golden hashes
$B bench --data DATA --out OUT --mode streams --seconds 600 --burst-at 120 --run N
$B bench --data DATA --out OUT --mode ingest --groups 10000 --run N
$B bench --data DATA --out OUT --mode restart --starts 10
$B bench --data DATA --out OUT --mode suite --observations 20 --run N
$B layout --data DATA
$B replay-templates
```

The local container runs used a Linux build (`rust:1.99-slim-bookworm`, `cargo build -j 2 --release`, because unlimited parallel C++ compilation of DuckDB ran out of memory in the VM), then:

```sh
MALLOC_ARENA_MAX=2 HOLOCRON_ENGINE_MEMORY_MIB=1024 docker run --rm --cpus=2 --memory=4g --memory-swap=4g \
  -e MALLOC_ARENA_MAX -e HOLOCRON_ENGINE_MEMORY_MIB \
  -v "$PWD/.bench/full/data":/data -v "$PWD/.bench/results":/results -v "$PWD/fixtures":/fixtures:ro \
  -v "$PWD/.bench/linux-target/release":/opt/holocron:ro \
  debian:bookworm-slim /opt/holocron/holocron-gate --fixtures /fixtures bench --data /data --out /results/RUN ...
```

On the reference host, run the same commands with the binary built there, `--starts 40` and `--observations 200`, and three suite and streams runs. Machine-readable samples (`queries.jsonl`, `offers.jsonl`, `visible.jsonl`, `probes.jsonl`, `resources.jsonl`, `summary.json`) are written to `OUT`. Raw data and samples stay outside Git.

## Generated fixture

- `holocron-gate oracle` streams all 12 months in 106 s on 8 threads and checks the result against the manifest arithmetic. Every total matches: 120,000,000 primary events (90M pageviews, 24M custom, 6M performance), the per-site split, 4,000,000 identified groups, 80,000,000 records with 240,000,000 property values, 8,000,000 session property values, all four currencies' gross, refund and net sums, and `groups_per_local_day` of every month.
- Two oracle runs gave identical hashes for every unchanged case. Canonical input hash over all months: `632a1d91adfcd183dd92fad8a1d24a81f55b22d5b29e04d35423ea0a008ba332`, per month in [`inventory.json`](../../fixtures/workload/generated/inventory.json).
- Generation keeps one group in memory per worker; the oracle peaked at 4.7 GB, mostly distinct-count sets for the annual cases.
- Filled in: [`generated/inventory.json`](../../fixtures/workload/generated/inventory.json) (counts per kind, site, site-local day, UTC day and site/UTC month; per-key cardinality, null, missing and byte sizes; consent coverage; planted groups, 14,600 = 40 per day; site-scoped person counts; nearest-rank performance percentiles per metric and device; joint distributions), [`generated/sample-month-11.jsonl`](../../fixtures/workload/generated/sample-month-11.jsonl) (the first 1,000 groups of September 2026), [`golden/`](../../fixtures/workload/golden/) (25 canonical result documents) and `expected` in each early-gate case file (`row_count`, `matched_records`, `golden_result_hash`).
- Replay templates: four masked rrweb event templates; 64 KiB fragments compress 11.75 times with gzip (5,576 bytes per fragment).

### Selectivity

Every labeled early-gate case is inside its class. The four medium Q04 cases first matched 10.35% to 10.52% of in-range events with `country = AT`. As the manifest allows, f3 changed to `FR` before hashes were fixed: they now match 9.39% to 9.58%. Each file records the change. `Q04-30d-A-vienna-selective` matches 0 records: selective by definition, but its result is empty. Proposed: give it a sku literal with at least one match.

### Generator choices where the manifest is silent

- Geo: the country rank uses sub 1 and the region sub 2. The generic rule puts zipf ranks on sub 2, but the geo rule also puts the region there.
- Property key draws use sub 1. The empty-string test of `search_term` uses sub 2.
- Custom events carry the URL of the pageview before them (slot − 1). Performance events do the same, as the manifest states for slot 8.
- Burst dimensions draw from `burst.dims` at ordinal `ordinal * 16 + dimension`. Burst property values and performance metrics use the retained streams at ordinals offset by 10^10.
- Loader day keys are derived from a fixed gate secret instead of random stored keys, so a later run reaches the same pseudonyms. Results depend only on pseudonym equality.
- Series buckets are local wall-clock hours or days; a repeated fall-back hour is one bucket.
- The result documents are provisional until milestone 03 fixes the wire results. The oracle and probes compute raw numbers independently and share only rendering, rounding and the bucket list.

### Canonical input lines

Per group one line `G g site local_day t0 ip user_agent supplied_id device browser os country region referrer campaign identify`, then per event `E e time kind path query event_name revenue performance properties`. Fields are tab-separated, `-` marks an absent value, properties are `key=type:value` joined by `;`. The per-month hash is SHA-256 over these lines in day-placement order.

## Correctness

| Check | Result |
| --- | --- |
| Smoke, 24,000 groups (each month's first placement values) | 25 of 25 probe hashes equal a limited oracle run |
| Full 120M, host | 25 of 25 match |
| Full 120M, container, before and after clustering | 25 of 25 match, three times |
| Under load (two streams runs and the suite run) | 0 hash mismatches in 1,291 completed reports; 20 more timed out at 30 s |

The probes are typed bounded SQL compiled from each case state with bound parameters. They are provisional; milestone 06 owns the query compiler. Session fields come from each event's own row; the fixture keeps them equal within a session.

## Durable transfer

`cargo test --release -p holocron-storage --features failpoints` kills a child process with SIGKILL at each boundary, for 20 seeds per boundary. The hit number varies from 1 to 4, and batches have 3 to 11 records. Requests mix events with 0 to 3 properties, late events up to 300 days old, identify messages, 1 to 65 KiB replay fragments and heatmap interactions. A second process reopens the store, drains it and checks:

- every acknowledged request is stored completely, exactly once, with its exact property rows
- no request is partly stored
- a request killed before commit is absent, and one killed after commit is present
- ledger batches increase without overlap, and their record counts equal the stored records
- the watermark equals the last ledger entry, the inbox is empty, and every stored identity day is in `dirty_days`

| Boundary | macOS | Linux container |
| --- | --- | --- |
| Before inbox commit | 20 of 20 | 20 of 20 |
| After SQLite commit, before response | 20 of 20 | 20 of 20 |
| After acknowledgement, before assignment | 20 of 20 | 20 of 20 |
| After stable assignment commit | 20 of 20 | 20 of 20 |
| During DuckDB writes, before commit | 20 of 20 | 20 of 20 |
| After DuckDB commit, before notification | 20 of 20 | 20 of 20 |
| After notification, before SQLite retirement | 20 of 20 | 20 of 20 |
| During SQLite retirement | 20 of 20 | 20 of 20 |
| During correction application | not built (milestone 05) | not built |

Four focused tests also pass on both: visibility with a covering watermark, duplicate `eventId` suppression, whole-request refusal at a full inbox, and interrupting a slow read at the timeout.

The two OOM-killed burst runs below were also real process kills during transfer. Each next open resumed the assigned batch, but those runs kept no acknowledgement log, so their records were not reconciled one by one.

## Load and storage

- Bulk load through admission, inbox and transfer on the host: 124,000,000 records in 2,624 s (47,000/s), with a 4 GiB inbox backlog and 33,346 busy retries. Associated data: 9,000,000 heatmap interactions in 58 s, and 250,000 replay fragments (16.4 GB) in 701 s, which is 23 MB/s.
- Provisional sessions: 12,000,000 in 45 s. Clustering by site within each month: 342 s for 368M rows.
- Layout after loading: `events` 6.0 GB (50 bytes per event; `id` is the largest column, uncompressed), `event_properties` 3.7 GB (16 bytes per value), heatmap 0.56 GB, session properties 0.37 GB, sessions 0.29 GB, replay 16.4 GB of BLOB overflow.
- Disk amplification: `analytics.duckdb` is 44 GB with 107,400 used and 61,291 free 256 KiB blocks (26 GB live). `inbox.sqlite` stayed at 5.2 GB after the backlog drained; `VACUUM` brought it to 37 KB. Peak spill was 3.2 GB.

## Collection and visibility

Collection latency counts from the scheduled send, so a late send is not hidden.

| Run | Phase | Offered | Refused | Ack p50 / p95 / p99 / max (ms) | Ack to visible p50 / p95 / p99 / max (ms) |
| --- | --- | --- | --- | --- | --- |
| 3, interleaved layout | burst events | 30,000 | 0 | 22.8 / 74.1 / 137.9 / 295.8 | 117 / 415 / 805 / 1,372 |
| 3 | burst fragments | 3,000 | 0 | 34.9 / 93.3 / 162.6 / 311.8 | not probed |
| 4, clustered | burst events | 30,000 | 0 | 26.3 / 89.5 / 202.0 / 549.5 | 208 / 813 / 1,305 / 1,594 |
| 4 | burst fragments | 3,000 | 0 | 37.4 / 118.7 / 244.3 / 549.7 | not probed |
| 4 | baseline events, 4/s | 2,492 | 0 | 5.4 / 46.7 / 72.4 / 460.9 | 212 / 428 / 690 / 1,504 |
| ingest, no reports | events, 500/s for 200 s | 100,000 | 0 | 2.7 / 6.2 / 12.6 / 107.2 | 70 / 306 / 1,031 / 2,381 |

Send to visible in run 4 was p95 918 ms. Visibility probes run on the live lane every 100 ms with batched ID lookups (p95 42 ms per probe), so the visibility uncertainty is up to 100 ms plus one probe. An event counts as visible once a snapshot both returns its row and has a watermark covering its sequence. Every accepted burst event became visible.

## Reports

Two users followed the frozen `streams.json` early-gate order, with 500 ms ± 50 ms think time and baseline ingestion, and a burst at 120 s. Latency is the full read (queue wait plus execution plus rendering). p95 is nearest-rank; with 3 to 93 samples per case it is close to the maximum.

Run 4 (clustered layout), p95 in ms:

| Case | n | p95 | Target | | Case | n | p95 | Target |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Q01-30d-A-vienna | 24 | 3,009 | 2,000 | | Q01-12m-A-utc | 8 | 16,348 | 10,000 |
| Q01-30d-D-utc | 24 | 232 | 2,000 | | Q01-12m-D-vienna | 8 | 1,702 | 10,000 |
| Q02-30d-A-vienna | 12 | 2,065 | 2,000 | | Q02-12m-A-utc | 6 | 6 of 6 timed out | 10,000 |
| Q02-30d-A-vienna-page-99 | 12 | 2,257 | 2,000 | | Q02-12m-A-utc-tail | 5 | 4,932 | 10,000 |
| Q02-30d-A-vienna-tail | 12 | 1,884 | 2,000 | | Q02-12m-D-vienna | 5 | 759 | 10,000 |
| Q02-30d-D-utc | 12 | 246 | 2,000 | | Q03-12m-A-utc | 5 | 20,406 | 10,000 |
| Q03-30d-A-vienna | 16 | 1,318 | 2,000 | | Q03-12m-A-utc-drill-at | 5 | 11,576 | 10,000 |
| Q03-30d-A-vienna-drill-at | 16 | 3,404 | 2,000 | | Q03-12m-D-vienna | 5 | 831 | 10,000 |
| Q03-30d-D-utc | 16 | 239 | 2,000 | | Q04-12m-A-utc | 11 | 23,960 | 10,000 |
| Q04-30d-A-vienna | 24 | 4,308 | 2,000 | | Q04-12m-A-utc-selective | 10 | 13,970 | 10,000 |
| Q04-30d-A-vienna-broad | 23 | 5,635 | 2,000 | | Q04-12m-D-vienna | 10 | 2,343 | 10,000 |
| Q04-30d-A-vienna-selective | 23 | 1,467 | 2,000 | | | | | |
| Q04-30d-D-utc | 23 | 422 | 2,000 | | | | | |
| Q06-30d-A-vienna | 93 | 2,544 | 2,000 | | | | | |

Queue wait p95 was 0 to 4 ms in every case. The time goes to DuckDB execution, with two users sharing two engine threads.

Run 3 (interleaved layout, same budgets): every site D annual case took 3.3 to 12.9 s and every site A annual case 11.6 to 30 s. That run is the reason for clustering.

One user, process restart before each first query (OS page cache stays warm, so this is not a cold run), 10 starts per cold case. Startup to ready was 20 to 41 ms at p95.

| Case | p50 | p95 | Target |
| --- | --- | --- | --- |
| Q01-30d-A-vienna | 922 | 969 | 2,000 |
| Q02-30d-A-vienna | 795 | 877 | 2,000 |
| Q03-30d-A-vienna | 707 | 783 | 2,000 |
| Q04-30d-A-vienna | 836 | 867 | 2,000 |
| Q06-30d-A-vienna | 491 | 537 | 2,000 |
| Q01-12m-A-utc | 5,840 | 7,060 | 10,000 |
| Q02-12m-A-utc | 18,817 | 20,004 | 10,000 |
| Q03-12m-A-utc | 4,842 | 6,881 | 10,000 |
| Q04-12m-A-utc | 7,966 | 8,285 | 10,000 |

After the table probe stopped materializing unfiltered match sets, one-user `Q02-12m-A-utc` took 14.6 s.

Suite run (clustered, run 6): both users loop over their early-gate cases with think time and baseline ingestion until every case has 20 samples (1,819 s, 686 reports). p95 in ms:

| Case | p95 | | Case | p95 |
| --- | --- | --- | --- | --- |
| Q01-30d-A-vienna | 3,993 | | Q01-12m-A-utc | 15,466 |
| Q01-30d-D-utc | 276 | | Q01-12m-D-vienna | 1,233 |
| Q02-30d-A-vienna | 5,091 | | Q02-12m-A-utc | 9 of 20 timed out |
| Q02-30d-A-vienna-page-99 | 5,688 | | Q02-12m-A-utc-tail | 6,296 |
| Q02-30d-A-vienna-tail | 1,526 | | Q02-12m-D-vienna | 2,030 |
| Q02-30d-D-utc | 393 | | Q03-12m-A-utc | 14,941 |
| Q03-30d-A-vienna | 2,574 | | Q03-12m-A-utc-drill-at | 11,574 |
| Q03-30d-A-vienna-drill-at | 1,870 | | Q03-12m-D-vienna | 1,537 |
| Q03-30d-D-utc | 213 | | Q04-12m-A-utc | 28,870 |
| Q04-30d-A-vienna | 4,175 | | Q04-12m-A-utc-selective | 16,645 |
| Q04-30d-A-vienna-broad | 3,886 | | Q04-12m-D-vienna | 3,125 |
| Q04-30d-A-vienna-selective | 1,555 | | | |
| Q04-30d-D-utc | 277 | | | |
| Q06-30d-A-vienna | 2,203 | | | |

Across the three concurrent runs, every site D case passes. Site A fails `Q01-30d`, `Q02-30d` (top page and page 99), `Q04-30d` (default and broad) and `Q06-30d` at least twice, and every annual site A case except the path-tail case. Baseline collection in the suite run: 7,274 events, acknowledgement p95 42.4 ms, visibility p95 396 ms, with one outlier at 11.5 s.

### Profile of the slowest case

`EXPLAIN ANALYZE` of `Q02-12m-A-utc` in the container (plans in `.bench/results/plans/` of the run, not committed): the 57M-row match set took 2.3 s to scan, then four reads of the materialized set took 4.7 to 6.4 s each. Distinct daily identities for the totals and for the page rows run over the whole site-A year. Without materialization, the remaining cost is two scans of 57M rows and one exact distinct count over them. Two report users on two threads roughly double every time.

## Memory

| Run | Engine limit | Allocator | Outcome | cgroup peak | Anonymous peak | Spill peak |
| --- | --- | --- | --- | --- | --- | --- |
| 1, interleaved | 1.5 GiB | default arenas | OOM-killed at 182 s, 62 s into the burst | not sampled | not sampled | not sampled |
| 2, interleaved | 1.5 GiB | default arenas | OOM-killed at 280 s, after the burst | 3.86 GB at the last sample | 3.84 GB | 2.66 GB |
| 3, interleaved | 1 GiB | `MALLOC_ARENA_MAX=2` | completed | 3.76 GB | 2.39 GB | 3.2 GB |
| 4, clustered | 1 GiB | `MALLOC_ARENA_MAX=2` | completed | 3.75 GB | 2.43 GB | 2.7 GB |
| ingest | 1 GiB | `MALLOC_ARENA_MAX=2` | completed | 2.69 GB | 1.98 GB | 0 |
| suite, clustered | 1 GiB | `MALLOC_ARENA_MAX=2` | completed | 3.84 GB | 2.07 GB | 2.5 GB |

The OOM kills came from the VM's global OOM killer (the cgroup `oom_kill` counter stayed 0), because the VM has 4.18 GB in total. Runs 1 and 2 changed two settings at once, so the share of each change is unknown. Kernel memory and PSS were not reported by this kernel's cgroup files. Swap was off.

## Not verified

- **Reference host.** No 2 vCPU / 4 GiB / 250 GiB Linux VM was available. Shared host CPUs, a quota instead of dedicated cores, virtualized bind-mount I/O and a 4.18 GB VM all differ from the contract. The results show direction, not a pass or fail of the gate.
- **Power loss.** Not tested. Process kills do not test it. The bind mount's flush guarantee through OrbStack to APFS is unknown.
- **Cold cache.** No rebooted-VM runs. Restart runs kept the OS page cache.
- **Sample counts.** Required: 200 per case, 40 cold starts, three passing runs. Done: 3 to 93 per case in streams runs, 20 in the suite, 10 restarts.
- **24-hour soak, 10-minute transfer pause, ENOSPC injection, retention advance, backup and restore, deletion.** Not run.
- **Correction workload.** `dirty_days` is written in the batch transaction and survives every kill boundary, but no correction runs, so the late-event and identify-change probes, oracle deltas and a correction completion target are open. Needs milestone 05.
- **Umami baseline comparison.** Not run. Needs the pinned Umami and PostgreSQL deployment on the same host with the matched workload.
- **Q05, Q07 to Q14.** Outside the early gate.
- **HTTP.** Admission was measured in process; network and response encoding come with milestone 04.

## Open items

- For Mike: site A (the dominant site) misses its annual targets and several 30-day targets under two concurrent users. Lowering a target or a feature is your call; I did neither. Next design to test: exact identity-day rollups ([0008](../decisions/0008-storage-engine-and-budgets.md) Unresolved risks).
- For milestone 04: wire `Storage::admit`, set two Tokio workers and allocator arenas, and vacuum the inbox.
- For milestone 12: site-clustering compaction, `client_event_ids` pruning, and restore coordination of batch IDs with the ledger.
