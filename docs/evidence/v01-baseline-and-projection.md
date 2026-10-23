# v01 evidence: typical-weekday baseline and pace projection

Status: in progress. Every work item of the [brief](../ux-v2/v01-baseline-and-projection.md) is built and its fixtures pass. Baselines now read their days once ([Grouped baseline read](#grouped-baseline-read)). The workload criterion holds at a 10% load that fills just the first three days of each month. At full scale it fails, for `previous_period` as well ([v09 Workload](v09-acceptance.md#workload)), and three questions in Open items need Mike.

## Acceptance criteria

| Criterion | Result | Evidence |
| --- | --- | --- |
| Hand-computed fixtures: 10 weeks of hourly data, a spike Tuesday, a 3-week-old site, a retention-clipped baseline, both Vienna DST transitions, zero baselines | Passed | `crates/analytics/tests/baselines.rs`. The traffic rule and every derivation are in the file header and the test comments. Values are in [Baseline fixtures](#baseline-fixtures). |
| Clock pinned to Tuesday 15:20: totals compare 00:00 to 15:20 on both sides, the series has baseline points for all 24 hours, the projection's value, low and high match | Passed | `today_at_15_20_compares_with_the_median_tuesday_until_15_20`: baseline 95 pageviews until 15:20 against 111 today, 24 hourly baseline points, projection 168 (167.6643 to 168.6076) from 8 basis days. See [Projection fixtures](#projection-fixtures). |
| A filtered and an unfiltered request over the same data return identical baselines when the filter matches everything | Passed | The same test runs with a `path = /` filter (events scan) and without (rollups) and compares totals, rows, series points and both projections as JSON. The workload's `matches_raw` (rollups against the raw planner) is true for the unfiltered scratch cases in [Workload](#workload). |
| Totals, rows, series and projection share one snapshot: the same `watermark` and `metric_version` | Passed | `explore` computes every cut, baseline window and projection inside one `Snapshot` (one `storage.read` closure) and the result carries one `freshness`. The 15:20 test asserts `metric_version` and equal watermarks across the filtered and unfiltered runs. |
| Refusals name the field (`compare`) and reason; no refused state falls back to `previous_period` | Passed | `typical_weekday_refuses_measures_without_a_baseline_and_ranges_over_31_days` (`tests/explore.rs`) and `only_the_goals_report_compares_with_a_typical_weekday` (`tests/reports.rs`): `compare_unsupported` on `compare`. The goals report no longer answers `typical_weekday` with an internal refusal; it computes it. |
| Generated types have no drift | Passed | `cargo run -p holocron-protocol --bin export-types -- --check` and `crates/protocol/tests/drift.rs`. |
| Workload: the declared 30-day suite with `typical_weekday` keeps its p95 target with two users; extra cost against `previous_period` recorded | Passed at 10%, failed at full scale | 10%: all 30-day cases under 2 s, the worst at 914 ms after the grouped read ([Workload](#workload)). Full scale (124M records, two users, no summary step, [v09 Workload](v09-acceptance.md#workload)): 12 of 31 `typical_weekday` cases miss 2 s, all site A, worst Q12-30d-A-vienna-pivot at 7,406 ms p95; `previous_period` misses 16 of 38 in the same run setup and 11 in `holocron-gate explore`, so most misses are not caused by the baseline. Extra cost, median execution per case: 1.1 to 1.5 times for cases reading baselines, about 2 times for Q04-broad and Q12 pivot, session and tail, none for the reports Q05 to Q09. |

## What was built

| Work item | Where | Behaviour |
| --- | --- | --- |
| 1, 8 wire and limits | `crates/protocol/src/exploration.rs`, `limits.rs`, `errors.rs` | `Compare::TypicalWeekday`, `compare_days`, `SeriesPoint.compare`, `Cut.projection`, `Series.projection` (`ProjectedPoint`), `Projection { estimate, unavailable }`, `GoalResult.compare` (`GoalComparison`) and `GoalResult.projection`; reasons `compare_unsupported`, `insufficient_history`; limits 8 weeks, 4 days, 31 days, 20-unit floor |
| 2, 4, 7 baselines | `crates/analytics/src/explore/baseline.rs` | `Windows` plans the baseline days per range day; `Baseline` materializes them once, from the rollups when the request reads them, else from events, in the request's snapshot, and reads every window with one grouped query per aggregate. Medians per bucket for counts, pooled populations for rates and spans |
| 3, 9 until now | `explore/mod.rs`, `resolve.rs` | Now comes from the application clock in `Context`. Totals of a range containing now compare up to the same local time for both comparisons |
| 5 projection | `baseline.rs` `Windows::pace`, `run.rs` `Compared` | See [Projection rules](#projection-rules) |
| 6 goals | `explore/report/goals.rs`, `report/mod.rs` | Each baseline window is counted like the range: median completions per range day, the rate over the pooled windows, a projection of completions |
| 5 export | `apps/server/src/explorations/export.rs`, `export/reports.rs` | CSV totals get `<measure>_projection`, `_projection_low`, `_projection_high`; goals get `completions_previous`, `rate_previous` and `completions_projection*`. JSON carries the whole result |

## Projection rules

0015 Pace projection, with these readings where it is silent:

- **When.** Only with `compare: "typical_weekday"` and a range containing the application clock's now. A range entirely in the past or the future has `projection` absent. Only count measures project; rates and spans get `null`.
- **Basis day `k`.** The range shifted back `k` weeks. It counts only when today's baseline day `k` exists (it sets the scale) and every range day from today on has its baseline day `k`. So a range ending a week after today loses `k = 1`, whose day would be today.
- **Formula.** `p_k = observed + s_k × a_k`. `observed` is the range's measured total so far, `s_k = today / u_k` with `today` the count measured today so far and `u_k` today's baseline day `k` up to now's local time, and `s_k = 1` when `u_k < 20` of that measure's units. `a_k` is what the basis days add after now: the whole baseline days `k` of today and later range days, minus `u_k`.
- **Distinct counts.** `a_k` is whole minus until, also for visitor-days and sessions. A distinct count of `[now, end)` would count a visitor seen before and after now twice, and at scale 1 the projection would exceed the basis day's own total.
- **Multi-day ranges.** "This week" on Tuesday: `observed` is Monday plus Tuesday so far, the scale comes from Tuesday alone, and `a_k` is the rest of Tuesday `k` plus Wednesday to Sunday of week `k`. Today's measured part is materialized on its own (`zt_`) when the range starts before today.
- **Band.** Nearest-rank over the `p_k`, the same convention as the baseline median (lower middle of an even count): value = the ⌈n/2⌉-th smallest, low = ⌈n/4⌉-th, high = ⌈3n/4⌉-th. Of 8: 4th, 2nd, 6th. Rounded to 4 places. Under 4 basis days: `{ t: "unavailable", reason: "insufficient_history" }`.
- **Series.** Every bucket that ends after now gets a `ProjectedPoint`: its measured value so far plus the scaled baseline inside the bucket after now, with the same scale and basis days. Points before now have none. `points` keep the measured values.

## Baseline fixtures

Traffic rule (header of `baselines.rs`): a day of weight `w` has, per hour, `w` bouncing visitors at `:05` and one engaged visitor at `:10` and `:30`. Site A runs from Tuesday 2026-08-04 at weight 1, its eight Tuesdays before 10-13 weigh 1, 2, 3, 4, 50 (spike), 5, 6, 7, and 10-13 weighs 5. Clock Tuesday 2026-10-13 15:20 Vienna unless noted.

| Fixture | Expected | Derivation |
| --- | --- | --- |
| Today until 15:20, pageviews | baseline 95, today 111, change 16, 0.1684 | 16w + 31 at the median w = 4; the mean would give 187 |
| Visitor-days, sessions | 80 each; visitor-days +0.2 | 16 (w + 1) |
| Bounce rate, span mean, span median | 1248 / 1376 = 0.907; 111,628 ms; 0 | Pooled over eight days, not a median of rates |
| Path panel row | 80 | Same as the total |
| Hourly series | 24 points, each 6 | w + 2 at w = 4 from whole days, also after now |
| Week 10-13 to 10-20 by day | 144, 72 × 6, 168 | Today is never a baseline day: 10-20 reads 7 Tuesdays, median w = 5 |
| `previous_period` today | 47 until 15:20; series 3 per hour | Monday until 15:20 against the whole Monday |
| Site 3 weeks old | `insufficient_history` on totals and every point | 3 baseline Tuesdays |
| Traffic on Mondays only | 0 baseline, change 47, relative `null`, `zero_baseline`; both 0: change 0; bounce `no_denominator` | 0003 zero baselines |
| Retention clip, 2025-11-11 | 96 | Reads 10-14 to 11-04 (72 96 120 144); with the clipped days 120 |
| Spring forward, 2026-03-29 | 23 points, no 02:00, total 96 | Hour 2 empty on both sides |
| Sunday after, 2026-04-05 | hour 2: 4, other hours 5, total 115 | Hour 2 reads the empty spring-forward hour |
| Fall back, 2025-10-26 | 24 points, hour 2: 4, total 96 | One bucket holds both instants |
| Sunday after, 2025-11-02 | hour 2: 6, other hours 5, total 125 | The doubled hour holds 10 |

## Projection fixtures

`u_k`, `a_k` and `p_k` as in [Projection rules](#projection-rules). Every value below is hand-derived in the test comments and matched on the first run.

| Fixture | Expected (value, low, high, basis) | Derivation |
| --- | --- | --- |
| Pageviews at 15:20 | 168, 167.6643, 168.6076, 8 | `111 + 111 (8w + 17) / (16w + 31)`; the spike day gives 166.7004, the lowest |
| Visitor-days, sessions at 15:20 | 144, 144, 144, 8 | `96 + 96 × 8 (w + 1) / 16 (w + 1)` for every `k` |
| Path panel visitor-days | 144, 144, 144, 8 | The panel projects its own totals |
| Series hour 15 (measured 6 so far) | 7, 6.7762, 7.4051, 8 | `6 + 111 / (16w + 31)`; 9 projected buckets, 15:00 to 23:00 |
| Series hour 16 | 7, 6.986, 7.0253, 8 | `111 (w + 2) / (16w + 31)` |
| Scaling floor, clock 03:20, pageviews | 162.7826, 108, 167.2258, 8 | `u_k = 4w + 7` is 11, 15, 19 for w = 1 2 3, so scale 1: 88 108 128. Scaling all eight would give 168 |
| Scaling floor, visitor-days | 144, 84, 144, 8 | w = 4 sits exactly on 20 and scales |
| Week 10-13 to 10-20 | 977.9241, 734.1732, 1417.0213, 7 | Week 1 lacks 10-20 − 7 = today. Each basis week is scaled by today's pace |
| Site 3 weeks old | `insufficient_history`, totals and buckets | 3 basis days |
| Range before now (2025-11-11) | no `projection`, no `series.projection` | |
| Goals, signups at 15:20 | completions 80 of 96; baseline 64, +16, 0.25; rate baseline 1248 / 1376 = 0.907; projection 120, 120, 120, 8 | `u_k = 16w`, `a_k = 8w`; w = 1 is under the floor (88), the rest give 120 |
| Goals, past Tuesday 10-06 | 168 against 72, no projection | Median of 6 5 50 4 3 2 1 1 is 3 |
| CSV columns | `pageviews,pageviews_previous,…,pageviews_projection,pageviews_projection_low,pageviews_projection_high` | `today_compares_with_yesterday_until_the_same_time` in `apps/server/tests/explorations.rs` |

The until-now correction (item 9) has its own server test, `today_compares_with_yesterday_until_the_same_time`: 3 pageviews today against 2 on Monday until 15:20, with the bounds naming the whole Monday. The acceptance fixture `fixtures/correctness/expected/acceptance-exploration.json` lists event acc-06 as excluded: "previous totals stop at 2026-09-03 12:00 local, the offset of now into the range".

## Workload

Release `holocron-gate`, 10% load, 2 users, 20 samples per case, reference budgets, measurement clock 2026-10-01T00:00Z:

```
HOLOCRON_DISK_RESERVE_MIB=256 $B load --data GATE --groups-per-month 100000
HOLOCRON_DISK_RESERVE_MIB=256 $B sessions --data GATE
HOLOCRON_DISK_RESERVE_MIB=256 $B explore --data GATE --out OUT --observations 20 --contains=-30d- --compare previous_period
HOLOCRON_DISK_RESERVE_MIB=256 $B explore --data GATE --out OUT --observations 20 --contains=-30d- --compare typical_weekday
```

Baselines without projection, as recorded when they landed (49a63b5): every 30-day case under 2 s with `typical_weekday`. Unfiltered Q01 80 to 1242 ms, Q02 61 to 562 ms. Filtered cases took 3 to 9 times as long, the worst Q12-pivot at 1664 ms. The overview default (today, hourly) took 50 to 168 ms.

None of the declared cases contains the clock, so none of them projects. Three scratch cases (copies of Q01-30d-A-vienna and Q04-30d-A-vienna-selective with other ranges, run through `--fixtures` and then deleted) measured the projection, before (114073b) and after (26a733a), p95 in ms:

| Case | `previous_period` | `typical_weekday` before | `typical_weekday` with projection |
| --- | --- | --- | --- |
| Today hourly, unfiltered, 5 panels | 51 | 174 | 230 |
| Today, filtered table (Q04 selective) | 35 | 82 | 138 |
| 2026-09-02 to 10-01 hourly, unfiltered | 111 | 1750 | 1753 |

The projection adds about 56 ms on a day range. Most of it is the whole baseline days a request without a series now materializes, and the hourly reads of the until-now windows.

**The 10% load understates baselines.** A partial load fills only the first days of each month (`Fixture::month`: "smaller limits cover its first days only"), so `--groups-per-month 100000` holds events on the 1st to 3rd of each month. Of the 79 baseline days of a 30-day September case, about ten hold events, and 2026-10-01 holds none. The numbers above measure the per-window overhead (statements, temp tables, rollup reads), not scan volume. The 30-day range containing now at 1753 ms is already close to the 2 s target.

### Grouped baseline read

Before, every baseline window was its own materialization and its own totals, rows, bucket and median statements: 79 whole days plus up to 8 parts up to now for a 30-day range, each with several statements. Now (`baseline.rs`, `run::totals_by`, `rows_by`, `buckets_by`, grouped `rollup` cells):

- The whole baseline days form runs of consecutive days: one run `[start − 56 d, end − 7 d]` for a range of 7 days or more, up to 8 single-day runs for a shorter one, whose baseline days are a week apart. Each run is materialized once (`zb`): from the rollups when the request reads them, else from events through the existing 31-day chunking, so a 79-day window is three statements.
- Today's baseline days up to now (`zu`) are read from events when `zb` comes from rollups. When `zb` comes from events, it already holds them.
- The view `zs_ev` (and `zs_se`, `zs_days`) puts each row in the slot `g` of its window: the local day number for a whole day, and `−1 − day` again for a row before now's local time on today's baseline days. Totals, page rows and hourly buckets are each one query grouped by `g`. Span medians are one query that joins the sessions to a list of `(g, set)` pairs, so a day shared by two range days still counts twice.
- Rollups serve only days that lie inside one baseline day and carry its date, so a served day's slot is that day's. When a run is not exact, the whole window falls back to events, as before per window.

Results are unchanged. Every fixture in `tests/baselines.rs` and the server tests pass without edits, and `matches_raw` stays true.

Same load and commands, `--compare typical_weekday`, p95 in ms. Run 1 has before (fd8c839) and after back to back. Run 2 repeated both while other work drove the machine's load average from 30 to 120, so its absolute numbers are inflated. The ratio is the signal. `previous_period` is run 1 on fd8c839.

| Case | `previous_period` | before, run 1 | after, run 1 | before, run 2 | after, run 2 |
| --- | --- | --- | --- | --- | --- |
| Q01-30d-A-vienna | 92 | 1366 | 388 | 5364 | 515 |
| Q01-30d-D-utc | 69 | 1326 | 236 | 6750 | 357 |
| Q02-30d-A-vienna | 64 | 650 | 222 | 3446 | 214 |
| Q02-30d-A-vienna-page-99 | 70 | 661 | 186 | 3047 | 326 |
| Q02-30d-A-vienna-tail | 59 | 238 | 187 | 1205 | 253 |
| Q02-30d-D-utc | 44 | 620 | 115 | 2481 | 171 |
| Q03-30d-A-vienna | 94 | 839 | 320 | 3700 | 302 |
| Q03-30d-A-vienna-drill-at | 89 | 601 | 312 | 2212 | 494 |
| Q03-30d-D-utc | 67 | 481 | 176 | 2093 | 466 |
| Q04-30d-A-vienna | 165 | 744 | 577 | 2465 | 891 |
| Q04-30d-A-vienna-broad | 200 | 590 | 676 | 2617 | 816 |
| Q04-30d-A-vienna-selective | 87 | 692 | 221 | 2407 | 380 |
| Q04-30d-D-utc | 88 | 478 | 271 | 2377 | 929 |
| Q12-30d-A-vienna-broad | 148 | 570 | 491 | 2545 | 437 |
| Q12-30d-A-vienna-pivot | 241 | 1745 | 914 | 5524 | 986 |
| Q12-30d-A-vienna-selective | 37 | 309 | 130 | 1176 | 107 |
| Q12-30d-A-vienna-session | 256 | 1164 | 841 | 3916 | 1292 |
| Q12-30d-A-vienna-tail | 112 | 851 | 473 | 2292 | 732 |

The suite ran in 143 s before and 73 s after in run 1. Q05 to Q11 are reports or refuse `typical_weekday`, so they don't read baselines. Q04-broad stays about even, since its broad filter makes the event scan itself the cost.

Ranges containing the clock, scratch copies run through `--fixtures` (deleted afterwards), at load average 120. `now30d` is 2026-09-02 to 10-01, `now1d` is 2026-10-01 alone:

| Case | `previous_period` | before | after |
| --- | --- | --- | --- |
| Q01-now30d-A-vienna, hourly, unfiltered | 538 | 9453 | 655 |
| Q04-now30d-A-vienna-selective, filtered | 239 | 3267 | 293 |
| Q01-now1d-A-vienna, hourly, unfiltered | 193 | 1588 | 540 |
| Q04-now1d-A-vienna-selective, filtered | 169 | 832 | 245 |

A one-day range keeps 8 single-day runs and 8 up-to-now reads, so it saves the per-window reads but not the per-window materializations.

**Full scale not measured.** The 124M-record fixture (`load --groups-per-month 1000000`, about 13 GB) needs 40 GB free afterwards. Other work on the machine moved free disk between 36 and 89 GB during this work. One attempt started at 56 GB free and was stopped by its disk guard at 36 GB free, after 123M of 124M records were admitted and 18 GB written, before `sessions` and any explore run. The gate data was deleted. The full-scale run followed in [v09 Workload](v09-acceptance.md#workload).

## Query plans

From `holocron-gate explore --plans --compare typical_weekday` on the 10% load after the grouped read, trimmed (the files say what was cut). The earlier per-window plans were removed, since those statements no longer run.

- [Q01-now30d-A-vienna.txt](v01/plans/Q01-now30d-A-vienna.txt), unfiltered, range containing the clock: the baseline window's event remainder (`zb`, the rest served from rollups), one of today's baseline days up to now from events (`zu`), and the grouped row read of a panel: `dimension_aggregates` joined to the served days with their slot `g`, unioned with the remainder grouped by `g`.
- [Q04-30d-A-vienna.txt](v01/plans/Q04-30d-A-vienna.txt), filtered: the first 31-day chunk of the 79-day window, a `SEQ_SCAN` of `events` with the site, kind and time filters joined to `session_members` (role 1), `sessions` and the property partitions, then the grouped row read over `zs_ev`.

Regenerate them with the `explore` commands in [Workload](#workload) plus `--plans`.

## Commands

```
cargo fmt --all --check
cargo clippy --workspace --all-targets
cargo test -p holocron-analytics -p holocron-protocol -p holocron-server -p holocron-workload
cargo run -p holocron-protocol --bin export-types -- --check
cd apps/web && bun install && bun run typecheck
```

## Open items

- Mike: at hour granularity the totals use the median of the baseline days' day-so-far totals, not the sum of the hourly medians, so a total and its hourly line can differ slightly.
- Mike: on a multi-day range each basis week is scaled by today's pace (0015 read literally), which makes the band wide when weeks differ. The week fixture spans 734 to 1417. A narrower reading scales only today's rest and adds the later days at their typical value.
- Mike: person-scoped goals on a multi-day range add up per-day medians, which count a profile once per day, while completions count it once per range. The overview reads today only, where both agree.
- Full-scale workload: run on 2026-10-15 in [v09 Workload](v09-acceptance.md#workload). Site A 30-day cases miss 2 s in both comparison modes; the failing cases, measured cause and alternatives are there.
