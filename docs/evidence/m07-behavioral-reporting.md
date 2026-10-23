# Milestone 07 evidence: behavioral reporting

Status on 2026-10-05: in progress. Funnels, goals, journeys, campaigns, attribution and the session list compute on the exploration model, match the hand-audited fixture exactly, survive save, pin, export, board reopening and drill-downs over HTTP and in a browser, and pass every declared report case at 1% and 10% of the workload. The milestone cannot close: full-scale latency is not measured, and at 10% the annual site A journey and attribution cases already take 3.6 s and 4.8 s p95, so they are likely to miss 10 s at full scale.

## Run

- Source: commit `a0b6440` plus the uncommitted milestone 07 and 08 work in this worktree (08 ran in parallel in the same checkout).
- Engines: DuckDB 1.5.6 (duckdb-rs 1.10506.0, bundled), SQLite through SQLx 0.9.0. Web: Vue 3, Vite, Vitest 4.1, Playwright (Chromium).
- Host: Apple M1 Pro, 10 cores, 32 GiB, macOS (Darwin 27.0.0), APFS SSD. No reference host.

Commands, from the repository root, with `CFLAGS=-g0 CXXFLAGS=-g0`:

```sh
cargo test -p holocron-analytics -p holocron-server -p holocron-protocol
cargo clippy -p holocron-server -p holocron-analytics -p holocron-protocol -p holocron-storage -p holocron-workload --all-targets -- -D warnings
cargo run -p holocron-protocol --bin export-types -- --check
(cd apps/web && bun run typecheck && bun run lint && bun run test)

# browser journeys: a fresh data directory, port 8097
holocron init --data-dir DATA --admin-email mike@example.test --admin-password-file PW
cargo run -p holocron-server --example seed_demo -- DATA mike@example.test VIEWER_PASSWORD
cargo run -p holocron-server --example seed_traffic -- DATA 45
(cd apps/web && bunx vite build --outDir DIST)
HOLOCRON_WEB_DIR=DIST holocron serve --data-dir DATA --bind 127.0.0.1:8097
cd apps/web && HOLOCRON_E2E_URL=http://127.0.0.1:8097 HOLOCRON_E2E_PASSWORD=… HOLOCRON_E2E_SHOTS=SHOTS \
  bunx playwright test e2e/behavioral.spec.ts

# report latency, memory and cancellation through the planner
cargo build --release -p holocron-workload
B=target/release/holocron-gate
HOLOCRON_DISK_RESERVE_MIB=256 $B load --data GATE --groups-per-month 10000      # 1%; 100000 for 10%
HOLOCRON_DISK_RESERVE_MIB=256 $B sessions --data GATE
for f in Q06 Q07 Q08; do
  HOLOCRON_DISK_RESERVE_MIB=256 /usr/bin/time -l $B explore --data GATE --out OUT-$f --observations 20 --plans --cancel --only $f
done
```

## What was built

