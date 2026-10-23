# Development workflow

Use this document when assigning work, modifying shared interfaces, or closing a milestone. Product requirements live in [the technical briefing](technical-briefing.md). Milestone acceptance criteria live in [the milestone files](milestones/README.md).

## Begin an assignment

Read the assigned milestone, its dependencies, and the referenced source files. State the files the task owns, the interface it consumes, the behavior it adds, and the check that will exercise it. Inspect existing implementation before editing. Ask the integration owner to resolve overlapping ownership before two agents change the same schema or generated registry.

Each task should produce one usable behavior, such as a durably collected event becoming visible in overview, an exploration restoring from its URL, or an import resuming without duplicates. Tasks should be small enough to review in one pass. A milestone may contain several tasks; its gate requires their integrated behavior.

## Module ownership

The following layout is proposed. The team can rename paths while preserving the responsibilities and keeping this map current.

| Module              | Owned implementation                                       | Interface and test surface                                                                |
| ------------------- | ---------------------------------------------------------- | ----------------------------------------------------------------------------------------- |
| Protocol            | `crates/protocol`, operation export tooling                | Versioned wire types, operation maps, typed errors, generated TypeScript and OpenAPI      |
| HTTP and startup    | `apps/server/src/http`, `config.rs`, `state.rs`, `main.rs` | Router, configuration, request limits, lifecycle, and explicit dependency composition     |
| Identity and access | `identity`, `sites`, `teams`, `sharing`                   | Principal resolution, opaque SiteScope, capability checks, site policy versions           |
| Durable ingestion   | `ingestion`, SQLite inbox migrations                       | Validate/admit/commit interface returning accepted, suppressed, or refused outcomes       |
| Analytical storage  | `storage`, DuckDB migrations, transfer worker              | Apply stable batch, execute bounded read, report watermark, coordinate checkpoint         |
| Analytics           | `analytics`                                                | Typed exploration plan, formulas, sessions, correction ranges, deterministic results      |
| Recording           | `recordings`                                               | Capture-policy validation, fragments, playback reads, heatmap aggregates, storage budgets |
| Import              | `imports`                                                  | Inspect source, map records, checkpoint, resume, reconcile manifest                       |
| Maintenance         | `maintenance`                                              | Backup/restore, retention, erasure, diagnostics, coordinated shutdown                     |
| Vue data adapters   | `apps/web/src/api`, `generated`, `services`, `composables` | Generated resources, normalized exploration state, query caching, live snapshot adapters  |
| Vue experience      | `pages`, `components`, `plugins`, `assets`                 | Kit-based accessible screens, routing, translations, and semantic tokens                  |

Keep deep modules with small interfaces. Collection callers should not know transfer ledger details. Pages should not know engine SQL or authentication serialization. Concrete SQLite and DuckDB code remain distinct because their transactions and ownership differ. Introduce interchangeable adapters only when a real variation needs the seam.

## Shared changes

The protocol owner integrates new operations and generated outputs. A domain owner proposes its input, result, error, authorization, limits, and freshness semantics alongside the operation. Types must match the actual dispatcher. Generated output is regenerated, not edited manually.

The storage owner integrates migrations. Every migration states engine, ownership, upgrade behavior, and compatibility with pending inboxes and existing backup manifests. Domain tasks may propose migrations, but one owner resolves ordering before integration.

The frontend owner integrates extracted Kit components, tokens, navigation, and locale catalogs. Domain tasks use those components instead of adding local substitutes. Preserve accessible semantics and the current version of Kit's implementation conventions.

Keep frontend server-result caching in Vue Query. Services can own normalized exploration state, route encoding, and operations. Avoid a second result cache with different invalidation rules.

## Decisions before dependent work

Milestone 01 records metric formulas, collection compatibility fixtures, source pins, consent extension, identity inputs, and supported query complexity. Milestone 02 records the measured engine choice and resource budgets. Milestone 03 records the protocol envelope and Kit extraction map.

Use a short decision record under `docs/decisions/` for a choice affecting several modules: problem, constraints, evidence, choice, rejected alternatives, consequences, and affected contracts. Existing agreed requirements do not need a new approval round. A proposal to relax retention, privacy, exactness, hardware, feature coverage, or latency needs Mike's decision.

For a failing gate, report the failing workload, root cause evidence, and alternatives that preserve the requirements. Mark dependent work gated. Continue independent tasks. An incomplete experiment does not authorize the team to publish a capacity claim.

## Testing and integration

Use deterministic small fixtures for formula correctness and a separately generated large workload for capacity. Test through real module interfaces. Keep upstream protocol compatibility tests distinct from redesigned metric tests.

A focused test should prove a stated behavior or invariant. For storage, exercise actual embedded engines and failure recovery. For Vue, exercise state serialization and meaningful interactions. For milestone completion, exercise the integrated Rust API and real Vue screen. Mock data closes the shell task only.

Run focused Rust tests and frontend type/lint checks appropriate to the changed paths. Regenerate contracts and verify no drift. Use the repository's pinned tools once scaffolded. Build release binaries for benchmarks and release packaging. Record actual commands; documentation must not claim a command exists before the team adds it.

Before closing a visual milestone, inspect the changed screen at desktop and phone widths in light and dark modes. Exercise keyboard focus, menus, error states, and horizontal overflow. Store screenshots with the result report. A screenshot alone does not verify calculations or permissions.

## Completion report

Each task handoff states:

- Behavior delivered and files changed.
- Interface, migration, or contract changes.
- Checks run and their outcomes, including concrete failed or unavailable checks.
- Evidence locations and fixture/source revisions.
- Remaining work and the next task that consumes this result.

A milestone closes only when every acceptance criterion has evidence. Set its status to complete and link the evidence in the index. A task with placeholders, disabled required controls, skipped fixtures, or unresolved failures remains in progress.

This handoff requests implementation planning and documentation. Commits, publishing, production cutover, and destructive source operations require the user's authorization when those steps become relevant.
