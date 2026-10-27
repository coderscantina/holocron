# 0006: Retention cutoff, application clock, and operator commands

Status: Adopted (milestone 01, team decision) unless an item says otherwise. [0016](0016-configurable-retention.md) makes the 12-month event retention the installation default and lets each site choose 1, 2, 3, 5 or 10 years or forever; the cutoff rule below applies with that many months. Items marked Confirmed restate [the briefing](../technical-briefing.md). Items marked Confirmed (Mike, 2026-10-03) were decided by Mike on that date.

## Problem

The workload froze a retention boundary that no record defined, and its load, soak and boundary runs need an application clock they can set. Milestone 12 needs exit codes for `holocron init` and a schema version scheme for `holocron upgrade`. Milestone 09 needs to know whether saved replays outlive the 14-day replay retention.

## Constraints

Confirmed:

- Retention: 12 months analytical events, 14 days raw replay, 90 days aggregated heatmaps. 25 GiB replay cap. Budgets enforced during admission and transfer, not only by periodic cleanup.
- Deletion is an ordered operation across both engines; a replayed inbox or resumed import must not bring deleted data back.
- Generate or require bootstrap credentials; no known default password.
- Back up schema versions with the data. Stable applied-batch entries carry enough schema metadata to detect an incompatible replay after upgrade or restore.
- Performance evidence uses release builds.

## Evidence

- [Workload guide](../contracts/workload-and-query-suite.md) and `fixtures/workload/manifest.json` `retention_advance`: "an event is eligible for removal when its site-local date is before (site-local date of new_clock) minus 12 calendar months". At `2026-11-01T00:00:00Z` that is local date `< 2025-11-01` for site A and `< 2025-10-31` for site B.
- The same manifest freezes boundary cases at exact receive times (24 h and 5 min edges), which HTTP delay would move under a real clock.
- Upstream 3.4.0 never expires saved replays (`session_replay_saved`); the [capability matrix](../contracts/capability-matrix.md) lists the interaction with 14-day retention as undecided.

## Choice

### Retention cutoff

Adopted: the workload's rule, stated per data kind. `today` is the local date of the application clock in the site's current timezone (UTC for links and pixels).

| Data | Removed when | Basis |
| --- | --- | --- |
| Analytical events, properties, sessions, performance, revenue | `event_time` < start of local date `today − 12 calendar months` | 12 months confirmed |
| Raw replay | the capture's first fragment is before the start of `today − 14 days`; the whole capture goes at once; saved replays are exempt (below) | 14 days confirmed |
| Heatmap aggregates | the cell's local date < `today − 90 days` | 90 days confirmed |
| Identity day-key tombstones ([0001](0001-identity-and-consent.md)) | with that day's events | |

