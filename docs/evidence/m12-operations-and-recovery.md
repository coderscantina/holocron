# Milestone 12 evidence: operations and recovery

Status on 2026-10-05: built and tested locally, in progress. Backup and clean-directory restore, mismatch detection, erasure and retention across restarts, interrupted upgrade, shutdown and health all have passing checks through the real engines. Import integration is wired (see below). Open: real ENOSPC and power loss are untested, disk growth was measured at 1M events on a laptop instead of the retained fixture on the reference host, and two Proposed items of [0014](../decisions/0014-operations-and-recovery.md) wait for Mike.

Decision: [0014 operations and recovery](../decisions/0014-operations-and-recovery.md). Operator steps: [runbook](../operations/runbook.md).

## Run

- Source: commit `63e76d5` plus the uncommitted milestone 12 changes in this worktree. `Cargo.lock` sha256 `0499045d0b38d0c2db1cf651d28b5d2d162b88cec921d78c05787d28a98e6f19`.
- Engines: DuckDB 1.5.6 (duckdb-rs 1.10506.0, bundled), SQLite 3.51.3 through SQLx 0.9.0. Rust 1.99.0.
- Host: Apple M1 Pro, 10 cores, 32 GiB, macOS (Darwin 27.0.0), APFS SSD. Not the reference host.
- Schemas after this milestone: control 9, inbox 4, recordings 1, analytics 4; `inbox_format` 2.

Commands, from the repository root:

```sh
cargo test -p holocron-storage                                  # unit tests, tests/operations.rs, tests/storage.rs
cargo test --release -p holocron-storage --features failpoints  # transfer kill boundaries, unchanged
cargo test -p holocron-server --no-fail-fast                    # every server test, tests/operations.rs included
cargo clippy -p holocron-storage -p holocron-server --all-targets
cargo check -p holocron-workload -p holocron-analytics --all-targets
cargo fmt --all
cargo run --release -p holocron-server --example operations_disk -- 1000000 <dir>
# runbook rehearsal with target/release/holocron: init, serve, collect, backup while serving,
# diagnostics, SIGTERM, restore, refused restore onto a live directory, retention, upgrade
```

## What was built

| Work item | Where | Behaviour |
| --- | --- | --- |
| 1 commands | `apps/server/src/main.rs`, `maintenance/commands.rs` | `backup`, `restore`, `upgrade`, `retention`, `erase-profile`, `diagnostics` beside `init` and `serve`, with 0006 exit codes and JSON results. Job commands go to a running server through `maintenance_jobs`, or run offline under the directory lock |
| 2 shutdown | `main.rs` `serve`, `Storage::shutdown` | Readiness turns `shutting_down`, 10 s drain, background step commits, admission stops, transfer finishes its batch, read workers close, DuckDB checkpoints |
| 3 backup | `maintenance/backup.rs`, `Storage::snapshot` | One verified directory with a manifest; analytics and inbox copied while the analytical writer waits |
| 4 restore | `maintenance/restore.rs`, `holocron_storage::inspect` | Checksums, versions, pair positions, credentials, inbox applied and counted before the directory is marked initialized |
| 5 retention, erasure | `maintenance/{retention,erasure}.rs`, `crates/storage/src/fence.rs`, `Storage::{fence, erase, expire}` | Hourly retention; site deletion, site reset and profile erasure as fenced, resumable jobs; `sites.reset` implemented |
| 6 disk | `Storage::disk`, `examples/operations_disk.rs` | File sizes per engine, WAL, spill and free space; measurement below |
| 7 health | `maintenance/{health,diagnostics}.rs` | `/health/ready` with problem names; diagnostics from counts and states only |
| 8 upgrade | `maintenance/{versions,upgrade}.rs`, `holocron_storage::upgrade` | Per-file versions, `serve` refuses older, newer and interrupted data; upgrade with pre-upgrade backup, marker, resume |

Migrations: `apps/server/migrations/0009_maintenance.sql` (control, `maintenance_jobs`) and `crates/storage/migrations/0004_operations.sql` (inbox, `pairing` and `fences`). DuckDB schema 4 adds `schema_info`. Both are applied only by `holocron upgrade` on existing data and by `init` on new data; a pending inbox survives them, and applied batches of the old schema are retired before DuckDB changes. Backups from before this milestone do not exist.

## Acceptance criteria

