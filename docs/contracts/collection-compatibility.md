# Collection compatibility contract

Milestone 01, work item 2. This document lists every public collection surface of Umami 3.4.0, what an unchanged tracker or recorder sends, what upstream answers, and where Holocron deliberately differs. Fixtures live in [fixtures/collection](../../fixtures/collection/README.md). Upstream validation results are in [the evidence report](../evidence/m01-collection-validation.md). Holocron's implementation results (milestone 04: 98 of 120 request fixtures, 22 of 23 browser scenarios, recording deferred to 09) are in [the m04 evidence report](../evidence/m04-durable-modern-collection.md).

## Sources and labels

- Source: `.sources/umami-3.4.0` at commit `ec0ff50388c264ed8ce46f00967e92f7e71476ae` (tag v3.4.0). All `file:line` references point into that tree.
- Runtime: `ghcr.io/umami-software/umami@sha256:85909afc45bdcda1917394594a087421fdbb05610fded0fa9f6fb861abb2f367` (tag 3.4.0).
- Built scripts: `sources/umami-3.4.0/script.js` sha256 `654f12c7548be726eba5b71d5ef8bffc2a41a39a0053e2a49a847869ccd7ac3f`, `sources/umami-3.4.0/recorder.js` sha256 `21e6312866db01a71841b772c9882333231be1bc3fdbeeac1ed5193794ad80c6`. Both are byte-identical to `/app/public/` in the image and to what the container serves.

Labels used for Holocron behavior:

- **Confirmed**: stated in [the technical briefing](../technical-briefing.md).
- **Adopted**: team decision in docs/decisions: [0001](../decisions/0001-identity-and-consent.md) identity and consent, [0002](../decisions/0002-sessions-and-events.md) sessions and events, [0005](../decisions/0005-collection-admission-policy.md) collection admission policy.
- **Confirmed (Mike, 2026-10-03)**: decided by Mike and recorded in docs/decisions. Fixtures use `decision: "confirmed"` with the record cited in `notes`.
- **Proposed**: introduced here; needs review before implementation.

Identity derivation, consent semantics, session rules and property types are owned by docs/decisions (identity and consent record, sessions and events record). This document only records how they show up on the wire.

## Surface inventory

| Surface | Route | Fixture surface ids |
| --- | --- | --- |
| Tracker and recorder assets, aliases | `GET /script.js`, `GET /recorder.js`, `TRACKER_SCRIPT_NAME` names | `script.asset` |
| Collect endpoint alias | `COLLECT_API_ENDPOINT` path | `send.alias` |
| Event, identify, performance | `POST /api/send` | `send.event`, `send.identify`, `send.performance`, `send.link`, `send.pixel` |
| Batch | `POST /api/batch` | `batch` |
| Recorder configuration | `GET /api/websites/{id}/recorder` | `recorder.config` |
| Replay fragments and heatmaps | `POST /api/record` | `record.replay`, `record.heatmap` |
| Tracked links | `GET /q/{slug}` | `link` |
| Pixels | `GET /p/{slug}` | `pixel` |
| Real script behavior | browser captures | `script-captures/` (upstream), [`docs/evidence/m04/browser-captures/`](../evidence/m04/browser-captures/) (Holocron) |

## Script assets and aliases

