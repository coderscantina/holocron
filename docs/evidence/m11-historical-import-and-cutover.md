# Milestone 11 evidence: historical import and cutover

Status on 2026-10-05: built and tested locally. Every acceptance criterion has a passing check. The milestone stays in progress until Mike decides the Proposed items of [0013](../decisions/0013-historical-import.md); each could change tested behaviour.

Rules are in [0013](../decisions/0013-historical-import.md). The operator runbook is [import and cutover](../operations/import-and-cutover.md).

## Run

- Source: commit `63e76d5` plus the uncommitted milestone 11 changes in this worktree.
- Host: Apple M1 Pro, 10 cores, 32 GiB, macOS (Darwin 27.0.0), OrbStack Docker. Debug build. Chromium 1243 from Playwright for the screenshots.
- Source engines: `postgres:15-alpine` and `mysql:8.0` pinned by digest, Umami `3.4.0` and `mysql-v2.20.2` images pinned by digest, all in `tools/import-rehearsal/compose.yaml` (project `holocron-m11`, ports on 127.0.0.1 only).

Commands, from the repository root:

```sh
cargo test -p holocron-server --lib --test imports            # inspection rules, mapping, chunk ledger
tools/import-rehearsal/test.sh --nocapture                     # the 5 database tests in tests/imports.rs
tools/import-rehearsal/rehearse.sh                             # cutover and rollback, both engines
cargo test -p holocron-server                                  # every server suite, for the shared edits
cargo clippy -p holocron-server -p holocron-storage -p holocron-analytics -p holocron-protocol --all-targets
cargo fmt --check -p holocron-server -p holocron-storage -p holocron-analytics -p holocron-protocol
cargo run -p holocron-protocol --bin export-types -- --check
(cd apps/web && bun run typecheck && bun run lint && bun run test)
```

## What was built

