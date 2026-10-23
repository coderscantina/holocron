# Milestone 04 evidence: durable modern collection

Status on 2026-10-04: in progress. The unchanged pinned tracker collects through Rust in a real browser, every acknowledged event survived 20 SIGKILL rounds and all 8 transfer boundaries, and the 500 events/s burst passes its acknowledgement and visibility targets locally. Two items keep it open: full disk is tested only through the disk reserve and not through real ENOSPC, and the burst ran without report users and off the reference host. Mike confirmed the two refusal rules and the disk reserve flag on 2026-10-04. Decisions: [0005](../decisions/0005-collection-admission-policy.md) and [0008](../decisions/0008-storage-engine-and-budgets.md).

## Run

- Source: commit `a02861b` plus the uncommitted milestone 02 to 05 changes in this checkout. `Cargo.lock` sha256 `c3c119c533b0517aaffcbbd34a121dc6d5e5b4b9dee61c927bae53f40b26f01f`.
- Engines: DuckDB 1.5.6 (duckdb-rs 1.10506.0, bundled), SQLite 3.51.3 through SQLx 0.9.0, Tokio 1.53.2 with two workers, axum 0.8.9. Rust 1.99.0, release profile with line tables.
- Host: Apple M1 Pro, 10 cores, 32 GiB, macOS (Darwin 27.0.0), APFS SSD, native. No container and no reference host.
- Browser: Playwright 1.63.0, headless Chromium revision 1243, Node 24.21.0.
- Record format: `inbox_format` 2. Records now carry `ClientEventId {id, fingerprint}`, and a later copy of a claimed `eventId` returns `Outcome::Duplicate`. Storage migration `crates/storage/migrations/0002_event_id_fingerprint.sql`, server migration `apps/server/migrations/0002_collection.sql`.

Commands, from the repository root:

```sh
cargo test --release -p holocron-storage --features failpoints
cargo test --release -p holocron-server --test collection --test collection_fixtures --test crash -- --nocapture
cargo clippy --release -p holocron-server --all-targets
cargo run --release -p holocron-server --example collect_burst      # 500 events/s for 60 s, run three times

# Browser compatibility: a scratch data dir with the harness source IDs, then
cargo build --release -p holocron-server
target/release/holocron init --data-dir /tmp/h/data --admin-email admin@capture.test --admin-password-file /tmp/h/pw
# insert the four websites, link and pixel from tools/collection-harness/lib.mjs into control.sqlite
target/release/holocron serve --data-dir /tmp/h/data --bind 127.0.0.1:33400 --disk-reserve-mib 1024
UMAMI_URL=http://127.0.0.1:33400 node tools/collection-harness/capture.mjs --out docs/evidence/m04/browser-captures
```

## Compatibility

### Pinned scripts

`GET /script.js` and `GET /recorder.js` served by Holocron hash to `654f12c7...ac3f` and `21e63128...80c6`, byte-identical to the pinned Umami 3.4.0 builds. Headers: `application/javascript; charset=UTF-8`, `public, max-age=86400, must-revalidate`, `Access-Control-Allow-Origin: *`.

### Real browser

The capture harness from milestone 01 ran unchanged scenarios against Holocron instead of the Umami container. Three scenarios were added for surfaces the tracker does not drive itself: a page posting `/api/batch` cross-origin, a click on a `/q/{slug}` link, and an `<img>` for `/p/{slug}`. Captures: [`m04/browser-captures/`](m04/browser-captures/).

| Scenario | Requests | Result |
| --- | --- | --- |
| pageview-on-load | 1 | pass |
| spa-navigation | 2 | pass |
| custom-event-data-attribute | 2 | pass |
| track-api (name and data, payload object, function) | 4 | pass |
| identify | 3 | pass |
| distinct-id-attribute | 2 | pass |
| before-send-cancel | 0 | pass |
| before-send-consent-extension | 2 | pass |
| do-not-track-respected, do-not-track-without-attribute | 0, 1 | pass |
| domains-mismatch, domains-match | 0, 1 | pass |
| exclude-search-and-hash | 1 | pass |
| tag | 2 | pass |
| auto-track-off | 1 | pass |
| local-storage-disabled | 0 | pass |
| performance (10 s timer) | 2 | pass |
| bot-user-agent | 3 | pass: `{beep:"boop"}`, no token, recorder sends nothing |
| batch-from-page (2 valid, 1 invalid) | 1 | pass: size 3, processed 2, errors 1, invalid item at index 1, token returned |
| link-click | 1 | pass: 307 to `https://example.com/landing?from=holocron` |
| pixel-image | 1 | pass: 200 `image/gif` |
| recorder-disabled-site | 2 | pass |
| recorder-replay-and-heatmap | 2 | fail, expected: recording ingest closes in milestone 09; configuration answers `enabled:false` |

