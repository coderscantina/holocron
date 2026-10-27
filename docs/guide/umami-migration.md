# Migrate from Umami

Holocron moves an Umami installation into a new data directory: accounts, teams, sites, history, replays, shares and saved reports. Website IDs stay the same, so your tracker snippets keep working once they point at Holocron. The source database is only read, so the way back stays open.

Two layouts are supported, and nothing else:

- Umami 3.4.0 on PostgreSQL
- Umami 2.20.2 on MySQL

A 2.x installation on PostgreSQL must upgrade to Umami 3.4.0 first. Other versions are refused with a message that says what to do. Every refusal ends with "Holocron did not change the source."

The setup wizard does the import in the browser. `holocron import` on the command line does the same import and stays the alternative. This page is the short version. The [import and cutover runbook](../operations/import-and-cutover.md) has every step and code, and decisions [0013](../decisions/0013-historical-import.md) and [0021](../decisions/0021-web-setup-and-onboarding.md) the rules behind them.

## Before you start

1. On the Umami host, make sure `prisma migrate deploy` has finished.
2. Create a database user that can only read. Where you can't, an existing user works: Holocron warns with `credentials_writable` and imports anyway, through a session it makes read only.

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

3. Decide how Holocron reaches the database. A source on a private network needs nothing more: loopback, a private or link-local address, a socket, or a name without a dot such as a Docker service. Anything else should be read over TLS. A URL that doesn't require it still works, but the inspect report and the wizard warn with `source_unencrypted`, because the credentials and the data may cross the network in plain text. To require TLS:

   | Engine | Add to the URL | Certificate |
   | --- | --- | --- |
   | PostgreSQL | `?sslmode=verify-full` | Checked against the public roots |
   | PostgreSQL | `?sslmode=verify-full&sslrootcert=/path/ca.pem` | Checked against your CA, such as the Amazon RDS bundle |
   | PostgreSQL | `?sslmode=require` | Encrypted, not checked |
   | MySQL | `?ssl-mode=VERIFY_IDENTITY`, with `&ssl-ca=/path/ca.pem` for a private CA | Checked |
   | MySQL | `?ssl-mode=REQUIRED` | Encrypted, not checked |

   Host names are judged as written, not resolved, so `host.docker.internal` or an internal DNS name gets the warning too. The CA file must be readable by the Holocron process, so mount it into the container.
4. If people use two-factor sign-in, have Umami's `TWO_FACTOR_ENCRYPTION_KEY` at hand: 64 hex characters. With it, authenticator apps keep working.
5. Stop the Umami app so no event arrives after the snapshot. Leave its database running. Trackers fail from now until you point them at Holocron.

## Import in the setup wizard

Start Holocron on an empty data volume and open it in the browser. The [install guide](install.md) shows how.

