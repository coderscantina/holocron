# 0015: Typical-weekday baseline, pace projections, and noticed changes

Status: Adopted by Mike on 2026-10-15 for the [dashboard v2](../ux-v2/README.md) overhaul, as proposed plus the cross-family ranking and the fallback cadence below. Decisions records each answer.

## Problem

The v2 overview answers three questions the current one leaves to the reader: how today compares with a normal day, where the current day or range is heading, and which few changes explain the difference. [0004](0004-exploration-state-and-limits.md) offers only `previous_period`, which compares a Tuesday with a Monday. The product counts stored events exactly and has no estimated value anywhere. Nothing computes changes in the background.

## Constraints

- Exact stored-event counts, no hidden sampling (technical briefing). An estimate may appear only when it is labelled and kept apart from measured values. Relaxing exactness is Mike's decision ([development workflow](../development-workflow.md#decisions-before-dependent-work)).
- Zero baselines and missing denominators follow [0003](0003-metric-formulas.md) Comparison and zero baselines: no infinity, NaN or invented 100 %.
- `compare` is a string enum; a new variant does not bump the state's `v` ([0004](0004-exploration-state-and-limits.md) Rejected alternatives).
- Budgets per installation, not per site ([0008](0008-storage-engine-and-budgets.md) Budgets): two report workers, one live worker, one analytical writer, 30 s timeout. Unfiltered reports read `day_aggregates`, `hour_aggregates` and `dimension_aggregates` (`path`, `referrer_domain`, `browser`, `os`, `device`).
- Retention removes events at least hourly and queries apply the cutoff as a predicate ([0006](0006-retention-clock-and-operations.md)). A baseline cannot reach before `retention_start`.
- `weekday` and `hour` are wall-clock values in the state's timezone, with the DST rules of 0004 Weekday and hour.

## Proposed choice

### Typical-weekday baseline

New comparison variant `compare: "typical_weekday"`.

- For each local day `d` in the resolved range, the baseline days are `d − 7k` for `k = 1..8`, clipped at `retention_start` and at the site's first stored event.
- Count measures (`pageviews`, `events`, `visitor_days`, `sessions`, `hits`, `revenue_count`): the baseline of a series bucket is the median of that bucket's value over the baseline days. Hour buckets match by local wall-clock hour, so a spring-forward day has an empty hour 2 on both sides. The baseline total of the range is the sum of its bucket baselines, so the line and the total agree.
- When the range contains now, the comparison value of a total uses each baseline day only up to the same local time, so "12 % above a typical Tuesday" at 15:20 compares 00:00 to 15:20 with 00:00 to 15:20. The rest of each baseline day feeds only the chart's baseline line and the projection. Today `previous_period` compares today so far with all of yesterday ([v01](../ux-v2/v01-baseline-and-projection.md) lists this as a separate fix).
- Rates and spans (`bounce_rate`, `span_mean`, `span_median`): computed over the pooled baseline days, as one population. A median of eight rates would weight a quiet day like a busy one.
- The `goals` report kind accepts it: completions are counts and get a median baseline per goal; the rate is pooled. This is what the overview's goal rows need. Every other report kind keeps `compare: "none"`.
- Refused with `compare_unsupported`: money measures, percentiles, `identified_people`, the other report kinds, and ranges longer than 31 days. A 12-month range would need 14 months of history, which retention removes.
- Series gain comparison points (today `Series` has none, so the chart cannot draw a baseline line). The same field serves `previous_period`.
- Fewer than 4 baseline days for a bucket: baseline `null` with `reason: "insufficient_history"`. The UI says "not enough history yet".
- Resolved bounds echo `compare_days` (the local dates used) instead of `compare_start`/`compare_end`.

Rejected:

- Mean: one launch day doubles next week's typical value. The 33rd-birthday spike in the reference is exactly that case.
- Previous period as the default for a single day: compares different weekdays.
- Seasonal or learned models: not explainable in one sentence, and not cheap enough to run on the reference host for every site.

### Pace projection

A projection exists only when the resolved range ends after the application clock's now, and only for count measures.

- `observed` is the exact value up to now. For each baseline day `k`, `scale_k = observed_today / baseline_k(start of today, now)` and `projection_k = observed + scale_k × baseline_k(now, end)`. If a baseline day has fewer than 20 counted units before now, `scale_k = 1`, so a quiet morning does not multiply noise.
- The result is `{ value: median(projection_k), low, high, basis_days }`, where `low` and `high` are the 25th and 75th percentiles of the `projection_k`. The series carries projected buckets the same way, in a separate array.
- The projection never enters `totals`, `rows` or `series`. It has its own field, and exports carry it only in a separately named column.
- Fewer than 4 basis days: no projection, `reason: "insufficient_history"`.

The UI rule that comes with it: measured values are solid and estimates are dashed or marked "estimate", and every projected number names its basis ("estimate from the last 8 Tuesdays").

