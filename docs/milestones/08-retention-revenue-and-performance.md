# Milestone 08: retention, revenue, and performance

## Outcome

Consented identified retention/cohorts, revenue reporting, and performance analytics work with explicit populations, formulas, and historical limits.

## Dependencies and ownership

Requires 05 and 06; attribution integration uses 07 when available. Analytics owns formula and execution. Frontend owns Kit tables/builders and population labels. Read [analytics contract](../analytics-contract.md).

## Work

1. Implement cohort entry/activity predicates, calendar/elapsed interval definitions, incomplete-period handling, eligible denominators, and versioned cohort definitions.
2. Render retention only for consented persistent populations. Show coverage and explain why anonymous daily data cannot answer person-level retention.
3. Implement validated revenue records, currency-separated totals, defined orders/refunds/deduplication, and exact decimal accumulation.
4. Implement modern performance payload mapping, named measures, dimensions, units, missing-value handling, and declared percentile methods.
5. Connect these reports to shared exploration, comparison, saved views, boards, and exports. Use Kit semantic surfaces and accessible tables alongside visualizations.
6. Add these workloads to correction handling, import mapping, retention maintenance, and the measured query suite.

## Acceptance criteria

- Cohort fixtures prove entry selection, activity counting, interval boundaries, incomplete periods, and denominator coverage.
- Default daily-anonymous data never yields an invented cross-day retention result.
- Currency/refund fixtures reconcile without floating-point accumulation errors or silent conversion.
- Performance fixtures match the specified percentile and missing-value rules.
- Late events and policy changes trigger deterministic corrections with visible version/freshness state.
- Monthly/annual representative cases pass the declared resource/latency gate.
- Population and historical-limit labels survive saving, sharing, and export.

## Evidence and handoff

Publish formula cases, generated operation fixtures, Kit screen evidence, and query measurements. Hand source-to-target mapping requirements to 11. A zero-data eligible population has a useful explanatory empty state rather than a misleading 0% report.