- `GET /script.js` and `GET /recorder.js` are static files built by rollup (`rollup.tracker.config.js:6-22`, `rollup.recorder.config.js:7-23`). Build-time replacements: `__COLLECT_API_HOST__` (default empty) and, for the tracker only, `__COLLECT_API_ENDPOINT__` (default `/api/send`).
- Response headers: `Access-Control-Allow-Origin: *` and `Cache-Control: public, max-age=86400, must-revalidate` (`next.config.ts:54-63`, `docker/proxy.ts:23-26`, `docker/proxy.ts:66-75`). Content type `application/javascript; charset=UTF-8`. Observed.
- `TRACKER_SCRIPT_NAME`: comma-separated names, trimmed, leading slashes removed, each rewritten to `/script.js` with tracker headers (`docker/proxy.ts:44-56`). Matching is exact on the path, or on `BASE_PATH` + path (`src/lib/match-configured-path.ts:17-38`). There is no alias for the recorder.
- `TRACKER_SCRIPT_URL`: `/script.js` is rewritten to an external URL (`docker/proxy.ts:58-64`). Not validated; it proxies a third-party script.
- `COLLECT_API_ENDPOINT`: the configured path is rewritten to `/api/send` (`docker/proxy.ts:31-42`). At container start `scripts/update-tracker.js:6-14` replaces every `/api/send` in `public/script.js` with the configured value (`scripts/start-docker.sh:14-16`). The served tracker then differs from the pinned bytes (observed sha256 `26bcdce2...` with `/api/collect`). Only `/api/send` is aliased; batch, record and recorder configuration have no alias.
- An unconfigured name such as `/umami.js` returns 404.

## Tracker behavior (`src/tracker/index.ts`)

Configuration is read from the script element's attributes (`index.ts:238-253`):

| Attribute | Effect | Ref |
| --- | --- | --- |
| `data-website-id` | Required. Without it nothing is sent | `:241`, `:379` |
| `data-host-url` | Collection host. Default is the script's directory | `:242`, `:256-258` |
| `data-before-send` | Name of a global function `(type, payload)`. A falsy return cancels the send; it may return a promise | `:243`, `:387-395` |
| `data-distinct-id` | Calls `identify(id)` at script start | `:244`, `:648-650` |
| `data-tag` | Sent as `payload.tag` | `:245`, `:291` |
| `data-auto-track="false"` | No automatic pageview, no history or click hooks | `:246`, `:422-430`, `:652-658` |
| `data-auto-pageview="false"` | Hooks stay active but no automatic pageviews | `:253`, `:312`, `:425` |
| `data-do-not-track="true"` | Honor `doNotTrack` values `1`, `"1"`, `"yes"` | `:247`, `:295-298` |
| `data-exclude-search`, `data-exclude-hash` | Strip query or fragment from url and referrer | `:248-249`, `:266-276` |
| `data-domains` | Comma list; tracking only when `location.hostname` is in it | `:250`, `:255`, `:381` |
| `data-fetch-credentials` | fetch `credentials`, default `omit` | `:251`, `:408` |
| `data-performance="true"` | Enable web vitals | `:252`, `:428` |

Behavior:

- Payload fields: `website, screen ("WxH"), language, title, hostname, url, referrer, tag, id` (`:283-293`). Same-origin referrers are reduced to a path (`:280-281`).
- Disabled when: `disabled` flag from a previous response, no website id, `localStorage["umami.disabled"]` set, hostname not in domains, or opted-in DNT (`:377-382`). Upstream never sends `disabled` in 3.4.0; the flag is read from any response (`:411-415`).
- Pageview on load once `document.readyState` is `complete` (`:652-658`).
- SPA: `pushState` and `replaceState` are wrapped. A changed URL sends a pageview after 300 ms with the previous URL as referrer (`:302-333`). `popstate` is not hooked. Captured: `spa-navigation`.
- Clicks: capture-phase listener on `[data-umami-event]`. `data-umami-event-*` attributes become string data. For anchors without `_blank` or modifier keys the navigation waits for the send (`:335-373`).
- `umami.track()` forms: none (pageview), `(name, data)`, `(payload)` (sent as given, no defaults added), `(fn)` (fn receives defaults) (`:432-440`).
- `umami.identify(id, data)` or `identify({id, ...data})`: sets the id for later payloads, clears the cache token to `""`, and sends `type: "identify"` (`:442-460`). The next request carries an empty `x-umami-cache` header (`:406`). Captured: `identify`.
- Performance: observers for navigation, paint, LCP, layout shift (session windows) and event timing (INP, p98, 40 ms threshold). One payload per page on hidden, pagehide, the 10 s timer, or the next SPA navigation. Adds `duration` in ms, which the server strips (`:464-627`).
- Request: `fetch(endpoint, {keepalive: true, method: "POST"})` with headers `Content-Type: application/json`, `x-umami-website-id`, `x-umami-hostname`, and `x-umami-cache` when a token is held (`:398-409`). The custom headers make every send a CORS preflight.
- **Retry: none.** Errors and non-JSON responses are swallowed (`:417-419`). Status codes are never inspected. A 4xx, 5xx, 429 or 503 response just leaves the token unchanged; a non-JSON body leaves it unchanged; a JSON body without `cache` clears it.

