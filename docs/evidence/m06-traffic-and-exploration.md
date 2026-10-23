# Milestone 06 evidence: traffic and exploration

Status on 2026-10-05: in progress. The exploration planner, its operations, the Kit dashboard (overview, Explore, boards, realtime) and the acceptance workflow work end to end against the Rust server, in API tests and in a browser. Small-fixture results are exact. The declared latency suite passes at 10% of the fixture with two users. The milestone cannot close: at full scale every report completes, but five annual site A cases miss 10 s (up to 24.5 s) and two 30-day site A cases miss 2 s (2.2 s) ([time-ordered members and property partitions](m06-member-order-and-property-partitions.md)).

## Run

- Source: commit `a02861b` plus the uncommitted milestone 02 to 05 changes in this worktree, plus this milestone.
- Engines: DuckDB 1.5.6 (duckdb-rs 1.10506.0, bundled), SQLite 3.51.3 through SQLx 0.9.0. Web: Vue 3, Vite, Vitest 4.1, Playwright (Chromium).
- Host: Apple M1 Pro, 10 cores, 32 GiB, macOS (Darwin 27.0.0), APFS SSD. No reference host.

Commands, from the repository root:

```sh
cargo test -p holocron-analytics -p holocron-server -p holocron-protocol -p holocron-storage
cargo clippy -p holocron-server -p holocron-analytics -p holocron-protocol -p holocron-storage -p holocron-workload --all-targets -- -D warnings
cargo run -p holocron-protocol --bin export-types          # regenerates apps/web/src/generated and openapi.json
(cd apps/web && bun run typecheck && bun run lint && bun run test && bun run build)

# browser journeys: a fresh data directory with demo workspaces and 45 days of traffic
holocron init --data-dir DATA --admin-email mike@example.test --admin-password-file PW
cargo run -p holocron-server --example seed_demo -- DATA mike@example.test VIEWER_PASSWORD
cargo run -p holocron-server --example seed_traffic -- DATA 45
HOLOCRON_WEB_DIR=apps/web/dist holocron serve --data-dir DATA --bind 127.0.0.1:8096
cd apps/web && HOLOCRON_E2E_URL=http://127.0.0.1:8096 HOLOCRON_E2E_PASSWORD=… HOLOCRON_E2E_VIEWER_PASSWORD=… \
  HOLOCRON_E2E_DB=DATA/control.sqlite HOLOCRON_E2E_SHOTS=SHOTS bunx playwright test

# latency suite through the planner
cargo build --release -p holocron-workload
B=target/release/holocron-gate
HOLOCRON_DISK_RESERVE_MIB=256 $B load --data GATE --groups-per-month 10000      # 1%; 100000 for 10%
HOLOCRON_DISK_RESERVE_MIB=256 $B sessions --data GATE
HOLOCRON_DISK_RESERVE_MIB=256 $B explore --data GATE --out OUT --observations 20 --plans
```

C and C++ code was built with `CFLAGS=-g0 CXXFLAGS=-g0`: the bundled DuckDB build directory shrinks from about 3 GB to 466 MB without debug info, which mattered on a nearly full disk. Rust debug info is unchanged.

## What was built