1. Enter the setup token from the server log and look at the server check.
2. Choose **Import from Umami** and create your admin. Use an email that no Umami account uses as its login; an Umami account with the same login is skipped and listed as `login_conflict`. Holocron stays in setup mode from here: collection answers 503 and the dashboard waits until the import is done.
3. Paste the database URL and inspect. You see the source version, the rows per table, the oldest and newest event and the size of the source, which is an upper bound for the disk the import needs on top of the 4 GiB reserve. A refusal says what failed and how to fix it. A warning, such as a user that can write, says the same and lets you continue.
4. Choose the options: the timezone of every imported site (your browser's by default), how long to keep events, with how many older events each choice leaves behind, and the 2FA key. Login domain and rows per chunk sit under Advanced.
5. Start the import. Each step shows its rows against the source's count and an estimate of the time left. You can close the page; opening it again picks up the progress.
6. Read the report: reconciled or the differences, every login with what happened to its password and 2FA, and what was not imported and why. Download the manifest and keep it.
7. **Start Holocron** ends setup. The same server starts collecting and serving the dashboard without a restart. The wizard then shows the tracker cutover and turns green when the first hit arrives.

The database URL and the 2FA key stay in the server's memory only and are never written to disk. If the server restarts during the import, it comes back in setup mode: sign in with your admin, paste the same URL and resume. The import continues at its last checkpoint and ends with the same result. A different or changed source is refused with `source_changed`.

**Discard import and start over**, on the report, removes everything imported and keeps your admin and the secret key. Type the instance name to confirm it.

**Continue without import**, below the source, options and report steps, ends setup as a fresh start instead: it discards an import first, with the same confirmation, then the wizard adds your first site.

## Import with the CLI

### Inspect: the dry run

Keep the source URL in the environment so it stays out of your shell history:

```sh
export HOLOCRON_IMPORT_SOURCE='postgres://holocron_import:<password>@127.0.0.1:5432/umami'
holocron import inspect --timezone Europe/Vienna
```

With Docker, run the image on the network your Umami database is on:

```sh
docker run --rm --network <umami network> -e HOLOCRON_IMPORT_SOURCE \
  ghcr.io/coderscantina/holocron:latest import inspect --timezone Europe/Vienna
```

Inspect writes nothing, not even to a data directory. Read its report:

- `outcome: accept` means the source can be imported. A refusal names the failing migration, table or column and exits 3.
- `preview` counts rows per table, the oldest and newest event, and `preview.events.outside_retention`: how many events each `--retention` value would leave behind.
- `preview.source_bytes` is an upper bound for the disk the import needs, on top of the 4 GiB reserve.
- `tolerated` lists MySQL columns whose type differs from Umami 2.20.2 in a way the import reads the same, such as `datetime(3)` for `timestamp`.

If you can, rehearse on a copy: restore a backup of the Umami database elsewhere and run the whole import against it.

### Run the import

Create a new data directory and import into it:

```sh
holocron init --data-dir /srv/holocron --admin-email ops@example.org
holocron import run --data-dir /srv/holocron --timezone Europe/Vienna \
  --two-factor-key-file umami-2fa.key > manifest.json
```

With docker compose, mount the key file and pass the source:

```sh
docker compose run --rm holocron init --admin-email ops@example.org
docker compose run --rm -e HOLOCRON_IMPORT_SOURCE -v ./umami-2fa.key:/umami-2fa.key:ro \
  holocron import run --timezone Europe/Vienna --two-factor-key-file /umami-2fa.key > manifest.json
```

The compose service must reach the Umami database. Add the database's network to the service, or use `docker run --network` as for inspect.

| Option | Meaning |
| --- | --- |
| `--timezone` | The timezone of every imported site. Umami keeps none. Default `UTC` |
| `--retention` | Event retention of every imported site: `1y`, `2y`, `3y`, `5y`, `10y` or `forever`. Older events are not imported. Default `forever` |
| `--login-domain` | Umami usernames that are not email addresses sign in as `<username>@<domain>`. Default `umami.invalid` |
| `--two-factor-key-file` | Umami's `TWO_FACTOR_ENCRYPTION_KEY`. Without it, people with 2FA enroll again at their first sign-in |
| `--chunk-rows` | Source rows per chunk, default 5000. Lower it to use less memory |

The import runs in steps, each with a checkpoint: accounts and configuration, events, identify data, heatmaps, session links, replays, then session correction and reconciliation. If it stops (Ctrl-C, a crash, a full disk), run the same command again. It resumes at the last checkpoint and ends with the same result. A changed source is refused with `source_changed`, and different options with a usage error.

`holocron import status --data-dir /srv/holocron` shows the steps at any time. The operator commands refuse to start (`import_in_progress`) until the import is complete. `serve` on an unfinished import starts in setup mode, where the wizard resumes it.

## Reconcile and verify

The import ends with a manifest, shown by the wizard and printed by the CLI:

- `status` is `reconciled` when every count, time range, revenue total and sampled event matches the source. Otherwise `differences` lists each mismatch.
- `inventory` counts what was not imported, by reason.
- `logins` lists every account and what happened to its password and 2FA.

Sign in with an imported account that owns sites. Check a site's overview and open one old share link. The admin you created manages accounts and teams but reads no analytics until it joins a workspace. After the import, rename an imported account's email under Account, for example the imported admin to your real address. An address your admin already uses can't be taken.

## Run both side by side first

You can let Holocron collect next to Umami before you switch, without touching the sites. Copy each tracker hit to Holocron at the proxy in front of Umami. Behind CloudFront, [deploy/cloudfront-mirror](../../deploy/cloudfront-mirror/README.md) does that with a Lambda@Edge function. When you cut over, import again into a new data directory, so the history is exact and reconciled.

Before you switch Umami off, [tools/shadow-compare](../../tools/shadow-compare/README.md) checks per site and day that Holocron saw the same traffic: pageviews within 1%, visitors within 5%, and the same top pages and countries. It lists days with no traffic on a side as possible outages. Holocron counts visitor-days and groups sessions by its own rules, so visitors never match Umami's exactly.

## Cut over the tracker

Point the tracker at Holocron. Either move the Umami host name to Holocron at your proxy or DNS, or change the `src` of the script tags:

```html
<!-- before -->
<script defer src="https://umami.example.com/script.js" data-website-id="..."></script>
<!-- after -->
<script defer src="https://analytics.example.com/script.js" data-website-id="..."></script>
```

Website IDs don't change, so `data-website-id` stays as it is. If you served the tracker under another name with `TRACKER_SCRIPT_NAME`, or moved `/api/send` with `COLLECT_API_ENDPOINT`, set [`HOLOCRON_TRACKER_SCRIPT_NAME` and `HOLOCRON_COLLECT_API_ENDPOINT`](configuration.md#environment-variables) to the same values.

Send one pageview and check that it shows up in Realtime. Then send people their logins, and create new API keys for scripts.

## What comes over

| Umami | Holocron |
| --- | --- |
| Websites | Sites with the same IDs, in the same team or a personal workspace |
| Events, custom events, properties, performance, revenue | The same events, with typed properties. Revenue comes from `data.revenue` and `data.currency` |
| Session data | Identify records |
| Replays and heatmaps | Replays from the last 14 days and every saved replay. Heatmaps from the last 90 days |
| Users | Accounts with the same IDs. Passwords keep working and are rehashed at the first sign-in |
| Umami admins | Install admins |
| Team roles | Owner, Admin, Member and Viewer |
| 2FA | Kept with `--two-factor-key-file`. Installation, team and user requirements carry over |
| Segments | Segment filters |
| Funnel, journey, retention, goal and attribution reports | Saved explorations of the report's owner, over the last 30 days |
| Boards and annotations | Boards keep their name, annotations their time and text |
| Links and pixels | Their slug and URL keep answering at `/q/<slug>` and `/p/<slug>`, in the owner's team or personal workspace. Manage them under Links and pixels there |
| Public website and board shares | Restricted shares that keep working at their old URLs for 366 days |

## What doesn't, or changes

- **Visitors.** Umami history has no input for Holocron's daily pseudonym, so imported visitors are Umami session IDs per day. They never become persistent profiles, so retention, cohorts and identified people exclude imported history. Reports that reach imported data say "Includes Umami history through <date>".
- **Sessions** are recomputed from event times with Holocron's inactivity rule.
- **Location.** Umami looks addresses up in a bundled GeoLite2 City database. Holocron reads CDN headers first, then a GeoIP database you add: download DB-IP Lite City in the dashboard, or set `HOLOCRON_GEOIP_DB` to the file `GEOLITE_DB_PATH` named. Without either, new events have no location. Imported events keep the location Umami stored. See [Location](configuration.md#location).
- **Logins.** Usernames that are not email addresses become `<username>@umami.invalid`, or your `--login-domain`. Recovery codes don't carry over.
- **API keys and sign-in sessions** are not imported. Create scoped keys in Holocron.
- **Board layouts, cohorts, link and pixel shares** and some report settings are listed in the inventory with a reason.
- **Saved reports** become private to their owner. In Umami everyone who could see the site saw them.
- **Members can no longer delete sites.** Admins and owners can. The same goes for links and pixels, and deleting one erases its hits.
- **Deleted links and pixels** come over as deleted and keep their slug reserved, so a new link can't take it.
- **Install admins** read analytics only in workspaces they belong to.
- **Short share slugs** that someone chose by hand are flagged as `share_slug_guessable` in the inventory.

Imported shares expire 366 days after the import unless their owner extends them.

## Rollback

The Umami database was only read, so the old system is intact.

1. Stop Holocron.
2. Point the proxy, DNS entry or script tags back at Umami.
3. Start the Umami app. It collects into the database it stopped with.

Events Holocron collected in between stay in Holocron only. To try again, start over with a new data directory: an import always starts from an empty one. Keep the Umami database and a backup of it until you no longer need the way back.