## POST /api/send

Request (`src/app/api/send/route.ts:33-73`): `{ type: "event" | "identify" | "performance", payload }`. Unknown keys are stripped by zod. The body is read with `request.json()` regardless of content type (`src/lib/request.ts:62-68`); invalid JSON becomes `undefined` and fails validation.

| Payload field | Type and validation | Stored limit |
| --- | --- | --- |
| `website`, `link`, `pixel` | UUID; exactly one present (`:37-39`, `:62-72`) | |
| `data` | object (`z.record`), arrays rejected (`:40`) | keys 500, strings 500, arrays dropped over 500 (`src/lib/data.ts:53-61`) |
| `hostname`, `language`, `screen`, `title` | string (`:41-45`) | 100, 35, 11, 500 (`src/lib/constants.ts:276-286`) |
| `url`, `referrer` | string parseable by `new URL(v, "https://localhost")` (`:43`, `:46`, `src/lib/schema.ts:113-125`) | path and query 500 each |
| `name`, `tag` | string, must not start with `= + - @ \t \r` (`:25-31`, `:47-48`) | 50 each |
| `ip`, `userAgent`, `browser`, `os`, `device` | string; override connection values for any caller (`src/lib/detect.ts:136-148`) | 20 for browser/os/device |
| `timestamp` | `z.coerce.number().int()`, unix seconds; zod 4 `int()` also caps at `Number.MAX_SAFE_INTEGER` (fixture `send-event-timestamp-i64-overflow`); no other range check (`:51`, `:152`) | |
| `id` | string; truncated to 50 (`:52`, `:154`) | 50 |
| `lcp`, `inp`, `fcp`, `ttfb` | number 0..60000 inclusive, no coercion (`:56-60`) | |
| `cls` | number 0..100 inclusive (`:58`) | |

No request body size limit exists on `/api/send`. A 2,000,000-character data string is accepted and stored truncated to 500 (fixture `send-event-oversize-body`, observed).

Processing order (`:75-386`): schema; cache token; website lookup (website source only, skipped when the token has any `websiteId`); client info; bot check; IP block; session and visit; write; token.

Event kind: link source → link event, pixel source → pixel event, `name` present → custom event, else pageview (`:257-263`). Revenue is stored when `data.revenue > 0 && data.currency` (`src/queries/sql/events/saveEvent.ts:155-167`).

identify: with a website and `id`, writes a session link and updates the session's distinct id, best effort (`:313-338`). With `data`, writes session data, also for link and pixel sources (`:340-348`).

Responses:

| Case | Status | Body | Ref |
| --- | --- | --- | --- |
| Stored | 200 | `{cache, sessionId, visitId}` | `:378-383` |
| Schema error | 400 | `{error:{message:"Bad request", code:"bad-request", status:400, errors, properties}}` (zod tree) | `src/lib/response.ts:11-18` |
| Unknown website | 400 | `{error:{message:"Website not found.", code:"bad-request", status:400}}` | `:127-129` |
| Bot | 200 | `{beep:"boop"}` | `:142-145` |
| `IGNORE_IP` match | 403 | `{error:{message:"Forbidden", code:"forbidden", status:403}}` | `:147-150` |
| Exception | 500 | `{error:{message:"Server error", code:"server-error", status:500}}` | `:384-386` |

