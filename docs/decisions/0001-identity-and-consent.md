# 0001: Identity and consent

Status: Adopted (milestone 01, team decision) unless an item says otherwise. Items marked Confirmed restate [the briefing](../technical-briefing.md). Items marked Confirmed (Mike, 2026-10-03) were decided by Mike on that date.

## Problem

Collection, sessions, retention, and exports all need one rule for what a visitor identity is, how it is derived, how long the inputs live, and when a visitor becomes a consented persistent profile. Without this record each module would invent its own.

## Constraints

Confirmed by the briefing:

- Anonymous identity is cookieless, site scoped, keyed, and rotates daily in the website's timezone.
- Raw identity inputs stay transient and out of logs. Visitor counts are pseudonyms, not people.
- `identify(id, data)` uses the daily scope by default. Persistent identity needs the site to enable it and a consent signal. Event properties never establish consent.
- Confirmed (Mike, 2026-10-03): live events may carry any timestamp, as upstream. Day keys are still destroyed at `end(D) + 25h`; key retention is not extended for late events.
- Unchanged Umami 3.4.0 trackers must keep working.

## Evidence

Upstream (`.sources/umami-3.4.0`):

- `src/app/api/send/route.ts`: `sessionId = uuid(sourceId, ip, userAgent, salt, distinctId)`. The salt is `hash(startOfMonth(createdAt))` by default (`SALT_ROTATION`, day or week optional) and the secret is `APP_SECRET || DATABASE_URL`. Inputs are site, IP, user agent, period salt, and the raw `payload.id`. `payload.ip` and `payload.userAgent` override headers for any caller. The raw `distinct_id` is stored on sessions and events (`FIELD_LENGTH.distinctId = 50`).
- `src/lib/ip.ts`: IP comes from a header list (`cf-connecting-ip`, `x-real-ip`, `x-forwarded-for` first entry, `forwarded`, and more) without checking whether the peer is a trusted proxy. `CLIENT_IP_HEADER` can pin one header.
- `src/lib/detect.ts`: geo comes from CDN headers (`cf-ipcountry`, `x-vercel-ip-country`, `cloudfront-viewer-country`, `eo-ipcountry`) when present, else MaxMind. Country is 2 chars, region `CC-RR`, city text.
- `src/tracker/index.ts`: every payload carries `website, screen, language, title, hostname, url, referrer, tag, id`. `identify(id, data)` sends `type: "identify"` with `data` and clears the cache token. A `data-before-send` callback may change the payload before transmission. Payload `hostname` is `location.hostname`. Zod `z.object` strips unknown payload keys upstream, so an extra key is harmless to an unchanged server.

## Choice

### Transient inputs

Anonymous identity inputs are exactly: site ID, client IP address, and the `User-Agent` request header. Nothing else.

- Hostname is a dimension, not an identity input. A visitor moving between `www.example.com` and `shop.example.com` on one site stays one pseudonym.
- `payload.ip` and `payload.userAgent` are honored only on requests authenticated with a site collection key (server-side integrations). Unauthenticated requests use the connection. This tightens upstream; it does not change what the browser tracker sends.
- Inputs live in request memory only. They are never written to SQLite, DuckDB, logs, or metrics labels. Error reports hash or omit them.

Adopted 2026-10-06 (collection keys): a site collection key travels as `Authorization: Bearer <secret>` on `/api/send` and `/api/batch`; other schemes are not keys. With a valid key, an item for the key's site takes its visitor from `payload.ip` and `payload.userAgent` only, each empty when absent, never from the connection or its headers, and gets no location. A malformed `payload.ip` is a 400. Transport 2 of Persistent consent transport is this request carrying `id` and `consent`.

### Keyed derivation

Construction: BLAKE3 keyed hashing (the `blake3` crate, keyed mode and `derive_key`).

