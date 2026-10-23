# Milestone 06: traffic and exploration

## Outcome

Operators can investigate real traffic through a Kit dashboard and reopen an exact exploration from URL, saved view, board, or export.

## Dependencies and ownership

Requires 03 and 05. Analytics owns planner and exact query execution. Frontend owns exploration service, generated resources, and Kit views. Read [Kit implementation](../kit-implementation.md) and [analytics contract](../analytics-contract.md).

## Work

1. Connect the supplied Roadhouse overview layout to real scoped data: five metrics, traffic series, pages/referrers, browsers, operating systems, and devices.
2. Implement normalized typed exploration state with grouped AND/OR, repeated predicates, typed custom properties, date/timezone, comparison, sort, pagination, and metric version.
3. Adapt Kit TableFilter, chips, SavedViews, query-state behavior, cards, controls, loading/error/empty states, and mobile navigation.
4. Add include/exclude/split-by drill-downs, intentional browser history, full-state saving, segments, board cards, annotations, and exact exports.
5. Compile bounded SQL from the typed model. Use consistent result snapshots for totals/chart/breakdowns. Return resolved dates and freshness.
6. Use Vue Query for server results and a separate state service for exploration behavior. Cancel stale work; prevent historical reports from rerunning on every collected event.
7. Implement scoped realtime snapshots/subscriptions with resume/resync and displayed stale/processing state.

## Acceptance criteria

- The acceptance workflow in the briefing works end to end against Rust: mobile signup events with plan=pro, country/referrer breakdown, country drill-down, previous-period comparison, save, board pin, reopen.
- URLs, exports, saved views, and board cards retain the same predicates, dates, versions, and dimensions.
- Back undoes one deliberate step. Clearing filters preserves unrelated state.
- Unsupported combinations and unavailable fields fail visibly.
- Revoked scope clears browser data and is rejected by server reads/exports.
- Small-fixture results are exact. The declared monthly/annual suite meets its latency targets with two users.
- Desktop/phone, light/dark, keyboard, focus, and accessible table alternatives satisfy Kit acceptance.

## Evidence and handoff

Save integrated API/browser cases, visual references, query plans, and measured results. Fixture-only charts or nonfunctional save/export controls leave this milestone open. Hand shared report infrastructure to 07 and 08.