Rejected: a straight-line extrapolation of the elapsed fraction of the day. It ignores the daily curve: at 09:00 it projects the night's traffic for the whole day.

### Noticed changes

A background step computes at most a handful of deterministic findings per site. It never generates text with a model; every finding is a fixed template filled with measured values, and it carries the exploration state that shows it.

Rule families, first set:

| Family | Source | Fires when |
| --- | --- | --- |
| Dimension mover | `dimension_aggregates` for `path`, `referrer_domain`, `browser`, `os`, `device` | today-so-far vs typical-weekday-so-far for one value: absolute change ≥ 20 visitor-days, ratio ≥ 1.5 or ≤ 0.67, and the value explains ≥ 25 % of the site's total change |
| Goal pace | saved goals, `goals` report kind, today | completions differ from the typical-weekday baseline at this hour by ≥ 30 % and ≥ 5 completions |
| Funnel step | saved funnels, today vs typical weekday | a step's drop-off changes by ≥ 5 percentage points with ≥ 50 sessions entering the step |
| Performance | `p75` per `perf_metric` and `device`, last 24 h vs the previous 28 days | the 24 h value exceeds the slowest daily value of those 28 days, with ≥ 100 observations |

- Ranking across families by fixed priority: goal pace, performance, dimension mover, funnel step. Within a family: share of the site's total change first, then absolute change. The overview shows three; the rest are one click away.
- Statements are facts only: "LCP p75 3.4 s on Android, slowest in 30 days". The step never claims a cause or links two findings ("likely one cause" in the first concept is out).
- Each finding stores its exploration state (0004 v1), the measured values, the rule ID and version, the analytical watermark and `computed_at`. Actions in the UI are operations on that state: filter, open the report, list recordings with the same filter, add an annotation.
- Thresholds are constants in the protocol so the UI can explain why something fired.

Rejected: anomaly scores over every dimension pair. On the reference host it is a full scan per site per run, and its findings cannot be explained in one sentence.

### Where it runs

- A `site_summaries` step runs per site after the analytical watermark advances, at most every 10 minutes per site, and only while the report queue is empty and the oldest inbox record is under 10 s old (as compaction yields in [0014](0014-operations-and-recovery.md)). It reads through the report executor with the normal timeout, one site at a time, so it can never take both report workers.
- It runs as a task next to compaction in `apps/server/src/background.rs`, not as a `maintenance_jobs` entry: those run one at a time, and an erasure retried every second must not starve it, nor it them.
- It writes one row per site to control storage (SQLite): today-so-far and baseline-so-far for pageviews and visitor-days, pace, the findings, the watermark, and `computed_at`. The dashboard reads that row with one SQLite query; opening the site menu or the overview never starts a summary computation.
- A row older than 30 minutes, or computed under an older metric version, is shown as stale: the header and site menu drop the pace and keep the name.
- Fallback: if the step with all four families breaks the report latency targets on the reference budget, performance and funnel step run at most hourly per site; dimension mover and goal pace keep the 10-minute cadence.
- The row is a derived record with its own watermark, not a report cache: the browser's result cache stays in Vue Query ([development workflow](../development-workflow.md#shared-changes)).
- The overview's own headline and chart still come from `analytics.explore` with `compare: "typical_weekday"` in the same snapshot as everything else on the page. The summary row feeds only the header, the site menu, the workspace page and the noticed cards.

## Decisions

Decided by Mike on 2026-10-15.

1. Baseline: `typical_weekday` with the median of the same weekday over 8 weeks, at least 4 baseline days, ranges up to 31 days, `goals` accepted.
2. Projections ship as a value with a 25th to 75th percentile band and the 20-unit scaling floor, under the labelling rule above.
3. All four rule families ship in the first set: dimension mover, goal pace, funnel step and performance. The thresholds in the table are the starting values. Findings rank by family priority, then within a family as above.
4. The overview defaults to today with hour granularity and `typical_weekday`. The hourly chart in the Kit contract is the default view. Existing URLs and saved views keep their state.
5. The summary step keeps the proposed cadence (10 minutes, idle queue, stale after 30 minutes), with the hourly fallback for performance and funnel step.

## Consequences

- `compare` gains a variant, resolved bounds gain `compare_days`, results gain `projection` fields, and three error reasons are added. All generated from Rust.
- A new control migration for `site_summaries` and the findings.
- The correctness fixture needs DST weeks, a site younger than 8 weeks, a retention-clipped baseline and a spike week, each with hand-computed expectations.
- The performance suite gains the summary step: every site of the 124M-record fixture once, with two report users running at the same time, and their latency targets held.

## Affected contracts

[0003](0003-metric-formulas.md) Comparison, [0004](0004-exploration-state-and-limits.md) state shape and limits, [0008](0008-storage-engine-and-budgets.md) budgets, [0014](0014-operations-and-recovery.md) jobs, [Kit implementation](../kit-implementation.md), [workload and query suite](../contracts/workload-and-query-suite.md).
