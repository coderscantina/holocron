# 0005: Collection admission policy

Status: Adopted (milestone 01, team decision) unless an item says otherwise. Items marked Confirmed restate [the briefing](../technical-briefing.md). This record settles the open questions of [the collection compatibility contract](../contracts/collection-compatibility.md) and its proposals on token binding and source existence. Identity and consent rules come from [0001](0001-identity-and-consent.md), event rules from [0002](0002-sessions-and-events.md).

## Problem

The collection fixtures need one answer for what each public route does when storage is unavailable, when a batch is partly invalid, when an event is a duplicate, when a consent value is malformed, when a body is large, and when a token or source ID does not belong to the request. Upstream either has no rule or a rule Holocron does not want.

## Constraints

Confirmed:

- Collection success waits for a durable SQLite commit. Nothing is acknowledged into memory.
- Bounded admission and explicit backpressure when durable storage is unavailable or a backlog cannot fit its budget.
- Unchanged Umami 3.4.0 trackers and recorders keep working. Batch keeps per-item results, ordering, limits and cache behavior.
- Admission has a byte budget (briefing, module interfaces).
- Rejection during the agreed healthy burst fails the capacity gate ([performance validation](../performance-validation.md)).

## Evidence

From [the collection compatibility contract](../contracts/collection-compatibility.md) and `.sources/umami-3.4.0`:

- The tracker swallows errors and never retries (`src/tracker/index.ts:417-419`). The recorder does the same (`src/recorder/index.js:109-128`). Status codes are never inspected.
- `/api/batch` replays each item through the send handler and answers `{size, processed, errors, details, cache}` (`src/app/api/batch/route.ts:21-54`).
- `/api/send` has no body limit; a 2,000,000-character string was accepted. `/api/record` refuses bodies over 1,000,000 bytes with 413.
- A cache token from site A skips the website lookup for any other UUID (`send/route.ts:124`); upstream stored an event and a session for a nonexistent website. `/api/record` tokens are not bound to the website either.
- `/api/send` stores events for link and pixel UUIDs that do not exist (`route.ts:112-131`).
- `GET /q/{slug}` and `GET /p/{slug}` ignore the send result and always redirect or serve the GIF.
- A captured tracker pageview body is 244 bytes on a short URL. With 500-character URLs, titles and referrers a tracker payload stays under 2 KiB. 50 properties of 500 characters add about 26 KiB.

## Choice

### Backpressure and lost client events

When storage is unavailable, `/api/send`, `/api/batch` and `/api/record` answer 503 with `Retry-After`; when the admission budget is full they answer 429 with `Retry-After` (statuses as in the compatibility contract).

The unchanged tracker and recorder never retry. Every 429 or 503 loses that event, batch or replay chunk on the client. `Retry-After` helps only server integrations that read it. Consequences:

- Refusals are counted per route and reason (`refused_backpressure{route, reason}`) and shown in operations status as lost client events, not as "retried later".
- Product documentation says so in plain words.
- The admission budget must be sized so the confirmed 500 events/s burst produces zero backpressure refusals; one refused event in that burst fails the gate.

### Links and pixels during a storage outage

`GET /q/{slug}` still answers its 307 redirect and `GET /p/{slug}` still serves its GIF when the hit cannot be stored. The hit is not stored and increments `lost_hits` for that link or pixel; link and pixel reports show the count ([0003](0003-metric-formulas.md) Links and pixels).

This loses nothing that a 503 would keep: a browser following a link does not retry a 503 either, so the hit is lost both ways. A 503 would only add a broken link for the visitor. No stored count is affected, so exactness holds.

Unknown slugs keep upstream's 404. Bots still get the redirect or image, and the hit is suppressed.

### Batch commit

Each `/api/batch` item is validated and classified on its own: one invalid item never refuses its neighbours, and `details` lists every item's outcome in order, as upstream does. The accepted items of one request are written in one durable commit (group commit may merge it with other requests) before the response is sent.

If that commit fails, no item of the request is stored and the whole request is answered 503. A request is therefore never half stored because of storage, only because of per-item validation, which the client can see in `details`. All-or-nothing validation was rejected because it changes upstream's per-item result meaning and one bad item would drop 499 good ones.

### Duplicate eventId response

A duplicate ([0002](0002-sessions-and-events.md) Client event-ID deduplication) is answered 200 with the unchanged stored-event body `{cache, sessionId, visitId}`, computed for the current request as for a stored event. In a batch its `details` entry has the same body. The outcome is visible only in counters (`duplicate`, `dedup_conflict`). An extra `duplicate: true` key was rejected: unchanged clients ignore it, and the counters already tell the operator.

### Event time outcomes

Confirmed (Mike, 2026-10-03): no payload is refused for its timestamp, as upstream ([0002](0002-sessions-and-events.md) Event time). Adopted wire behavior:

- An event older than the retention cutoff on arrival: 200 with the unchanged stored-event body, outcome `suppressed`, reason `outside_retention`, not stored, counted.
- An event more than 5 minutes ahead of receive time: stored, 200, counted in `event_time_ahead`.
- A late event for a destroyed day key: stored keyless, 200 ([0001](0001-identity-and-consent.md)). The `sessionId` and `visitId` in its body are derived from the request as usual; they identify no stored session.
- The upstream gap "no timestamp range" (a millisecond value stored as year 58692) is now accepted Holocron behavior with the known risk recorded in 0002.

Confirmed (Mike, 2026-10-04, milestone 04):