Bot detection is `isbot` 5.2.2 on `payload.userAgent` or the `User-Agent` header, disabled by `DISABLE_BOT_CHECK` (`:143`). The default headless Chromium UA is flagged.

Client IP: `CLIENT_IP_HEADER` if set, else the first present header of `true-client-ip`, `cf-connecting-ip`, `fastly-client-ip`, `x-nf-client-connection-ip`, `do-connecting-ip`, `x-real-ip`, `x-appengine-user-ip`, `x-forwarded-for` (first entry), `forwarded`, `x-client-ip`, `x-cluster-client-ip`, `x-forwarded` (`src/lib/ip.ts:3-17`, `:75-88`). No proxy trust check. `IGNORE_IP` accepts addresses and CIDRs (`src/lib/detect.ts:150-179`).

## Cache token handshake

- Token: HS256 JWT signed with `secret() = sha512(APP_SECRET || DATABASE_URL)` (`src/lib/crypto.ts:56-58`, `src/lib/jwt.ts:4-14`), claims `{websiteId, sessionId, visitId, iat, sessionLinkId, type:"cache"}`, no expiry set (`route.ts:378-381`).
- Sent back in `x-umami-cache`. An invalid or wrong-type token is ignored (`:113-121`).
- With a token: the website lookup is skipped (`:124`). The token's `websiteId` is not compared with the payload website, so a token from site A lets a request for any UUID through (fixture `send-event-cross-site-cache-token`; upstream stored an event and a session for a nonexistent website, checked in PostgreSQL).
- Session id is `uuid(source, ip, userAgent, salt, distinctId)` with a monthly salt by default (`:156-162`). If it differs from the token's, a session row is created and the visit restarts (`:163-192`).
- Visit expiry: a new visit when `now - iat > 1800` seconds, only for payloads without `timestamp` (`:194-198`).
- The recorder reads the token through `umami.getSession().cache` and sends it on every `/api/record` call (`src/recorder/index.js:53`, `:109-128`).

## POST /api/batch

- Body: array of at most 500 objects (`src/app/api/batch/route.ts:7`). 501 items, a non-array, or a non-object item fails the whole request with 400.
- Each item is replayed sequentially through the `/api/send` handler with the original request headers (`:21-36`). The cache token is not updated between items.
- Response 200: `{size, processed, errors, details:[{index, response}], cache}` (`:48-54`). `cache` is the first successful item's token, `null` for an empty batch, and absent when every successful item returned no token (bot batch).
- Bot items count as processed because the send handler returns 200 (fixture `batch-bot-user-agent`).

## Recorder (`src/recorder/index.js`)

- Attributes: `data-website-id` (required), `data-host-url` (`:13-22`).
- Bootstrap: `GET {host}/api/websites/{id}/recorder` with `credentials: "omit"`. Stops on non-OK, non-JSON, or `enabled` false (`:678-702`).
- Waits for the tracker token, polling every 100 ms up to 51 times (about 5 s), then gives up for the page (`:636-645`). Captured: `bot-user-agent`.
- Sampling per page: `Math.random() <= rate`, 1 always, 0 never (`:324-329`). Defaults if the server omits values: replay 0.15, heatmap 0.15, mask `moderate`, max duration 300000 ms (`:35-41`).
- Masking: `strict` = `maskAllInputs` plus `maskTextSelector: "*"`; otherwise `maskAllInputs` only (`:310-322`). `blockSelector` is passed to rrweb. Canvas and cross-origin iframes are not recorded (`:479-495`). Captured: strict mode sends text as `*` and never the typed input value.
- Replay flush: every 2 s, at 100 events, or when a payload would exceed 500,000 bytes. Full snapshots go in their own chunk. Events larger than 500,000 bytes are split into `umami:rrweb-event-fragment` events `{id, index, total, value}` (`:24-28`, `:143-250`, `:432-501`). Chunk `timestamp` is a strictly increasing unix second used as `chunkIndex` (`:78-85`).
- Heatmap: trusted primary-button clicks and max scroll percentage per URL; flush after 5 s idle, at 20 events, on hidden and beforeunload (`:32-33`, `:282-294`, `:503-634`).
- Requests: `POST {host}/api/record` with `Content-Type: application/json` and `x-umami-cache`, `keepalive` only for bodies under 60,000 bytes. Errors are swallowed; no retry (`:109-128`).