- Root secret: 32 random bytes created at initialization, stored in SQLite control storage, included in backups. Never derived from a connection string.
- Site persistent key: `derive_key("holocron/v1/site-persistent/<site_uuid>", root)`. Deterministic, used only for consented persistent profiles.
- Day key: 32 random bytes per `(site, timezone_epoch, local_day)`, generated on first use, stored in SQLite `identity_day_keys(site_id, timezone_epoch, local_day, key, created_at, destroyed_at)`. Random rather than derived so that destroying the key makes that day's pseudonyms unlinkable even to someone holding the root secret. `timezone_epoch` starts at 1 and increments with each site timezone change (see "Timezone change" below).
- Link and pixel sources have no timezone setting. Their day keys use UTC days under the same rules, keyed by the link or pixel ID. UTC avoids inventing a timezone setting that upstream does not have.
- Anonymous daily pseudonym: `keyed_hash(day_key, 0x00 || ip || 0x00 || user_agent)` truncated to 128 bits. IP is the canonical textual form (IPv4-mapped IPv6 unwrapped, port stripped). Empty inputs stay empty strings; the derivation still runs.
- Daily identified pseudonym: `keyed_hash(day_key, 0x01 || distinct_id)` truncated to 128 bits. It does not include IP or user agent, so one person on two devices is one daily pseudonym after `identify`. The raw `distinct_id` is never stored in daily mode.
- Persistent profile ID: `keyed_hash(site_persistent_key, distinct_id)` truncated to 128 bits. Separate namespace; never equal to a daily pseudonym by construction (different keys).

A pseudonym is stored as a 16-byte value, rendered as a UUID in the API.

### Key retention and keyless events

Day keys are destroyed by the maintenance job once `now >= end(D) + 25h` (one hour of slack for clock drift). Destruction overwrites `key` with NULL and sets `destroyed_at`; the row stays as a tombstone until the day leaves the 12-month analytical retention ([0006](0006-retention-clock-and-operations.md)). The import path does not use day keys.

