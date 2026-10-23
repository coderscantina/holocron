# Milestone 07: behavioral reporting

## Outcome

Funnels, goals, journeys, campaigns, and attribution work on the same versioned exploration model as traffic reports. Operators can follow a report result into a filtered investigation without losing state.

## Dependencies and ownership

Requires 06. Analytics owns ordered-report semantics and execution. Frontend owns Kit-based report builders/results. Read [analytics contract](../analytics-contract.md), [Kit implementation](../kit-implementation.md), and the declared report workloads.

## Work

1. Implement versioned goal definitions, ordered funnel steps, matching predicates, scope, conversion window, and repeat/tie behavior.
2. Implement bounded journeys with defined entry/exit populations, loop treatment, sequence depth, and result limits.
3. Implement campaign breakdown and named attribution rules with declared lookback and direct/internal-traffic handling.
4. Reuse Kit filter/operator/value controls, saved views, table actions, cards, and validation patterns. Step editing uses accessible controls and a table/text representation of results.
5. Preserve full report state when saving, comparing, exporting, pinning to a board, or drilling into matching events/sessions.
6. Profile ordered queries at retained scale. Add materialization only where it preserves declared filters, time bounds, and identity population.

## Acceptance criteria

- Small fixtures independently prove step order, repeated steps, equal timestamps, incomplete funnels, loop compression, and attribution exclusions.
- Anonymous reports stay inside daily identity limits. Cross-day identified analysis is labeled and requires eligible consent.
- Goal edits produce explicit definition versions; saved reports retain their meaning.
- Report/table/export totals agree on the same snapshot.
- High-cardinality and worst supported step/window cases are included in the benchmark, with bounded memory and cancellation.
- All required controls call real operations. Screen interactions, mobile layouts, and keyboard use meet Kit acceptance.

## Evidence and handoff

Publish deterministic expected results, report definitions, integrated browser cases, and query-plan/resource measurements. Complete the capability-matrix entries for funnels, goals, journeys, campaigns, and attribution. Share query and UI infrastructure with 08; leave no placeholder report route counted as shipped.