| Criterion | Result | Evidence |
| --- | --- | --- |
| A coordinated backup with a nonempty inbox restores on a clean machine with reconciled counts and usable credentials | Pass in a new directory on the same machine. Not run on a second machine | `a_backup_with_waiting_records_restores_into_a_new_directory`: 3 applied and 2 waiting events backed up from a serving installation, restored into an empty directory, `records_applied` 2, 5 events, admin login with the init password. Storage: `a_snapshot_with_pending_records_reopens_with_every_record` |
| Independent or mismatched engine snapshots are detected rather than silently served | Pass | `mixed_or_damaged_snapshots_are_refused`: damaged file `backup_corrupt`, earlier analytics with a rewritten manifest `engines_mismatched` at restore, the same file copied into a live directory `engines_mismatched` at open. Storage: `files_of_another_pair_or_moment_are_refused` (other installation, older analytics, deleted analytics), pairing unit tests |
| Retention and deletion pass interruption/restart cases, including assigned batches and resumed imports | Pass for assigned and pending batches and restarts. Resumed imports: an unfinished import refuses `serve` and every operator job (`import_in_progress`), so erasure cannot run between an interrupted import and its resume; no dedicated test yet | `site_deletion_erases_applied_waiting_and_replay_data_after_a_restart` (server stopped between the command and its job, event waiting in the inbox, replay capture, day keys), `site_deletion_removes_applied_and_pending_records_and_survives_restart`, `reset_keeps_records_admitted_after_the_fence`, `a_reset_keeps_the_site_and_events_sent_after_it`, `profile_erasure_removes_only_that_profile`, `erasing_a_profile_removes_its_history_and_sessions_and_keeps_other_visitors`, `retention_removes_events_past_twelve_months_and_keeps_the_rest`, `fence::tests` (import origin) |
| Full disk, transfer failure, stale analytical state, and unhealthy recorder budget are distinguishable | Pass, with full disk through the disk reserve, not real ENOSPC | `health_tells_disk_full_failing_transfer_stale_analytics_and_recorder_budget_apart`: each state gives exactly one problem name; only `disk_full` answers 503 |
| Maintenance fits declared resource and disk budgets, or measured limitations are resolved before 13 | Partly: measured at 1M events; the retained-scale run is open | [Disk growth](#disk-growth) |
| Diagnostics contain no raw identity inputs, credentials, or event bodies | Pass for the canaries that can be planted through this milestone's paths | `diagnostics_carry_state_but_no_identity_inputs_credentials_or_bodies`: IP, user agent, event name and data, identify ID and bootstrap password from `fixtures/gaps/diagnostics-canaries.json` absent; version, schema, storage, inbox, watermark and recording budget present. API key, share token, TOTP, user password and source session canaries were not planted; the bundle reads no table that holds them |
| The packaged one-process deployment and documented commands work from a fresh installation | Pass for the binary; no package exists yet | `the_binary_backs_up_while_serving_and_stops_in_order_on_sigterm` (debug binary) and the release rehearsal below |

Other checks: `a_failed_upgrade_is_refused_by_serve_and_resumes` (fixture `schema-upgrade.json` up-fail-duckdb, up-resume, su-older, su-newer, su-interrupted), `upgrade_retires_batches_the_old_schema_applied`, `shutdown_refuses_admission_and_leaves_no_analytics_wal`, `versions::tests::serve_starts_only_on_current_data`. The access fixture's `sites.reset` rows now reach the real handler.

## Shutdown

The binary test sends 20 events, backs up and runs diagnostics through the running server, then SIGTERM: exit 0 in under a second, analytics WAL 0 bytes, all 20 events after restart. A second `serve` on the same directory exits 4. The 20-round SIGKILL test (`tests/crash.rs`) still passes, so a hard stop keeps the milestone 04 guarantees.

## Release rehearsal

`target/release/holocron` on a scratch directory, 2026-10-05:

| Step | Result |
| --- | --- |
| `init` with a password file | exit 0 |
| `serve`, 50 tracker events, `GET /health/ready` | 200, `{"status":"ready","problems":[]}` |
| second `serve` on the directory | exit 4 |
| `backup` while serving | exit 0, pause 40 ms, ledger records 50 |
| `diagnostics` while serving | exit 0, 1,798 bytes |
| SIGTERM | exit 0 within 1 s |
| `restore` onto the live directory | exit 3 `already_initialized` |
| `restore` into a new directory | exit 0, `ready` |
| `retention` without a server | exit 0 |
| `upgrade` on current data | exit 0, `current` |

## Disk growth

`examples/operations_disk.rs`, release build, reference engine budgets (2 threads, 1 GiB), 1,000,000 events over 396 days in one site, loaded through admission and transfer. Correction did not run, so derived tables are empty; their size is in the milestone 05 and 06 evidence. APFS clones files, so copy times on this laptop are lower bounds.

| Measurement | Result |
| --- | --- |
| Encoded inbox record | 131 bytes per event: the 1 GiB inbox budget holds 8.2M events, 4.5 hours of the 500 events/s burst |
| Peak during the load (12.7 s) | analytics WAL 9.1 MB, inbox file 14.8 MB, inbox WAL 8.3 MB |
| After the load | analytics 29.4 MB plus 9.1 MB WAL until checkpoint |
| Retention one month later | 156,566 events removed in 18 ms; DuckDB still 103 used and 9 free blocks of 256 KiB in the same snapshot, WAL 10.6 MB; after the checkpoint at stop the file stays 29.4 MB and the WAL is 0. Freed blocks are reused, the file does not shrink (0008) |
| Query spill, sort of every event at a 16 MiB engine limit | 19.7 MB spilled, then the query failed out of memory; `spill/` empty afterwards. At 64 MiB the same sort did not spill |
| Backup while serving | pause 19 ms, 0.11 s total, 30.0 MB in the destination (data directory 30.0 MB) |
| Backup temporary space | the `.partial` directory in the destination, the size of the backup; nothing on the data volume |
| Interrupted backup | an 8 MiB partial directory removed by the next backup, none left |
| Restore into a new directory | 0.07 s, `ready` |

Extrapolation, not measured: the pause is the time to copy `analytics.duckdb` and `VACUUM INTO` the inbox. For the 44 GB analytics file of the milestone 02 fixture at 1 to 2 GB/s that is 20 to 45 s without a copy-on-write filesystem, during which the inbox grows at 131 bytes per event (4 MB at the burst rate). Hashing adds about the same again twice, outside the pause. Backup storage needs the data directory's size per kept backup and is not part of the 250 GiB working budget.

## Full disk

Run on 2026-10-06 with `apps/server/examples/enospc.rs` (release, reserve 0) on a 512 MB HFS+ image, which fills to the last block; APFS still accepts small writes after a failed file write. Each scenario fills the volume with a file, drives one engine path, frees the file, then restarts and counts every acknowledged event.

| Scenario | While full | After freeing space | Lost |
| --- | --- | --- | --- |
| Inbox commit | 50 of 50 sends 503 `Retry-After`; readiness `storage_unavailable` | 20 of 20 sends acknowledged without a restart; readiness keeps `storage_unavailable` for its one-minute refusal window | 0 of 40 |
| Transfer, inbox volume full | 6 failed transfer attempts, `transfer_failing` | drained without a restart | 0 of 20,005 |
| Transfer, DuckDB WAL full | 5 failed commits with a clear WAL write error, `transfer_failing` | drained by itself | 0 of 20,005 |
| Spill | the 300,000-row query answered without spilling in the rerun (the first run failed it with `No space left on device` and left no spill files); sends 503 | query answers | 0 |
| Replay part | `database or disk is full` | next part accepted, integrity ok | none |
| Backup destination | `storage_unavailable … os error 28`, nothing left behind, collection unaffected | next backup succeeds | 0 of 10 |
| Retention | job fails with `database or disk is full`, data untouched | removes 50,000 of 100,005 | 0 |

The first run found two bugs, both fixed the same day. A failed SQLite COMMIT left sqlx's single writer connection inside a transaction, so the inbox, replay store and control writers failed every later write until a restart; each writer pool now replaces a connection that still reports a transaction (`before_acquire`). A collection request whose control write hit the full disk answered 500; a full disk (`SQLITE_FULL` or ENOSPC) now answers 503 with `Retry-After` like other storage refusals.

## Open items

- **Decided by Mike, 2026-10-06:** online backup is the default and public readiness names its problems ([0014](../decisions/0014-operations-and-recovery.md)).

- **Import integration (milestone 11).** Done by exclusion rather than per-record fence checks: the import holds the data lock and an unfinished import refuses `serve` and operator jobs. Site deletion, reset and retention remove `import_provenance` and `imported_history` rows. Since 2026-10-06 also: `a_killed_import_refuses_erasure_and_retention_until_it_resumes` (through `tools/import-rehearsal/test.sh`) and `erasure_reaches_the_import_inventory_and_provenance_of_its_site_or_visitor` ([0014](../decisions/0014-operations-and-recovery.md) Erasure fences).
- **Retained-scale measurement.** Rerun `operations_disk` with the 120M fixture on the reference host: retention one month later, backup pause and size, spill under load, restore and time to fresh reports.
- **Read-only storage** on a disposable volume: not run.
- **Second machine restore and power loss**: not run.
- **Compaction** is scheduled since 2026-10-06 ([0014](../decisions/0014-operations-and-recovery.md) Compaction). `compaction_resumes_after_a_crash_and_clusters_closed_months_by_site` copies the files after three committed steps and resumes: every event moves once and each site's month ends as one block in time order. At 1M events (release, laptop, `operations_disk`) it moved 843,434 events in 11 steps, 6.8 s in total, slowest step 1.19 s, WAL peak 10.9 MB, and the file did not grow. Full scale is not measured; the estimate is about 480 steps, 10 minutes of writer time.
- `client_event_ids` pruning runs with retention; its growth was not measured.