- Calendar-month subtraction clamps to the month's last day: from 2028-02-29 the cutoff date is 2027-02-28.
- The rule keeps the current partial day plus 12 full months. Site A at `2026-11-01T00:00:00Z` (local 2026-11-01) removes events before `2025-10-31T23:00:00Z`, which is the workload's "local date < 2025-11-01".
- A replay capture is removed whole, so a playback never loses its beginning while its end remains.
- Live events that are already older than the cutoff when received are not stored: outcome `suppressed`, reason `outside_retention`, counted (Adopted, consequence of Mike's 2026-10-03 decision to accept any timestamp, [0002](0002-sessions-and-events.md)). Skipping uses the same rule as imports, so one cutoff decides what is stored on every path; storing and deleting on the next run would write rows no query can show.
- Events with far-future times are never older than the cutoff, so retention never removes them. This is a known risk of accepting any timestamp; `event_time_ahead` in operations status shows it (0002).
- Queries apply the same cutoffs as a predicate, so a result never depends on whether the removal job has run yet. A resolved range that starts before the cutoff returns `retention_start` in its bounds; the same holds for comparison bounds.
- The removal job runs at least hourly and follows the ordered deletion protocol, including pending inbox records.
- Imported events before the cutoff are not imported; the import manifest counts them as `outside_retention`.

### Application clock

All wall-clock reads in domain code go through one `Clock` in the application state: receive time, the retention check on arrival, day-key creation and destruction, dedup pruning, realtime `now`, relative range resolution, retention cutoffs, share, login session and idempotency expiry, and TOTP checks. Timeouts and the group-commit delay are durations and use Tokio's monotonic clock.

- Default: `system`.
- Test mode, set only in the configuration file: `testing.clock.start` (RFC 3339 instant) and `testing.clock.mode`:
  - `run`: `start` plus monotonic time elapsed since boot. Used by load, soak and correction runs.
  - `frozen`: stays at `start` until changed. Used by exact boundary cases.
- In test mode the operator-only command `testing.setClock { instant }` moves the clock. It moves forward only, like a real clock. Keyless events need no clock tricks: a payload with an old `timestamp` reaches a destroyed day key directly ([0001](0001-identity-and-consent.md)).
- Test mode is in release builds, because performance evidence uses release builds. It cannot be on silently: startup logs a warning, and health and operations status report `clock: "test"`.

### `holocron init` outcomes

`holocron init --data-dir <path> --admin-email <email> [--admin-password-file <path>]`.

| Situation | Exit code | Result |
| --- | --- | --- |
| Directory missing or empty | 0 | Creates SQLite and DuckDB files, root secret, schema, and the first install admin. Without `--admin-password-file`, prints a 24-character generated password once on stdout, never to logs |
| Invalid arguments, password file under 12 characters | 2 | Usage error, nothing written |
| Directory already initialized | 3 | `already_initialized`, nothing changed |
| Directory not empty and not Holocron's | 3 | `directory_not_empty`, nothing changed |
| Path not writable, read-only filesystem, disk full, data directory lock held | 4 | `storage_unavailable` with the path and OS error |
| Unexpected internal error | 1 | Error report without secrets |

- Init writes `holocron.init-in-progress` first and `holocron.initialized` last. A rerun that finds the in-progress marker removes its own leftovers and starts over, so an interrupted init never needs manual cleanup and never leaves a half-initialized directory that starts.
- The same codes apply to every `holocron` command: 0 success, 1 internal error, 2 usage, 3 refused by data-directory state, 4 storage unavailable. File names inside the data directory belong to milestone 02.

### Schema versions

- Two integers: `control_schema` (SQLite) and `analytics_schema` (DuckDB). Each starts at 1 and increments by one per migration. Each engine stores its own value in a one-row `schema_info` table, with the binary version that wrote it.
- A release declares the exact pair it runs on and the oldest pair it can upgrade from.
- Inbox records carry `inbox_format` (integer). Applied-batch ledger entries record `analytics_schema` and `inbox_format`, so a replay from another schema is detected and refused.
- Backups record both schema integers, `inbox_format`, and the binary version.
- `serve` never migrates. Startup outcomes, all exit code 3:
  - `upgrade_required`: data is older than the binary; run `holocron upgrade`.
  - `data_newer_than_binary`: refuse; install the matching or newer release.
  - `upgrade_interrupted`: the engines disagree with the release pair (one engine migrated, the other not); rerun `holocron upgrade`, which resumes.
- `holocron upgrade` takes a verified pre-upgrade backup, migrates SQLite then DuckDB, and records progress per migration. If a migration fails, it stops with `upgrade_failed`, leaves the pre-upgrade backup in place, and prints the restore command. Rerunning resumes from the last completed migration.

### Saved replays

Confirmed (Mike, 2026-10-03): saved replays are exempt from the 14-day removal and count against the 25 GiB replay cap, within a 5 GiB sub-cap per installation. Adopted details:

- A save that would exceed 5 GiB of saved replays is refused with `saved_replay_budget_exceeded`.
- Unsaving makes the capture subject to the 14-day rule again, so it goes at the next run if it is older.
- Site deletion, recording permissions and erasure still remove saved replays.
- The sub-cap keeps at least 20 GiB for new recording, so saved replays can never stop capture on their own.

### Replay storage format

Not decided here. The chunk format and playback isolation belong to milestone 09, together with the hostile-content fixtures at `fixtures/replay-hostile/`. The workload treats replay fragments as opaque 65,536-byte payloads, which any format can store.

## Rejected alternatives

- Retention by elapsed time (`now − 365 days`): cuts days in half and disagrees with calendar reports.
- Removing replay fragments one by one by age: playback of a capture straddling the boundary would start in the middle.
- Queries that show whatever the removal job has not yet deleted: results would change with job timing.
- A clock that can move backwards in tests: it reaches states a real server cannot, such as day keys recreated after destruction.
- A test-only build with the injectable clock: performance evidence must come from the release binary.
- Automatic migration on `serve`: an upgrade then runs without its backup, at an unplanned moment.
- One combined schema number: the engines migrate separately, and a half-finished upgrade must be detectable.

## Consequences

- Milestone 02 implements the cutoff predicate and the `Clock`; the workload's `retention_advance.cutoff` moves from "proposed oracle rule" to this record.
- Milestone 12 implements the init and upgrade outcome tables; CAP-OPS-01 and CAP-OPS-09 have a rule source.
- Imports skip every source event older than the 12-month cutoff at import time and count them as `outside_retention` in the import manifest (milestone 11). This follows the confirmed 12-month retention and widens nothing, so it is Adopted. Operators should know it before cutover: an Umami installation with three years of history arrives with the last 12 months only, and the import preview must show the skipped count before the import starts.

## Affected contracts

[Workload and query suite](../contracts/workload-and-query-suite.md) "Choices made here"; workload manifest `clock` and `retention_advance`; [capability matrix](../contracts/capability-matrix.md) CAP-OPS-01, CAP-OPS-05, CAP-OPS-09, CAP-RECORD-04; milestones 02, 09, 11, 12.

## Amendment (0021)

Dated 2026-10-27. Follows [0021](0021-web-setup-and-onboarding.md), adopted by Mike on 2026-10-22. It changes the bootstrap credentials for a container install. The rest of this record stands.

- `holocron serve` on an empty data directory starts in setup mode. The one-time setup token is the bootstrap credential. It is logged at each boot in setup mode, or pinned with `HOLOCRON_SETUP_TOKEN`.
- The first admin chooses their password in the web setup wizard. That path shows no generated password.
- `holocron init` stays for the CLI path, with its generated or file-supplied password and the outcomes above.
- No known default password exists on either path.
