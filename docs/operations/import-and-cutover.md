# Import from Umami and cut over

This runbook moves an Umami installation to Holocron and keeps the way back open. It covers Umami 3.4.0 on PostgreSQL and Umami 2.20.2 on MySQL, the only two layouts Holocron imports. Rules and their reasons are in [decision 0013](../decisions/0013-historical-import.md). `tools/import-rehearsal/rehearse.sh` runs every step below against throwaway Umami deployments.

## What changes for your users

- Logins: a username that is an email address stays the login. Any other username becomes `<username>@<login domain>`, by default `@umami.invalid`. The import prints every login; send people theirs.
- Passwords keep working. Holocron replaces the stored Umami hash at each person's first login.
- 2FA keeps working when you pass Umami's `TWO_FACTOR_ENCRYPTION_KEY` to the import. Without it, people with 2FA enroll again at their first login. Recovery codes do not carry over; people create new ones under Account.
- API keys do not carry over. Create scoped keys in Holocron and update the scripts that use them.
- Public share links keep working at their old URLs for 366 days, then expire unless their owner extends them.
- The whole history is imported by default (`--retention forever`). With a shorter `--retention` (`1y`, `2y`, `3y`, `5y`, `10y`) older events stay behind; the inspection shows how many for each value. Imported sites keep that retention until an owner or admin changes it ([0016](../decisions/0016-configurable-retention.md)).
- Reports become saved explorations of their owner, visible only to that person. Board layouts, link and pixel shares, cohorts and some report settings do not carry over; the import's inventory lists each one with its reason.
- Install admins read analytics only in workspaces they belong to. Team members can no longer delete sites; admins and owners can.

## Before the day

1. Check the source is at the supported version. On the Umami host, `prisma migrate deploy` must have finished. A 2.x installation on PostgreSQL must first upgrade to Umami 3.4.0.
2. Create a database user that can only read:

   ```sql
   -- PostgreSQL
   CREATE ROLE holocron_import LOGIN PASSWORD '<password>';
   GRANT CONNECT ON DATABASE umami TO holocron_import;
   GRANT USAGE ON SCHEMA public TO holocron_import;
   GRANT SELECT ON ALL TABLES IN SCHEMA public TO holocron_import;
   ```

   ```sql
   -- MySQL
   CREATE USER 'holocron_import'@'%' IDENTIFIED BY '<password>';
   GRANT SELECT ON umami.* TO 'holocron_import'@'%';
   ```

   Credentials that could write get the warning `credentials_writable`, and the import proceeds through a read-only session. The connection has no TLS: run the import on the database host or through an SSH tunnel.

3. Inspect, with the source URL in the environment so it stays out of the shell history:

   ```sh
   export HOLOCRON_IMPORT_SOURCE='postgres://holocron_import:<password>@127.0.0.1:5432/umami'
   holocron import inspect --timezone Europe/Vienna
   ```

   `outcome: accept` means the source can be imported. A refusal names the failing migration, table or column and what to do; exit code 3. `preview.events.outside_retention` is how many events stay behind for each `--retention` value. `tolerated` lists MySQL columns whose type differs from Umami 2.20.2 within a family the import reads the same, such as `datetime(3)` for `timestamp`; `datetime` values are read as UTC. `preview.source_bytes` is an upper bound for the disk the import needs on top of Holocron's 4 GiB reserve.

4. Rehearse on a copy if you can: restore a backup of the Umami database elsewhere and run the import against it.

## Cutover

1. **Stop collection for the final snapshot.** Stop the Umami app (for example `docker compose stop umami`). Leave its database running and untouched. From now on tracker requests fail until step 5.
2. **Import into a new data directory.**

   ```sh
   holocron init --data-dir /srv/holocron --admin-email ops@example.org --admin-password-file admin.pw
   holocron import run --data-dir /srv/holocron --timezone Europe/Vienna \
     --two-factor-key-file umami-2fa.key > manifest.json
   ```

   `--timezone` is the timezone of every imported site; Umami has none. `--retention` (default `forever`) is the event retention every imported site, link and pixel keeps. `--login-domain` sets the domain of username logins. The import prints the reconciliation manifest when it is done.

   If it stops (crash, Ctrl-C, full disk), run the same command again. It resumes at the last checkpoint and ends with the same result. Change nothing in the source in between: a changed source is refused with `source_changed`. `holocron import status --data-dir /srv/holocron` shows the steps at any time. `holocron serve` and the operator commands of the [runbook](runbook.md) refuse until the import is complete.

3. **Reconcile.** Read the manifest:
   - `status` is `reconciled` when every count, range, revenue total and sampled event matches. Otherwise `differences` lists each mismatch.
   - `sites[].events` compares events per kind and time range; `outside_retention` counts what the retention cutoff left behind.
   - `sites[].revenue` compares revenue per currency with Umami's `revenue` table. A MySQL source whose manual revenue data migration never ran has an empty `revenue` table and shows a difference here.
   - `configuration` must list no `unaccounted` rows.
   - `inventory` counts what was not imported, by reason. `logins` lists every account and what happened to its password and 2FA.
4. **Verify the destination.** Start Holocron (`holocron serve --data-dir /srv/holocron`). Sign in with an imported account that owns sites or belongs to a team, check a site's overview, and open one old share link. The init admin manages accounts and teams but reads no analytics until it is a member of a workspace ([0007](../decisions/0007-access-and-account-security.md)).
5. **Redirect collection.** Point the tracker script and `/api/send` at Holocron: change the proxy or DNS entry of the Umami host, or the `src` of the script tag. Website IDs are unchanged, so the snippets stay as they are. Send one pageview and check it appears in Realtime.
6. **Issue new credentials.** Send the login list, create API keys for scripts, and ask people with 2FA without a carried-over secret to enroll.

## Rollback

The Umami database was only read, so the old system is intact.

1. Stop Holocron.
2. Point the proxy, DNS entry or script tags back at Umami.
3. Start the Umami app again. It collects into the same database it stopped with.

Events Holocron collected between cutover and rollback stay in Holocron only. To try again later, start over from step 1 of the cutover with a new data directory: an import always starts from an empty destination.

Keep the Umami database and a backup of it until you no longer need the way back.

## Codes

| Code | Exit | Meaning |
| --- | --- | --- |
| `not_umami`, `unsupported_engine`, `unsupported_pairing`, `migration_failed`, `migration_missing`, `migration_unknown`, `migration_modified`, `table_missing`, `column_missing`, `column_changed` | 3 | The source is not one of the two supported layouts; the message says what to fix |
| `destination_not_new` | 3 | The data directory already has sites or analytics |
| `source_changed` | 3 | A resume found another source than the run started with |
| `import_in_progress` | 3 | An operator command found an unfinished import; resume it. `serve` starts in setup mode instead, where the wizard resumes it |
| `usage` | 2 | A bad argument, or a resume with other `--timezone`, `--login-domain` or `--chunk-rows` than the start |
| `source_unavailable` | 4 | The source database did not answer |
| `storage_unavailable` | 4 | The data directory or its disk reserve is the problem |

Warnings don't stop the import. `holocron import inspect` lists them under `warnings`, and both inspect and run log them:

| Code | Meaning |
| --- | --- |
| `source_unencrypted` | The host is off a private network and the URL doesn't require TLS |
| `credentials_writable` | The source user can write; a read-only user is safer |