| Work item | Where | Behaviour |
| --- | --- | --- |
| 2, 5 planner | `crates/analytics/src/explore/` | `validate` checks a typed `ExplorationState` against 0004 and names the refused field (`filter.<id>`, `page`, `granularity`, …). `explore` resolves dates in the state's timezone, applies the retention cutoff, materializes the matching events with session-scoped fields once per period, and computes totals, comparison, series and every panel from that one snapshot |
| 2 state | `crates/protocol/src/exploration.rs`, generated TS | One state document for URL, saved view, board card and export. `METRIC_VERSION` moved into the protocol limits so the browser pins new states to it |
| 4 definitions | `apps/server/src/explorations/`, migration `0004_explorations.sql` | `explorations.save/list/get/delete` (per user), `segments.*` and `annotations.*` (site definitions, `site.definitions.write`), `boards.*` with card order and `expected_revision`, `analytics.export` (CSV and JSON with the state) |
| 7 realtime | `apps/server/src/explorations/live.rs` | `realtime.snapshot` (active identities in 5 minutes, 30 one-minute buckets, latest events, processing and correction state) and `GET /api/subscribe` with `subscribed`, `events`, `resync` and `expired` frames from a topic cursor |
| 6 cancellation | `crates/storage/src/reader.rs` | A read whose caller stops waiting is interrupted, so a replaced exploration frees its worker instead of finishing |
| 2, 4, 6 web service | `apps/web/src/services/exploration.ts` | Framework-free state operations: canonical text (sorted keys), base64url URL form, version and size refusal, predicate IDs that are never renumbered, include/exclude/split, grouping, clear filters, segments copied with fresh IDs |
| 6 adapters | `composables/useExploration.ts`, `useExplore.ts`, `useLive.ts` | URL adapter (one history entry per deliberate step, `replace` only to canonicalize); Vue Query keyed by canonical state and a refresh epoch, `staleTime: Infinity`, previous result kept and labelled only within one site; one live subscription per screen with resume, resync, expiry and 1 to 30 s backoff, at most one snapshot read per event batch |
| 1, 3 views | `pages/overview/`, `pages/explore/`, `pages/boards/`, `pages/realtime/`, `components/exploration/` | Overview on real data (five measures, series, five panels). Explore with measures, dimensions, table, bars, line and week grid. Filter bar adapted from Kit TableFilter to the predicate tree; SavedViews adapted from Kit; Kit Popover and SplitBadge copied. Boards with board-wide range override, reorder, rename and delete |
| measurement | `crates/workload/src/explore_bench.rs`, `explore::record_plans` | `holocron-gate explore` runs every declared `analytics.explore` case through the planner with two users and records `EXPLAIN` plans of each statement |
| test data | `apps/server/examples/seed_traffic.rs` | Deterministic traffic for every site of a seeded directory, through admission, transfer and correction |

The fixture's overview layout no longer uses fixture data: `pages/overview/fixture.ts`, `HourlyChart.vue` and `BreakdownTable.vue` are deleted.

## API and fixture cases

`crates/analytics/tests/correctness_fixture.rs`, through the planner on the hand-audited fixture: `acceptance_exploration_matches_the_fixture` (mobile `signup` with `plan=pro`, country and referrer, previous period, drill-down to AT, all equal to `acceptance-exploration.json`), `typed_property_filters_and_cells_match_the_fixture` (`properties.json`), `realtime_matches_the_fixture`, `explorations_reproduce_the_traffic_cases` (every `traffic.json` case).

`crates/analytics/tests/explore.rs`: include and exclude partition the total and not-set is its own cell; repeated conditions in an OR group combine; session fields come from the session's first event and a session counts once in every cell it touches; paging after sorting with ties by dimension value; series and panels sum to the totals.

`apps/server/tests/explorations.rs`, through the router:

| Test | Shows |
| --- | --- |
| `acceptance_workflow_saves_pins_exports_and_reopens_the_same_state` | `/api/send` payloads, correction, the acceptance state (3 events, 3 visitor-days, previous 1, change +2), drill-down to AT; saved, pinned and exported states are byte-equal to the drilled state; the JSON export carries the same rows and resolved range; the CSV header |
| `unsupported_states_are_refused_naming_the_field` | duplicate predicate ID, filter depth 4, page past 10,000 rows, metric version 2, `funnel` without a report, an unknown property key, a wrong value type and 2 months hourly each refused with the field named; a later state version does not parse |
| `revoked_scope_is_refused_by_reads_exports_and_definitions` | viewers read and export but cannot save segments or create boards; after removal from the workspace, explore, export, realtime and save answer `not_found` |
| `segments_and_annotations_are_site_definitions` | names trimmed and unique per site, depth limit, annotation create, update, delete and range listing |
| `realtime_snapshot_and_live_subscription_resume_from_the_cursor` | snapshot shape; a current cursor gets `events` after new data; a cursor behind the head resumes at once; one ahead is `expired`; another topic's cursor is 400 and a caller without the site 404 before streaming |

`crates/storage/tests/storage.rs` `abandoned_read_frees_its_worker`: a caller that gives up after 200 ms on a query that would run for minutes leaves the single report worker free for the next read.

