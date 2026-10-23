# Milestone 08 evidence: retention, revenue and performance

Status on 2026-10-05: in progress. Retention of consented profiles, currency-separated revenue and web-vital percentiles work end to end: planner, HTTP operations, save, pin, export and reopen, and three Kit screens in a browser. The small fixture's retention, revenue and percentile cases are exact. Every Q09, Q10 and Q11 case passes its latency target at 1% and 10% of the workload. Full scale and the reference host were not run, and the imported population label waits for milestone 11, so the milestone stays open.

## Run

- Source: commit `a0b6440` plus the uncommitted shared report contract, milestone 07 running in the same worktree, and this milestone.
- Engines: DuckDB 1.5.6 (duckdb-rs 1.10506.0, bundled), SQLite through SQLx 0.9.0. Web: Vue 3, Vite, Vitest 4.1, Playwright (Chromium).
- Host: Apple M1 Pro, 10 cores, 32 GiB, macOS (Darwin 27.0.0), APFS SSD. No reference host.

Commands, from the repository root. C and C++ were built with `CFLAGS=-g0 CXXFLAGS=-g0`.

```sh
cargo test -p holocron-analytics -p holocron-server -p holocron-protocol -p holocron-storage
cargo clippy -p holocron-server -p holocron-analytics -p holocron-protocol -p holocron-storage -p holocron-workload --all-targets -- -D warnings
cargo run -p holocron-protocol --bin export-types -- --check
(cd apps/web && bun run typecheck && bun run lint && bun run test)

# browser journeys: demo workspaces, 45 days of traffic, then profiles, purchases and vitals
holocron init --data-dir DATA --admin-email mike@example.test --admin-password-file PW
cargo run -p holocron-server --example seed_demo -- DATA mike@example.test VIEWER_PASSWORD
cargo run -p holocron-server --example seed_traffic -- DATA 45
cargo run -p holocron-server --example seed_commerce -- DATA 45
(cd apps/web && bunx vite build --outDir DIST)
HOLOCRON_WEB_DIR=DIST holocron serve --data-dir DATA --bind 127.0.0.1:8098
cd apps/web && HOLOCRON_E2E_URL=http://127.0.0.1:8098 HOLOCRON_E2E_PASSWORD=… HOLOCRON_E2E_SHOTS=SHOTS \
  bunx playwright test e2e/retention-revenue.spec.ts

# measured suite: Q09 (retention), Q10 (revenue), Q11 (performance)
cargo build --release -p holocron-workload
B=target/release/holocron-gate
HOLOCRON_DISK_RESERVE_MIB=256 $B load --data GATE --groups-per-month 10000      # 1%; 100000 for 10%
HOLOCRON_DISK_RESERVE_MIB=256 $B sessions --data GATE
HOLOCRON_DISK_RESERVE_MIB=256 $B explore --data GATE --out OUT --observations 20 --only Q09   # and Q10, Q11
```

## What was built

