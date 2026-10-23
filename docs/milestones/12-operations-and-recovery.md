# Milestone 12: operations and recovery

## Outcome

One-server operators can initialize, diagnose, retain, delete, back up, restore, and upgrade Holocron without losing accepted data or reintroducing erased records.

## Dependencies and ownership

Requires 04, 05, 09, and 10. Begin operations work alongside their failure hooks; close import integration after 11. Maintenance owns operator commands and lifecycle. Storage owns engine checkpoints, transfer fencing, and disk accounting. Read [performance validation](../performance-validation.md).

## Work

1. Provide initialization, source inspection/import, diagnostics, retention, backup, restore, and controlled maintenance commands with clear exit outcomes.
2. Implement graceful shutdown: stop admission, finish or safely checkpoint durable work, close query workers, and reopen through reconciliation.
3. Coordinate SQLite, DuckDB, inbox assignments, ledgers, calculation state, schema versions, and required secrets in one backup manifest. Start with a controlled maintenance pause.
4. Restore into a new directory, verify manifest/checksums/version compatibility, reconcile transfer/correction state, and validate the result before serving.
5. Implement analytical/replay/heatmap retention plus user/site erasure across pending and applied records. Fence pending ingestion/import work so deletion cannot resurrect data.
6. Measure WAL growth, checkpoint/reclamation, query spill, inbox reserve, backup temporary space, and interrupted cleanup. Document backup destination storage separately from the reference working SSD budget.
7. Provide liveness, collection readiness, analytics lag/correction state, recording caps, operational counters, and scrubbed diagnostics.
8. Define schema upgrade/recovery procedure with pre-upgrade backup and compatibility checks. A failed migration must leave an explicit recoverable state.

## Acceptance criteria

- A coordinated backup with a nonempty inbox restores on a clean machine with reconciled counts and usable credentials.
- Independent or mismatched engine snapshots are detected rather than silently served.
- Retention and deletion pass interruption/restart cases, including assigned batches and resumed imports.
- Full disk, transfer failure, stale analytical state, and unhealthy recorder budget are distinguishable.
- Maintenance fits declared resource and disk budgets, or measured limitations are resolved before 13.
- Diagnostics contain no raw identity inputs, credentials, or event bodies.
- The packaged one-process deployment and documented commands work from a fresh installation.

## Evidence and handoff

Publish clean-machine restore, erasure/retention, interrupted upgrade, shutdown, and disk-growth evidence. Deliver an operator runbook with recovery steps actually rehearsed. Hand the package and evidence to 13.
