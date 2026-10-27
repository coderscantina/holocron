# Milestone 15 evidence: report queries for agents

Status on 2026-10-27: complete. The work merged into `main` locally on 2026-10-27 and is not released yet. Built on macOS (Darwin 27, arm64) with Rust 1.99.1, Bun 1.4.4 and Playwright Chromium 1.64.1. The rules are in [0022](../decisions/0022-agent-report-queries.md).

## Acceptance criteria

| Criterion | Result | Evidence |
| --- | --- | --- |
| One report query over the seeded commerce site returns views, visitors, a grouped funnel with durations, revenue by currency, sources and the weekday-hour grid within 30 s | Passed | The `event_sales_report` MCP recipe ran its eight blocks against a seeded server (Roadhouse commerce) through `/mcp`, as did the other three recipes. The first try was refused at `blocks.3.body.sort` (sorting revenue needs the currency dimension); the recipe and the guide were fixed. |
| Each block equals the matching `analytics.explore` result on the same snapshot, for a cut, a funnel and a goals report | Passed | `blocks_answer_the_numbers_of_their_explorations_from_one_snapshot` (`apps/server/tests/report_queries.rs`) |
| A grouped funnel's groups add up to the ungrouped funnel; durations agree with a hand-computed fixture | Passed | `groups_split_the_funnel_by_the_entering_event_and_add_up_to_it`, `durations_are_timed_from_the_previous_step_and_from_the_first`, `at_most_fifty_groups_are_returned` (`crates/analytics/tests/funnels.rs`) |
| `matches` follows its glob rules; `not_matches` is the exact complement | Passed | Glob unit tests in `crates/analytics/src/explore/glob.rs`, `matches_needs_a_glob_field_and_lists_take_the_caller_limit` (`crates/analytics/tests/explore.rs`). Browser: `/*/events/**` keeps `/de/events/seefest` and drops `/de/events`. |
| Refusals name their path; over-limit requests are refused before running; `session_list` is refused; no session IDs in results | Passed | `refusals_name_their_path_before_anything_runs`, `for_each_and_refs_take_values_from_earlier_blocks_and_derive_shares` |
| A key without the grants is refused on HTTP and MCP alike; a key limited to one site cannot read another site's block | Passed | `a_key_reads_blocks_on_its_own_sites_only`, `a_key_without_the_grant_gets_a_tool_error`, `requests_without_a_valid_key_get_a_bearer_challenge` (`apps/server/tests/mcp.rs`), access fixture rows AX-G1388 to AX-G1399 and AX-G801 |
| An MCP client lists the tools, reads the schema and runs a recipe against a local server | Passed | Claude Code 2.1.311: `claude mcp add --transport http` showed Connected; a headless run listed the 5 tools and 2 resources and called `list_sites`. Headless mode did not list prompts; they answer over plain JSON-RPC (`a_prompt_fills_in_its_arguments`). |
| Rust and web checks pass; dashboard screens pass visual acceptance at 1440, 1024, 768 and 390 px, light and dark | Passed | See Checks run and Visual acceptance |

## What was built

- **Planner:** `matches` / `not_matches` glob operators. Funnel v2 with `group_by` (up to 50 groups, computed in one pass). Step durations on every funnel. A 1,000-item list limit for report queries.
- **Report queries:** `analytics.report`, with named blocks on one snapshot, `for_each`, refs, derived columns, flat tables with notes, and the budget checked before anything runs.
- **Transports:** `POST /api/queries/{name}`. `/mcp` over Streamable HTTP (revision 2026-07-28, older clients through `initialize`) with tools, resources and recipe prompts. The `holocron query` CLI.
- **Access:** the "Read analytics" key preset. `sites.list` reachable by keys, listing only the sites the key reaches.
- **Dashboard:**
  - The key preset in the key dialog.
  - `matches` in the filter bar.
  - The funnel split table and the picked-group re-read.
  - Medians between steps with an upward popover, and the time to convert.
  - The split in Details and on board cards ([design](../ux-v2/design.md#split-and-step-durations)).

## Checks run

| Check | Command | Outcome |
| --- | --- | --- |
| Rust format, lint | `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings` | Clean |
| Rust tests | `cargo test --workspace --no-fail-fast` | No failures |
| Drift check | `cargo run -p holocron-protocol --bin export-types -- --check` | Up to date |
| Web | `bun run typecheck`, `bun run lint`, `bun run test` | Clean; 200 tests passed |
| Browser journeys | packaged and seeded server, `bun run e2e` | 64 passed, 7 failed, 2 skipped. The same 7 failed against a `main` build: a stale site name and missing summary rounds in the specs. `scripts/e2e.sh` (added after the milestone) runs the suite green, 73 passed. |

## Visual acceptance

Funnels with a split, a picked group, the duration popover and Details at 1440, 1024, 768 and 390 px, light and dark. The key dialog and the glob filter at all four widths. The popover first flipped below its tag onto the leak flows; it was made wide and short so it opens above (`3b7b5b0`). Screenshots were reviewed in the session and not kept.

## Limitations

- **Budget:** a `for_each` fed by a ref is counted by its tightest known bound, not its real count, so a request whose bound exceeds 200 sub-queries is refused even when fewer would run.
- **Group leaks:** leaks are not split per group on the server; the dashboard re-reads the funnel filtered to the picked group instead.
- **No OAuth:** MCP clients authenticate with an API key only.
