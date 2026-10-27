# 0022: Report queries for agents

Status: Proposed. Settled with Mike question by question on 2026-10-27; it becomes Adopted when he marks it. It amends [0004](0004-exploration-state-and-limits.md) (glob operator, funnel v2), [0009](0009-protocol-envelope.md) (POST queries) and [0012](0012-access-implementation.md) (read preset), and moves the MCP endpoint in the [capability matrix](../contracts/capability-matrix.md) from deferred to milestone 15.

## Problem

An AI agent asked for a report the Bar can be shown: views and visitors of 46 tagged event pages, the buyer flow per event, revenue, where buyers came from, when they buy and how far ahead. On Umami it wrote raw SQL against MySQL, normalised event names by hand and joined vivenu transactions itself. Holocron has no way for an agent to ask that: `analytics.explore` answers one cut per call, takes its state as JSON inside an 8 KiB query string, cannot split a funnel per page, has no step durations, no path patterns, and lists of at most 50 values.

## Constraints

- Briefing: "no user SQL" (0004), "one versioned typed state for ... URLs, saved explorations, board cards, exports" (briefing, Exploration model). Metric formulas are those of [0003](0003-metric-formulas.md).
- Scoped API keys act as their owner, limited to operations and sites (0012). Session IDs, timelines and recordings are restricted reads.
- The reference host is 2 vCPU and 4 GiB. A read is cancelled at 30 s (0004 Limits).
- Joining external data (ticketing, CRM) stays with the agent. Holocron returns keys it can join on: paths, event properties such as `checkout_id`, order IDs as properties.

## Choice

### A report query is a list of named blocks

`analytics.report` is a new query operation. Its argument is one `ReportQuery`, sent as a JSON body (0009 amendment). The DSL is for agents only: it is not stored, and boards and saved explorations keep `ExplorationState`. Each block compiles onto the existing validator and planner, so a block and the equivalent exploration return the same numbers.

```text
ReportQuery {
  v: 1
  range?: Range                  default for blocks without one
  timezone?: IanaTimezone        default for blocks without one; else the block site's timezone
  blocks: Block[]                1..16, names unique
}

Block {
  name: string                   ^[a-z][a-z0-9_]{0,31}$
  site: Uuid                     one site per block; blocks may name different sites
  range?: Range                  required here unless the query has one
  timezone?: IanaTimezone
  filter?: Group                 0004 Group; `in` / `not_in` take up to 1,000 items and may use a Ref
  for_each?: { field: Field, values: Value[] 1..50 | Ref }
  body: Cut | Report             exactly one
  derive?: Derived[]             0..8, cuts only
}

Cut    = { kind: "cut", measures: Measure[] 1..8, dimensions: Field[] 0..3,
           granularity?: Granularity, series?: bool, compare?: Compare,
           sort?: Sort[] 0..2, limit?: 1..1000 (default 100) }
Report = { kind: "report", report: ReportDefinition,    funnel, journey, attribution, retention, goals;
           granularity?: "day" | "week" }                session_list is refused (Privacy);
                                                         granularity is the retention cohort period

Ref     = { ref: { block: string, column: string, limit?: 1..1000 } }
          the distinct values of a dimension or for_each column of an earlier block, in that block's
          row order, without null and not-set cells; in for_each the limit is 1..50 (default 50)

Derived = { name: string, op: "ratio" | "difference", a: Operand, b: Operand }
        | { name: string, op: "share", a: Operand }        a / total of the same column
Operand = { column: string } | { block: string, column: string, total: true }
```