| Work item | Where | Behaviour |
| --- | --- | --- |
| 1, 2 retention | `crates/analytics/src/explore/report/retention.rs` | Consented profiles only. Cohort entry is a profile's first eligible event since the retention cutoff, in the local day or Monday week of the state's timezone. Activity is any eligible event in a calendar interval counted from the cohort period. Every cohort period in range is a row, size 0 included; size 0 rates are `no_denominator`. Intervals ending after the request clock are `complete: false` and left out of the size-weighted averages. Coverage is consented eligible events over all eligible events, per cohort and in total. Cohort predicates are the state's `filter`, matched on the profile's events in its cohort period. Reads immutable events only, unless a predicate names a session field |
| 1, 5 labels | `crates/protocol/src/exploration.rs` | `RetentionResult` carries `coverage`, `history_start` (the first retained instant, so cohort entry says how far back it looked) and `watermark` (the instant intervals are judged against). They travel in the result, so save, pin, export and reopen show the same labels |
| 3 revenue | `explore/run.rs`, `validate.rs` | `revenue_net`, `revenue_gross`, `revenue_refunds` are a `money` value: one exact amount per currency, summed as DuckDB `HUGEINT` from integer minor units and never added across currencies. `revenue_count` is a count. A state measuring only revenue reads revenue records only. Breakdowns by any dimension and `currency`, series per bucket and currency, comparison when one currency is on both sides. Sorting by an amount needs `currency` and orders by currency first |
| 3 encoding | `Money` in `exploration.rs` | `{ currency, exponent, minor_units, amount }`. `minor_units` is a JSON number up to 2^53 − 1 and a decimal string beyond it (was always a number). `amount` is the decimal string by ISO 4217 exponent (`"49.91"`, `"4500"`). The exponent table moved from collection into the protocol so both use one list |
| 4 performance | `explore/run.rs`, `sql.rs` | A state with a percentile or `perf_metric` reads performance events, one observation per metric reported (`lcp, fcp, ttfb, inp, cls`); a missing metric is no observation, an explicit zero is one. p50, p75, p95 are nearest rank, `r = ceil(p × n)` in integers. `lcp, fcp, ttfb, inp` render as `millis`, `cls` as a 4-place decimal. A percentile over several metrics is `null` with `mixed_metrics`. Device, browser, page and country work like any field; attached reports take their session's fields, unattached ones their own |
| 5 export | `apps/server/src/explorations/export.rs` | JSON already carried the whole result. CSV writes money as `15.00 EUR`, one per currency, and a retention state as one row per cohort with size, coverage, and per offset active, rate and completeness, then the averages |
| 5 screens | `pages/retention/`, `pages/revenue/`, `pages/performance/`, `components/retention/`, `components/performance/` | Retention: daily or weekly cohorts, interval menu, labels, a cohort table that is its own visualization (shade and text in the same cells), dashed incomplete cells, empty states. Revenue: a card per currency, net series per chosen currency, breakdown by currency and one field, results table. Performance: a card per vital at p75 with p50 and p95, a rating in words, a table per metric and optional group. All three reuse the exploration header (range, filter bar, saved views, export, pin, freshness) and `ResultTable` or Kit tables |
| 5 boards | `pages/boards/BoardCard.vue`, `lib/state-pages.ts` | A pinned retention card shows its population and first averages; "Open report" opens the retention, revenue or performance page with the pinned state. Saved views list only the views the current page draws |
| 6 measurement | `crates/workload/src/explore_bench.rs` | Q10 and Q11 cases join the explore suite (Q09 runs as a report state) |
| test data | `apps/server/examples/seed_commerce.rs` | Turns persistent identity on for the first seeded site and adds 140 consented returning profiles, 220 purchases in EUR, USD and JPY with refunds, and 900 performance reports, some without INP or CLS |

Navigation: retention, revenue and performance left `pendingSections` and have routes `/sites/:siteId/retention`, `/revenue`, `/performance`. The comparison toggle is hidden for report states, which refuse comparison. `currency` joined the field pickers.

