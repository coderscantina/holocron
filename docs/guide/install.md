# Install

Holocron ships as one Docker image, `ghcr.io/coderscantina/holocron`, for linux/amd64 and linux/arm64. The image holds the `holocron` binary and the built dashboard. It needs no database server, queue or Node runtime.

## Image tags

| Tag | Use |
| --- | --- |
| `:latest` | The newest release |
| `:YY.M.D`, for example `:26.10.19` | A release by date. Pin this in production |
| `:YY.M.D-<shorthash>`, for example `:26.10.19-1a2b3c4` | The same release, named with its commit |

The image runs as a non-root user (uid 10001), keeps its data in the `/data` volume and listens on port 8080. Its entrypoint is `holocron` and its default command is `serve`, so anything after the image name is a `holocron` command. It has a healthcheck on `/health`.

## What the host needs

Plan for a host with 2 vCPUs, 4 GiB of memory and a local SSD. That is the reference host Holocron is built and budgeted for. See [Sizing](../../README.md#sizing) for how far it goes, and what is measured and what is estimated.

Keep the data directory on one local volume. Holocron keeps 4 GiB of it free as a reserve and refuses new events below that.

## Install with Docker

Start the server on an empty volume:

```sh
docker run -d --name holocron --restart unless-stopped --stop-timeout 35 \
  -v holocron-data:/data -p 127.0.0.1:8080:8080 \
  -e HOLOCRON_PUBLIC_URL=https://analytics.example.com \
  -e HOLOCRON_COLLECT_TRUSTED_PROXIES=172.16.0.0/12 \
  ghcr.io/coderscantina/holocron:latest
```

On an empty volume the server starts in setup mode and logs a setup token. Open the public URL and finish [setup in the browser](#set-up-in-the-browser).

Give the container 30 seconds or more to stop. On SIGTERM the server finishes open requests, lets background work commit its step, stops admitting events and moves the batch in hand into analytics before it exits. Events still waiting are moved at the next start.

To set up from the shell instead, run `init` once on the volume before the first start. It creates the install admin and prints its password a single time; `--admin-password-file` takes your own:

```sh
docker run --rm -v holocron-data:/data ghcr.io/coderscantina/holocron:latest \
  init --admin-email you@example.com
```

`init` refuses a data directory that already holds files (`directory_not_empty`) or one it already set up (`already_initialized`).

## Install with docker compose

[deploy/docker-compose.yml](../../deploy/docker-compose.yml) runs the server:

```sh
curl -O https://raw.githubusercontent.com/coderscantina/holocron/main/deploy/docker-compose.yml
export HOLOCRON_PUBLIC_URL=https://analytics.example.com
docker compose up -d
docker compose logs holocron | grep 'setup token'
```

The compose file publishes port 8080 on `127.0.0.1` only, for a reverse proxy on the same host. Set `HOLOCRON_IMAGE` to pin a release tag, for example `ghcr.io/coderscantina/holocron:26.10.19`, and `HOLOCRON_INSTANCE_NAME` to name the instance.

It trusts `172.16.0.0/12` as a proxy, because a proxy on the host reaches the container through Docker's bridge network. See [Proxies and visitor addresses](configuration.md#proxies-and-visitor-addresses) before you change that.

## Install on Coolify

[deploy/coolify/docker-compose.yml](../../deploy/coolify/docker-compose.yml) is the Coolify variant. It is set up for Coolify's Traefik with Amazon CloudFront in front, and reads the visitor address from `cloudfront-viewer-address`.

1. Create a Docker Compose resource from the file.
2. Set `HOLOCRON_IMAGE` to a release, for example `ghcr.io/coderscantina/holocron:26.10.19`, and `HOLOCRON_PUBLIC_URL` to the dashboard address.
3. Set the domain of the `holocron` service to the host with the container port, for example `https://analytics.example.com:8080`.
4. Deploy. The resource's Logs show the setup token.
5. Open the domain and finish [setup in the browser](#set-up-in-the-browser).

Without CloudFront, remove `HOLOCRON_COLLECT_CLIENT_IP_HEADER` so the default `x-forwarded-for` applies, or set the header your CDN uses.

### Back up on Coolify

The compose file mounts `HOLOCRON_BACKUP_DIR` (default `/srv/holocron-backups` on the host) at `/backups`. Put it on other storage than Docker's volumes if the host has any, and let the container's user write to it: `sudo install -d -o 10001 -m 0700 /srv/holocron-backups`.

Add a scheduled task to the `holocron` service, for example daily at `0 3 * * *`, that takes a backup and keeps the newest 7:

```sh
sh -c 'holocron backup --to /backups && ls -1d /backups/holocron-backup-* | head -n -7 | xargs -r rm -rf'
```

Backup names start with the UTC time, so they sort oldest first. Don't use Coolify's volume backup for `holocron-data`: it copies the files of a running server one by one, and `serve` refuses such a mixed directory with `engines_mismatched`. Copy the backup directory off the host as well, a backup on the same machine doesn't survive losing it. See [Operations](operations.md#backup) for restore.

## Reverse proxy

Run TLS at a reverse proxy. Holocron serves plain HTTP on one host name: the dashboard, the tracker scripts and the collection endpoints all live there.

[deploy/Caddyfile](../../deploy/Caddyfile) is an example. Caddy gets the certificate and sets `X-Forwarded-For`:

```
analytics.example.com {
	reverse_proxy 127.0.0.1:8080
}
```

Other proxies:

- **nginx** needs `proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;` and, because the dashboard's live updates use server-sent events, `proxy_buffering off;`.
- **Traefik and Coolify** set `X-Forwarded-For` by default.

Two settings matter behind any proxy:

- `HOLOCRON_PUBLIC_URL`: the dashboard address. Commands from the dashboard must come from that origin, and an `https` URL marks the session cookie `Secure`. Without it the server logs a warning at start.
- `HOLOCRON_COLLECT_TRUSTED_PROXIES`: the proxy's address as Holocron sees it. Without it every visitor has the proxy's address, so they all collapse into one visitor a day. Check the visitor count after the first pageviews arrive.

Behind a CDN (Cloudflare, Vercel, CloudFront, EdgeOne), country, region and city come from its headers. Behind a plain reverse proxy, download the free DB-IP Lite City database after the first sign-in, in Administration, Security policy, Visitor location. Or point `HOLOCRON_GEOIP_DB` at your own GeoLite2 City file. Without either, reports show no location. See [Location](configuration.md#location).

## Set up in the browser

On an empty data directory `serve` starts in setup mode. It answers only the setup wizard and `/health`, and collection answers 503 until setup ends.

1. Open the public URL, for example `https://analytics.example.com`. It shows the setup wizard.
2. Enter the setup token from the server log, from the line with "setup token". It changes at every start; set `HOLOCRON_SETUP_TOKEN` to pin it for scripted installs.
3. The server check warns about a missing `HOLOCRON_PUBLIC_URL`, an untrusted proxy, low disk or the version. Each warning names the setting that fixes it. You can continue anyway.
4. Pick Fresh start or Import from Umami. Restore a backup still runs from the shell, see [Operations](operations.md#restore).
5. Create your admin. Passwords need at least 12 characters.
6. Fresh start: add the first site in the Default team. The wizard shows its tracking code and turns green when the first pageview arrives. Import from Umami continues as described in [Migrate from Umami](umami-migration.md); "Continue without import" leaves it for a fresh start at any point, discarding what was imported.

Then open Account, then Security, and set up two-factor authentication.

The first admin is an install admin and the owner of the Default team. Install admins manage accounts and the security policy. They read a site's analytics only through a workspace they belong to. See [Teams and access](teams-and-access.md).

## Build from source

Docker is the supported way to run Holocron. To build the image yourself from a checkout:

```sh
docker build --build-arg HOLOCRON_GIT_SHA=$(git rev-parse --short HEAD) -t holocron .
```

DuckDB compiles from source and needs a lot of memory per job. On a small builder add `--build-arg CARGO_BUILD_JOBS=2`.

`cargo build --release -p holocron-server --bin holocron` builds the binary alone. It serves the dashboard from a `web` directory next to the executable, or from `--web-dir`, which you build with Bun in `apps/web`.