- **Order.** Blocks run in the order given. A `Ref` or a derived operand names only an earlier block. A cycle cannot be written.
- **Fan-out.** `for_each` runs the block once per value with an added `eq` condition on `field`, and puts the value in a leading column named after the field. A value list longer than 50 is refused. Prefer a dimension or a funnel `group_by` where one exists: they run in one pass.
- **Refs.** A ref fills only an `in` or `not_in` list or the `for_each` values. A ref that finds no values skips its block, which answers no rows and the note `ref_empty`.
- **Cuts.** `compare` adds a `<measure>_previous` column after each measure. `series: true` makes the rows the time buckets of the totals in a leading `time` column, and takes no dimensions. A cut without dimensions or series answers its totals as its one row, so `for_each` runs line up as rows.
- **Derived columns.** Operands are measure columns other than ratios. `ratio` and `share` are ratios as in 0004 (4 decimals, `null` with `reason: "no_denominator"`, the 0003 name). `difference` keeps the operand type and needs two columns of the same type. Money operands pair only with money and must have the same currency per cell; otherwise the cell is `null` with `reason: "mixed_currencies"`. A `total` operand names a cut block without `for_each`. `for_each` blocks have no `totals`, since visitors do not add up across runs; `share` there divides by the total of the same run.
- **Strict parsing.** Unknown fields and versions are refused. Every refusal names its path, `blocks.2.filter.f3` or `blocks.0.derive.1.b`, with a message written for an agent to fix the request. A body that does not parse is `malformed_request` with the path leading its message (`blocks.0.body: unknown field ...`).

### Budget

- One request shares the 30 s budget of 0004. Blocks run one after another on one snapshot, so every block reports the same watermark.
- At most 16 blocks and 200 sub-queries in total, counting each `for_each` value as one and a `for_each` ref at its limit. Both are checked before anything runs, also before authorization; a request over the limit is refused (`report_too_large`) and never returns partial results.
- A cut's `limit` above 100 widens the first page after validation, as an export does, so it stays inside the 10,000 addressable rows.

### Result

Each block returns a typed table and its provenance, so an agent can cite method notes.

```text
ReportQueryResult { v: 1, watermark: { batch, sequence, at, stale }, metric_version, blocks: BlockResult[] }

BlockResult {
  name, site, resolved?: ResolvedRange, timezone
  freshness?: Freshness             as on an exploration; resolved and freshness are absent for a skipped block
  columns: { name, type, role }[]   type: string | integer | decimal | money | ratio | duration_ms | date | instant | bool
                                    role: for_each | dimension | time | measure | derived
  rows: (Cell | Metric)[][]         keys are 0004 cells, values 0004 metrics; a money cell is
                                    { t: "money", v: Money[] } in currency code order
  totals?: (Cell | Metric)[]        cuts without for_each, one per column, null for dimensions
  notes: { code, message, path? }[] coverage, daily_limit, groups_truncated, rows_truncated, ref_empty, visitor_days
}
```

Reports flatten into rows:

| Kind | Columns |
| --- | --- |
| `funnel` | `group` (with `group_by`), `step`, `from_step`, `reached`, `drop`, `conversion` from step 1, `p25_ms`/`median_ms`/`p75_ms` from the previous step, five `bucket_*` counts. Steps count from 1. After the step rows comes one row from step 1 to the last step with the time to convert. With `group_by` the rows are per group only |
| `journey` | `step_1` to `step_<length>` (not set past the end of a sequence), `identities` |
| `attribution` | `conversion` (from 1), `touch`, `direct`, `conversions`, `revenue`; per conversion the touch rows, then the direct row |
| `retention` | `cohort` (local date), `offset`, `cohort_size`, `retained`, `rate`, `complete` |
| `goals` | `goal` (from 1), `completions`, `denominator`, `rate` |

### Funnel v2 and glob matching (0004 amendment)

These change the shared planner, so the dashboard gets them too.