## GET /api/websites/{id}/recorder

- 200 `{enabled:false}` for a disabled, unknown, or non-UUID website (`route.ts:28-30`, observed for non-UUID).
- 200 `{enabled:true, replayEnabled, heatmapEnabled, sampleRate, heatmapSampleRate, maskLevel, maxDuration, blockSelector}` with server defaults 0.15, 0.15, `moderate`, 300000, `""` (`:32-46`).
- The route sets `Cache-Control: public, max-age=60, stale-while-revalidate=300` (`:23-26`), but the `/api/:path*` header rule in `next.config.ts:65-92` replaces it, so the wire value is `no-cache` (observed).
- `OPTIONS` returns 204 (`:6-8`).

## POST /api/record

Order (`src/app/api/record/route.ts:104-252`): size check; schema; empty events; token; website; recorder enabled; cloud plan check; bot; IP block; type-specific enable flag; write.

| Case | Status | Body |
| --- | --- | --- |
| Body over 1,000,000 bytes (`content-length`, else measured) | 413 | `{error:{message:"Payload too large", code:"payload-too-large", status:413, reason:"payload_too_large", maxBytes:1000000, size}}` (`:23`, `:106-116`) |
| Schema error | 400 | zod tree |
| `events` empty | 200 | `{ok:true}`, no token needed (`:128-130`) |
| No `x-umami-cache` | 400 | `Missing session token.` (`:135-137`) |
| Token without sessionId/visitId | 400 | `Invalid session token.` (`:139-143`) |
| Unknown website | 400 | `Website not found.` (`:150-152`) |
| Recorder off | 200 | `{ok:false, reason:"recorder_disabled"}` (`:158-160`) |
| Bot | 200 | `{beep:"boop"}` (`:177-179`) |
| `IGNORE_IP` match | 403 | Forbidden (`:181-183`) |
| `record` with replay off | 200 | `{ok:false, reason:"replay_disabled"}` (`:186-188`) |
| `heatmap` with heatmaps off | 200 | `{ok:false, reason:"heatmap_disabled"}` (`:216-218`) |
| Stored | 200 | `{ok:true}` |

Schemas (`:25-70`): `record` = `{website: uuid, events: any[] max 200, timestamp?: int}`. `heatmap` = `{website, events: max 200 of click {url, x, y, pageX, pageY, pageW, pageH, viewportW, viewportH, timestamp} | scroll {url, scrollPct, pageW, pageH, viewportW, viewportH, timestamp}, timestamp?}`, numbers coerced.

The token is not bound to the website: a token from site A records for site B (fixture `record-cross-site-token`). Heatmap write failures are logged and still answered `{ok:true}` (`:243-248`). CORS headers are added to every response (`src/lib/cors.ts:3-32`).

## Tracked links and pixels