| Work item | Where | Behaviour |
| --- | --- | --- |
| 1 inspection | `imports/{signature,source}.rs` | `holocron import inspect` reads `_prisma_migrations` and the column catalog through a read-only session and compares them with the fixture snapshots embedded in the binary. Ten refusal codes with fix-it messages, a write-credential refusal, and a preview of rows, retention skips and source size |
| 2 snapshot import | `imports/mod.rs`, `0008_imports.sql` | `holocron import run` into a directory made by `holocron init`, in one repeatable-read read-only transaction, clock pinned at the first start, fingerprint checked on resume. Website, user, team, link, pixel, segment, report, board, annotation, share and event IDs stay the source UUIDs |
| 3 mapping | `imports/{control,definitions,events,replays}.rs` | Users (bcrypt kept until first login), teams and roles, personal workspaces, sites with session and recording policies, links, pixels, 2FA (decrypted with the operator's key), installation and team 2FA requirements, segments, funnel, journey, retention, goal and attribution reports, boards, annotations, events with properties, revenue and web vitals, identify data, heatmaps, replays with saved flags. Public shares become restricted shares whose token is the old slug |
| 4 inventory | `imports/state.rs`, `import_inventory` | Every object not imported, or imported with a loss, kept with its source row (credentials redacted) and a reason code |
| 5 recomputation | `imports/mod.rs`, `analytics/freshness.rs` | Sessions and aggregates recomputed by correction after the last chunk. Imported visitors are Umami session IDs per day and never persistent profiles. `freshness.imported` marks results that reach imported history; the dashboard shows it |
| 6 checkpoints | `storage/{inbox,lib}.rs`, `0003_import_chunks.sql` | Chunks of `--chunk-rows` admitted through the inbox under a key with a ledger row in the same transaction, then checkpointed. Imports wait for the inbox budget and stop at the disk reserve |
| 7 reconciliation | `imports/reconcile.rs` | Manifest with per-site counts by kind and time range, properties, identify, heatmaps, replay parts, revenue per currency, configuration objects, logins, 50 sampled events, inventory counts and destination digests |
| 8 cutover | `docs/operations/import-and-cutover.md`, `tools/import-rehearsal/` | Runbook with user-facing changes, cutover and rollback, rehearsed with real Umami deployments |

Shared edits, each small: `main.rs` (`import` subcommand, `serve` refuses an unfinished import), `lib.rs`, `collect/mod.rs` (two modules visible to the crate), `identity/{mod,auth}.rs` (bcrypt verify and rehash at login), `http/web.rs` (share paths with dots serve the app), `sites/analytics.rs` and `analytics/{freshness,explore/mod,lib}.rs` (imported history in freshness), `protocol/operations/analytics.rs` (`Freshness.imported`, regenerated TypeScript and OpenAPI), `storage/{lib,inbox,analytics}.rs` (chunk ledger, `import_provenance`, `imported_history`), `apps/web` (`FreshnessNote.vue`, two locale strings each, one redirect route).

## Acceptance criteria

| Criterion | Result | Evidence |
| --- | --- | --- |
| Both exact snapshots import and malformed layouts refuse safely | Pass. Both snapshots import with `status: reconciled`. All ten refusal fixtures load into real PostgreSQL and MySQL databases and refuse with their expected code and details, as do the pure rule tests. Exit code 3 | `both_snapshots_…`, `every_fixture_layout_…`, `signature.rs` tests |
| Sources unchanged, importer read only | Pass. Content checksums of every table are equal before and after (md5 of all rows on PostgreSQL, `CHECKSUM TABLE` on MySQL). The importer refuses admin credentials before reading any data, in `inspect` and `run`. The rehearsal checks the same with its own checksums | `both_snapshots_…`, `every_fixture_layout_…`, `rehearsal.log` |
| Interrupt, resume and retry yield the same destination without duplicates | Pass. 6,000 generated events and 400 sessions with 150-row chunks; the import was killed with SIGKILL 7 times, at 7, 14, 21, 28, 35, 43 and 51 committed chunks, and resumed each time. Its manifest (status, digests of events, properties, identify data, sessions, heatmaps and provenance, per-site counts, configuration, inventory, samples, step counters) equals the uninterrupted import's. A retry of the finished import returns the same manifest. A chunk admitted twice across a crash is stored once | `killed_imports_…`, `an_import_chunk_retried_…` |
| Public share URLs keep working as restricted shares | Pass. `Authorization: Share sample-share` resolves the 3.x share and the 2.x `website.share_id`; `/share/sample-share/overview` and `/share/sample-share/example.test` serve the app, and in Chromium both land on the share view. Unknown slugs are 404 | `imported_public_shares_…`, screenshots, `rehearsal.log` |
| Stable website IDs; accounts behave as documented with fresh sessions and keys and explicit 2FA | Pass. Site, link and pixel IDs are the source UUIDs and the live tracker snippet with the same website ID collects in Holocron after cutover. A bcrypt account signs in with its Umami password and its hash becomes argon2; a TOTP secret encrypted with Umami's key works after import; a secret that does not decrypt makes 2FA required at first login. No session or API key comes over. Umami's own seeded `admin` (renamed) signs in with `umami` in the rehearsal | `converted_accounts_…`, `rehearsal.log` |
| Supported reports use new formulas and show historical limitations | Pass. Imported reports are planner-checked states; an exploration over imported history returns `freshness.imported` with the source, and the share page shows "Includes Umami history through Oct 5, 2026" with the limitation text. Sessions are recomputed from event times: in the PostgreSQL rehearsal, site 201 has 2 sessions from its 4 events, the sample visit and the live pageview | `imported_public_shares_…`, `definitions.rs` tests, screenshots |
| Unmapped representations inventoried and preserved | Pass. Every configuration table reconciles to imported or inventoried rows with no unaccounted row. The PostgreSQL rehearsal inventories 14 reasons: API key, setting, board cards, report losses, session link, unreadable replay, saved replay without parts, guessable slug, team access code, member delete rights, 2FA reenrollment, recovery codes and install admin analytics access | manifests in `rehearsal.log` |
| Cutover and rollback rehearsed with real scripts and isolated deployments | Pass for both engines: live Umami collects, stops, read-only user, inspect, import, source unchanged, Holocron logs in, answers the old share URL and collects for the same website ID, then Holocron stops, Umami starts on the untouched source and collects again | `rehearse.sh`, [`m11/rehearsal.log`](m11/rehearsal.log) |

## Rehearsal outcomes

| | PostgreSQL 3.4.0 | MySQL 2.20.2 |
| --- | --- | --- |
| Source events at the final snapshot | 5 (4 sample, 1 live) | 4 (3 sample from 2024, 1 live) |
| Imported / outside retention | 5 / 0 | 1 / 3 |
| Sessions after recomputation | 3 | 1 |
| Revenue EUR, source table and destination | 4990 and 4990 minor units | outside retention |
| Status | reconciled | reconciled |
| Source events after rollback and one more Umami pageview | 6 | 5 |

## Screenshots

The imported share at its old 3.x URL, after a live-collected Umami 3.4.0 source was imported: [desktop light](m11/share-imported-desktop-light.png), [desktop dark](m11/share-imported-desktop-dark.png), [phone light](m11/share-imported-phone-light.png). The freshness line carries "Includes Umami history through Oct 5, 2026"; its title and screen-reader text give the limitations.

## Not verified

- Scale. The largest import was 6,000 events. Chunk memory is bounded by design, but throughput, the time `count(*)` fingerprints take on 100M-row tables, and the reconciliation's property digest join at the 120M-event workload are not measured.
- TLS to the source. The drivers are built without TLS; the runbook says to import on the database host or through a tunnel.
- Umami sources with real board layouts, cohorts and the full range of report parameters. The fixtures hold one of each simple kind.
- Phone and dark screenshots of other surfaces that show freshness; only the share page was inspected.
- `cargo test -p holocron-storage` alone was not run: on this host it needs a second 3 GB DuckDB build that did not fit on the disk at the time. The chunk ledger test lives in `apps/server/tests/imports.rs` instead, and the storage crate compiles and passes clippy within the workspace.

## Open points for Mike

Decided by Mike on 2026-10-06: the Proposed items of 0013 are adopted. One `--timezone` for every site stays, and imported reports stay personal saved explorations.

- The Proposed items of 0013: imported visitors as Umami session IDs per day; bcrypt kept until the first login; the `umami.invalid` login domain; shares expiring 366 days after import.
- One `--timezone` for every imported site. Umami stores no site timezone; per-site timezones would need a mapping file.
- Imported reports are personal saved explorations. Making them workspace explorations would keep their upstream visibility.

## Handoff

Milestone 12 can treat these as the import's interface:

- `holocron_server::imports::state(&Db) -> ImportState` is `None`, `Running { source, started_at }` or `Complete { … }`. `Running` covers a live run and an interrupted one; `serve` and every operator job (backup, retention, erasure, diagnostics) refuse then with `import_in_progress`, and `import run` holds the data-directory lock (wired when merging milestone 12, tested by `an_unfinished_import_refuses_operator_jobs_until_it_resumes`).
- Tables: control `import_runs`, `import_steps`, `import_inventory`; inbox `import_chunks`; DuckDB `import_provenance` (restricted, per event: source session, visit, distinct ID) and `imported_history`. Visitor erasure must reach `import_provenance` and the `session_link` rows of `import_inventory`.
- A finished destination for backup and restore checks: run `tools/import-rehearsal/rehearse.sh` with a breakpoint after "cutover 4", or the test helpers in `apps/server/tests/imports.rs`.
