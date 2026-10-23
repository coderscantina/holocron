# Milestone 01 collection validation

Fixture validation of [fixtures/collection](../../fixtures/collection/README.md) against official Umami 3.4.0. Contract: [collection compatibility](../contracts/collection-compatibility.md).

## Run

- Date: 2026-10-03, final clean run finished 08:50 UTC. Host: macOS (Darwin 27.0.0), Docker 29.4.0, Node 24.21.0.
- Umami: `ghcr.io/umami-software/umami@sha256:85909afc45bdcda1917394594a087421fdbb05610fded0fa9f6fb861abb2f367` (tag 3.4.0), source commit `ec0ff50388c264ed8ce46f00967e92f7e71476ae`.
- PostgreSQL: `postgres@sha256:f7d23353e1b15400d22ebe31189f4d314b87a4c129cc400c8c2d8d4ca127bf81` (tag 15-alpine).
- Served scripts: `/script.js` sha256 `654f12c7548be726eba5b71d5ef8bffc2a41a39a0053e2a49a847869ccd7ac3f`, `/recorder.js` sha256 `21e6312866db01a71841b772c9882333231be1bc3fdbeeac1ed5193794ad80c6`. Both equal `sources/umami-3.4.0/` and `/app/public/` in the image.
- Browser: Playwright 1.63.0, Chromium revision 1243 (153.0.8010.12), headless.

Commands, from the repository root, starting with no volume:

```sh
docker compose -f tools/collection-harness/compose.yaml down -v
docker compose -f tools/collection-harness/compose.yaml up -d --wait
node tools/collection-harness/seed.mjs
node tools/collection-harness/replay.mjs --markdown
UMAMI_COLLECT_API_ENDPOINT=/api/collect docker compose -f tools/collection-harness/compose.yaml up -d --wait umami
node tools/collection-harness/replay.mjs --pass collect-alias --markdown
docker compose -f tools/collection-harness/compose.yaml up -d --wait umami
node tools/collection-harness/capture.mjs
node tools/collection-harness/capture.mjs   # second run, output diffed: identical
docker compose -f tools/collection-harness/compose.yaml down -v
```

## Results

- Request fixtures, default pass: 112 of 112 passed (latest run 12:53 UTC, fresh volume). Earlier: 102 at 08:50 UTC, 109 at 11:08 UTC. Seven fixtures for docs/decisions/0005 (body caps, `collect.ignore_ips`) were added at 11:08 UTC, and three event-time fixtures for Mike's 2026-10-03 decision (outside retention, keyless day, i64 overflow) at 12:53 UTC. Each was a full default pass with the same images and seed.
- Request fixtures, collect-alias pass: 2 of 2 passed.
- Holocron-only fixtures not replayed: 6 (see below).
- Script captures: 20 scenarios, all assertions passed. A second run produced byte-identical normalized files.

Three expectations were wrong when first run and were corrected from a closer reading, not tuned to the output:

- `batch-mixed-invalid-item`: the exact matcher applies to nested objects; the expected item error now lists the full upstream envelope (`message, code, status, errors, properties`).
- `recorder-config-enabled`: the route sets `Cache-Control: public, max-age=60, stale-while-revalidate=300`, but the `/api/:path*` header rule in `next.config.ts:65-92` overrides it to `no-cache` on the wire. The fixture now expects `no-cache` and documents both.
- `send-event-timestamp-i64-overflow`: I expected 500 from an Invalid Date, but zod 4 `int()` rejects integers above `Number.MAX_SAFE_INTEGER`, so upstream answers 400 at the schema layer (failed run 12:52 UTC, corrected 12:53 UTC).

Database checks after the replay (PostgreSQL, `website_event`, `session`, `event_data`, `heatmap_event`):