- **Timestamps the engines cannot store are refused.** A `timestamp` whose millisecond value lies outside chrono's range less one day on each side (about years -262,000 to +262,000) is answered 400 `Bad request`, outcome `refused`, reason `invalid`. This extends the existing rule for values that overflow i64 (fixture `send-event-timestamp-i64-overflow`) to values that fit i64 but not a `TIMESTAMP` or a calendar day. Upstream accepts up to `Number.MAX_SAFE_INTEGER` seconds, so this narrows "no payload is refused for its timestamp". The alternative, clamping or storing receive time, would change the event silently. 0002 Event time carries the matching line.
- **Keyless from `end(D) + 25h`, whether or not the key is destroyed yet.** An event for a day whose destruction time has passed is stored keyless even when the maintenance job (every 15 minutes) has not yet overwritten that day's key. Read literally, [0001](0001-identity-and-consent.md) would still use a key that exists past its time. The stricter rule makes the outcome depend only on event time and receive time, not on when the job last ran. 0001 records it.

### Invalid consent value

Ignored and counted in `consent_ignored`; the identify is accepted ([0001](0001-identity-and-consent.md) Persistent consent transport).

### Request body caps

Provisional caps, to be confirmed or changed by the milestone 02 measurements:

| Route | Cap | Basis |
| --- | --- | --- |
| `POST /api/send` | 64 KiB | about twice the largest valid payload: 50 properties of 500 characters plus 500-character URL, title and referrer |
| `POST /api/batch` | 2 MiB | 500 tracker-sized items at 2 KiB with 2x headroom; a 500-item tracker batch measured at 120 to 250 KB |
| `POST /api/record` | 1,000,000 bytes | unchanged from upstream |

- The count is on received body bytes before JSON parsing: `Content-Length` when present, else a counted stream that stops reading at the cap.
- Over the cap: 413 with the upstream `payloadTooLarge` envelope, `maxBytes` and `size`. Nothing from the body is stored.
- Bodies under the cap still draw from the admission byte budget; the cap bounds one request, the budget bounds all of them.
- Upstream has no cap on send and batch. Unchanged trackers send far below these caps, so no supported client is affected.

### Cache tokens bound to their website

Holocron cache tokens carry the website ID they were issued for and are signed with a key derived from the root secret ([0001](0001-identity-and-consent.md)).

- `/api/send` and `/api/batch`: a token for another website is ignored, as if absent. The request goes through the normal website lookup, so an unknown website gets upstream's 400 `Website not found.`
- `/api/record`: a token for another website is refused with 400 `Invalid session token.`, the body upstream uses for a malformed token.
- Upstream tokens fail Holocron's signature check and are ignored, which matches the confirmed "old sessions are not continued after cutover".

### Operator IP block list

Upstream's `IGNORE_IP` is kept as `collect.ignore_ips`: a list of IP addresses and CIDRs, default empty. It matches the client IP resolved under [0001](0001-identity-and-consent.md) proxy trust, never a raw header, so an untrusted client cannot step around it or into it. A match keeps upstream's wire answer (403 `Forbidden` on `/api/send`, `/api/batch` items and `/api/record`) and has outcome `suppressed`, reason `ip_blocked`: not stored, counted, and the IP itself is not logged. Link and pixel routes still redirect or serve the image, like bots. Operators use it to exclude their own office traffic, which upstream users expect to keep working.

### Unknown link and pixel IDs

`/api/send` with `payload.link` or `payload.pixel` for an ID that does not exist (or is deleted) is refused with 400 in the upstream error envelope, with the new messages `Link not found.` and `Pixel not found.` written in the style of `Website not found.`. Storing rows for sources nobody can read wastes budget and lets anyone fill storage under random UUIDs.

### Collection keys

Adopted 2026-10-06. Keys are created, listed and revoked per site with `site.configure` (`collectionKeys.*`, at most 20 active per site), shown once and stored as a SHA-256 digest. They are deleted with their site. An empty, unknown or revoked key refuses the whole request with upstream's 401 `Unauthorized` envelope, outcome `refused`, reason `invalid_key`. A valid key used for an item of another source refuses that item with the same 401 before the source is looked up, so the answer reveals nothing about it; other items of the batch are unaffected. Unkeyed requests are unchanged.

Adopted by Mike on 2026-10-15: the `collectionKeys.*` operations cannot be granted to API keys, creating a key needs no reauthentication, and keys survive `sites.transfer`. Granting them to API keys would let an integration mint its own collection credentials; the new owner of a transferred site may not expect the old owner's server to keep sending.

## Rejected alternatives

- 503 for link and pixel routes during outages: same lost hit, plus a broken link.
- Accepting into memory during an outage and flushing later: violates the confirmed durability rule.
- No body caps, like upstream: one request could consume the whole admission budget, and a 2 MB string costs parse time before truncation.
- Keeping upstream's unbound tokens: a token from any site would let a client write events for arbitrary UUIDs.

## Consequences

- The collection fixtures move these rows from Proposed to Adopted: link and pixel outage behavior, batch commit, duplicate body, invalid consent, body caps, token binding, source existence.
- Milestone 02 measures payload sizes and confirms or changes the caps before milestone 04 freezes them.
- Operations status (milestone 12) shows `refused_backpressure` and `lost_hits` as lost events.
- Milestone 04 implements every Adopted item here, with the caps above unchanged; `collection.status` shows the refusal counts and `lost_hits` is stored per link and pixel. Evidence: [m04 durable modern collection](../evidence/m04-durable-modern-collection.md).

## Affected contracts

[Collection compatibility](../contracts/collection-compatibility.md) "Preserved wire behavior and Holocron changes" and "Open questions" 1 to 6; collection fixtures; milestone 04 admission; [0003](0003-metric-formulas.md) Links and pixels.