22 of 23 scenarios pass, 57 of 62 assertions. The 5 failed assertions are all in the recording scenario.

### Request fixtures

`collection_fixtures` replays every request fixture from milestone 01 through the real router, identity derivation and storage: 98 of 120 pass with Holocron's documented status, the upstream body and headers where the status is preserved, and the counted outcome. The other 22 (21 on the `record.*` surfaces and `recorder-config-enabled`) are deferred to milestone 09. One case keeps the upstream status with a different body on purpose: `payload.userAgent` is ignored from unauthenticated callers, so `send-event-bot-payload-user-agent` is stored instead of answered as a bot.

These include invalid sources, unknown websites, links and pixels, cross-site tokens, malformed and out-of-range timestamps, schema errors, the 64 KiB and 2 MiB body caps (413), blocked IPs (403), duplicates and the 501-item batch.

## Crash boundaries

### Transfer, storage crate

`cargo test --release -p holocron-storage --features failpoints`, rerun after the record format change: a child process is killed with SIGKILL at each boundary, 20 fixed seeds per boundary, and a second process checks the store (checks as in the [milestone 02 report](m02-storage-gate.md#durable-transfer)).

| Boundary | macOS, release |
| --- | --- |
| Before inbox commit | 20 of 20 |
| After SQLite commit, before response | 20 of 20 |
| After acknowledgement, before assignment | 20 of 20 |
| After stable assignment commit | 20 of 20 |
| During DuckDB writes, before commit | 20 of 20 |
| After DuckDB commit, before notification | 20 of 20 |
| After notification, before SQLite retirement | 20 of 20 |
| During SQLite retirement | 20 of 20 |

The focused storage tests also pass: covering watermark, duplicate `eventId`, whole-request refusal at a full inbox, slow read interrupted at the timeout, and a drained inbox giving its pages back through incremental vacuum.

### Server under HTTP load

`crash` starts the real `holocron` binary, runs 4 clients posting events over HTTP, and SIGKILLs the process at a seeded moment 50 to 430 ms into each of 20 rounds. After each round the next start resumes transfer. At the end every acknowledged event must be stored, none twice, and none that was never sent.

| Attempted | Acknowledged | Stored | Committed, not acknowledged | Lost | Doubled |
| --- | --- | --- | --- | --- | --- |
| 1,619 | 1,539 | 1,559 | 20 | 0 | 0 |

The 20 committed but unacknowledged events are the lost-response case: the commit finished, the kill cut the answer off. A client that retries such an event without an `eventId` stores it twice; with an `eventId` inside 48 hours the copy is a `duplicate`. Exactly-once browser delivery is not claimed, and the unchanged tracker never retries.

## Count reconciliation

Every stored event is counted once as `accepted`. After the browser run, `collection.status` ([`m04/browser-collection-status.json`](m04/browser-collection-status.json)) read:

| Route | Accepted | Suppressed | Refused |
| --- | --- | --- | --- |
| send | 26 (22 events, 3 identify, 1 performance) | 2 (bot) | 0 |
| batch | 2 | 0 | 1 (invalid item) |
| link | 1 | 0 | 0 |
| pixel | 1 | 0 | 0 |

30 accepted, watermark sequence 30, inbox empty: every accepted record reached analytics. The capture files show the same 26 send, 2 batch, 1 link and 1 pixel answers.

`stored_counts_reconcile_with_counted_outcomes` checks the same with storage queried directly: 20 pageviews, 3 copies of one `eventId` (1 stored, 2 `duplicate`), an event older than retention (`suppressed`, `outside_retention`), an invalid one (`refused`), a bot (`suppressed`) and a batch of 10. Stored rows equal `accepted` exactly.

## Backpressure

| Condition | Test | Result |
| --- | --- | --- |
| Disk headroom below the reserve | `storage_outage_refuses_collection_but_links_still_redirect` (reserve set above free space) | send and batch 503 with `Retry-After: 5`; `/q/go` still 307 and `lost_hits = 1`; inbox stays at 0 records |
| Locked inbox | `locked_inbox_is_refused_within_the_busy_timeout` (another connection holds `BEGIN EXCLUSIVE`) | 503 in under 10 s; accepted again after the lock is released |
| Inbox backlog full, transfer stalled | `full_inbox_backlog_answers_429_and_stops_growing` (2,000-byte budget) | 429 once full; inbox records equal the acknowledged count and bytes stay at or under 2,000 |
| In-flight body budget | `in_flight_body_budget_refuses_instead_of_buffering` (100 KiB budget, a slow client holds 60 KiB) | a second 60 KiB request gets 429 without being read; budget released after the slow request ends |

Memory is bounded by construction: request bodies by the 32 MiB in-flight budget and the per-route caps, pending records by the 1 GiB inbox budget on disk, admissions by the 64 MiB queue. No memory measurement was taken while saturated.

## Load trace

Open-loop generator, 500 events/s for 60 s over real HTTP to an in-process server with real SQLite and DuckDB, three runs. Visibility is probed every 100 ms with batched lookups; an event is visible once a snapshot returns its row with a watermark covering it. Summaries: [`m04/burst-run-1.json`](m04/burst-run-1.json), [`-2`](m04/burst-run-2.json), [`-3`](m04/burst-run-3.json).

| Run | Offered | 200 | Refused | Ack p50 / p95 / p99 / max (ms) | Ack to visible p50 / p95 / p99 / max (ms) | Send to visible p95 (ms) | Peak RSS |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | 30,000 | 30,000 | 0 | 11.8 / 21.3 / 28.5 / 52.2 | 70 / 117 / 126 / 300 | 131 | 113 MiB |
| 2 | 30,000 | 30,000 | 0 | 10.8 / 17.2 / 23.1 / 45.1 | 74 / 122 / 131 / 304 | 133 | 113 MiB |
| 3 | 30,000 | 30,000 | 0 | 14.6 / 64.0 / 114.6 / 204.3 | 79 / 132 / 164 / 306 | 175 | 121 MiB |

Targets: acknowledgement p95 250 ms, visibility p95 2 s. All three runs pass locally with every event visible. Probes: 574 to 584 per run, 1.5 to 2.9 ms mean. Inbox peak 94 to 108 records (17 to 20 KB). These runs come from the previous session of this milestone; I did not rerun them.

## Lost responses

The tracker and recorder never retry and never read status codes. A 429 or 503 loses that event on the client, and the refusal is counted per route and reason. A response cut off after commit leaves a stored event the client cannot know about (20 cases in the crash run). Product documentation must say both in plain words ([0005](../decisions/0005-collection-admission-policy.md)).

## Not verified

- **Power loss.** Not tested. Process kills do not test it.
- **Reference host.** All numbers are from a laptop, generator and server in one process, no network between them.
- **Burst with two report users.** The performance contract measures the burst next to two report users. These runs had none. Milestone 02 measured that mix in process: acknowledgement p95 89.5 ms, visibility p95 813 ms.
- **Sample size.** The contract asks for 100,000 acknowledged events per warm phase; each run had 30,000.
- **Real ENOSPC.** Full disk is covered by the reserve check, which refuses before the disk fills. ENOSPC during a SQLite commit, DuckDB transfer or spill was not injected.
- **Recording.** `/api/record` ingest, replay and heatmap storage, and the enabled recorder configuration are deferred to milestone 09 (22 request fixtures, 1 browser scenario).
- **Outcome counters** are saved every 10 seconds when they changed and at a clean stop, and reloaded at start (`saved_counts_survive_a_restart`, 2026-10-06). `since` is the first start with migration 0010; a crash loses at most the last 10 seconds of counts.
- **`payload.ip` and `payload.userAgent`** are honored only with a site collection key (`collection_keys_make_payload_ip_and_user_agent_the_visitor`, 2026-10-06, [0001](../decisions/0001-identity-and-consent.md)).
- **`sites.trackingCode`** and the tracking settings tab exist since 2026-10-06.
- **User agent classifier coverage.** Browser, OS and device come from a small hand-written classifier in `apps/server/src/collect/client.rs`, checked only against the fixture and capture user agents.
- **Bot list.** Bots are detected with the `isbot` Rust crate 0.1.3, not upstream's npm `isbot` 5.2.2. The two lists were not compared; the fixture bots and `HeadlessChrome` match.

## Changed behavior

- The web fallback from milestone 03 now answers 404 for unknown paths with a file extension instead of serving the app shell, so an unconfigured script name gets a 404 as upstream does (fixture `script-unknown-name`; `web.rs` covers a missing asset with a web app present).
- `--disk-reserve-mib` (default 4096) sets the free space below which collection answers 503.
- Two Tokio workers by default (`--worker-threads`) and `M_ARENA_MAX=2` on glibc, as 0008 sets.

## Open items

- Settled by Mike on 2026-10-04 in [0005](../decisions/0005-collection-admission-policy.md) and [0008](../decisions/0008-storage-engine-and-budgets.md): refusing timestamps the engines cannot store, storing events keyless once `end(D) + 25h` has passed even before the destruction job runs, and the `--disk-reserve-mib` flag.
- For milestone 09: recording ingest and the enabled recorder configuration.
- For milestone 12: real ENOSPC injection, persistent outcome counters, `client_event_ids` pruning.
- For milestone 13: the burst with two report users on the reference host, 100,000 events per phase, power loss.