- `GET /q/{slug}`: looks up a non-deleted link (Redis cache 86400 s when enabled), calls the send handler with `{type:"event", payload:{link, url: request.url, referrer: Referer}}` and the original headers, ignores its result, and answers `307` to the link URL (`src/app/(collect)/q/[slug]/route.ts:10-63`). Unknown slug: 404 `{error:{message:"Not found", code:"not-found", status:404}}`.
- `GET /p/{slug}`: same for pixels; answers a 43-byte transparent GIF with `Content-Type: image/gif`, `Cache-Control: no-cache, no-store, must-revalidate`, `Pragma: no-cache`, `Expires: 0` (`src/app/(collect)/p/[slug]/route.ts:10-73`).
- Bots still get the redirect or image; the event is not stored.
- `POST /api/send` with `payload.link` or `payload.pixel` performs no existence check (`route.ts:112-131`); upstream stores events for unknown link and pixel UUIDs (checked in PostgreSQL).

## CORS

- `/api/*`: `Access-Control-Allow-Origin: *`, `Access-Control-Allow-Headers: *`, `Access-Control-Allow-Methods: GET, DELETE, POST, PUT`, `Access-Control-Max-Age: 86400` (`CORS_MAX_AGE`), `Cache-Control: no-cache` (`next.config.ts:65-97`). Next.js answers `OPTIONS /api/send` and `/api/batch` with 204 itself.
- Record and recorder configuration routes define their own `OPTIONS` and CORS helper with `Allow-Headers: Content-Type, x-umami-cache` (`src/lib/cors.ts:3-11`), but the observed preflight carries the `next.config.ts` values.
- No origin is checked anywhere. `data-domains` is a client-side filter only.

## Preserved wire behavior and Holocron changes

Preserved means the fixture's `expected_holocron` matches `expected_upstream` on status and body shape.

| Behavior | Holocron | Label |
| --- | --- | --- |
| Pinned script bytes, script headers, `TRACKER_SCRIPT_NAME` aliases | Preserved | Confirmed (briefing, transport surfaces) |
| `COLLECT_API_ENDPOINT` alias and textual rewrite of the served tracker | Preserved; served hash recorded | Proposed |
| `/api/send` request schema, field limits, formula guard, truncation lengths | Preserved | Proposed |
| Exactly one of website, link, pixel | Preserved | Confirmed |
| Response `{cache, sessionId, visitId}` | Shape preserved, values from the new identity derivation | Confirmed (shape); values per docs/decisions 0001 |
| Error envelope `{error:{message, code, status}}` | Preserved; zod `errors`/`properties` detail not guaranteed | Proposed |
| Bot `{beep:"boop"}` with 200 | Wire preserved; outcome `suppressed`, not a stored event | Confirmed (distinct outcome); wire proposed |
| Batch limit 500, ordering, per-item results, response shape | Preserved; invalid items listed per item | Confirmed; per-item refusal adopted ([0005](../decisions/0005-collection-admission-policy.md)) |
| Recorder configuration body and sampling fields | Preserved; Holocron defaults strict masking and 10% replay sampling | Confirmed (briefing, replay) |
| Record schema, 200-event and 1,000,000-byte limits, token requirement, `{ok:false, reason}` policy bodies | Preserved | Confirmed (handshake, fragments, heatmaps); bodies proposed |
| Link 307 and pixel GIF, unknown slug 404, bots redirected | Preserved, also during storage outages (hit counted in `lost_hits`) | Adopted ([0005](../decisions/0005-collection-admission-policy.md)) |
| CORS wildcard on collection routes | Preserved; at least `content-type, x-umami-cache, x-umami-website-id, x-umami-hostname` allowed | Confirmed |

Deliberate changes:

