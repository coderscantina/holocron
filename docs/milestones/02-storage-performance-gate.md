# Milestone 02: storage performance gate

## Outcome

Measured evidence supports or rejects the SQLite plus DuckDB design on the reference machine before the team commits to its full analytical implementation.

## Dependencies and ownership

Requires 01. Read [performance validation](../performance-validation.md) and the durable-transfer section of [the briefing](../technical-briefing.md). Storage owns the prototype, migrations, executor, and batch transfer. Keep the prototype's useful code in the intended modules; remove throwaway experiments when the decision is complete.

## Work

1. Implement actual SQLite WAL/FULL inbox commits, stable assignments, DuckDB applied-batch transactions, and safe inbox retirement.
2. Generate the declared 120M-event dataset with representative properties, identity cardinality, and skew. Do not extrapolate the retained-data gate from a tiny dataset.
3. Exercise ingestion, two report users, dirty-range correction probes, and representative recording load. Use the frozen early subset in performance validation: exact traffic/property counts, distinct daily identities, an ordered funnel probe, and annual exploration. Label provisional probes; full feature correctness closes in later milestones.
4. Set installation-wide thread, memory, spill, queue-byte, disk-headroom, and report-concurrency budgets. Measure queue wait separately from execution.
5. Inject termination at each transfer step. Resume assigned batches, reconcile committed ledger entries, and compare acknowledged input to stored output.
6. Record engine layouts, query plans, profiles, disk amplification, compaction requirements, and cold/warm measurements. Compare the full deployment with the pinned Umami baseline using matched event workloads and clearly labeled metric differences.

## Acceptance criteria

- Stable batches apply once despite every tested interruption boundary.
- Acknowledged records survive supported crash tests. Report power-loss evidence separately from process-kill evidence.
- The reference workload satisfies the agreed latency, memory, disk, and visibility requirements in the declared experiment, with no hidden query sampling.
- Reports include properties and associated records, not just narrow primary event rows.
- Query spill and maintenance headroom fit the disk budget; total engine scheduling fits two CPUs.
- A decision record names the selected layout, budgets, unresolved risks, and alternative-engine evidence.

## Failure and handoff

If the design fails, profile the cause and test a simpler storage design against the same contract. The team may revise the engine implementation. Lowering agreed targets or feature scope needs Mike's decision. Independent Kit work can continue.

Hand the measured design to 04. This gate is provisional: milestone 13 repeats it with the completed application.
