# Holocron documentation

**Running Holocron?** Start with the [user guide](guide/README.md): [install](guide/install.md), [configuration](guide/configuration.md), [migrating from Umami](guide/umami-migration.md), [tracking](guide/tracking.md), [teams and access](guide/teams-and-access.md) and [operations](guide/operations.md). The [operator runbook](operations/runbook.md) and the [import and cutover runbook](operations/import-and-cutover.md) are the reference for every command and refusal code.

**Contributing?** The rest of this page is the development handoff: the contracts, decisions, milestones and evidence the code was built against. Read [CONTRIBUTING.md](../CONTRIBUTING.md) for the dev setup and the checks CI runs.

---

# Holocron development handoff

Build a Rust analytics application for existing self-hosted Umami operators. Use Kit for the production Vue dashboard and modern Umami 3.4.0 for collection compatibility. SQLite and DuckDB run inside one Rust process, subject to the storage benchmark gate.

This handoff describes work to build. The Rust server (`apps/server`, `crates/`) and the Vue dashboard (`apps/web`) are in progress, and so is the Umami importer. Capacity has been measured on a laptop only, not on the reference host (see Evidence and status).

## Read in this order

1. [Technical briefing](technical-briefing.md): agreed scope, architecture, compatibility, privacy, and release requirements.
2. [Milestone index](milestones/README.md): dependency order, parallel work, and individual implementation briefs.
3. [Development workflow](development-workflow.md): ownership, shared interfaces, decisions, and completion evidence.
4. Read the references for your assigned work: [Kit implementation](kit-implementation.md), [analytics contract](analytics-contract.md), and [performance validation](performance-validation.md).
5. For the dashboard overhaul, the [v2 index](ux-v2/README.md): design contract, briefs, and [0015](decisions/0015-baselines-projections-and-noticed-changes.md).

The technical briefing owns agreed product requirements. The analytics contract separates confirmed rules from proposed formulas that must be settled in milestone 01. Performance validation owns the workload and measurement procedure. Kit implementation owns the component and UX contract. Milestone files refer to these sources rather than redefining them.

## Requirements that drive the design

| Constraint                           | Consequence                                                                                                                                 |
| ------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------- |
| Modern tracker protocol only         | Preserve modern browser collection. Design a new typed dashboard API. Umami v2 exists only in the MySQL importer.                           |
| One process and persistent directory | Rust serves built Vue assets. Production needs no Node, external database, or queue. Development import fixtures may run database services. |
| Durable local acceptance             | Return stored-event success only after a durable commit. Recovery and backpressure are part of collection.                                  |
| Exact stored-event analytics         | Use bounded execution and valid aggregates. Query sampling cannot hide capacity problems.                                                   |
| Kit as the UI foundation             | Reuse shell, controls, tokens, table patterns, and interaction behavior. Replace Laravel adapters with generated Rust resources.            |
| Redesigned metrics                   | Version formulas and explain historical limitations. Collection compatibility does not imply metric parity.                                 |
| Small reference machine              | Prove capacity on 2 vCPU, 4 GiB RAM, and a 250-GiB local SSD with retained data and concurrent ingestion.                                   |
| Full first-release feature scope     | Milestones stage delivery. They do not authorize removing replay, heatmaps, imports, or advanced analytics.                                 |

## Team starting point

Assign an integration owner, a storage/collection owner, an analytics owner, and a frontend owner. These are responsibilities, not a requirement for four people. Give each active task explicit file ownership and an interface contract. The integration owner coordinates protocol generation, schema changes, and gate evidence.

Begin with [milestone 01](milestones/01-contracts-and-fixtures.md). Once the contracts are stable enough, [milestone 02](milestones/02-storage-performance-gate.md) and [milestone 03](milestones/03-kit-shell-and-generated-api.md) can run in parallel. Do not build the full analytical stack on an unvalidated storage design.

## Reviewable references

- [Interactive overview](../examples/overview/index.html), [desktop image](../examples/overview/overview-dark.png), [mobile image](../examples/overview/overview-mobile.png), and [header user menu](../examples/overview/header-user-menu.png). These show the agreed direction using sample values, not working report calculations.
- [Kit resources](/Users/mwallner/Development/products/kit/resources) and [Folio](/Users/mwallner/Development/products/folio). Read the specific source links in the briefing and Kit document before adopting a pattern. Werkstatt patterns are described in [architecture patterns](contracts/architecture-patterns.md).

These absolute paths describe Mike's current checkout. Before handing work to a remote team, record the source revisions and provide accessible read-only reference copies with their licenses. A source path is not a reproducible dependency pin.

## Evidence and status

Keep milestone status and evidence pointers in the milestone index. Store human-readable result reports under `docs/evidence/` when the team runs them. Keep large datasets, secrets, and raw replay outside Git. Reports identify source revision, command, fixture manifest, machine, engine versions, result, and limitations.

Milestones 01 and 03 are complete; the [milestone index](milestones/README.md) lists the rest and their evidence. Contracts live in [contracts](contracts/), [decisions](decisions/) and the repository's `fixtures/` directory. Fetch pinned sources with `scripts/fetch-sources.sh`. Documentation checks verify the handoff, not the product's correctness or performance.

Capacity so far, measured on an Apple M1 Pro laptop with the reference budget of 2 engine threads and 1 GiB ([0008](decisions/0008-storage-engine-and-budgets.md)): the full 124M-record fixture loads and every report completes, but reports that keep most of the largest site's 57M annual events miss their targets. That puts the reference budget's limit near 20M events a year for the largest site. Installation tiers are published as sizing guidance in [0008 Installation tiers](decisions/0008-storage-engine-and-budgets.md#installation-tiers-sizing-guidance); only the standard reference host is a promise, and the 20M figure comes from two measured scales, not a sweep.