Adopted (consequence of Mike's 2026-10-03 decision to accept any timestamp). One rule decides whether an event gets a daily pseudonym for its identity day D:

- D's row exists with a key: use it.
- No row exists and `now < end(D) + 25h`: create the key on demand. This covers today, yesterday's late events, and future event times.
- Otherwise (a tombstone, or a day already past its destruction time that never had a key): the event is **keyless**. It is stored with no daily pseudonym, and no key is ever created for D afterwards.
- Confirmed (Mike, 2026-10-04, milestone 04): an event for a day already past `end(D) + 25h` is keyless even while D's key row still holds a key, because the destruction job runs every 15 minutes. The literal rule above would use the key until the job runs. Collection uses the destruction instant as the boundary, so the outcome does not depend on job timing. This is stricter than the rule as written.

Keyless events are counted exactly in measures that do not need an identity: pageviews, events, revenue, performance, property statistics, link and pixel hits. They are excluded from visitors, visitor-days, sessions and every session measure, realtime, daily-scope funnels, session journeys, daily attribution and session lists. Their session fields come from their own request, like other events without a session (0004 Field scope). Reports show this as identity coverage ([0003](0003-metric-formulas.md)). Extending key retention was rejected, because a longer-lived key lets an operator re-link known IP and browser pairs for longer.

Consented persistent profiles have no expiring key, so the rule for them is: an event links to the profile when consent is active at its event time, `granted_at <= event_time < revoked_at`, whatever its age. A linked keyless event therefore counts in person measures and in profile sessions, and not in visitor-days. Comparing against event time keeps "events before the grant stay daily" true for late arrivals, so a delayed event never backfills a profile.

Day keys are in backups because they are in SQLite. A restored backup runs the same destruction job on startup. Day keys never appear in exports or report data.

### Proxy trust and headers

Configuration: `collect.trusted_proxies` (list of CIDRs, default empty) and `collect.client_ip_header` (default `x-forwarded-for`).

- Peer not in `trusted_proxies`: client IP is the peer address; all forwarding and geo headers are ignored.
- Peer in `trusted_proxies`: read `client_ip_header`. For `x-forwarded-for` and `forwarded`, scan the list right to left and take the first address not in `trusted_proxies`. Other headers are single-valued.
- Geo headers (`cf-ipcountry`, `x-vercel-ip-country`, `cloudfront-viewer-country`, `eo-ipcountry` and their region/city companions) follow the same trust rule, then MaxMind if a database is configured, else no location. Latitude and longitude headers follow the same rule but never fall back to MaxMind ([0017](0017-visitor-coordinates.md)). Proposed amendment ([0018](0018-geoip-fallback.md)): without a matching trusted country header, a MaxMind DB City database looks up the client address this rule picks, coordinates included.

Upstream's "first header that exists" lookup is not adopted; an untrusted client could choose its own IP and pseudonym.

### Hostname normalization

Stored hostname: payload `hostname`, else the host of `url`. Normalize to lowercase, IDNA ASCII (punycode), strip port and trailing dot, truncate at 100 characters. A leading `www.` is kept in the stored value. Self-referral and site-domain comparisons strip `www.` from both sides, matching upstream.

### Missing inputs

- Missing `User-Agent`: empty string. Bot detection (isbot) runs first and suppresses; suppression is a distinct outcome, not a stored event.
- Missing IP cannot happen for a connection; for authenticated server calls without `payload.ip` it is the empty string.
- Low-entropy inputs collapse into one pseudonym per site and day. Reports do not try to split them.

### Local date

Local day = calendar date of the event time (not receive time) in the site's IANA timezone. Day start is the first instant whose local date is D; the timezone library resolves DST gaps and overlaps. Untimestamped payloads use receive time as event time (upstream behavior).

### Timezone change

A site timezone change acts as an identity rotation at its effective instant, like midnight. Events before the instant use the old timezone and `timezone_epoch`; events at or after it use the new ones, so the same local date under the old and the new timezone has two different day keys and two different pseudonyms. Each key is destroyed at `end(D) + 25h` with `end(D)` computed in its own epoch's timezone; for the last old-epoch day that is the effective instant. A visitor active on both sides of the change counts as two identity-days, exactly as a visitor active on both sides of midnight does. Reusing one key per calendar date was rejected because a change that moves the local date backwards would merge yesterday's visitors with today's.

### Daily identity of every event

Every eligible event stores a daily pseudonym, also when it is linked to a persistent profile: the daily identified pseudonym when the payload carries an `id`, else the anonymous one. A linked event additionally stores its profile ID. The `id` is in request memory for both derivations, so nothing extra is stored.

- Daily measures (visitors, visitor-days, realtime, daily-scope funnels, anonymous attribution) use the daily pseudonym for every event.
- Person measures (identified people, person-scope goals, funnels, journeys and attribution, retention) use the profile ID and only linked events.
- Session derivation uses the profile ID for linked events and the daily pseudonym otherwise (0002).

So one `id` on one day is one visitor-day, whether its events were linked, unlinked, or both (for example, before and after a same-day revocation). Granting consent never changes an anonymous metric. Counting the profile per day was rejected because a same-day revocation would then count one person twice.

### identify(id, data)

- `id` is trimmed and truncated to 50 characters, then hashed. An empty `id` after trimming is treated as absent.
- Events on the same day that carry the `id` use the daily identified pseudonym. Earlier events without it keep the anonymous pseudonym. No retroactive merge.
- `data` becomes typed session properties under the rules in [0002](0002-sessions-and-events.md). An `identify` without `id` attaches properties to the current anonymous pseudonym.
- `identify` never creates or extends a session (see 0002).

### Persistent consent transport

Client extension: a top-level payload key `consent` with the string value `"persistent"` or `"none"`, sent alongside an `id`. Unchanged trackers never send it. Supported ways to send it:

1. Browser: the `data-before-send` callback adds `payload.consent = "persistent"` after the user consents, then calls `umami.identify(id)`. An unchanged upstream server strips the key and ignores it.
2. Trusted server integration: an authenticated `POST /api/send` with `type: "identify"`, `payload.id`, and `payload.consent`, using a site collection key.

Server rules:

- `consent` is honored only when the site has `persistent_identity_enabled = true` and the payload carries an `id`. Otherwise it is ignored and counted in `consent_ignored`.
- Any value other than the strings `"persistent"` and `"none"` is ignored the same way and counted in `consent_ignored`; the rest of the payload is accepted. A 400 would drop the whole identify in an unchanged tracker that cannot retry, for a field the operator's own callback wrote.
- Confirmed (Mike, 2026-10-03): consent asserted by the client, through the site's own page or a trusted server call, is accepted without a server-signed token.
- `"persistent"` creates or refreshes `consents(site_id, profile_id, granted_at, revoked_at = null)`. From that instant, events carrying the same `id` link to the persistent profile (by event time, see Key retention and keyless events). Events before the grant stay daily pseudonyms; there is no implicit backfill.
- `consent` inside `data` or inside event properties is a plain property and does nothing.

### Revocation and returning to anonymous

- `"none"` with an `id` sets `revoked_at` on the open consent row, closes any open identified session at that instant, and returns following events to daily mode.
- Disabling `persistent_identity_enabled` for a site acts as revocation for all profiles at the effective instant and bumps the site's identity policy version.
- Confirmed (Mike, 2026-10-03): revocation stops linking new events. Facts recorded while consent was active remain attributed to the profile and keep counting in cohorts for periods up to `revoked_at`. Deleting a profile's history is the separate, explicit maintenance erasure command.
- Adopted (milestone 05): collection sends each revocation, and each site-wide disable (no profile), through the durable inbox as a revocation record with its instant. Analytics closes open profile sessions there, also when a new grant follows within the inactivity window.

Adopted (milestone 05), where the rules above are silent:

- The signal counts on any payload type that carries an `id`, identify or event, since the transport only requires it "alongside an id".
- Grant and revocation instants are the payload's event time, the same clock linking uses. A grant while an interval is open keeps the open one. A revocation closes the open interval at its instant, or at its grant when the grant is later.
- Every honored `"none"` sends a revocation record, also when no interval is open. Consent state is written while the payload is processed, before the request's durable commit. A refused request therefore keeps its consent change, the privacy-safe direction for a revocation, and a client that retries sends the revocation record again.
- An event links only while the site has persistent identity on. After a site-wide disable, a late event whose time falls inside an old consent interval stays daily. That is stricter than the per-profile rule, because the operator turned the feature off.
- Turning persistent identity off closes every open interval at the change instant and admits the site-wide revocation record before the command commits. A refused admission fails the command, so control and analytics agree. A commit that fails after the admission leaves a revocation instant without a disable; it only closes open profile sessions there.

### Identified sessions and midnight

Consented persistent sessions do cross local midnight and use only the inactivity window. That is the purpose of the mode. Anonymous and daily-identified sessions close at midnight (0002).

### Privacy wording

Product documentation says: "Anonymous visitors are counted with a keyed pseudonym built from the IP address and browser signature. The key changes every day per website and is deleted about 25 hours after the day ends. While the key exists, an operator holding the database could check whether a known IP and browser produced a given pseudonym; afterwards they cannot. Pseudonyms are not people: shared networks merge visitors and device switches split them. Persistent profiles exist only when a site enables them and the site has recorded your consent, and are keyed separately."

## Rejected alternatives

- HMAC-SHA256: equally sound; BLAKE3 gives one primitive for keyed hashing and key derivation and is faster. Not a close call worth revisiting.
- Deriving day keys from the root secret: simpler storage, but the operator can then re-link any past day forever. Random stored keys make destruction meaningful.
- Including hostname in identity inputs: splits one visitor across subdomains for no privacy gain.
- Honoring `payload.ip` from any caller (upstream): lets any client forge location and pseudonym.
- Consent as a session property or `identify` data key: contradicts the confirmed rule that properties never establish consent.
- Signed consent tokens issued by the server: more machinery; the site operator already controls the page and bears the consent duty.

## Consequences

- `identity_day_keys` table with tombstones and a destruction job land in milestone 02 storage work.
- `identity_day_closed` no longer exists as a refusal. A late event for a destroyed day is stored keyless; the correctness fixture needs such an event (more than `end(D) + 25h` after its day) to check exact counts and identity coverage.
- Every identity-based result carries identity coverage.
- Linked events carry both a daily pseudonym and a profile ID; the storage schema needs both columns.
- Compatibility matrix must record the `payload.ip`/`userAgent` trust change and the `consent` payload key.
- Imported Umami history cannot be rekeyed; it keeps source identifiers as restricted provenance.

## Affected contracts

[analytics-contract.md](../analytics-contract.md) "Identity populations"; collection compatibility fixtures (work item 2); storage migrations (milestone 02); maintenance erasure (later milestone).
