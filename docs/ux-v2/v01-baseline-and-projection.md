# v01: Typical-weekday baseline and pace projection

## Outcome

`analytics.explore` can compare a range with a typical weekday and, when the range contains now, return a labelled projection of where it ends, both from the same snapshot as the rest of the result. The chart can draw a baseline line because series carry comparison points.

## Dependencies and ownership

Follows [0015](../decisions/0015-baselines-projections-and-noticed-changes.md), adopted 2026-10-15. Analytics owns resolution, planning and execution; the protocol owner merges the wire changes and regenerates TypeScript. No frontend files.

Owned: `crates/protocol/src/exploration.rs` (compare, comparison, series, projection types), `crates/protocol/src/limits.rs` (new constants), `crates/protocol/src/errors.rs` (new reasons), `crates/analytics/src/explore/resolve.rs`, `run.rs`, `rollup.rs`, `sql.rs`, `report/goals.rs`, and their tests. Read first: [0003](../decisions/0003-metric-formulas.md) Comparison and zero baselines, [0004](../decisions/0004-exploration-state-and-limits.md) State shape, Weekday and hour, Report definitions and Limits, [0006](../decisions/0006-retention-clock-and-operations.md) Retention cutoff and Application clock, [0008](../decisions/0008-storage-engine-and-budgets.md) Rollups and Budgets.

## Current code (2026-10-15)

- `Compare { None, PreviousPeriod }` in `crates/protocol/src/exploration.rs`. `resolve.rs` `resolve()` returns the previous span of equal local length.
- `Comparison { previous, change, relative_change, reason }` hangs on `Cut.compare` and `Row.compare`. `Series` and `SeriesPoint` carry no comparison.
- Relative ranges cover whole local days, so "today" ends at the next midnight and `previous_period` compares today so far with all of yesterday.
- Unfiltered requests read `day_aggregates`, `hour_aggregates` and `dimension_aggregates` per the source rule in 0008 Rollups; anything else scans events.

## Work

1. Add `Compare::TypicalWeekday` and resolve its baseline days per 0015: `d − 7k`, `k = 1..8`, clipped at `retention_start` and the site's first stored event. Echo them as `compare_days` in the resolved bounds.
2. Compute baselines per 0015: median per bucket for count measures, pooled population for rates and spans, refusal (`compare_unsupported`) for money, percentiles, `identified_people`, report kinds other than `goals`, and ranges over 31 days. `insufficient_history` below 4 baseline days, per bucket.
3. When the range contains the application clock's now, compute total comparisons against the baseline up to the same local time. Take now from the server's application clock (`apps/server/src/clock.rs`, `Clock::frozen` in tests) and pass it into the analytics crate with the request, never the request's or the engine's wall clock.
4. Add comparison points to `SeriesPoint` (one optional comparison metric per measure) for both `previous_period` and `typical_weekday`. Previous-period points align by bucket offset; typical-weekday points by local weekday and wall-clock hour.
5. Add `projection` to totals and series as separate fields: `{ value, low, high, basis_days }` per count measure, computed per 0015 Pace projection. Never fold it into `totals`, `rows` or `points`' measured values. Exports name it in its own column.
6. Accept `typical_weekday` on the `goals` report kind: per-goal median completions, pooled rate, and a projection of completions by the end of the range.
7. Serve unfiltered baselines from the rollups. Eight weeks of `hour_aggregates` for one site is a small read; a filtered request scans the baseline days through the existing 31-day chunking. Both run in the same DuckDB snapshot as the current values.
8. Add the three reasons (`compare_unsupported`, `insufficient_history`, and the existing `zero_baseline` for baseline zeros) to the generated error and reason types. Add the limits (8 baseline weeks, 4 minimum days, 31-day range, 20-unit scaling floor) to `limits.rs`.
9. Separate correction, adopted in [0003](../decisions/0003-metric-formulas.md) Comparison and zero baselines: make `previous_period` on a range containing now compare up to the same local time. Ship it as its own change so it can be reviewed apart from v2.

## Acceptance criteria

- Hand-computed fixtures pass: a site with 10 weeks of hourly data; a spike Tuesday that the median ignores; a site 3 weeks old (`insufficient_history`); a baseline clipped by `retention_start`; both DST transitions in Europe/Vienna, where hour 2 is empty or doubled on both sides; zero baselines per 0003.
- With the clock pinned to Tuesday 15:20, the totals compare 00:00 to 15:20 on both sides, the series has baseline points for all 24 hours, and the projection's value, low and high match the fixture.
- A filtered and an unfiltered request over the same data return identical baselines when the filter matches everything.
- Totals, rows, series and projection come from one snapshot: the same `watermark` and `metric_version` on all of them.
- Refusals name the field (`compare`) and reason. No refused state silently falls back to `previous_period`.
- Generated types have no drift (`export-types --check`, `crates/protocol/tests/drift.rs`).
- Workload: the declared 30-day suite with `typical_weekday` added keeps its p95 target with two users on the reference budget. Record the extra cost against the same cases with `previous_period`.

## Checks

`cargo fmt --all --check`, `cargo clippy --workspace --all-targets`, `cargo test -p holocron-analytics`, `cargo test -p holocron-protocol`, `cargo test -p holocron-server` for the query path, `cargo run -p holocron-protocol --bin export-types -- --check`. Workload numbers come from the gate tool used in [m06 evidence](../evidence/m06-traffic-and-exploration.md); record the command you ran.

## Handoff

Evidence report at `docs/evidence/v01-baseline-and-projection.md` with fixture tables, query plans for one filtered and one unfiltered case, and the workload comparison. v02 consumes the baseline functions; v04 consumes the wire types.
