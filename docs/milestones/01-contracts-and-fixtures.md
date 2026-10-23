# Milestone 01: contracts and fixtures

## Outcome

The team can implement collection, storage, analytics, and Kit screens against the same reproducible contracts. Runtime targets pinned modern Umami 3.4.0. MySQL 2.20.2 is an importer source only.

## Inputs and ownership

Read [the briefing](../technical-briefing.md), [analytics contract](../analytics-contract.md), [Kit contract](../kit-implementation.md), and [performance procedure](../performance-validation.md). The integration owner coordinates collection, metric, source, and fixture decisions. This milestone owns contract documents and fixture manifests; production modules remain minimal.

## Work

1. Pin accessible source snapshots for Umami, Kit, and Folio. Werkstatt is not distributable and is described in [architecture patterns](../contracts/architecture-patterns.md). Record exact revisions, licenses, package locks, script hashes, and source paths. Read upstream handlers and scripts together. An API route name alone is insufficient evidence of compatibility.
2. Inventory modern tracker, batch, identify, performance, recorder configuration/fragments, heatmaps, links, and pixels. Record payloads, limits, outcomes, token/cache handshake, CORS, and retry behavior. Separate preserved wire behavior from deliberate durability/privacy changes.
3. Settle the proposed metric formulas and session boundary decisions in the analytics contract. Specify persistent consent transport, identity derivation and late-day keys, property types, currencies, and calculation versions.
4. Inventory every first-release capability, including campaigns, goals, journeys, attribution, boards, annotations, tracked links/pixels, accounts, importable saved objects, replay, and heatmaps. Map it to a milestone, screen, operation, and fixture.
5. Produce a small independently calculated correctness fixture and the large-workload manifest/query suite. Set supported query complexity limits and timestamp/encoding conventions.
6. Capture read-only PostgreSQL 3.4.0 and MySQL 2.20.2 schema fixtures. Include malformed or partially migrated snapshots for inspection refusal.

## Acceptance criteria

- A new agent can obtain the same source and scripts without relying on Mike's absolute filesystem paths.
- Every public collection surface has a positive and negative fixture, including real script behavior.
- Confirmed and proposed decisions are labeled. No dependent module must invent identity, consent, metric, or deduplication policy.
- The capability matrix accounts for the full release scope.
- Expected small-fixture totals include a written explanation independent of future SQL.
- Query suite and workload manifest are fixed before storage tuning.

## Evidence and handoff

Publish the source manifest, compatibility matrix, metric decision record, fixture manifests, and capability matrix. Run fixture validation against upstream where applicable and state any unavailable upstream feature. Hand source/metric contracts to 02 and 03. Closure does not claim backend performance.
