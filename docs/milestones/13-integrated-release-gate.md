# Milestone 13: integrated release gate

## Outcome

The completed application satisfies the agreed product, compatibility, Kit UX, exactness, durability, and capacity requirements. Release claims are supported by reproducible evidence.

## Dependencies and ownership

Requires all preceding milestones. Integration owns the capability matrix and final report. Domain owners fix failures in their owned modules. Read [performance validation](../performance-validation.md) for exact fixtures and execution procedure.

## Work

1. Install the release package on the constrained reference machine. Include the real Vue assets, production authorization, corrections, recording, maintenance, and all required report types.
2. Run unchanged modern tracker/recorder compatibility, deterministic formula cases, both import fixtures, permission/revocation cases, and clean-machine restore.
3. Execute the fixed retained-data workload, cold/warm query suite, two dashboard user streams, 60-second 500/s bursts, and declared soak. Include representative properties and recording data.
4. Run transfer crash, power-loss where available, ENOSPC/backpressure, late-event/correction, deletion/retention, and backup/restore scenarios.
5. Measure the complete deployment against pinned Umami with equivalent offered workload. Separate structural savings, measured CPU/RAM/disk/I/O, analytical-layout differences, and changed metric semantics.
6. Exercise required screens with Kit acceptance: desktop/phone, light/dark, menu/navigation, filtering, report builders, account flows, export, error/empty/stale states, keyboard and accessible data alternatives.
7. Reconcile the full capability matrix and operator runbook. Replace provisional performance or compatibility wording with measured results and their exact conditions.

## Acceptance criteria

- All required features work through the real generated API and Vue application. No fixture-only or placeholder feature is counted.
- Durable collection p95 is at most 250 ms; analytical visibility p95 at most 2 s; declared 30-day reports p95 at most 2 s; declared 12-month exploration p95 at most 10 s under the agreed concurrent burst.
- The retained dataset, recording cap, working files, and measured headroom fit 250 GiB, with execution constrained to 2 vCPU and 4 GiB.
- Exactness and formula cases pass. Query sampling or silent history deletion does not mask failure.
- Recovery reconciles all acknowledged input under tested conditions; the report identifies the durability fault model actually exercised.
- Both supported imports, restored installations, privacy policy, and access isolation pass.
- Source revisions, commands, manifests, raw result summaries, and known limits are published with the report.

## Gate failures and release handoff

A failing criterion keeps this milestone open. Present measured alternatives that preserve the contract; ask Mike only for requirement changes that cannot be resolved within scope. Do not describe the application as capacity-validated from milestone 02 alone.

Deliver the reviewable release package, measured report, runbook, and remaining-risk list. Publishing or production cutover follows the user's separate authorization.
