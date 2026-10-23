# Milestone 01 evidence: contracts and fixtures

Status on 2026-10-03: complete. Every acceptance criterion passed. Mike settled the open policy decisions on 2026-10-03, and they are recorded as Confirmed (Mike, 2026-10-03) in the decision records.

This milestone owns contracts and fixtures. It claims nothing about backend correctness or performance. No Holocron server exists yet, so every `expected_holocron` value is a contract, not a measured result.

## Acceptance criteria

| Criterion | Result | Evidence |
| --- | --- | --- |
| A new agent can obtain the same source and scripts without Mike's paths | Passed. Werkstatt is described in prose and not fetched | [Source manifest](../contracts/source-manifest.md), `sources/manifest.json`, `scripts/fetch-sources.sh` |
| Every public collection surface has a positive and negative fixture, including real script behavior | Passed | [Collection compatibility](../contracts/collection-compatibility.md), [collection validation](m01-collection-validation.md) |
| Confirmed and proposed decisions are labeled; no dependent module invents identity, consent, metric, or dedup policy | Passed, no open items | [Decision records 0001 to 0007](../decisions/) |
| The capability matrix accounts for the full release scope | Passed | [Capability matrix](../contracts/capability-matrix.md) |
| Expected small-fixture totals include a written explanation independent of future SQL | Passed | [Correctness fixture](../../fixtures/correctness/README.md) |
| Query suite and workload manifest are fixed before storage tuning | Passed | [Workload and query suite](../contracts/workload-and-query-suite.md) |

## Deliverables

- **Sources.** Umami 3.4.0 (`ec0ff503`), Umami 2.20.2 (`3eb2e890`), Kit (`f447e797`), Folio (`219e9aaa`) are pinned with licenses, lockfile digests and hashes for the files the docs reference. Werkstatt is not distributable and is described in `docs/contracts/architecture-patterns.md`. The shipped `script.js` (4773 bytes) and `recorder.js` (190816 bytes) from image `ghcr.io/umami-software/umami@sha256:85909afc…` are stored in `sources/umami-3.4.0/`. A source build at the pinned commit is byte-identical.
- **Collection.** There are 120 request fixtures across 13 surfaces, each with at least one positive and one negative case, and 20 real-browser captures of the unchanged tracker and recorder. The replay harness is in `tools/collection-harness/`.
- **Decisions.** Seven records cover identity and consent, sessions and events, metric formulas, exploration state and limits, collection admission, retention/clock/operations, and access. Each choice is labeled Confirmed (briefing), Confirmed (Mike, 2026-10-03), or Adopted (milestone 01 team decision). Live events are accepted with any event time. An event whose identity day key is already destroyed is counted exactly but has no daily identity, and reports show identity coverage.
- **Capability matrix.** It lists 101 capabilities, each mapped to a closing milestone, screen, operations and fixture. It names the excluded upstream features and the briefing sentence that excludes each one.
- **Correctness fixture.** It has 166 input records over two sites (Europe/Vienna, and America/New_York switching to UTC) and 13 expected-result files, with a prose derivation for every number. It includes keyless, outside-retention and far-future events.
- **Workload.** The frozen generator contract covers the PRNG (SplitMix64 with test vectors), seeds, sites, distributions, the 48 property keys, revenue, replay, heatmaps, burst, soak, correction, retention advance and the cap test. Manifest version 3 adds a keyless-identity probe and a saved-replay sub-cap case. There are 80 query cases (Q01 to Q14), a 25-case early-gate subset and two dashboard streams. Counts that only generation can fix, such as hashes, matched records and golden results, are left to milestone 02.
- **Access.** There are 411 authorization cases and 27 scenarios, plus an Umami role mapping with seven documented lossy points.
- **Gaps.** Fixtures cover channels, the weekday grid, the realtime window, link/pixel totals, campaign normalization, segments, saved replays, init, schema upgrade, erasure and diagnostics.
- **Import schemas.** PostgreSQL 3.4.0 (26 migrations) and MySQL 2.20.2 (13 migrations) were captured from real databases migrated by the official images, with synthetic sample data. There are also nine refusal cases with operator guidance.

## Checks run

| Check | Command | Outcome |
| --- | --- | --- |
| Upstream collection replay | `node tools/collection-harness/replay.mjs`, see [collection validation](m01-collection-validation.md) | 112 of 112 default, 2 of 2 collect-alias |
| Real script capture | `node tools/collection-harness/capture.mjs`, run twice | 20 scenarios pass; the two runs gave identical output |
| Import sample loading | `tools/import-schemas/verify-samples.sh` | Both samples load into fresh containers |
| Correctness oracle | Throwaway independent Python checker, deleted after use | 934 checks, 0 failures; the checker also caught all 4 errors planted in a copy |
| Query cases against 0004 limits | Throwaway checker, deleted after use | 80 of 80 valid |
| Workload arithmetic | Throwaway checker, deleted after use | 57 checks pass on manifest version 3 |
| Cold source fetch | `HOLOCRON_SOURCES_DIR=/tmp/x scripts/fetch-sources.sh` | Four sources verified, exit 0 |
| JSON and links | `jq empty` over all fixtures, relative-link check over 41 Markdown files | All parse, no broken links |

## Not verified

- **`expected_holocron` outcomes.** Holocron-only results (storage unavailable, admission budget full, replay budget reached, batch commit failure) can't run until milestone 04 exists.
- **Exact time boundaries.** The day-key destruction and retention-cutoff edges need the injectable clock from decision 0006.
- **Far-future timestamps.** Accepted as decided. Such rows are never reached by the 12-month retention cutoff. They are counted as `event_time_ahead` in diagnostics, but nothing bounds their storage. This is a known risk.
- **Upstream modes left out.** `TRACKER_SCRIPT_URL`, `BASE_PATH` (baked into the image at build time), ClickHouse/Kafka/Redis modes, proxy and geo headers, and recorder fragmentation above 500 KB were not exercised.
- **amd64 image.** Only the arm64 variant was pulled. Its index digest is pinned, but its script hashes were not compared.
- **Upstream data migrations.** Umami's data migrations leave no `_prisma_migrations` row, so inspection can't detect a skipped run. Milestone 11 reconciliation has to cover it.

## Open items

For Mike, not blocking: add MIT `LICENSE` files to the Kit and Folio repositories. The manifest already records MIT as confirmed.

Handed to later milestones:

- **03:** error codes not yet named (duplicate predicate ID, retention-clipped bound field names), and the unadopted collection proposals (invalid `eventId` response, identify on link/pixel sources, replay-budget body, collect-alias serving).
- **09:** the replay format.
- **11:** cutover rehearsal.
- **12:** exit codes for `serve` on a half-initialized directory and for a failed upgrade.