## Browser journeys

After the merge with milestones 05 (settings screens) and 09 (replay and heatmaps) on 2026-10-04, all three suites pass together against one debug server on `seed_demo` plus `seed_traffic 45`: shell, exploration and recordings, 27 passed and 1 skipped (instance name). The revoked-scope journey ran with `HOLOCRON_E2E_DB`. Its Refresh locator is now exact, because a replay row's "Actions for /hostile/meta-refresh-and-base.json" button also matched "Refresh".

`apps/web/e2e/exploration.spec.ts` against the seeded server, 6 tests, all pass; `shell.spec.ts` 9 pass and 1 skips (instance name, needs a server restart, as before). Screenshots are in [`m06/`](m06/).

| Journey | Shows |
| --- | --- |
| overview at 1440, 1024, 768 and 390 px, light and dark | real figures, no fixture label, no horizontal overflow outside scroll regions |
| acceptance workflow | date range, three conditions typed into the filter bar, measures, two dimensions, comparison; drill-down to AT adds exactly one condition; Back restores the previous state and Forward the drilled one; save marks the view active; the JSON download's state equals the URL state; pin to a new board; the board card opens Explore with the same state; the saved view reopens it |
| clear filters and refusals | an OR group of two device conditions; Clear filters changes only `filter` and the page; an unreadable link shows a refusal with Reset view instead of computing; an unknown property is refused and its chip marked |
| phone | conditions collapse into a count, no page overflow, the results table scrolls inside its labelled region |
| realtime | `Live` status; an event sent to `/api/send` appears in the latest events without reload |
| revoked scope | removing the viewer's membership mid-session: Refresh replaces the page with "Website not available", no results remain, and `analytics.export` answers 404 |

Accessibility in these views: every chart has a text summary and an `sr-only` table with the same values; tables use `th scope`, `aria-sort` and sort buttons; row actions are named menu buttons ("Actions for AT · google.com"); filter stages are a combobox with `aria-activedescendant` and a polite live region announcing each step; direction of change is carried by an arrow and words, not colour alone.

## Latency suite through the planner

Every declared `analytics.explore` case (38) runs its own state through `validate` and `explore`, two users looping from opposite ends of the list, 20 samples per case, no ingestion alongside. Time is the full report-lane read. p95 is nearest-rank over 20 samples, so close to the maximum. Every resolved range equals the case's declared one; no case was refused.

| Case | 1% p50 / p95 | 10% p50 / p95 | Target p95 |
| --- | --- | --- | --- |
| Q01-30d-A-vienna | 70 / 77 | 699 / 995 | 2,000 |
| Q02-30d-A-vienna | 30 / 30 | 179 / 229 | 2,000 |
| Q03-30d-A-vienna | 90 / 97 | 760 / 1,554 | 2,000 |
| Q04-30d-A-vienna-selective | 89 / 104 | 559 / 1,049 | 2,000 |
| Q12-30d-A-vienna-session | 65 / 71 | 711 / 872 | 2,000 |
| Q01-12m-A-utc | 183 / 196 | 2,727 / 3,537 | 10,000 |
| Q03-12m-A-utc-drill-at | 247 / 282 | 3,991 / 6,400 | 10,000 |
| Q04-12m-A-utc | 369 / 384 | 6,104 / 7,293 | 10,000 |
| Q04-12m-A-utc-selective | 431 / 454 | 5,622 / 7,455 | 10,000 |
| Q12-12m-A-utc | 362 / 367 | 4,921 / 8,639 | 10,000 |
| Q12-12m-A-utc-broad | 429 / 484 | 6,528 / 7,384 | 10,000 |
| Q12-12m-D-vienna | 167 / 184 | 1,647 / 2,155 | 10,000 |

All 38 pass at both scales; the rows above are the slowest per family, and every site D case stays under a quarter of its target. The 1% suite took 47 s, the 10% suite 626 s. The 10% load was 12.4M records (1.2 GB of storage after transfer).

