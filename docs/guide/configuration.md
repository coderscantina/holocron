# Configuration

Holocron is configured with command-line flags. Most flags have a `HOLOCRON_*` environment variable, which is the easier way in Docker. A flag wins over its variable. `holocron <command> --help` prints the same list.

Settings you change at runtime live in the dashboard: sites, their timezone, retention, recording and identity policy, teams, the security policy, the installation's default retention and the location database.

## Commands

| Command | What it does |
| --- | --- |
| `holocron init` | Creates a data directory with the first install admin, the secrets key and every database file |
| `holocron serve` | Runs the server: dashboard, API, tracker scripts and collection. The image's default |
| `holocron import inspect` | Reads an Umami database without changing it and reports whether it can be imported. See [Migrate from Umami](umami-migration.md) |
| `holocron import run` | Imports an Umami database into a new data directory, or resumes an interrupted import |
| `holocron import status` | Prints the import state and manifest of a data directory |
| `holocron backup` | Writes a verified backup directory |
| `holocron restore` | Restores a backup into a new, empty data directory |
| `holocron upgrade` | Backs up a stopped data directory, then migrates it to this release |
| `holocron retention` | Removes data past retention now instead of at the next hourly run |
| `holocron erase-profile` | Erases one consented visitor profile's history |
| `holocron diagnostics` | Prints the operational state as JSON |

The last six are covered in [Operations](operations.md). The import and operator commands print their result as JSON on stdout.

## Environment variables

