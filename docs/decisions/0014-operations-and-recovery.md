# 0014: Operations and recovery

Status: Adopted (milestone 12, team decision) Mike adopted the two items below on 2026-10-06. Evidence: [m12 operations and recovery](../evidence/m12-operations-and-recovery.md).

## Problem

[0006](0006-retention-clock-and-operations.md) fixed exit codes, schema version outcomes and retention cutoffs, and [0008](0008-storage-engine-and-budgets.md) the transfer protocol. Milestone 12 still needs to fix how work that spans engines runs and resumes, how a backup stays coherent while collection goes on, how a restore or a hand-copied file is caught when the engines do not belong together, how erasure keeps every writer from bringing data back, and what health reports.

## Constraints

Confirmed: durable acknowledgement only after a SQLite commit; one analytical writer; stable batch IDs with an applied-batch ledger; deletion ordered across both engines so a replayed inbox or a resumed import cannot resurrect it; backups start with a maintenance pause and never copy live files independently; `serve` never migrates; diagnostics hold no identity inputs, credentials or event bodies. The unchanged tracker never retries, so every second collection is down loses events.

## Choice

### Jobs

- Work across engines runs as a job in `maintenance_jobs` (control migration `0009`): `erase_site`, `reset_site`, `erase_profile`, `retention`, `backup`, `diagnostics`. One runner per server works through them one at a time, queued jobs first.
- `sites.delete` and `sites.reset` queue their job in the command's own transaction, so a crash after the commit still erases.
- An erasure or retention job interrupted by a stop runs again from the start; every step finds nothing to do when it already ran. An interrupted backup or diagnostics run fails with `interrupted`; the operator asks again.
- An erasure never gives up: a failing step is logged and retried every second.
- Retention queues itself when none ran in the last hour (0006: at least hourly).

### Operator commands and the data directory lock

- `holocron.lock` in the data directory is held with `flock` by `serve`, `init`, `restore`, `upgrade`, and by operator commands that run without a server. A second holder gets exit 4 `storage_unavailable`.
- `backup`, `retention`, `erase-profile` and `diagnostics` find out whether a server holds the lock. If one does, they queue a job in the control database and wait for its row; SQLite in WAL mode is safe across processes. If none does, they take the lock, open the engines exactly as `serve` does, and run the queue themselves. One code path either way; no socket or admin HTTP route.
- Results are JSON on stdout. Failures follow 0006: `error: <code>: <message>` on stderr and its exit code.

### Shutdown

On SIGTERM or Ctrl-C: readiness turns `shutting_down`, the listener stops accepting, open requests get 10 s (live subscriptions never end on their own and are cut), background work commits its current step, then storage stops admission (queued requests still commit), lets transfer finish the batch in hand, closes the read workers and checkpoints DuckDB. Records still in the inbox transfer at the next start, which reconciles from the ledger as before.

### Pairing and positions