From 1% to 10%, site A p95 grows 6 to 23 times. At full scale the annual site A cases would be roughly 60 to 90 s and the 30-day site A cases 2 to 15 s, against 10 s and 2 s. This matches milestone 02's finding with hand-written SQL: scanning raw events per request does not meet the site A targets. Plans of three representative cases are in [`m06/plans/`](m06/plans/): each request materializes the matching events with a left join to `session_members` (and one join per property field), then aggregates the materialized set once per cut.

## Not verified

- Full-scale latency (three runs, 200 samples, reference host). One run with 20 samples on the laptop is in [filtered reports](m06-filtered-reports.md); memory failures removed most of its samples.
- Latency with ingestion running alongside. `bench` covers collection under load with the milestone 02 probes, not with the planner.
- Keyboard-only use of the filter bar end to end. The journeys type into it and press Enter but pick options by click; arrow keys, Backspace and Escape handling is not covered by a test.
- German copy for the new screens. Every key exists in both catalogs with equal placeholders (`parity.test.ts`), but the German text has not been read by a native speaker.

## Open items

- For Mike, 2026-10-05: at full scale five annual and two 30-day site A cases still miss; the designs to test next and the proposed installation tiers are in [time-ordered members and property partitions](m06-member-order-and-property-partitions.md#open-items) and [0008 Installation tiers](../decisions/0008-storage-engine-and-budgets.md#installation-tiers-sizing-guidance).
- Superseded, 2026-10-04: site A will miss its 30-day and annual targets at full scale (extrapolated above). Next design to test is the exact identity-day rollups of [0008](../decisions/0008-storage-engine-and-budgets.md) for unfiltered and dimension-only states. Lowering a target is your call; I did neither.
- Session derivation: correction's ASOF joins ran as nested loops; with that fixed a full rebuild at 10% takes 43 s and the full-scale backfill 1,124 s ([filtered reports](m06-filtered-reports.md)).
- A direct load of `/boards` has no site in the route; it shows the first workspace. Remembering the last site across reloads is not built.
- Charts are drawn in Vue without a chart library. Tooltips on hover are titles only.

## Identity-day rollups

Added on 2026-10-04 for the open item above: unfiltered and single-dimension reports now read exact identity-day rollups, with results byte-identical to the raw scan. At 10% with the overview's five panels, Q01-12m-A-utc went from 15.0 s to 0.83 s p95 and Q01-30d-A-vienna from 2.1 s to 0.12 s. Filtered site A reports still scan raw events. Details, tests and numbers: [m06 identity-day rollups](m06-identity-day-rollups.md).

## Filtered reports and full scale

Added on 2026-10-05: correction stores session-scoped fields on `sessions`, the planner filters inside the statement that reads events and keeps only the columns cuts read. At 10% Q04-12m-A-utc went from 6.5 s to 2.7 s p95 and Q12-12m-A-utc-broad from 7.6 s to 1.2 s; all 38 cases pass. At full scale filtered annual site A cases still miss 10 s, and property pivots and session properties run out of memory, one with a DuckDB assertion that aborts the process. Details: [m06 filtered reports](m06-filtered-reports.md).

## Time-ordered members and property partitions

Added on 2026-10-05: members carry their event time and stay in time order, event properties live in 64 key partitions, long ranges run in 31-day chunks, and DuckDB is built without its internal assertions, which aborted the server. At full scale all 760 samples complete (575 failed before) and 31 of 38 cases pass; Q04-12m-A-utc went from 33.5 s to 12.5 s p95, Q03-12m-A-utc from 11.5 s to 5.8 s. The property pivot (24.5 s), session-property (19.0 s), broad (15.2 s) and tail (10.5 s) annual cases, Q04-12m-A-utc and two 30-day Q04 cases still miss. Details: [m06 time-ordered members and property partitions](m06-member-order-and-property-partitions.md).

## Events report

Added on 2026-10-06: `/sites/:siteId/events` shows event names with counts and a series, and per-property values, numeric statistics and grouped pivots of up to three properties, all on `analytics.explore` in one URL state. Journey `apps/web/e2e/events.spec.ts`; screenshots `m06/events-*.png` at 1440 and 390 px, light and dark. Gaps against upstream, from what `analytics.explore` can express: no per-event-name series, no property catalog per event, no day/week/month buckets for date values, no per-element array counts, no statistics over session properties, and a grouped pivot instead of raw event rows.