| Change | Holocron behavior | Label |
| --- | --- | --- |
| Durable acknowledgement | 200 (or `ok:true`) only after the SQLite commit; no acknowledgement into memory. Upstream's Kafka path logs send failures and still answers 200 (`src/lib/kafka.ts:90-144`), and heatmap write errors still return `ok:true` | Confirmed (briefing, durability) |
| Storage unavailable | `/api/send`, `/api/batch`, `/api/record` refused with 503, `Retry-After`, `serviceUnavailable` envelope (`src/lib/response.ts:57-64`); counted in `refused_backpressure` as lost client events | Concept confirmed; adopted ([0005](../decisions/0005-collection-admission-policy.md)) |
| Admission budget full | Refused, 429 with `Retry-After`; sized so the 500 events/s burst gets zero refusals | Concept confirmed; adopted ([0005](../decisions/0005-collection-admission-policy.md)) |
| Request body caps | 64 KiB on `/api/send`, 2 MiB on `/api/batch`, 1,000,000 bytes on `/api/record` (unchanged), counted on received bytes before parsing; over the cap 413 with the upstream `payloadTooLarge` envelope, `maxBytes`, `size` | Adopted, provisional until milestone 02 measurements ([0005](../decisions/0005-collection-admission-policy.md)) |
| Outcomes | `accepted` (including keyless), `duplicate`, `suppressed` (bot, IP block, recording disabled, replay budget, `outside_retention`), `refused` (validation, unknown source, body cap, backpressure) | Confirmed for accepted/suppressed/refused; duplicate adopted ([0002](../decisions/0002-sessions-and-events.md)) |
| Event time | Any timestamp is accepted, as upstream. Older than the 12-month retention cutoff on arrival: 200 with the normal body, `suppressed`, reason `outside_retention`, not stored. More than 5 minutes ahead: stored and counted in `event_time_ahead`. Late event whose day key is destroyed: stored keyless, excluded from identity measures. Untimestamped payloads use receive time | Confirmed (Mike, 2026-10-03), [0002](../decisions/0002-sessions-and-events.md) Event time; consequences adopted in [0005](../decisions/0005-collection-admission-policy.md) Event time outcomes, [0006](../decisions/0006-retention-clock-and-operations.md) Retention cutoff, [0001](../decisions/0001-identity-and-consent.md) Key retention and keyless events |
| Client `eventId` | Optional payload `eventId`, 1 to 64 printable ASCII, scope `(source, eventId)`, 48 h window; first wins; later copies acknowledged as `duplicate` and not stored, differing payloads count `dedup_conflict` | Adopted ([0002](../decisions/0002-sessions-and-events.md)); a duplicate gets the unchanged `{cache, sessionId, visitId}` body ([0005](../decisions/0005-collection-admission-policy.md)). The 400 for an invalid `eventId` is proposed |
| Consent extension | Top-level `payload.consent` `"persistent"` or `"none"` next to `id`, sent via `data-before-send` or an authenticated server call. Unchanged trackers never send it; upstream strips it (captured `before-send-consent-extension`) | Transport adopted ([0001](../decisions/0001-identity-and-consent.md)). Client-asserted consent and revocation keeping history: Confirmed (Mike, 2026-10-03), [0001](../decisions/0001-identity-and-consent.md) |
| Invalid `consent` value or no `id` | Ignored and counted in `consent_ignored`; the identify is accepted | Adopted ([0001](../decisions/0001-identity-and-consent.md), [0005](../decisions/0005-collection-admission-policy.md)) |
| `payload.ip`, `payload.userAgent` | Honored only for requests authenticated with a site collection key | Adopted ([0001](../decisions/0001-identity-and-consent.md)) |
| Forwarding and geo headers | Read only from peers in `collect.trusted_proxies` (default empty), right-to-left scan | Adopted ([0001](../decisions/0001-identity-and-consent.md)) |
| Token bound to website | A token for another website is ignored on `/api/send` and `/api/batch` and refused (`Invalid session token.`) on `/api/record`; upstream tokens fail the signature check and are ignored | Adopted ([0005](../decisions/0005-collection-admission-policy.md)) |
| Source existence | Unknown link and pixel ids refused with 400, `Link not found.` / `Pixel not found.` | Adopted ([0005](../decisions/0005-collection-admission-policy.md)) |
| Batch commit | Items validated and reported one by one as upstream; accepted items of one request share one durable commit; if it fails, nothing is stored and the request gets 503 | Adopted ([0005](../decisions/0005-collection-admission-policy.md)) |
| Operator IP block list | `IGNORE_IP` becomes `collect.ignore_ips` (IPs and CIDRs, default empty), matched against the client IP resolved under proxy trust, never a raw header. Match keeps 403 on send, batch items and record; outcome `suppressed`, reason `ip_blocked`, counted, IP not logged. Links and pixels still redirect or serve | Adopted ([0005](../decisions/0005-collection-admission-policy.md)) |
| identify for link or pixel sources | Refused with 400 | Proposed |
| Timestamps beyond the storable range | A `timestamp` within `Number.MAX_SAFE_INTEGER` seconds but outside chrono's range less one day (about ±262,000 years) is refused with 400 `Bad request`, outcome `refused`, reason `invalid`, like the i64 overflow case | Confirmed (Mike, 2026-10-04, [0005](../decisions/0005-collection-admission-policy.md) Event time outcomes) |
| Properties over 50 leaves | Dropped and counted; the event is accepted | Adopted ([0002](../decisions/0002-sessions-and-events.md)) |
| Replay budget reached | `{ok:false, reason:"replay_budget_exceeded"}`, 200, analytics collection continues | Concept confirmed (briefing, replay); body proposed |
| Upstream tokens | Accepted as opaque hints; old sessions are not continued after cutover | Confirmed (briefing, identity) |
| Privacy | Raw IP, user agent and distinct id are transient and not stored or logged (0001). Masking is the client's job; the server cannot prove a client masked (briefing, replay) | Confirmed / adopted |

