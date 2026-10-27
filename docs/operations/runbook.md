# Holocron operator runbook

For one-server operators. Every command takes `--data-dir` (or `HOLOCRON_DATA_DIR`). Results print as JSON on stdout. Failures print `error: <code>: <message>` on stderr and exit with:

| Exit | Meaning |
| --- | --- |
| 0 | success |
| 1 | internal error, or `upgrade_failed` |
| 2 | usage error |
| 3 | refused by the state of the data directory |
| 4 | storage unavailable: path not writable, disk full, or another holocron process holds the directory |
| 5 | `holocron query` only: the server answered with an error envelope |

Decisions behind this: [0006](../decisions/0006-retention-clock-and-operations.md) and [0014](../decisions/0014-operations-and-recovery.md). Importing from Umami: [import and cutover](import-and-cutover.md).

## Install

`holocron serve` on an empty data directory starts in setup mode and logs a setup token; the wizard at the public URL creates the admin and the first site, imports from Umami, or restores a backup ([install guide](../guide/install.md#set-up-in-the-browser)). From the shell instead:

```sh
holocron init --data-dir /var/lib/holocron --admin-email you@example.com --admin-password-file pw.txt
holocron serve --data-dir /var/lib/holocron --bind 0.0.0.0:8080 --public-url https://analytics.example.com
```

Without `--admin-password-file`, init prints a 24-character password once on stdout. Init creates every engine file, the root secret and `secrets.key`. Keep the data directory on one local volume and leave at least the disk reserve (`--disk-reserve-mib`, default 4096) free.

### Host size

`serve` runs the standard budget (2 vCPU, 4 GiB) unless told otherwise. Pick a tier with `--budget small|standard|large` (env `HOLOCRON_BUDGET`):

| Tier | Host | Engine | Tokio / report workers / queue |
| --- | --- | --- | --- |
| small | 1 vCPU, 2 GiB | 1 thread, 512 MiB | 1 / 1 / 4 |
| standard | 2 vCPU, 4 GiB | 2 threads, 1 GiB | 2 / 2 / 8 |
| large | 4 vCPU, 8 GiB | 4 threads, 2 GiB | 4 / 4 / 8 |

Only standard is measured. Small and large are estimates from the standard results, and small has not passed the 500 events/s burst. The server never picks a tier from the host, so set it yourself. An explicit `--worker-threads` wins over the preset. The startup log line `resource budget` shows what is in effect. Largest-site volumes and disk per tier are in [0008](../decisions/0008-storage-engine-and-budgets.md#installation-tiers-sizing-guidance), the choice in [0019](../decisions/0019-budget-presets.md).

Run `serve` under a supervisor that sends SIGTERM and waits at least 30 s before SIGKILL. A second `serve` on the same directory exits 4.

## Health

- `GET /health`: liveness, 200 while the process serves.
- `GET /health/ready`: 200 while collection stores events, 503 when it cannot. The body lists problems:

| Problem | What to do |
| --- | --- |
| `disk_full` | Free space on the data volume. Collection answers 503 and trackers lose events until then. |
| `storage_unavailable` | Commits failed in the last minute. Check the volume (read-only remount, I/O errors) and the log. |
| `shutting_down` | A stop is in progress. |
| `transfer_failing` | Events are stored but not reaching reports. Check the log for `transfer failed`; run `holocron diagnostics`. Do not delete files. |
| `analytics_stale` | Transfer works but is over a minute behind, often during a backup or a long correction. Watch `inbox.oldest_age_ms` in diagnostics. |
| `recorder_budget` | Replay storage is at or near its cap. Ordinary collection is unaffected. Raise `--recording-cap-mib` or wait for the 14-day removal. |

`holocron diagnostics --data-dir D` prints versions, file sizes, inbox depth and age, watermark, transfer failures, correction backlog, recording budget, collection counters, recent jobs and settings as counts. It holds no addresses, user agents, event data, identify IDs or credentials, so it can go into a bug report. It works with the server running or stopped.

`holocron query --url https://stats.example.com query.json` posts a JSON query from a file (or `-` for stdin) to a running server and prints the answer. It calls `analytics.report` unless you pass `--operation <name>`. The API key comes from `HOLOCRON_API_KEY`, never from a flag, and `--url` can come from `HOLOCRON_URL`. It does not open the data directory. A refusal prints `error: <error_code>: <message>` with one line per rejected field and exits 5 (1 when the server is unreachable); a missing key or a query that is not a JSON object under 1 MiB exits 2.

## Back up

```sh
holocron backup --data-dir /var/lib/holocron --to /mnt/backups/holocron
```

- Write backups to other storage than the data volume. A backup is about the size of the data directory; its temporary copy lives in the destination, not on the data volume.
- With the server running, analytics and transfer pause while DuckDB and the inbox are copied; collection and reports go on, and events wait in the inbox. Expect reports to lag by the pause (diagnostics shows it as `pause_ms`).
- With the server stopped, the command takes the directory itself.
- Each backup is a new directory `holocron-backup-<UTC time>-<id>` with `manifest.json`. The command verifies every file before it reports success. A backup interrupted half way leaves `.holocron-backup-*.partial`, which the next backup removes.
- Keep `secrets.key` in the backup private: it opens every stored second-factor secret. Files are written with mode 0600.
- The GeoIP download in `geoip/` is not backed up ([0018](../decisions/0018-geoip-fallback.md)). After a restore, download it again in the administration area, or keep `HOLOCRON_GEOIP_DB`.

## Restore

```sh
holocron restore --from /mnt/backups/holocron/holocron-backup-20261005T214501Z-1a2b3c4d --data-dir /var/lib/holocron-restored
holocron serve --data-dir /var/lib/holocron-restored
```

- The target must be missing or empty. Restore never overwrites a data directory.
- Restore verifies every checksum, checks that the inbox and analytics files belong together, opens everything once, checks that an install admin and the root secret exist, and applies the backed-up inbox before it marks the directory initialized. It prints `records_applied`.
- `backup_corrupt`, `backup_invalid` (exit 3): the backup is damaged. Use another one.
- `engines_mismatched` (exit 3): engine files from different backups or moments were mixed. Restore a whole backup directory.
- `data_newer_than_binary` (exit 3): install the release that took the backup or a newer one.
- `state: "upgrade_required"`: the backup is from an older release. Run `holocron upgrade` on the restored directory.
- An interrupted restore leaves `holocron.restore-in-progress`; `serve` refuses the directory (`restore_incomplete`). Run the same restore again; it starts over.

### Restore in setup mode

An empty data directory can restore from the wizard: Restore a backup lists `holocron-backup-*` in `HOLOCRON_BACKUP_DIR` (`/backups` in the compose files) with date, size and release, and takes any other host path. It runs `restore`, then `upgrade` when the backup is older (the backup it restored from is the pre-upgrade backup, no second one is taken), on one progress stream with the phases `restoring` and `upgrading`. When it finishes, setup spends the token and `serve` starts on the same port without a restart; the restored accounts sign in.

- Refusals use the codes above (`data_newer_than_binary`, `backup_corrupt`, `backup_invalid`, `directory_not_empty`) with a message in the wizard's words; a failed restore leaves setup mode as it was and the person picks again.
- A lock file from an earlier process does not make the directory non-empty.
- A directory with `holocron.restore-in-progress` starts in setup mode and the wizard preselects Restore. The step starts over. If the server is killed after the restore and before the upgrade ends, `serve` refuses with `upgrade_required` or `upgrade_interrupted`; run `holocron upgrade` as above.
- The token guards every restore request. The progress stream takes it in the `X-Holocron-Setup-Token` header.

Never copy single engine files into a data directory. `serve` refuses a mixed directory with `engines_mismatched`; restore a whole backup instead.

## Upgrade

1. Install the new binary. Stop the server.
2. `holocron upgrade --data-dir /var/lib/holocron --backup-to /mnt/backups/holocron`
3. Start the server.

`serve` never migrates. On data from an older release it exits 3 `upgrade_required`. The upgrade takes and verifies a backup first, then migrates the SQLite files, then DuckDB.

If a step fails, the command exits 1 `upgrade_failed` and prints both ways out:

- Fix the cause (disk space, permissions) and run `holocron upgrade --data-dir ...` again. It resumes from the last completed migration and uses the backup it already took.
- Or restore the pre-upgrade backup into a new directory with the printed `holocron restore` command and run the old release on it.

Until one of them succeeds, `serve` exits 3 `upgrade_interrupted`.

## Retention and erasure

- Retention runs hourly by itself: events after the site's retention by its local date (the installation default, `1y` unless changed, see [0016](../decisions/0016-configurable-retention.md)), raw replay after 14 days (saved replays excepted), heatmaps after 90 days. `holocron retention --data-dir D` runs it now. Reports never show data past retention, whether or not the job ran.
- Deleting or resetting a site in the dashboard queues its erasure at once. Records still in the inbox and requests in flight are fenced, so they never reach reports. The job survives restarts.
- Erase one consented visitor's history: `holocron erase-profile --data-dir D --site <site id> --distinct-id <id the site passed to identify>`. Events, identifies, sessions, consents and replays recorded under that profile go. The identify ID is not stored anywhere; the shell history may keep it, so clear it if that matters.
- Erased rows leave the database files over time: SQLite gives pages back, DuckDB reuses freed blocks without shrinking its file.

## Disk budget

Measured figures and the reference budget are in the [milestone 12 evidence](../evidence/m12-operations-and-recovery.md#disk-growth). Plan the data volume for the engine files, their WAL files, the inbox backlog (at most 1 GiB), query spill (at most 40 GiB in `spill/`) and the 4 GiB reserve. Plan backup storage separately at the size of the data directory per backup kept.

## Rehearsed

These procedures ran on 2026-10-05 against the release binary and through the integration tests in `apps/server/tests/operations.rs`: backup while serving, restore into a new directory with waiting inbox records and an admin login afterwards, a refused mixed restore, a failed upgrade resumed after the fault was removed, SIGTERM shutdown and restart, site deletion across a restart, profile erasure, retention, and diagnostics. Not rehearsed: a real full disk (ENOSPC) on a small volume, power loss, and a restore on a second machine.