- `inbox.sqlite` (`pairing` table, storage migration `0004`) and `analytics.duckdb` (`schema_info.pair`) carry one random pairing ID. Two different IDs are refused. A file without one takes the other's.
- Every open checks positions: every batch ID ever assigned (the inbox's `AUTOINCREMENT` counter) is either in the ledger or still assigned in the inbox, a batch in both has the same membership, and the inbox's counters are not behind the ledger. An analytics file older than the inbox, an inbox older than the analytics file, a deleted file, or a file from another installation is refused with exit 3 `engines_mismatched` and the advice to restore a whole backup.
- Residual risk: two snapshots of one installation with no batch applied between them pass, and they are consistent.

### Backup

- One directory per backup: the four engine files, `secrets.key`, and `manifest.json` with the format, the release, every schema version, `inbox_format`, the pair's positions, the pause, and the size and BLAKE3 hash of each file. Files are mode 0600.
- On a running server the analytical writer is held while DuckDB is checkpointed and copied and the inbox is copied with `VACUUM INTO`. Transfer, correction and maintenance writes wait; admission and reads go on, and the inbox absorbs new events (1 GiB budget). The control database and replay store are copied right after with `VACUUM INTO`; a job they hold that the analytics copy predates runs again after a restore.
- On a stopped installation (the pre-upgrade backup) the files are copied at whatever schema they have.
- The copy is opened as a pair, its positions recorded, hashed, verified by hashing again, and only then renamed from `.<name>.partial` into place. The next backup removes partial directories an interrupted one left.

### Restore

`holocron restore --from <backup> --data-dir <new dir>`: verify the manifest and every hash; refuse data newer than the release and a non-empty inbox of another `inbox_format`; copy into a missing or empty directory under `holocron.restore-in-progress`; check the pair's positions equal the manifest's; then, at the release's schema, open everything, require an install admin and the root secret, let transfer apply the inbox, and require `ledger records = manifest ledger records + manifest inbox records`. Only then `holocron.initialized` is written. Older data restores and reports `upgrade_required`.

### Erasure fences

The fence is the interface every writer into analytics respects (`holocron_storage::{Erasure, Fence, Fences, Origin}`):

| Erasure | Covers | Used by |
| --- | --- | --- |
| `Site(id)` | every record of the site, admitted at any time | site deletion |
| `SiteHistory(id)` | records admitted up to the sequence read when the reset was asked for | site reset |
| `Profile { site, profile }` | that profile's records admitted up to the fence | `holocron erase-profile` |

- Order: fence (committed in the inbox, serialized with admission), replays and raw heatmap interactions, analytics (canonical rows, derived rows, heatmap cells, dirty days for correction), control rows (day keys of a deleted site, a profile's consents), then done once transfer moved past the fence. Transfer reads the fences while it holds the writer, so no apply that started before a fence can land after its erase.
- Transfer leaves covered records out and still records the whole batch in the ledger.
- An import and erasure never overlap. `holocron import run` holds the data-directory lock, and while an import is unfinished `serve` and every operator job refuse with `import_in_progress`. Imported records pass through the inbox, so transfer applies the fences to them like to collected ones.
- A profile is named by its keyed ID; the identify ID given on the command line is never stored. Its replays are the captures recorded under the daily pseudonyms of its erased events.

Since 2026-10-06, site deletion also removes the site's control-side `import_inventory` rows, found through the stored source row's `website_id` or a website share's `entity_id` (imported sites keep their Umami ID). A reset removes only the imported history rows (`session_link`, `session_replay`, `session_replay_saved`). Profile erasure removes the `session_link` rows and the `import_provenance` rows whose stored distinct ID derives to the erased profile.

### Compaction

Adopted 2026-10-06. `serve` clusters closed UTC months (ended more than a day ago) by site and time, in steps of about 250,000 events with their session and event properties. Each step is one analytical transaction that also commits the month's cursor in a DuckDB `compaction_months` table, so an interrupted month resumes from its last committed slice and no event moves twice. The first pass runs 10 minutes after start, then hourly; steps are 0.5 s apart and take the single analytical writer, so transfer, correction, backup, erasure and retention take turns with it. A pass stops while the oldest inbox record is over 10 s old or free space is below the reserve plus a quarter of the analytics file, since a month's old blocks are freed only after its last slice. A clustered month is redone once rows arriving after its pass reach 5% of it. Adopted by Mike on 2026-10-15: the cursor table lives in DuckDB outside the versioned analytics schema (created when compaction first runs), so it commits with the moved rows. The alternative is a control-side table with a migration, which needs a second commit per step and a check that both agree. Rejected: one table rewrite, which took 342 s for 368M rows in the milestone 02 gate while holding the writer, and needs room for a second copy.

### Schema versions and upgrade

- One version per engine file: SQLite files count their applied migrations; DuckDB records `schema_info.analytics_schema` (now 4). This refines 0006's two integers, because three SQLite files migrate separately. A missing file counts as current.
- `serve` checks versions before opening anything: newer is `data_newer_than_binary`, an upgrade marker is `upgrade_interrupted`, older is `upgrade_required`; all exit 3. The storage crate refuses an older schema on its own as well.
- `holocron upgrade --backup-to <dir>`: verified pre-upgrade backup, then `holocron.upgrade-in-progress` naming it, then control, recordings, inbox, retirement of batches the ledger already holds (their ledger entries name the old schema), then DuckDB, then a version check, then the marker goes. A failure exits 1 `upgrade_failed` with the resume and restore commands. A rerun verifies the named backup and resumes without a second one.

### Health

`GET /health` is liveness. `GET /health/ready` answers 200 while collection can store events, else 503, and lists problem names only:

| Problem | Meaning | Collection |
| --- | --- | --- |
| `disk_full` | free space below the disk reserve | 503 |
| `storage_unavailable` | durable commits failed in the last minute | 503 |
| `shutting_down` | stop signal received | 503 |
| `transfer_failing` | every transfer attempt since a moment failed | ready |
| `analytics_stale` | transfer works but the oldest waiting record is over 60 s old | ready |
| `recorder_budget` | a replay part was refused at the cap in the last minute, or 90% used | ready |

`holocron diagnostics` builds its JSON field by field from counts, sizes, versions, states and fixed names, so there is nothing to scrub afterwards.

### Adopted by Mike (2026-10-06)

- **Online backup is the default.** The briefing's "brief maintenance pause" is read as pausing the analytical writer, not collection: stopping collection loses every event the unchanged tracker sends meanwhile.
- **Readiness is public and names problems.** It reveals `disk_full` to anyone who can reach it, as upstream's heartbeat is public. The alternative is a bare status code.

## Rejected alternatives

- Offline-only backup: collection down for the copy, 44 GB on the reference fixture.
- A control socket or admin HTTP route for operator commands: a second protocol where a table in the control database already serves.
- Matching records to an erasure by decoding pending inbox rows: transfer already decodes them once; leaving them out there needs no second pass.
- Inferring an interrupted upgrade from versions alone: a release that changes only some files would look interrupted.

## Consequences

- An unfinished import blocks serving and operator jobs until it is resumed to completion. Site erasure and retention also remove the import provenance rows in DuckDB.
- `sites.reset` is implemented; the access fixture's deferred row is gone.
- `holocron init` now creates every engine file, so a fresh directory can be backed up and upgraded before its first start.
- Data directories from before this release need `holocron upgrade` once.

## Affected contracts

[0006](0006-retention-clock-and-operations.md) Schema versions (per-file versions, marker), [0008](0008-storage-engine-and-budgets.md) Durable transfer (fences, pairing), [capability matrix](../contracts/capability-matrix.md) CAP-OPS-01 to 10, `fixtures/gaps/schema-upgrade.json`, `fixtures/gaps/diagnostics-canaries.json`.
