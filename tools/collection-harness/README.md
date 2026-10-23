# Collection harness

Runs official Umami 3.4.0 with PostgreSQL, seeds fixed collection sources, replays [fixtures/collection](../../fixtures/collection/README.md) against it, and captures the real tracker and recorder in headless Chromium.

Requirements: Docker with Compose v2, Node 22 or newer. Host port 33400 (Umami) and 33401 (capture test page) must be free. PostgreSQL publishes no host port.

| File | Purpose |
| --- | --- |
| `compose.yaml` | Umami and PostgreSQL pinned by digest, project `holocron-m01-collect` |
| `lib.mjs` | Base URL (`UMAMI_URL`, default `http://127.0.0.1:33400`), fixed source IDs, browser user agent |
| `seed.mjs` | Logs in as the default `admin`/`umami` account and creates four websites, one link and one pixel with fixed IDs. Idempotent |
| `replay.mjs` | Sends every replayable fixture and compares status, headers and body. Exit code 1 on any mismatch |
| `capture.mjs` | Serves a test page on 127.0.0.1:33401, loads `/script.js` and `/recorder.js` from Umami, runs each scenario, writes `fixtures/collection/script-captures/*.json` (or `--out <dir>`, used with `UMAMI_URL` pointing at Holocron), exit code 1 on a failed assertion. Also drives batch, link and pixel requests from the page |

## Commands

Run from the repository root.

```sh
# Start and seed
docker compose -f tools/collection-harness/compose.yaml up -d --wait
node tools/collection-harness/seed.mjs

# Request fixtures, default pass
node tools/collection-harness/replay.mjs
node tools/collection-harness/replay.mjs --only send-event   # subset by id prefix
node tools/collection-harness/replay.mjs --markdown          # table rows for the evidence report

# Collect-endpoint alias pass. COLLECT_API_ENDPOINT rewrites the served script.js,
# so run captures only after switching back.
UMAMI_COLLECT_API_ENDPOINT=/api/collect docker compose -f tools/collection-harness/compose.yaml up -d --wait umami
node tools/collection-harness/replay.mjs --pass collect-alias
docker compose -f tools/collection-harness/compose.yaml up -d --wait umami

# Script captures (installs Playwright 1.63.0 locally; Chromium revision 1243)
npm --prefix tools/collection-harness install
npx --prefix tools/collection-harness playwright install chromium
node tools/collection-harness/capture.mjs
node tools/collection-harness/capture.mjs --only recorder

# Tear down this project's containers and volume only
docker compose -f tools/collection-harness/compose.yaml down -v
```

## Configuration used by fixtures

- `APP_SECRET` is fixed, so tokens are reproducible in shape but carry real `iat` values.
- `TRACKER_SCRIPT_NAME: analytics.js, /stats/a.js` for the alias fixtures.
- `IGNORE_IP: 203.0.113.7` for the IP block fixtures.
- Seeded sources: plain website (recorder off), recorder website (replay and heatmaps, sample rate 1, strict masking), heatmap-only website, replay-only website, link `holocron-link`, pixel `holocron-pixel`.

The capture scenarios use a desktop Chrome user agent. Headless Chromium's default user agent contains `HeadlessChrome`, which isbot flags; the `bot-user-agent` scenario uses it on purpose.
