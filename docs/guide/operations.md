# Operations

How to back up, restore and upgrade Holocron, what it cleans up by itself, and how to check its health. The examples use a data directory at `/var/lib/holocron`. In Docker the data directory is `/data`, so you can leave out `--data-dir` there.

The [operator runbook](../operations/runbook.md) is the full reference, with every refusal code and what was rehearsed. [Decision 0014](../decisions/0014-operations-and-recovery.md) explains the design.

## Data directory

| File | Holds |
| --- | --- |
| `control.sqlite` | Accounts, teams, sites, settings, saved explorations, boards, jobs and import state |
| `inbox.sqlite` | Accepted events waiting to move into analytics |
| `analytics.duckdb` | Events, properties, sessions, rollups and heatmaps |
| `recordings.sqlite` | Compressed replay parts |
| `spill/` | DuckDB's temporary files for large queries, at most 40 GiB |
| `secrets.key` | The key for second-factor secrets. See [Secrets key](configuration.md#secrets-key) |
| `geoip/` | The DB-IP Lite City file downloaded in the dashboard, if any. See [GeoIP database](configuration.md#geoip-database) |
| `holocron.lock` | Held by the process that uses the directory |
| `holocron.initialized` | Written when `init` or `restore` finished |

`holocron.init-in-progress`, `holocron.restore-in-progress` and `holocron.upgrade-in-progress` mark an unfinished command. `serve` refuses a directory that has one. Run the same command again to finish it.

Never copy single files into or out of a data directory. The inbox and the analytics file carry a shared pairing, and `serve` refuses a mixed directory with `engines_mismatched`. Use `backup` and `restore`.

## Disk

Holocron keeps 4 GiB of the data volume free, set with `HOLOCRON_DISK_RESERVE_MIB`. Below the reserve, collection answers 503 and the tracker loses those events, so the server can still stop cleanly.

Plan the volume for the database files and their write-ahead logs, an inbox backlog of up to 1 GiB, query spill of up to 40 GiB, replays up to their cap (25 GiB by default) and the reserve. As a rough measure, a retained event with its properties and sessions takes about 105 bytes without replays. DuckDB reuses freed space but never shrinks its file.

Server status in the administration area charts disk use and forecasts it. `holocron diagnostics` prints file sizes and free space.

## Health endpoints

- `GET /health` answers `200` with the version while the process serves: `{"status":"ok","version":"..."}`. The image's healthcheck calls it every 30 seconds.
- `GET /health/ready` answers `200` while collection stores events and `503` when it can't. The body names the problems:

| Problem | Collection | What to do |
| --- | --- | --- |
| `disk_full` | refuses | Free space on the data volume |
| `storage_unavailable` | refuses | Commits failed in the last minute. Check the volume and the log |
| `shutting_down` | refuses | A stop is in progress |
| `transfer_failing` | stores | Events are stored but don't reach reports. Check the log for `transfer failed` and run `diagnostics`. Don't delete files |
| `analytics_stale` | stores | Reports are over a minute behind, often during a backup |
| `recorder_budget` | stores | Replay storage is at or near its cap. Raise `HOLOCRON_RECORDING_CAP_MIB` or wait for the 14-day removal |

`/health/ready` is public, like Umami's heartbeat, so it reveals `disk_full` to anyone who can reach it. Block it at the proxy if that matters to you.

## Logs

The server logs to stderr, so `docker logs holocron` shows them. Set the level with `HOLOCRON_LOG`, see [Logs](configuration.md#logs). At start it logs the address it listens on, and warns when `HOLOCRON_PUBLIC_URL` is not set or the dashboard files are missing.

## Backup

```sh
holocron backup --data-dir /var/lib/holocron --to /mnt/backups/holocron
```

With Docker, start the container with a backup directory mounted, for example `-v /mnt/backups/holocron:/backups`, and run the command in it:

```sh
docker exec holocron holocron backup --to /backups
```

- Each backup is a new directory `holocron-backup-<UTC time>-<id>` inside `--to`, with the four database files, `secrets.key` and a `manifest.json` listing schema versions and a BLAKE3 hash of every file.
- `geoip/` is left out, because the file can be downloaded again. After a restore, download it again in the administration area, or keep `HOLOCRON_GEOIP_DB`.
- You can back up while `serve` runs. Collection and reports go on; moving events into analytics pauses for the copy and catches up after. The result reports the pause as `pause_ms`.
- The command hashes the copy again before it reports success. An interrupted backup leaves a `.holocron-backup-*.partial` directory, which the next backup removes.
- Write backups to other storage than the data volume. A backup is about the size of the data directory, and its temporary copy lives in the destination.
- Files are written with mode 0600. The backup holds `secrets.key`, which opens every stored second-factor secret, so keep backups private.

## Restore

Restore into a new, empty directory, then start the server on it:

```sh
holocron restore --from /mnt/backups/holocron/holocron-backup-20261019T214501Z-1a2b3c4d \
  --data-dir /var/lib/holocron-restored
holocron serve --data-dir /var/lib/holocron-restored
```

- Restore never overwrites a data directory.
- It checks every hash, checks that the inbox and the analytics file belong together, opens everything once, and moves waiting events into analytics before it marks the directory ready. It prints `records_applied`.
- A backup of an older release restores as `upgrade_required`. Run `holocron upgrade` on the restored directory before `serve`.
- An interrupted restore leaves `holocron.restore-in-progress`. Run the same restore again.

| Refusal (exit 3) | What to do |
| --- | --- |
| `backup_corrupt`, `backup_invalid` | The backup is damaged. Use another one |
| `engines_mismatched` | Files from different backups were mixed. Restore a whole backup directory |
| `data_newer_than_binary` | Install the release that took the backup, or a newer one |
| `directory_not_empty`, `already_initialized` | Restore into a new, empty directory |

## Upgrade

`serve` never migrates. On data from an older release it exits 3 with `upgrade_required`.

1. Pull the new image or install the new release. Stop the server.
2. Back up and migrate:

   ```sh
   holocron upgrade --data-dir /var/lib/holocron --backup-to /mnt/backups/holocron
   ```

   With Docker, run the new image on the volume:

   ```sh
   docker stop holocron
   docker run --rm -v holocron-data:/data -v /mnt/backups/holocron:/backups \
     ghcr.io/coderscantina/holocron:26.10.19 upgrade --backup-to /backups
   ```

   With docker compose: `docker compose pull`, `docker compose stop`, `docker compose run --rm -v /mnt/backups/holocron:/backups holocron upgrade --backup-to /backups`, `docker compose up -d`.

3. Start the server with the new release.

`upgrade` takes and checks a backup first, then migrates the SQLite files, then DuckDB. On current data it changes nothing.

If a step fails, the command exits 1 with `upgrade_failed` and prints both ways out: fix the cause and run the same command again, which resumes with the backup it already took, or restore that backup into a new directory and run the old release on it. Until one of them succeeds, `serve` exits 3 with `upgrade_interrupted`.

## Retention

`serve` runs retention every hour. Reports never show data past retention, whether or not the job has run.

| What | Kept |
| --- | --- |
| Events, properties, sessions, performance, revenue, identify data | 1 year by default. Set per installation and per site to 1, 2, 3, 5 or 10 years, or forever |
| Raw replays | 14 days. Saved replays stay |
| Heatmaps | 90 days |
| Daily visitor keys | Deleted about 25 hours after their day ends |
| Sign-in sessions | 30 days, or 7 days without use |

Install admins change the default under the installation's data retention setting. Owners and admins set a site's own value in its settings. Shortening shows how many events it deletes and asks for your password. Lengthening can't bring back data already removed.

Run it now:

```sh
holocron retention --data-dir /var/lib/holocron
```

Deleting or resetting a site in the dashboard queues its erasure at once, including events still waiting in the inbox. The job survives restarts.

## Erase a visitor profile

To erase one consented visitor's history, name the site and the ID the site passed to `identify`:

```sh
holocron erase-profile --data-dir /var/lib/holocron \
  --site 94db1cb1-74f4-4a40-ad6c-962362670409 --distinct-id user-123
```

Events, identify data, sessions, consents and replays recorded under that profile go. Holocron never stores the ID itself, but your shell history may, so clear it if that matters. Anonymous daily visitors can't be erased one by one. Their key is deleted about a day later, and after that nobody can tell which pseudonym was whose.

## Diagnostics

```sh
holocron diagnostics --data-dir /var/lib/holocron
```

It prints JSON: versions, file sizes, free disk and the reserve, inbox depth and age, the analytics watermark, transfer failures, correction backlog, the replay budget, collection counters, recent jobs and settings as counts. It holds no addresses, user agents, event data, identify IDs or credentials, so it can go into a bug report. It works with the server running or stopped.

## Query from the command line

```sh
export HOLOCRON_API_KEY=...
holocron query --url https://stats.example.com report.json
```

It posts the JSON object in the file (or `-` for stdin) to a running server and prints the answer. It calls `analytics.report`; pass `--operation sites.list` or another name to call a different query. The key must be an API key that holds the operation, and it comes from the environment so it stays out of your shell history. `--url` can also come from `HOLOCRON_URL`. The command never opens the data directory, so it can run on another machine. A refused query prints `error: <error_code>: <message>`, one line per rejected field, and exits 3.

## Commands beside a running server

While `serve` runs, `backup`, `retention`, `erase-profile` and `diagnostics` hand their work to it through the control database and wait for the result. Without a server they take the data directory themselves. `restore`, `upgrade` and `import run` need the server stopped. A second process on the same directory exits 4.

## Exit codes

| Code | Meaning |
| --- | --- |
| 0 | Success |
| 1 | Internal error, or `upgrade_failed` |
| 2 | Wrong usage, such as a missing flag or a bad value |
| 3 | Refused by the state of the data directory, such as `upgrade_required` |
| 4 | Storage unavailable: path not writable, disk full, or another process holds the directory |

Errors print as `error: <code>: <message>` on stderr.