The unchanged tracker and recorder do not retry. Any refusal, 429 or 503 loses that event or chunk on the client. Holocron documentation must say so; Retry-After helps only server integrations ([0005](../decisions/0005-collection-admission-policy.md), backpressure and lost client events).

## Upstream gaps found

These are upstream behaviors that the fixtures pin as "upstream does X". Holocron does not reproduce them, except item 3:

1. A cache token from one website skips the website check for any other UUID (`send/route.ts:124`).
2. `/api/send` stores events for link and pixel UUIDs that do not exist.
3. No timestamp range: a millisecond value is read as seconds and stored as year 58692 (checked in PostgreSQL). Holocron now keeps this behavior by Mike's decision and counts it in `event_time_ahead` ([0002](../decisions/0002-sessions-and-events.md), [0005](../decisions/0005-collection-admission-policy.md)). Only values the engines cannot store at all are refused (Confirmed 2026-10-04, see the table above).
4. Any client can set `ip` and `userAgent`, which feed location, bot detection, the IP block list and the session id.
5. Record tokens are not bound to the website.
6. No body limit on `/api/send` or `/api/batch`.
7. Heatmap write failures and Kafka send failures are acknowledged as success.
8. The 50-property limit in tracker comments (`src/tracker/index.ts:69-76`) is not enforced; all 51 properties were stored.

## Resolved questions

The six open questions of the first revision are settled in [0005](../decisions/0005-collection-admission-policy.md):

1. Links and pixels during a storage outage: still redirect or serve, count `lost_hits`.
2. Batch: per-item outcomes, one durable commit per request, 503 for the whole request if it fails.
3. Duplicate `eventId`: unchanged `{cache, sessionId, visitId}` body.
4. Invalid `consent` value: ignored and counted in `consent_ignored` (also [0001](../decisions/0001-identity-and-consent.md)).
5. Body caps: 64 KiB send, 2 MiB batch, provisional until milestone 02.
6. `IGNORE_IP`: kept as `collect.ignore_ips` on the trusted-proxy-resolved IP, 403, suppressed and counted.

The three items that were pending Mike are now Confirmed (Mike, 2026-10-03): any event time is accepted ([0002](../decisions/0002-sessions-and-events.md)), and client-asserted consent and revocation keeping history ([0001](../decisions/0001-identity-and-consent.md)). Still open: the proposed rows above.
