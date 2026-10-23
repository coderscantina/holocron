# v02 evidence: site summaries and findings

Status: in progress. Work items 1 to 8 are built. The gate workload runs (500 events/s burst with two dashboard users, the 124M-record fixture, the step's share of engine time) are not run (see Open items).

## What shipped

- v02-a: the `summaries` background task (`apps/server/src/summaries/`), migration `0012_site_summaries.sql`, `summaries.forSites`, the `summary` event on `analytics:<site>`, and the counters `summary_runs`, `summary_skips`, `summary_timeouts` and `summary_run_ms` in `operator.status.collection.counters`. The counters are install-wide, not per site.
- v02-b: four rule families in `apps/server/src/summaries/findings/` behind one `Rule` trait (`rule`, `version`, `hourly`, `findings(site, snapshot)`), findings stored with the summary row in one transaction, `summaries.findings { site, limit }`, threshold constants in `crates/protocol/src/limits.rs` (exported to `limits.ts`), and `holocron_analytics::explore::observations`, the one new analytics function: percentile results carry no observation count, and a state that reads observations may measure only percentiles.

## Acceptance criteria

| Criterion | Result | Evidence |
| --- | --- | --- |
| Each rule family fires exactly on its hand-computed cases and stays silent on near misses on both sides of every threshold, clock pinned | Passed | Unit tests per family at and just past each threshold: `findings::mover::tests` (20 visitor-days, 1.5 and 0.67 times, 25 % of the site's change, direction, zero site change), `goal_pace::tests` (30 %, 5 completions, both directions), `funnel_step::tests` (5 points, 50 entering today and on the baseline days), `performance::tests` (strictly above the slowest day, 4 days with observations, `cls` units). End to end, `every_family_fires_on_its_fixture_ranks_and_shows_its_values` in `tests/summaries.rs` builds one Vienna site over four Tuesdays with the arithmetic in its doc comment: `/sale` moves 15 to 60 (share 0.7031) while `/near` moves by 19; `signup` 10 to 15 while `newsletter` reaches 14; the "Sale to buy" funnel drops 50 % against a pooled 40 % while "Near to buy" has 29 entering; phone LCP 2500 over a slowest day of 2400 with 100 observations while phone FCP equals its slowest day and desktop has 99 observations. Exactly 8 findings fire. |
| Findings rank by family, then share, then size of change | Passed | `findings::tests::findings_rank_by_family_then_share_then_change`, and the end-to-end order: goal pace, performance, five movers (four at share 1 before `/sale` at 0.7031), funnel step. |
| A finding's stored state, run through `analytics.explore`, shows the values the finding states | Passed | The same test validates and runs every stored state over HTTP and compares: mover totals and typical value, goal completions and baseline, funnel step reached and drop, performance p75. States are built from protocol types and checked with `explore::validate` before storing. |
| No finding text claims a cause or relates two findings | Passed | No text is stored or generated. A finding is a rule ID, version, rank, window start, share, state and typed values (`FindingValues`, tagged by `kind`); the only string a user wrote is a funnel card's title. |
| The step never runs while a report waits | Passed | `a_round_waits_while_a_report_waits_or_transfer_lags` (v02-a). Every rule read checks `reports_idle` again first; a rule that cannot start keeps its last findings and is due next run. |
| Findings are replaced as a set per run; hourly rules keep theirs | Passed | Second half of the end-to-end test: after archiving `signup` and one more newsletter, the next run lists `newsletter` and no `signup`; performance and funnel findings keep their first `computed_at`, and all eight carry the new time an hour later. |
| 500 events/s burst with two dashboard users keeps the release gate targets; step's share of engine time | Not run | `holocron-gate` has no `AppState`, control database or sites, so it cannot run `summaries::round` beside the explore suite. |
| 124M-record fixture: every site summarized within 10 minutes while two users run the suite | Not run | Same reason; no full-scale gate directory on this machine. |
| Erasing a site removes its summary and findings; a restored backup recomputes them | Passed, restore by design | `reset_and_erasure_remove_the_summary_and_findings` (v02-a). The schedule and the kept findings live in memory, so every site is summarized again in the first round after start, a restored backup included; until then a restored row older than 30 minutes reads as stale. A finding computed before a queued reset or erasure is never written: the erasure guard uses the oldest `computed_at` being written. |
| A user without access to a site gets no summary or findings and no hint that it exists | Passed | `only_readers_get_a_summary_or_findings_and_their_stream_hears_of_it`: `summaries.findings` answers `not_found` for a hidden site with the same bytes as an unknown one; limits 0 and 21 are refused. Access rows AX-G1134 to AX-G1146 (`forSites`) and AX-G1147 to AX-G1159 (`findings`, like `goals.list`; API keys as for `forSites`). |
| Generated types have no drift | Passed | `export-types -- --check`: up to date, 330 files. |

## Rules as built

| Family | Reads | Fires when | Cadence |
| --- | --- | --- | --- |
| Goal pace | One `goals` report, today, `typical_weekday`, active goals oldest first (8 at most) with condition IDs renumbered so they stay unique | typical > 0, \|change\| ≥ 5 and ≥ 30 % of typical | every run |
| Performance | p75 by `perf_metric` and `device` over the last 24 whole hours (yesterday from the current hour, today before it, as a weekday and hour filter); per pair the observation count and one day-granularity series over the 28 days before yesterday | ≥ 100 observations, ≥ 4 days with observations, p75 strictly above the slowest day | hourly |
| Dimension mover | One exploration: visitor-days by `path` with `referrer_domain`, `browser`, `os`, `device` as panels, today's top 100 values, `typical_weekday` | typical > 0, \|change\| ≥ 20, ratio ≥ 1.5 or ≤ 0.67, same direction as the site's change and ≥ 25 % of it | every run |
| Funnel step | Daily-scope funnels pinned to boards for the site (8 at most, duplicates once), today and the pooled baseline days, both from midnight to the current hour | ≥ 50 entering today and on the baseline days, drop-off moved ≥ 5 points | hourly |

## Commands

```
cargo fmt --all --check
cargo clippy --workspace --all-targets
cargo test -p holocron-analytics -p holocron-server
cargo run -p holocron-protocol --bin export-types -- --check
cd apps/web && bun install && bun run typecheck
```

## Open items

- Gate runs: the burst, the 124M fixture and the engine-time share need a gate harness that can run `summaries::round` (an `AppState` with control database and sites).
- Only movers have a site total, so `share` is 0 for the other families and they rank by the size of their change.
- Funnels saved only as personal explorations are not read: a finding is visible to every reader of the site and would show a private exploration's steps.
- A mover reads today's top 100 values per dimension, so a value that vanished today is not seen.
