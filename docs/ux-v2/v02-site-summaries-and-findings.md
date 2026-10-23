# v02: Site summaries and findings

## Outcome

A background step keeps one small summary per site: today's pace against a typical weekday, and the site's current findings. The site chip, site menu, workspace page, sidebar badges and finding banner and cards read it with one SQLite query each. Opening a menu or a page never starts an analytical computation.

## Dependencies and ownership

Requires v01. All four rule families of [0015](../decisions/0015-baselines-projections-and-noticed-changes.md) ship with its starting thresholds, family ranking and fallback cadence. Analytics owns the rules; server owns the step, storage and queries; the protocol owner merges the operations; the storage owner orders the migration.

Owned: a new `apps/server/src/summaries/` module, its task registration in `apps/server/src/background.rs`, a control migration after `0010_collection_keys.sql`, a new `crates/protocol/src/operations/summaries.rs`, and tests. Read first: 0015 Noticed changes and Where it runs, [0014](../decisions/0014-operations-and-recovery.md) Jobs and Compaction (how background work yields), [0008](../decisions/0008-storage-engine-and-budgets.md) Budgets, [0003](../decisions/0003-metric-formulas.md) Goals and Funnels.

## Current code (2026-10-15)

- `Background::start` in `apps/server/src/background.rs` starts six tasks under one shutdown signal: day-key destruction, counter flush, recordings maintenance, the job queue, compaction, and correction (`holocron_analytics::follow`).
- Compaction yields while the oldest inbox record is over 10 s old; reuse that check, do not copy it.
- Goals are versioned (`goals`, `goal_versions`, migration `0006`; `goals.list`). Funnels exist only inside saved explorations. Nothing computes findings or alerts today.
- `workspaces.list` returns sites with id, name and domain only.

## Work

1. Add a `summaries` task to `Background::start`. Per site, it runs when the analytical watermark has advanced since its last run and at least 10 minutes have passed, and only while the report queue is empty and ingestion is current. It runs one site at a time through the report executor with the normal 30 s timeout. A site that times out is skipped with a logged outcome and retried next round.
2. Compute the summary through v01's functions, not new SQL: today-so-far and typical-so-far for pageviews and visitor-days, relative pace, 24 hourly pageview values for today and for the typical day (v06's sparkline), and `insufficient_history` when it applies.
3. Implement the rule families from 0015 as one module per family behind a small `Rule` trait: `id`, `version`, and a function from site context to findings. Dimension movers read `dimension_aggregates`; goal pace runs one `goals` report with all the site's active goals (8 at most); funnel step runs each funnel saved on a board or as a saved exploration of that site, at most 8; performance reads `p75` per `perf_metric` and `device`.
4. Each finding stores: rule ID and version, kind, the measured values the template needs, the exploration state that shows it (0004 v1, validated like any saved state), the start of the window, share of total change, and rank. Text is not stored; the UI renders templates from values.
5. Store one row per site in `site_summaries` (site, computed_at, watermark, metric_version, pace fields, finding count) and the findings in `site_findings` (replaced as a set per run in one transaction). Erasure and reset of a site delete both, inside the existing erasure job.
6. Add queries: `summaries.forSites { sites: SiteId[] }` (1 to 100 sites, each authorized; unauthorized IDs are dropped without disclosure) and `summaries.findings { site, limit }`. Results carry `computed_at`, the watermark and a `stale` flag computed by the server (older than 30 minutes or older metric version).
7. Publish a topic event when a site's summary changes, so open dashboards can refetch the one small query instead of polling.
8. Count runs, skips, timeouts and run time per site in the collection counters, so v07 can show whether the step keeps up.

## Acceptance criteria

- With the clock pinned and the fixture from v01, each rule family fires exactly on its hand-computed cases and stays silent on the near misses on both sides of every threshold.
- A finding's stored exploration state, run through `analytics.explore`, shows the values the finding states.
- No finding text claims a cause or relates two findings.
- The step never runs while a report waits, and a 500 events/s burst with two dashboard users keeps the collection and report targets of the release gate. Record the step's share of engine time.
- 124M-record fixture: every site gets a summary within 10 minutes of a watermark change while two users run the declared suite.
- Erasing a site removes its summary and findings; a restored backup recomputes them rather than serving old ones.
- A user without access to a site gets no summary for it and no hint that it exists.
- Generated types have no drift.

## Checks

`cargo fmt --all --check`, `cargo clippy --workspace --all-targets`, `cargo test -p holocron-analytics`, `cargo test -p holocron-server`, export-types `--check`, and the burst and suite runs with the gate tool. Record the commands.

## Handoff

Evidence at `docs/evidence/v02-site-summaries-and-findings.md`. v03, v04, v05 and v06 consume the two queries and the topic.