| Variable | Flag | Used by | Default | Meaning |
| --- | --- | --- | --- | --- |
| `HOLOCRON_DATA_DIR` | `--data-dir` | all but `import inspect` | `/data` in the image; `data` for `serve` outside it | The data directory. Every command but `serve` requires it outside the image |
| `HOLOCRON_BIND` | `--bind` | `serve` | `0.0.0.0:8080` in the image, `127.0.0.1:8080` otherwise | Address and port to listen on |
| `HOLOCRON_PUBLIC_URL` | `--public-url` | `serve` | none | The URL the dashboard is reached at, see [Public URL](#public-url) |
| `HOLOCRON_INSTANCE_NAME` | `--instance-name` | `serve` | `Holocron` | Name shown in the dashboard |
| `HOLOCRON_WEB_DIR` | `--web-dir` | `serve` | `web` next to the executable | The built dashboard. The image sets it up for you |
| `HOLOCRON_BUDGET` | `--budget` | `serve` | `standard` | Host size preset: `small`, `standard` or `large`. Sets engine threads and memory, worker threads and report workers together, see [Host size](../operations/runbook.md#host-size) |
| `HOLOCRON_WORKER_THREADS` | `--worker-threads` | `serve` | from the budget | Worker threads of the server's runtime. Overrides the budget's value |
| `HOLOCRON_DISK_RESERVE_MIB` | `--disk-reserve-mib` | `serve`, `import run` | `4096` | Free space the data volume keeps, in MiB. Below it collection answers 503 and an import stops |
| `HOLOCRON_BACKUP_DIR` | `--backup-dir` | `serve` | `/backups` | The directory whose `holocron-backup-*` backups the setup wizard lists for a restore. The compose files mount the backup storage there. A backup elsewhere on the host is entered by path |
| `HOLOCRON_RECORDING_CAP_MIB` | `--recording-cap-mib` | `serve` | `25600` | Compressed replay storage, in MiB. Saved replays may use a fifth of it |
| `HOLOCRON_COLLECT_TRUSTED_PROXIES` | `--collect-trusted-proxies` | `serve` | none | Comma-separated IPs or CIDRs of proxies whose forwarding and location headers are trusted |
| `HOLOCRON_COLLECT_CLIENT_IP_HEADER` | `--collect-client-ip-header` | `serve` | `x-forwarded-for` | Header a trusted proxy puts the visitor's address in |
| `HOLOCRON_COLLECT_IGNORE_IPS` | `--collect-ignore-ips` | `serve` | none | Comma-separated IPs or CIDRs whose events are not stored. Umami's `IGNORE_IP` |
| `HOLOCRON_TRACKER_SCRIPT_NAME` | `--tracker-script-names` | `serve` | none | Comma-separated extra paths the tracker is served under. Umami's `TRACKER_SCRIPT_NAME` |
| `HOLOCRON_COLLECT_API_ENDPOINT` | `--collect-endpoint` | `serve` | none | Extra path for `/api/send`, such as `/api/collect`. The served tracker posts there. Umami's `COLLECT_API_ENDPOINT` |
| `HOLOCRON_GEOIP_DB` | `--geoip-db` | `serve` | none | A MaxMind DB City file that locates visitors no CDN header located, see [GeoIP database](#geoip-database). Umami's `GEOLITE_DB_PATH` |
| `HOLOCRON_IMPORT_SOURCE` | `--source` | `import inspect`, `import run` | none | `postgres://` or `mysql://` URL of a read-only Umami database user |
| `HOLOCRON_LOG` | none | all | `info,sqlx=warn` | Log filter, see [Logs](#logs) |

`HOLOCRON_FROZEN_CLOCK` (`--frozen-clock`) exists for browser tests on seeded data. Release builds refuse it.

## Flags without a variable

| Command | Flag | Meaning |
| --- | --- | --- |
| `init` | `--admin-email` | Email of the first install admin. Required |
| `init` | `--admin-password-file` | File holding the admin password, at least 12 characters. Without it a 24-character password is generated and printed once on stdout |
| `import inspect` | `--timezone` | Timezone the retention preview counts days in. Default `UTC` |
| `import run` | `--timezone` | IANA timezone of every imported site, since Umami keeps none. Default `UTC` |
| `import run` | `--retention` | Event retention of every imported site: `1y`, `2y`, `3y`, `5y`, `10y` or `forever`. Default `forever` |
| `import run` | `--login-domain` | Domain for Umami usernames that are not email addresses. Default `umami.invalid` |
| `import run` | `--chunk-rows` | Source rows per chunk, 1 to 100000. Default 5000. Lower it to use less memory |
| `import run` | `--two-factor-key-file` | File holding Umami's `TWO_FACTOR_ENCRYPTION_KEY`, 64 hex characters |
| `backup` | `--to` | Directory the backup is written into, on other storage than the data directory. Required |
| `restore` | `--from` | A backup directory written by `backup`. Required |
| `upgrade` | `--backup-to` | Where the pre-upgrade backup goes. Required unless an interrupted upgrade resumes |
| `erase-profile` | `--site` | The site ID the profile belongs to. Required |
| `erase-profile` | `--distinct-id` | The ID the site passed to `identify`. Required |

## Public URL

Set `HOLOCRON_PUBLIC_URL` whenever Holocron runs behind a reverse proxy. It does two things:

- Commands from the dashboard must come from this origin. Without it they must come from the origin named by the request's Host header.
- An `https://` URL marks the session cookie `Secure`.

Only the scheme and host count, so `https://analytics.example.com/` and `https://analytics.example.com` are the same. It must start with `http://` or `https://`.

## Proxies and visitor addresses

A visitor's address is a transient input: Holocron uses it to build the day's pseudonym and to apply `HOLOCRON_COLLECT_IGNORE_IPS`, and never stores or logs it.

- When the connection comes from an address outside `HOLOCRON_COLLECT_TRUSTED_PROXIES`, that address is the visitor's, and all forwarding and location headers are ignored.
- When it comes from a trusted proxy, Holocron reads `HOLOCRON_COLLECT_CLIENT_IP_HEADER`. For `x-forwarded-for` and `forwarded` it walks the list from the right and takes the first address that is not a trusted proxy.

This is stricter than Umami, which takes the first header it finds from any client. A visitor can't pick their own address and pseudonym in Holocron.

Behind Docker's bridge network the proxy shows up as a bridge gateway, which is why [deploy/docker-compose.yml](../../deploy/docker-compose.yml) trusts `172.16.0.0/12`. Behind a CDN, list the addresses of the hop that connects to Holocron and set the header the CDN uses, for example `cloudfront-viewer-address` for CloudFront.

## Location

Country, region and city come from two sources, in this order:

1. CDN headers sent by a trusted proxy, in the table below.
2. A [GeoIP database](#geoip-database), for every visitor without a trusted country header. It looks up the address Holocron picked under [Proxies and visitor addresses](#proxies-and-visitor-addresses) and never stores it. Requests with a collection key get no location.

| CDN | Country, region and city | Coordinates for the Realtime globe |
| --- | --- | --- |
| Cloudflare | `cf-ipcountry`, `cf-region-code`, `cf-ipcity` | `cf-iplatitude`, `cf-iplongitude`. Turn on the "Add visitor location headers" managed transform |
| Vercel | `x-vercel-ip-country`, `x-vercel-ip-country-region`, `x-vercel-ip-city` | `x-vercel-ip-latitude`, `x-vercel-ip-longitude`, sent by default |
| CloudFront | `cloudfront-viewer-country`, `-country-region`, `-city` | `cloudfront-viewer-latitude`, `-longitude`. Forward them with an origin request policy |
| EdgeOne | `eo-ipcountry`, `eo-region-code`, `eo-ipcity` | none, pins sit at the country's center |

Coordinates from either source are rounded to a tenth of a degree, about 11 km, before they are stored. Without a CDN or a database, reports have no location and the globe has no pins.

### GeoIP database

There are two ways to add one:

- **Download DB-IP Lite City in the dashboard.** As an install admin, open Administration, Security policy, Visitor location and press Download. Holocron fetches the current month's file from db-ip.com, about 60 MB and 130 MB unpacked, checks it and puts it into `geoip/` in the data directory. Lookups use it at once, without a restart. The section shows the installed month and when DB-IP has published a newer one. Press Update to get it: nothing updates by itself. The download needs 512 MiB free above the disk reserve.
- **Bring your own file** with `HOLOCRON_GEOIP_DB`: any MaxMind DB City database, such as GeoLite2 City (a free MaxMind account and licence key, kept current with `geoipupdate`), GeoIP2 City or DB-IP Lite City. Holocron reads it at start, so restart after replacing it. While the variable is set, the dashboard shows the file as managed by the server and offers no download. A missing or broken file logs a warning, and visitors without a CDN location get none.

DB-IP Lite City has no region codes, so region reports stay empty with it. Country, city and coordinates work.

If your proxy is not listed in `HOLOCRON_COLLECT_TRUSTED_PROXIES`, every visitor gets the proxy's location. A proxy on a private address, as in Docker, has none.

DB-IP Lite City is licensed under [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/). Its terms ask a web application to link to DB-IP on pages that show its results. While a DB-IP database is in use, the dashboard sidebar and shared pages show "IP geolocation by DB-IP" with a link to db-ip.com. GeoLite2 comes with MaxMind's own licence, which you accept with your MaxMind account.

## Secrets key

`secrets.key` in the data directory is a 32-byte key that encrypts every stored second-factor secret. `init` creates it with mode 0600, and `serve` creates it if it is missing. It never goes into the database.

Backups carry the key, so keep backups private. If the key is lost, time-based codes stop working for everyone. Recovery codes keep working, and an install admin can reset a person's second factor.

## Logs

Holocron logs to stderr. `HOLOCRON_LOG` sets the filter with the usual `tracing` syntax: a level such as `debug`, or a list such as `info,holocron_server=debug`. The default is `info,sqlx=warn`.

```sh
docker run ... -e HOLOCRON_LOG=debug ghcr.io/coderscantina/holocron:latest
```

Holocron does not log visitor addresses, user agents or identify IDs. The generated admin password from `init` goes to stdout only.