- `FunnelReport` v2 adds `group_by?: Field`, one event, session or property field. An identity belongs to the group of its entering step-1 event. Groups are ordered by step 1 reached, descending, then by value. At most 50 groups are returned; the rest are left out and the result says `groups_truncated`. The overall funnel and its leaks are unchanged. Leaks are not split per group.
- Every funnel result gains durations per transition: for identities reaching step `n`, the time from their step `n − 1` event to their step `n` event, as p25, median and p75 in integer milliseconds, plus counts in fixed buckets `< 1 h`, `1–24 h`, `1–7 d`, `7–30 d` and `≥ 30 d`. A last entry from step 1 to the last step gives the time to convert. Version 1 funnels get them too, since they are output only.
- A new string operator `matches` takes a glob: `*` matches any run of characters except `/`, `**` any run including `/`, `\` escapes the next character. The pattern must match the whole value, case-sensitively. Patterns are 1 to 500 bytes with at most 8 wildcards. It works on `path`, `referrer_path`, `title`, `query`, `hostname` and string properties. Its negation is `not_matches`, with the complement rule of 0004.
- `in` and `not_in` take up to 1,000 items in a report query. `ExplorationState` keeps 50, since it lives in URLs.

### Privacy

- Report queries return aggregates only. Funnel leak session IDs are dropped, and `session_list` is refused with `report_field_unsupported`.
- Session timelines, realtime and recordings stay behind their own operations. A key reaches them only when it is granted them explicitly. There is no minimum cell size.

### Access

- Agents use the person-owned API keys of 0012. The key dialog gains a "Read analytics" preset: `analytics.report`, `analytics.export`, `fields.catalog`, `fields.values`, `analytics.freshness`, `sites.get` and the workspace site list. Export still needs the owner's `site.export`.
- Each block is authorized on its own site, before the request runs.
- Keys already record `last_used_at` (0012). Each report query adds one info log line: key ID, blocks, sub-queries and duration, without the body.

### Transports

- **HTTP.** `POST /api/queries/analytics.report` with the query as its body. OpenAPI and TypeScript are generated from the registry as for every operation.
- **MCP.** `/mcp` serves MCP over Streamable HTTP in the same process. It is always on; a call without a valid `Authorization: Bearer` key gets 401. Each tool call dispatches through the operation registry with the key's grants, so MCP adds no access path of its own.
  - Tools: `list_sites`, `catalog`, `field_values`, `query` (a report query), `export` (`analytics.export`, up to 10,000 rows).
  - Resources: the report query JSON schema, generated from the protocol types, and a guide with worked examples.
  - Prompts, one per recipe: event or product sales, campaign, monthly site report, content performance. They are written in English; the agent writes the report in its reader's language.
- **CLI.** `holocron query --url <origin> <file|->` posts a report query to a running server with the key from `HOLOCRON_API_KEY` and prints the result. It does not open the data directory.

## Rejected alternatives

- Read-only SQL over curated views: overturns "no user SQL", and isolation and the 30 s budget become hard to guarantee.
- A relational pipeline with its own planner: answers more questions natively, but every 0003 formula would exist twice.
- A per-block 30 s budget or async jobs: a long agent request would compete with the dashboard on the reference host.
- Service accounts and OAuth for MCP: a new identity type or an authorization server, for a need the existing keys cover.
- A data-directory CLI: bypasses access rules and competes for the DuckDB lock.
- A second export operation: `analytics.export` already returns up to 10,000 rows as CSV or JSON.
- Uploading lookup tables or ingesting server-side purchases: new storage and privacy surface. Agents join outside.

## Consequences

- Two query formats exist: `ExplorationState` for the dashboard and its URLs, `ReportQuery` for agents. Both compile onto one planner and are tested against each other.
- The funnel planner carries step times and a group per identity.
- The dashboard gets a `matches` filter, funnel split-by and step durations, after a design pass.

## Affected contracts

- `crates/protocol/src/exploration.rs` (operator, funnel v2, durations), a new `crates/protocol/src/report_query.rs`, `limits.rs`, `operations/explorations.rs`, the api key preset.
- `apps/server/src/http` (POST queries, `/mcp`), `apps/server/src/main.rs` (`query` command).
- [0004](0004-exploration-state-and-limits.md), [0009](0009-protocol-envelope.md), [0012](0012-access-implementation.md), the capability matrix, [milestone 15](../milestones/15-agent-query-api.md).
