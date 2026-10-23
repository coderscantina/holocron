# Milestone 11: historical import and cutover

## Outcome

Operators can inspect and import pinned PostgreSQL Umami 3.4.0 or MySQL Umami 2.20.2 into a new Holocron data directory, reconcile the result, and cut over collection without modifying the source.

## Dependencies and ownership

Requires implemented schemas from 04, 05, 07, 08, 09, and 10. Inspection/fixture work may begin after 01. Import owns mapping, checkpoints, provenance, and manifests. Storage integrates bulk-write scheduling and versioned schemas.

## Work

1. Implement read-only source inspection with explicit supported schema/migration signatures. Refuse partial/unsupported layouts with actionable upgrade/export guidance.
2. Import from consistent source snapshots into a new destination. Preserve website UUIDs, timestamped facts, source data and restricted identifier provenance.
3. Map available users, teams, roles, credential encodings, 2FA, sites, events/properties, performance/revenue, segments, reports, boards, links/pixels, recordings, and heatmaps where the source contains them. Public shares (`share.slug`, `website.share_id`) become restricted shares that keep answering at their old URLs (Mike, 2026-10-04).
4. Inventory unmappable objects, preserve their source representation, and expose a reason/count. Do not silently invent missing history or discard configuration.
5. Recompute supported metrics/session definitions. Label imported visitor and identified-cohort limitations. Historical visit IDs do not prove inactivity sessions.
6. Use deterministic provenance keys and resumable chunk checkpoints. Budget import memory/disk and coordinate correction jobs.
7. Reconcile per-site/per-kind counts, properties, date ranges, amount/currency totals, configuration objects, and sampled content checks.
8. Provide cutover instructions: stop live source collection for the final snapshot, import/reconcile, verify destination, redirect scripts/endpoints, issue fresh logins/API keys, and test collection. Preserve the original system for rollback.

## Acceptance criteria

- Both exact supported snapshots import successfully and malformed layouts refuse safely.
- Source databases remain unchanged and importer credentials have read-only permissions.
- Interrupt/resume/retry yields the same reconciled destination without duplication.
- Imported public share URLs keep working as restricted shares.
- Website IDs remain stable. Converted accounts/authentication behave as documented, with fresh sessions/keys and explicit 2FA handling.
- Supported reports use new formulas and visible historical limitations.
- Unmapped representations are inventoried and preserved.
- Operator cutover and rollback rehearsals use real scripts and isolated fixture deployments.

## Evidence and handoff

Publish source/destination manifests, resume cases, read-only evidence, conversion inventory, and cutover rehearsal. Umami v2 runtime APIs and other historical snapshots remain outside scope. Hand the completed destination to 12 for backup/restore verification.