- Events and a session were stored for the unknown website `...00ff` sent with another site's cache token.
- Link and pixel events were stored for the unknown source `...00fe`.
- The millisecond timestamp was stored as `58692-11-03 14:13:20+00`.
- 60-character event names were stored with 50 characters; the 2,000,000-character data string with 500.
- All 51 properties of `send-event-properties-over-50` were stored.
- Every `eventId` duplicate was stored as a separate event.
- Heatmap click and scroll rows were stored.

### Default pass

| Fixture | Surface | Status | Result | Problems |
| --- | --- | --- | --- | --- |
| batch-body-over-2mib | batch | 200 | pass |  |
| batch-bot-user-agent | batch | 200 | pass |  |
| batch-empty | batch | 200 | pass |  |
| batch-ignore-ips | batch | 200 | pass |  |
| batch-item-not-object | batch | 400 | pass |  |
| batch-max-500 | batch | 200 | pass |  |
| batch-mixed-invalid-item | batch | 200 | pass |  |
| batch-not-array | batch | 400 | pass |  |
| batch-over-500 | batch | 400 | pass |  |
| batch-two-items | batch | 200 | pass |  |
| link-bot-user-agent | link | 307 | pass |  |
| link-ignore-ips | link | 307 | pass |  |
| link-redirect | link | 307 | pass |  |
| link-unknown-slug | link | 404 | pass |  |
| pixel-gif | pixel | 200 | pass |  |
| pixel-unknown-slug | pixel | 404 | pass |  |
| record-bot-user-agent | record.replay | 200 | pass |  |
| record-cross-site-token | record.replay | 200 | pass |  |
| record-empty-events | record.replay | 200 | pass |  |
| record-heatmap-click-scroll | record.heatmap | 200 | pass |  |
| record-heatmap-disabled | record.heatmap | 200 | pass |  |
| record-heatmap-missing-url | record.heatmap | 400 | pass |  |
| record-heatmap-unknown-event-type | record.heatmap | 400 | pass |  |
| record-ignore-ips | record.replay | 403 | pass |  |
| record-invalid-token | record.replay | 400 | pass |  |
| record-missing-token | record.replay | 400 | pass |  |
| record-over-200-events | record.replay | 400 | pass |  |
| record-oversize | record.replay | 413 | pass |  |
| record-preflight | record.replay | 204 | pass |  |
| record-recorder-disabled | record.replay | 200 | pass |  |
| record-replay-chunk | record.replay | 200 | pass |  |
| record-replay-disabled | record.replay | 200 | pass |  |
| record-replay-fragment | record.replay | 200 | pass |  |
| record-unknown-type | record.heatmap | 400 | pass |  |
| record-unknown-website | record.replay | 400 | pass |  |
| record-website-not-uuid | record.replay | 400 | pass |  |
| recorder-config-disabled-site | recorder.config | 200 | pass |  |
| recorder-config-enabled | recorder.config | 200 | pass |  |
| recorder-config-not-uuid | recorder.config | 200 | pass |  |
| recorder-config-preflight | recorder.config | 204 | pass |  |
| recorder-config-unknown-site | recorder.config | 200 | pass |  |
| script-alias-nested-path | script.asset | 200 | pass |  |
| script-alias-tracker-name | script.asset | 200 | pass |  |
| script-recorder-get | script.asset | 200 | pass |  |
| script-tracker-get | script.asset | 200 | pass |  |
| script-unknown-name | script.asset | 404 | pass |  |
| send-alias-unconfigured | send.alias | 404 | pass |  |
| send-event-body-over-64kib | send.event | 200 | pass |  |
| send-event-body-under-64kib | send.event | 200 | pass |  |
| send-event-bot-payload-user-agent | send.event | 200 | pass |  |
| send-event-bot-user-agent | send.event | 200 | pass |  |
| send-event-consent-in-data | send.event | 200 | pass |  |
| send-event-cross-site-cache-token | send.event | 200 | pass |  |
| send-event-custom-with-data | send.event | 200 | pass |  |
| send-event-data-array | send.event | 400 | pass |  |
| send-event-empty-body | send.event | 400 | pass |  |
| send-event-eventid-duplicate-different-payload | send.event | 200 | pass |  |
| send-event-eventid-duplicate-same-payload | send.event | 200 | pass |  |
| send-event-eventid-first | send.event | 200 | pass |  |
| send-event-eventid-too-long | send.event | 200 | pass |  |
| send-event-explicit-timestamp | send.event | 200 | pass |  |
| send-event-ignore-ips-trusted-proxy | send.event | 403 | pass |  |
| send-event-ignored-ip-header | send.event | 403 | pass |  |
| send-event-ignored-ip-payload | send.event | 403 | pass |  |
| send-event-invalid-cache-token | send.event | 200 | pass |  |
| send-event-invalid-type | send.event | 400 | pass |  |
| send-event-malformed-json | send.event | 400 | pass |  |
| send-event-missing-source | send.event | 400 | pass |  |
| send-event-name-formula-trigger | send.event | 400 | pass |  |
| send-event-name-over-50 | send.event | 200 | pass |  |
| send-event-oversize-body | send.event | 200 | pass |  |
| send-event-pageview | send.event | 200 | pass |  |
| send-event-properties-over-50 | send.event | 200 | pass |  |
| send-event-text-plain-content-type | send.event | 200 | pass |  |
| send-event-timestamp-boundary-minus-24h-1s | send.event | 200 | pass |  |
| send-event-timestamp-boundary-minus-24h | send.event | 200 | pass |  |
| send-event-timestamp-boundary-plus-5min-1s | send.event | 200 | pass |  |
| send-event-timestamp-boundary-plus-5min | send.event | 200 | pass |  |
| send-event-timestamp-future-10min | send.event | 200 | pass |  |
| send-event-timestamp-i64-overflow | send.event | 400 | pass |  |
| send-event-timestamp-keyless-day | send.event | 200 | pass |  |
| send-event-timestamp-late-25h | send.event | 200 | pass |  |
| send-event-timestamp-milliseconds | send.event | 200 | pass |  |
| send-event-timestamp-non-integer | send.event | 400 | pass |  |
| send-event-timestamp-not-numeric | send.event | 400 | pass |  |
| send-event-timestamp-outside-retention | send.event | 200 | pass |  |
| send-event-unknown-fields-stripped | send.event | 200 | pass |  |
| send-event-url-invalid | send.event | 400 | pass |  |
| send-event-website-and-link | send.event | 400 | pass |  |
| send-event-website-not-uuid | send.event | 400 | pass |  |
| send-event-website-unknown | send.event | 400 | pass |  |
| send-event-with-cache-token | send.event | 200 | pass |  |
| send-identify-consent-invalid-value | send.identify | 200 | pass |  |
| send-identify-consent-none | send.identify | 200 | pass |  |
| send-identify-consent-persistent | send.identify | 200 | pass |  |
| send-identify-consent-without-id | send.identify | 200 | pass |  |
| send-identify-data-not-object | send.identify | 400 | pass |  |
| send-identify-data-only | send.identify | 200 | pass |  |
| send-identify-id-and-data | send.identify | 200 | pass |  |
| send-identify-id-over-50 | send.identify | 200 | pass |  |
| send-identify-link-source | send.identify | 200 | pass |  |
| send-link-source | send.link | 200 | pass |  |
| send-link-unknown-id | send.link | 200 | pass |  |
| send-performance-all-vitals | send.performance | 200 | pass |  |
| send-performance-boundary-max | send.performance | 200 | pass |  |
| send-performance-cls-over-max | send.performance | 400 | pass |  |
| send-performance-lcp-over-max | send.performance | 400 | pass |  |
| send-performance-negative | send.performance | 400 | pass |  |
| send-performance-string-metric | send.performance | 400 | pass |  |
| send-pixel-source | send.pixel | 200 | pass |  |
| send-pixel-unknown-id | send.pixel | 200 | pass |  |
| send-preflight | send.event | 204 | pass |  |