| Work item | Where | Behaviour |
| --- | --- | --- |
| 1, 2 funnels | `crates/analytics/src/explore/report/funnel.rs` | Candidate events (filter and any step) are materialized once over `[start, end + window)` with one bit per step. Entry is the first step-1 event in range per daily pseudonym or profile; each next step is an ASOF join to the first matching event strictly after the previous one in `(event_time, ingestion_sequence)` order, kept while `t < t1 + window`. No self-join of an identity's events |
| 1 goals | `report/goals.rs`, new report kind `goals` | Session goals over members of sessions starting in range, person goals over consented profiles' events in range; once or every; rate with unavailable `no_denominator`; identity or consented coverage |
| 2 journeys | `report/journey.rs` | Entering sessions start in range (pageview or custom event present, profile sessions for person scope, a matching event with a filter); consecutive repeats dropped over the whole session, then cut at the length; top 100 by sessions, then element by element with a prefix first |
| 3 attribution | `report/attribution.rs` | Conversions: first or every goal event per session or profile, or every revenue event. Touch: a session's first pageview, else its first event; direct and internal-referrer touches are not touches. Last touch is an ASOF join to the latest touch strictly before the conversion, first touch to the earliest inside the 30-day lookback. Revenue stays per currency (`Money`) |
| 3 campaigns | `apps/web/src/pages/reports/CampaignsView.vue` | A plain exploration split by `utm_campaign`, `utm_source`, `utm_medium`, `utm_content`, `utm_term` or `click_id`, with the usual drill-downs, comparison, saving and export, and a link into attribution by the same field |
| 5 session list | `report/session_list.rs` | The drill-down target: sessions starting in range with a matching event, paged by start then session ID; entry and exit paths read for the page only |
| shared | `report/matching.rs` | Materialization of matching events with the columns report predicates test, coverage, the `(t, seq)` order key |
| 1 goal versions | migration `0006_goals.sql`, `crates/protocol/src/operations/goals.rs`, `apps/server/src/explorations/goals.rs` | `goals.save` creates version 1 or the next version when the definition changes (a rename alone is not a version), `goals.archive`, `goals.list` (active, latest), `goals.get` (any version, archived too). Writing needs `site.definitions.write` |
| 5 export | `apps/server/src/explorations/export/reports.rs` | CSV per kind (steps, sequences, touch values with direct, goals, sessions); JSON carries state and complete result; `row_count` counts the CSV rows |
| 4 web | `apps/web/src/components/report/`, `pages/reports/`, `services/reports.ts` | `ReportScreen` (shared header: dates, saved views, export, pin, filter, freshness), `StepEditor` (numbered list, filter bar per step, move and remove buttons, polite announcements), `ChoiceGroup` (radio group with arrow keys), `PredicateQuickAdd`, `ReportBars` (one table with bars, the chart's text form), `PopulationNote`, `GoalEditor`, `DrillMenu`, `ReportSummary` for board cards |
| 4 service | `services/exploration.ts` | `nextId` scans the filter and every report tree; condition operations take a tree path, so `FilterBar` edits steps and goals as it edits the filter |
| 6 benchmark | `crates/workload/src/explore_bench.rs`, `fixtures/workload/queries/` | The bench now runs states with a `report` (Q06 to Q11 name their proposed operations) and `--cancel` abandons each case after 25 ms on both report workers and records the next read's wait. Six new cases: Q06 worst (8 repeated steps, 30-day window, daily and person), Q07 length 7 (path and event), Q08 by referrer domain (10,000 values, four conversions, last and first touch, person scope) |

Routes: `/sites/:siteId/funnels`, `goals`, `journeys`, `campaigns`, `attribution`, `sessions`. Funnels and sessions left `pendingSections`; only `events` (milestone 06) stays pending.

## API and fixture cases

`crates/analytics/tests/correctness_fixture.rs` on the hand-audited fixture, all equal to the expected files:

| Test | Shows |
| --- | --- |
| `funnels_match_the_fixture` | `funnels.json`: steps 5, 3, 1 with the repeated-step, equal-timestamp (ingestion order), exact-window-edge and midnight identities; 2, 1, 1 for a repeated predicate; person scope across midnight 2, 1; daily coverage 20/20 |
| `goals_match_the_fixture` | `goals.json`: 4/7 once, 5/7 every, person scope 2/3 on consented profiles with coverage 7/9 |
| `journeys_match_the_fixture` | `journeys.json`: compression before the step limit, one-step sessions, prefix-first ties, totals equal to entry sessions |
| `attribution_matches_the_fixture` | `attribution.json` under last and first touch: every conversion attributed once, `June`/`june` kept apart, internal and midnight cases direct, revenue EUR 3000 to `June` |
| `session_list_pages_the_sessions_of_the_range` | entry and exit paths, filter keeps sessions with a matching event, paging over the full count |

The fixture harness now fills `query`, the UTM fields and the click ID from the page URL, as collection does. `crates/analytics/tests/reports.rs` gained goals-kind validation cases.

`apps/server/tests/reports.rs`, through the router:

| Test | Shows |
| --- | --- |
| `goal_edits_are_versions_and_saved_reports_keep_theirs` | rename keeps version 1, a changed counting is version 2, version 1 stays readable; a saved report with `ref` version 1 reopens byte-equal and computes; viewers read but get `forbidden` on save; names unique case-folded; archive leaves the list, versions stay readable |
| `a_funnel_keeps_its_state_through_save_pin_and_export` | funnel from `/api/send` payloads (2, 1, conversion 0.5, daily limitation), saved, pinned, board card equal, card recomputes the same report, JSON export equals the explore report, CSV rows |
| `attribution_and_session_lists_export_their_rows` | campaign touch `autumn` from the landing URL, attribution CSV with the direct row, filtered session list paged and exported in full |
| `report_refusals_name_the_report_field` | `report.steps`, `compare`, `report.steps.1.s1` duplicate ID, unknown property `report.steps.1.p`, `report.v`, and a goal refused as `definition.predicate.w` |

## Browser journeys

`apps/web/e2e/behavioral.spec.ts` against `seed_demo` plus `seed_traffic 45` on port 8097, 4 tests, all pass. `seed_traffic` now gives newsletter and Instagram landings UTM parameters, derived from values it already draws, so the rest of its traffic is unchanged.

| Journey | Shows |
| --- | --- |
| goals | create a goal through the dialog and the staged filter input; its row shows rate and v1; edit saves v2 and the row moves to it; a link holding v1 still shows v1 and offers "Use version 2" |
| funnel | steps added through the quick form with arrow keys on the page/event radio group, defaults removed, window chosen; Move step 2 up and Back; saved view; JSON download state equals the URL state and its counts equal the table; pinned to a new board, the card shows the summary and opens the funnel page with the same state; Step 2's "Show sessions" opens the session list with the step's condition |
| journeys, campaigns, attribution | journeys with arrows and length 2; campaign values `autumn-tour` and `birthday-party`, then by UTM source; "Attribute conversions" opens attribution by the same field; a saved goal as conversion, credit by UTM campaign |
| screens | all six report pages at 1440 and 390 px in light and dark: heading, no loading placeholder left, no page overflow |

Screenshots are in [`m07/`](m07/). Accessibility: every result is a semantic table with a caption and `th scope`; bars are decorative inside it; tables scroll in labelled regions; step order changes with named buttons and is announced; choices are radio groups with one tab stop; row actions are named menu buttons.

## Latency, memory and cancellation

Every Q06 to Q08 case (12 frozen, 6 new), two users, 20 samples per case, p95 nearest-rank, milliseconds. Targets: 2,000 for 30-day, 10,000 for 12-month cases. All pass at both scales. Summaries, cancellation results and plans of the three heaviest new cases are in [`m07/`](m07/).

| Case | 1% p50 / p95 | 10% p50 / p95 |
| --- | --- | --- |
| Q06-30d-A-vienna | 22 / 25 | 101 / 109 |
| Q06-30d-A-vienna-worst (8 steps, 30-day window) | 31 / 35 | 140 / 168 |
| Q06-12m-A-utc | 143 / 149 | 870 / 933 |
| Q06-12m-A-utc-worst-person | 135 / 142 | 802 / 840 |
| Q07-30d-A-vienna | 36 / 41 | 372 / 419 |
| Q07-30d-A-vienna-events-7 | 34 / 42 | 326 / 353 |
| Q07-12m-A-utc | 285 / 298 | 3,085 / 3,240 |
| Q07-12m-A-utc-length-7 | 314 / 322 | 3,300 / 3,597 |
| Q08-30d-A-vienna | 50 / 56 | 357 / 523 |
| Q08-30d-A-vienna-referrer | 54 / 62 | 341 / 396 |
| Q08-12m-A-utc | 356 / 399 | 4,349 / 4,798 |
| Q08-12m-A-utc-referrer-person | 352 / 382 | 3,972 / 4,765 |

Site D cases stay under 1.1 s at 10%. The 10% load was 12.4M records, 1.5 GB on disk, 1.42M sessions.

- Memory: peak resident size of the whole bench process (two report workers, DuckDB caches) was 0.49, 0.46 and 1.30 GB at 1% and 1.64, 2.32 and 2.29 GB at 10% for the Q06, Q07 and Q08 runs.
- Cancellation: with both report workers running a case that was abandoned after 25 ms, the next read waited at most 0.7 ms at 1% and 3.4 ms at 10%; every 10% case was still running when abandoned.
- From 1% to 10% the annual site A journey and attribution cases grew about 11 to 12 times. At full scale that is roughly 35 to 50 s against 10 s. Funnels grew about 6 times and would land near 9 s.

## Not verified

- Full-scale latency and the reference host: no full-scale load was run for this milestone (disk and time). The Q06 golden hash from milestone 02 is a full-scale document and was not compared.
- Latency with ingestion running alongside.
- German copy: keys exist in both catalogs, not read by a native speaker.
- Screen reader runs; checks are by roles and names only. Zoom and 768/1024 px widths for the new screens.

## Open items

- Confirmed by Mike on 2026-10-06: the `goals` report kind and the assumptions below ([0003](../decisions/0003-metric-formulas.md) Reporting rules confirmed by Mike).

- For Mike: I added a `goals` report kind (0004 Report definitions, up to 8 goals) so goal conversion runs on the same state, URL, save, pin and export path as the other kinds. The capability matrix proposed separate `analytics.goals` and per-report queries; they are `analytics.explore` with `report` now.
- Assumptions: a pageview step is `path` plus `event_name is not set`, because custom events carry their page's path. An internal referrer counts as no referrer, so a touch with an internal referrer but a UTM source or click ID is still a touch. Session goals and session-scope attribution conversions use sessions starting in range (start mode), so attribution totals equal goal completions. Conversions without the scope's identity are left out and shown in coverage.
- Annual site A journeys and attribution will need materialization at full scale (per-session touch and compressed-sequence rollups by local day are the candidates); not built.
- Fixed 2026-10-06: collection trims UTM values and treats an empty value as missing (0003 UTM values). Collection still truncates UTM values at 500 characters where 0003 says 255, and stores an empty click ID as an empty string.
- `funnels.json` names the midnight limitation on its first daily case only; the result flags every daily funnel, which 0003 asks for.
- Journey rows have no per-sequence drill-down; the page offers the sessions behind the whole report.
