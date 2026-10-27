# Milestone 15: report queries for agents

## Outcome

An AI agent with a scoped API key answers a report request like the Rockhouse Bar statistics from Holocron alone: views and visitors of pages matching a pattern, a buyer funnel per event page with step durations, revenue, sources, places and times, in one request over MCP, HTTP or the CLI. The rules are in [0022](../decisions/0022-agent-report-queries.md).

## Dependencies and ownership

Requires 06, 07, 08 and 10. Analytics owns the planner changes and the block executor. Protocol owns the report query types, the POST query path and generation. Server owns `/mcp` and the `query` command. Frontend owns the key preset, the `matches` filter, funnel split-by and step durations.

## Work

Delivered in waves. Each wave closes with its checks before the next starts.

1. Planner: `matches` and `not_matches`, funnel v2 `group_by`, step durations, the 1,000-item list limit for report queries. Amend 0004.
2. Transport: `POST /api/queries/{name}` with a JSON body, the "Read analytics" preset in the protocol, the info log line per report query. Amend 0009 and 0012.
3. Report queries: `ReportQuery` types, `analytics.report`, blocks on one snapshot, `for_each`, refs, derived columns, flattened report tables, notes, the budget check. `holocron query` in the CLI.
4. MCP: `/mcp` over Streamable HTTP with the tools, the schema and guide resources and the four recipe prompts.
5. Dashboard, part one: the "Read analytics" preset in the key dialog and `matches` in the filter bar, en and de.
6. Dashboard, part two: design variants for funnel split-by and step durations; Mike picks one, the rule goes into the [design](../ux-v2/design.md), then the build.

## Acceptance criteria

- One report query over the seeded commerce site returns views, visitors, a funnel grouped by path with durations, revenue by currency, sources and the weekday-hour grid, inside the 30 s budget on the seeded data.
- Each block equals the matching `analytics.explore` result on the same snapshot, for a cut, a funnel and a goals report.
- A grouped funnel's groups add up to the ungrouped funnel when every identity has a group value. Durations agree with a hand-computed fixture.
- `matches` follows its glob rules on a fixture of paths, and `not_matches` is the exact complement.
- Refusals name their path; over-limit requests are refused before running; `session_list` is refused; no session IDs appear in any report query result.
- A key without the preset's operations gets 401 or `forbidden` on HTTP and MCP alike; a key limited to one site cannot read another site's block.
- An MCP client (Claude Code) lists the tools, reads the schema and runs a recipe against a local server.
- Rust and web checks from `AGENTS.md` pass. Dashboard screens pass the design's visual acceptance at 1440, 1024, 768 and 390 px, light and dark.