### Collect-alias pass

| Fixture | Surface | Status | Result | Problems |
| --- | --- | --- | --- | --- |
| send-alias-collect-endpoint | send.alias | 200 | pass |  |
| send-alias-script-rewritten | send.alias | 200 | pass |  |

### Script captures

| Capture | Requests | Result |
| --- | --- | --- |
| pageview-on-load | 1 | pass |
| spa-navigation | 2 | pass |
| custom-event-data-attribute | 2 | pass |
| track-api | 4 | pass |
| identify | 3 | pass |
| distinct-id-attribute | 2 | pass |
| before-send-cancel | 0 | pass |
| before-send-consent-extension | 2 | pass |
| do-not-track-respected | 0 | pass |
| do-not-track-without-attribute | 1 | pass |
| domains-mismatch | 0 | pass |
| domains-match | 1 | pass |
| exclude-search-and-hash | 1 | pass |
| tag | 2 | pass |
| auto-track-off | 1 | pass |
| local-storage-disabled | 0 | pass |
| performance | 2 | pass |
| bot-user-agent | 3 | pass |
| recorder-replay-and-heatmap | 6 | pass |
| recorder-disabled-site | 2 | pass |

## Not validated against upstream

| Surface or feature | Reason |
| --- | --- |
| `holocron-send-storage-unavailable` | Needs a storage fault. Upstream behavior (500 on the relational path, silent 200 on the Kafka path) is from source reading of `src/app/api/send/route.ts:384-386` and `src/lib/kafka.ts:90-144`; the harness runs no Kafka or ClickHouse |
| `holocron-batch-commit-failure`, `holocron-link-storage-unavailable`, `holocron-pixel-storage-unavailable` | Need a storage fault injected into Holocron; upstream behavior is from source reading |
| `holocron-send-admission-budget-full`, `holocron-record-budget-exceeded` | Holocron-only; upstream has no admission or replay budget |
| Holocron outcomes in general | No Holocron collection server exists yet. Every `expected_holocron` is a specification, not an observation |
| Holocron event-time outcomes (`outside_retention`, keyless, `event_time_ahead`) | No Holocron server yet. The former live-window boundary fixtures stay as regression checks; Mike's 2026-10-03 decision removed the window (docs/decisions/0002) |
| `TRACKER_SCRIPT_URL` | Proxies a third-party script URL; not exercised |
| `BASE_PATH` | Build argument of the image (`Dockerfile`, `ARG BASE_PATH`); the official image is built without one, so base-path routing cannot be tested without rebuilding |
| ClickHouse, Kafka, Redis modes | Not part of the self-hosted PostgreSQL deployment Holocron replaces; link/pixel Redis caching and Kafka drops are source-read only |
| `CLOUD_MODE` behaviors (business plan check on `/api/record`, `x-umami-client-*` headers) | Cloud-only |
| `CLIENT_IP_HEADER`, CDN geo headers, MaxMind lookups | Requests come from the Docker host with no forwarding; location is null. Source-read only (`src/lib/ip.ts`, `src/lib/detect.ts`) |
| CORS preflights from the browser | The capture runner skips OPTIONS requests, so captures do not list preflights. The cross-origin page (port 33401 to 33400) only works if preflights succeed; their responses are validated by the `send-preflight`, `record-preflight` and `recorder-config-preflight` request fixtures |
| Recorder fragmentation of a single event over 500,000 bytes | The capture page is small; fragment shape is covered by the `record-replay-fragment` request fixture built from `src/recorder/index.js:87-96` |
| Replay `maxDuration` stop and the 30 s rrweb checkout | Would need a capture longer than 5 minutes; source-read only |
| Performance flush on SPA navigation and pagehide | Capture used the 10 s timer path only |
| Playwright browser download | `npx playwright install chromium` did not finish in this environment; revision 1243 was already in the local cache and was used |