No correction change was needed. Revenue, percentile and retention values are read from immutable events, which are visible as soon as their batch applies. Derived rows they depend on (a performance report's session, session fields named in a filter or breakdown) are rebuilt by the existing dirty-day correction, and every result's freshness says when that is still pending. Migration 0007 was not needed.

## Formula cases

`crates/analytics/tests/correctness_fixture.rs`, through admission, transfer, correction and the planner on the hand-audited fixture. The harness now collects revenue as collection does (exact decimal rounding from the number text, ISO 4217 exponents, zero is no record).

| Test | Shows |
| --- | --- |
| `retention_matches_the_fixture` | `retention.json`: cohorts 2026-09-14 (anna, ben) and 2026-09-21 (cleo), entry at con-07 not the anonymous con-05, dora outside the range; every interval's bounds, active count, rate and completeness against the 2026-10-03T10:00Z watermark, including the not-started week; coverage 5/6 and 2/3; weighted averages 1.0, 0.5 and `no_denominator` |
| `revenue_matches_the_fixture` | Both `revenue.json` cases: per currency exponent, gross, refunds, net, count and display amount, 19.995 EUR as 2000, 0.125 USD as 12, 1500.5 JPY as 1500, the keyless key-02 counted, totals kept per currency |
| `percentiles_match_the_fixture` | All four `percentiles.json` cases: LCP and CLS over all reports (perf-06 without LCP and perf-04 without CLS excluded, perf-05's zero kept), LCP by mobile and desktop; the two-metric headline is `mixed_metrics` |

`crates/analytics/tests/reports.rs` `percentiles_need_a_perf_metric_condition_and_measure_nothing_else` replaces the "not available yet" refusals: percentiles without a `perf_metric` condition, percentiles mixed with another measure, `perf_metric` as a dimension of plain measures, `perf_metric` in a report tree, and an amount sort without `currency` are refused naming the field.

None of the three expected files disagreed with 0003.

## API cases

`apps/server/tests/retention_revenue.rs`, through the router from `/api/send` payloads:

| Test | Shows |
| --- | --- |
| `reports_keep_their_labels_through_save_pin_export_and_reopen` | Weekly retention after consent grants: cohort sizes, day-0 rate, an incomplete returning week, `no_denominator` average, coverage 1/2 with an anonymous event, `watermark` and `history_start`. Saved, pinned and exported states equal the request; the pinned state recomputes the identical report; the JSON export carries report and freshness; the CSV header and averages row. Revenue rows as exact `money` (`"15.00"`, `"-5.00"`, JPY `"1500"`) and the CSV `15.00 EUR`. Percentiles per metric with the missing LCP excluded and `mixed_metrics` totals |
| `unsupported_report_states_are_refused_naming_the_field` | `measures`, `sort`, `filter.m` and `report.intervals` refusals with their codes |
| `a_late_event_changes_reports_while_freshness_says_correction_is_pending` | A late consented event and a late purchase change retention coverage and net revenue as soon as they apply, while freshness reports `pending` with dirty days and a higher watermark batch; after correction the result is `corrected` and identical |

## Browser journeys

`apps/web/e2e/retention-revenue.spec.ts` against a debug server on port 8098 seeded as above, 4 tests, all pass. Screenshots are in [`m08/`](m08/), desktop (1440 px) and phone (390 px), light and dark.

| Journey | Shows |
| --- | --- |
| retention | the three labels, cohort table with `Week 0` and `Average`, incomplete cells named for screen readers, no comparison toggle, no page overflow at both widths and modes; the cohort-period radios focused and switched from the keyboard; 7 daily intervals; save, JSON export (state equal to the URL, report with coverage and history start), pin to a new board, the board card's population line, "Open report" back to the same state with the saved view active |
| no consent | `roadhouse.test` has anonymous traffic only: "No consented profiles in these periods", why daily pseudonyms cannot show returns, a link to turn on persistent identity, and no table or 0% |
| revenue | a card per currency (EUR, JPY, USD), the never-converted note, the USD series, breakdown by country, CSV header and `… EUR` amounts |
| performance | a card per vital with units and a rating in words, the per-metric table, breakdown by device |

Accessibility in these views: the cohort table uses `th scope` for cohorts and offsets and states incompleteness in text; tables scroll inside labelled regions; the menu triggers are named with their current value ("Intervals: 14 days", "Breakdown: By currency"); ratings are words, never colour alone.

## Measurements

Every Q09, Q10 and Q11 case runs its frozen state through `validate` and `explore` on the report lane, two users, 20 samples per case, no ingestion alongside. Times are milliseconds of the full read, p95 nearest rank over 20 samples. Every resolved range equals the case's declared one; no case was refused. Peak memory is the maximum resident set of the whole `holocron-gate explore` process per family.

| Case | 1% p50 / p95 | 10% p50 / p95 | Target p95 |
| --- | --- | --- | --- |
| Q09-30d-A-vienna | 29 / 30 | 193 / 213 | 2,000 |
| Q09-30d-D-utc | 19 / 20 | 88 / 116 | 2,000 |
| Q09-12m-A-utc | 141 / 146 | 1,120 / 1,176 | 10,000 |
| Q09-12m-D-vienna | 64 / 66 | 311 / 344 | 10,000 |
| Q10-30d-A-vienna | 11 / 12 | 38 / 42 | 2,000 |
| Q10-30d-A-vienna-breakdown | 14 / 15 | 50 / 61 | 2,000 |
| Q10-30d-D-utc | 10 / 11 | 32 / 36 | 2,000 |
| Q10-12m-A-utc | 32 / 33 | 159 / 175 | 10,000 |
| Q10-12m-A-utc-breakdown | 68 / 72 | 306 / 345 | 10,000 |
| Q10-12m-D-vienna | 27 / 28 | 100 / 113 | 10,000 |
| Q11-30d-A-vienna | 16 / 17 | 83 / 96 | 2,000 |
| Q11-30d-D-utc | 9 / 10 | 29 / 39 | 2,000 |
| Q11-12m-A-utc | 104 / 108 | 740 / 784 | 10,000 |
| Q11-12m-D-vienna | 31 / 33 | 72 / 90 | 10,000 |

Peak memory of the explore process: Q09 241 MB, Q10 180 MB, Q11 276 MB at 1%; Q09 1.18 GB, Q10 594 MB, Q11 1.34 GB at 10%. The 10% load was 12.4M records in 285 s (peak 1.8 GB for sessions), and the suite was rerun once the host was idle, because milestone 07's suite had been running beside the first pass.

From 1% to 10% the annual site A cases grew 8 times (Q09-12m-A-utc, 146 to 1,176 ms) and 7 times (Q11-12m-A-utc, 108 to 784 ms). If that holds to full scale, Q09-12m-A-utc would land near 12 s against its 10 s target and Q11-12m-A-utc near 8 s. Milestone 06 found that such extrapolation under-reads full-scale filtered annual cases, so this is a warning, not a claim.

The results are not empty: at 1%, Q09-12m-A-utc has 53 weekly cohorts with 7,691 profiles and 33% consented coverage, Q10-30d-A-vienna four currencies, Q11-30d-A-vienna 15 metric and device rows. The workload repeats a supplied ID once per month and never within one, so its day and week retention past offset 0 is close to zero; the cases measure the scan, not a realistic return rate.

## Mapping requirements for milestone 11

Import must produce the same rows these reports read, or say why it cannot:

- Revenue: one record per source event, from upstream's `revenue` rows (currency, amount, event). Uppercase the currency; an unknown code keeps the event without revenue and counts as invalid in the import report. Round the amount half to even from its decimal text to the ISO 4217 exponent; never pass it through a float. A zero amount is no record, a negative one a refund. When an event has both `revenue` rows and `revenue`/`currency` event data, take the rows once and do not count the data again.
- Performance: one performance event per upstream vitals row, at its time, with `lcp, fcp, ttfb, inp` rounded to whole milliseconds and `cls` in units of 0.0001 rounded half to even from its text. A missing value stays missing, never 0. Attach to the imported session where the source has one; MySQL sources have no vitals.
- Retention and cohorts: imported source identities are their own population. They must never set `profile_id`, so they never enter a cohort and count only in the coverage denominator. Reports need a marker for imported events to label that population separately (CAP-IDENT-06); it does not exist yet. Upstream `retention` report definitions map to a retention state (`report` v1, day or week cohorts, intervals); `segment` type `cohort` maps to the state's `filter` where its predicates map, else it is kept unmapped.
- History: cohort entry looks back to the retention cutoff (`history_start`). Imported history older than 12 months is not stored, so imported profiles' first events are bounded the same way.

## Not verified

- Full scale (100%) and the reference host: not run. Disk on this laptop had about 54 GB free, and milestone 06 showed that 1% and 10% do not predict full-scale latency of filtered annual reads. Monthly and annual cases pass at 1% and 10% only.
- Latency with ingestion running alongside.
- German copy of the new screens: every key exists in both catalogs; not read by a native speaker.
- Keyboard use beyond the cohort-period radios: menus are Kit dropdowns with their own keyboard handling, not covered by a test here.

## Open items

- Confirmed by Mike on 2026-10-06: the result watermark is the application clock, the weekly cohort rule and revenue sorting below ([0003](../decisions/0003-metric-formulas.md)).

- Assumption: the "result watermark" that decides `complete` (0003 Retention) is the request's application clock. Ingestion lag is reported separately by freshness. If it should be the ingestion watermark's receive time instead, that is a one-line change and a 0003 clarification.
- Assumption: a weekly cohort period is the whole Monday week that the range touches, also when the range starts mid-week, and coverage is not narrowed by cohort predicates.
- Assumption: sorting rows by a revenue amount orders by currency first; recorded in 0004.
- `count(DISTINCT profile_id)` grouped by two expressions returned 1 for two profiles in the retention activity query on DuckDB 1.5.6; it now counts distinct pairs in a subquery. Not reduced to a minimal case. `identified_people` uses `count(DISTINCT profile_id)` without grouping expressions; `identified_people_counts_two_profiles_as_two` checks it (2026-10-06). The retention shape did not reproduce in five variants from 2 to 5,000 profiles on 2 and 8 threads, so it stays unreduced.
- Fixed 2026-10-06: collection rounds CLS half to even from the number's text, as revenue does (0002).
- Imported-population labels (CAP-IDENT-06) wait for an import marker on events (milestone 11).
