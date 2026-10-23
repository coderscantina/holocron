# Holocron technical briefing

Revision 3. Development handoff, updated October 3, 2026.

Start with [the documentation index](README.md) for milestone order and ownership. This file defines the product and architecture contract. [Kit implementation](kit-implementation.md), [analytics semantics](analytics-contract.md), and [performance validation](performance-validation.md) define the implementation and evidence required at their respective seams. Each milestone has its own executable brief under [milestones](milestones/README.md).

Build a self-hosted Rust analytics application with a Vue dashboard, embedded databases, modern Umami collection compatibility, and better exploration workflows. Follow the implemented architectural patterns in Folio and the [Werkstatt patterns](contracts/architecture-patterns.md). Treat Kit as the required UX and component reference.

Umami v2 is relevant only to the MySQL importer. Runtime compatibility targets [Umami 3.4.0 at ec0ff50388c264ed8ce46f00967e92f7e71476ae](https://github.com/umami-software/umami/tree/ec0ff50388c264ed8ce46f00967e92f7e71476ae). Do not implement v2 tracker compatibility or legacy dashboard/report endpoints. The new dashboard uses a generated command/query API.

The storage architecture and capacity claims remain subject to the benchmark gate below. This is an implementation brief, not a report of completed performance tests.

**Product contract.** The audience is existing self-hosted Umami operators. Run one Rust process with a persistent data directory. Ship built Vue assets with the application. No production Node runtime, external database, external queue, or additional application service is required.

Operating cost, reliability, and analytics capability have equal priority. The first release includes traffic, campaigns, custom events and typed properties, realtime, session exploration, funnels, goals, journeys, attribution, retention, cohorts, revenue, performance, replay, heatmaps, annotations, boards, and tracked links/pixels. Include teams, site roles, 2FA, scoped API keys, and restricted shared explorations.

Persistent person-level retention and cohorts are available only for explicitly enabled, consented identified analytics. Anonymous traffic remains daily scoped. All analytics features are in the release scope; staged development does not authorize dropping features from that release.

**Architecture references.** Reuse actual boundaries found in these projects, with the changes stated here. Do not assume that every proposal in their architecture documents is implemented.

| Reference                                                                                                                                                                                                      | Verified pattern                                                       | Application here                                                                                                            |
| -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------- |
| [Folio router](/Users/mwallner/Development/products/folio/apps/server/src/lib.rs), [static hosting](/Users/mwallner/Development/products/folio/apps/server/src/web.rs)                                         | Axum hosts API, health, configuration, and SPA fallback                | One server hosts modern collection, dashboard operations, health, runtime configuration, and built Vue assets               |
| [Folio startup](/Users/mwallner/Development/products/folio/apps/server/src/main.rs), [AppState](/Users/mwallner/Development/products/folio/apps/server/src/state.rs)                                           | Explicit startup composition and shared services                       | Assemble configuration, control storage, inbox, analytics executor, authorization, and notifications once                   |
| [Folio Workspace](/Users/mwallner/Development/products/folio/apps/web/src/services/workspace.ts), [Vue adapters](/Users/mwallner/Development/products/folio/apps/web/src/composables.ts)                       | Framework-independent services expose methods and observable snapshots | An exploration service owns state normalization, saved-view behavior, and navigation; Vue renders it through thin adapters  |
| [Operation registry](contracts/architecture-patterns.md#operation-registry-and-generated-contracts), [schema export](contracts/architecture-patterns.md#schema-export-and-drift-check)        | Rust wire types and operation registries generate TypeScript contracts | Generate typed command/query maps, IDs, arguments, results, events, and snapshots; check generated files for drift          |
| [Named commands](contracts/architecture-patterns.md#named-commands-transactions-revisions-and-idempotency), [queries](contracts/architecture-patterns.md#snapshot-queries) | Named operations, transaction discipline, revisions, idempotency       | Named dashboard commands and queries call concrete application functions, with transport kept thin                          |
| [Explicit authorized scope](contracts/architecture-patterns.md#explicit-authorized-scope)                                                                                           | Authorized scope is passed explicitly to domain operations             | Use an opaque SiteScope obtained through authorization; require it for analytical reads, exports, recordings, and mutations |
| [Topics, cursors and subscriptions](contracts/architecture-patterns.md#topics-cursors-and-subscriptions)   | Durable topic cursors, bounded delivery, resync, scope checks          | Use authenticated subscriptions for realtime, import progress, and processing lag                                           |

Folio's CRDT, browser database, offline synchronization, and third-party authentication are not requirements here. Werkstatt's runners, provider adapters, and orchestration graph are not requirements here. Keep sibling domain modules in one server crate until an independent boundary warrants another crate.

Use Rust, Tokio, Axum, SQLx for SQLite control storage, and embedded DuckDB through its Rust bindings as the initial implementation choices. SQLx follows the inspected reference code. Run blocking analytical work on a bounded executor rather than an async HTTP worker. Pin dependencies and embedded engine versions in the prototype.

Keep domain behavior in concrete modules. The interfaces below are proposed responsibilities, not existing Rust methods. The storage experiment determines their exact types and limits.

| Module interface      | Caller receives                               | Hidden implementation and required invariant                                                                                        |
| --------------------- | --------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------- |
| Collection admission  | Typed accepted, suppressed, or refused result | Validation, transient identity derivation, byte budget, durable payload/context commit; accepted stored results wait for durability |
| Batch application     | Committed batch position or retryable failure | Stable membership, canonical writes, correction metadata, ledger; internal replay changes no totals                                 |
| Analytical execution  | Bounded typed results and snapshot metadata   | SQL planning, engine workers, cancellation, resource limits; authorized scope and exact semantics are enforced                      |
| Exploration state     | Normalized state and explicit transformations | URL/save/board serialization, compatible drill-downs, version validation; server results stay in Vue Query                          |
| Maintenance execution | Durable job state and verified outcome        | Transfer fencing, engine checkpoint, deletion/retention/backup coordination; interrupted work is resumable                          |

An ingestion watermark means the highest contiguous accepted position fully applied to analytics, or an equally precise explicit coverage model. Taking the maximum row ID can falsely claim coverage when earlier batches are missing. Correction completeness is reported separately. Stable applied-batch entries include enough schema/provenance metadata to detect an incompatible replay after upgrade or restore.

```text
Cargo.toml
apps/
  server/
    migrations/
    src/
      main.rs
      config.rs
      state.rs
      http/
        collect/
        commands/
        queries/
        subscribe.rs
        web.rs
      identity/
      sites/
      ingestion/
      analytics/
      explorations/
      recordings/
      imports/
      storage/
      events/
      maintenance/
  web/
    src/
      api/
      generated/
      services/
      composables/
      pages/
      components/
      plugins/
      assets/
crates/
  protocol/
    src/
      ids.rs
      collect.rs
      exploration.rs
      events.rs
      http/
docs/
examples/overview/
```

**Two transport surfaces.** Keep public browser collection separate from authenticated dashboard commands. They have different cross-origin, authentication, and durability requirements.

| Surface                                       | Required behavior                                                                                                                         |
| --------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- |
| Modern tracker and recorder assets            | Serve pinned supported scripts and their configured aliases; maintain the modern browser API                                              |
| `POST /api/send`                              | Accept modern event, identify, and performance payloads; validate exactly one website/link/pixel source; return compatible cache handling |
| `POST /api/batch`                             | Preserve modern per-item results, ordering, limits, and cache behavior                                                                    |
| Recorder configuration and `POST /api/record` | Preserve configuration, sampling, strict masking, session-cache handshake, recording fragments, and heatmap payloads                      |
| Link and pixel collection                     | Preserve the modern public source routes and corresponding source configuration                                                           |
| `POST /api/commands/{name}`                   | Named authenticated mutations with typed arguments/results, idempotency, and expected revisions where appropriate                         |
| `GET /api/queries/{name}`                     | Named authenticated reads with typed arguments/results and snapshot metadata                                                              |
| Topic subscription endpoint                   | Authorized realtime notifications, durable resume cursor, explicit resync                                                                 |
| Runtime configuration and health              | Deployment configuration at boot; distinguish collection availability from analytical freshness                                           |

Examples of commands are `sites.update`, `explorations.save`, `boards.update`, `imports.start`, and `sharing.update`. Examples of queries are `analytics.explore`, `realtime.snapshot`, `recordings.list`, and `imports.status`. Names are proposed protocol conventions, not existing Umami endpoints.

The Rust operation registry is the source of truth for dashboard dispatch and generated TypeScript operation maps. A focused generic client performs serialization, authentication, typed error handling, and cancellation. Components call generated API resources or services, never bare fetch. Generate OpenAPI documentation for the new HTTP surface as a separate representation of that registry; do not hand-maintain a second schema.

Validate dynamic property identifiers and filter operators. Compile bound SQL from the typed exploration model. Do not expose unrestricted user SQL. Browser command origin protections belong on dashboard mutations; cross-origin tracker collection remains supported.

**Control transactions and subscriptions.** Follow the [control database boundary](contracts/architecture-patterns.md#control-database-boundary): serialize writes, use explicit transactions, and use a read pool for control queries. Configure WAL, foreign keys, and bounded busy handling centrally. Choose `synchronous=FULL` for the acknowledged inbox. Folio's current `NORMAL` setting is not sufficient for the agreed power-loss durability guarantee.

For SQLite-only commands, commit the mutation, durable topic event, and idempotency response in one transaction. Publish a wakeup after commit. Control snapshots read data and their topic cursor in the same SQLite transaction. Resolve scope before entering the operation, and reject stale expected revisions without disclosing inaccessible resources.

Bind each idempotency key to the principal, operation, and normalized argument fingerprint. Reusing it with different arguments is an explicit conflict. Reauthorize access before returning a stored result after membership changes. One-time credentials and keys need a withholding policy rather than durable replay of the original secret. Network calls and analytical computation happen outside the SQLite write transaction. Commands enqueue durable jobs where work spans engines.

Subscriptions are notifications backed by a durable log, not the source of truth. Bound queues by bytes. On an expired cursor or stalled consumer, require a fresh snapshot. Reauthorize delivery when permissions or sessions change. Coalesce updates; a slow browser must not stall collection.

Do not copy the pattern's same-transaction snapshot guarantee across SQLite and DuckDB. Analytical responses have a DuckDB committed-ingestion watermark and a metric version; control snapshots have their SQLite revision and cursor. They are separate values. Read analytical results and their watermark from a consistent DuckDB snapshot. After an analytical commit, publish a watermark notification; reconcile missed notifications after restart. No cross-engine atomicity is implied.

**Embedded storage and durable transfer.** SQLite owns accounts, permissions, configuration, saved explorations, board definitions, import/job state, pending accepted records, and batch assignments. DuckDB owns canonical analytical events/properties, versioned session calculations, derived reporting data, performance/revenue, replay chunks, heatmaps, import provenance, and an applied-batch ledger.

Do not build one generic repository abstraction for both databases. They have different ownership and transaction boundaries. Start with one analytical writer and a bounded number of report readers. Batch writes and exact aggregates are allowed; hidden event sampling is not.

Collection success waits for a durable SQLite commit containing the accepted canonical payload and required collection-context state. Group commit may amortize synchronization, but cannot acknowledge before commit. Intentionally suppressed bot traffic and recording disabled by policy have distinct outcomes; they are not acknowledged stored events.

Choose batch byte/record caps and a short maximum group-commit delay from the latency experiment. Give ingestion and control writes a bounded scheduling policy so a long account/import command cannot monopolize the write connection. A timeout after commit can leave an event stored with an uncertain client outcome; optional request IDs handle that retry case. Storage durability depends on filesystem/device flush guarantees, which the test report must identify.

The transfer protocol is:

1. Persist a stable batch ID and inbox membership in SQLite.
2. Check the applied-batch ledger in DuckDB.
3. If absent, commit the analytical records, required correction/dirty-range state, and the ledger entry in one DuckDB transaction.
4. Only then retire the matching SQLite inbox records.
5. Resume the same assignment after interruption. A committed ledger entry permits cleanup without reinsertion.

This prevents duplication from internal transfer replay. Optional client event IDs address request retries separately. Preserve batch ledgers long enough that restored inboxes cannot replay forgotten batches. Coordinate restore positions rather than independently restoring database files.

Use bounded admission and explicit backpressure when durable storage is unavailable or a backlog cannot fit its budget. Never acknowledge into memory and hope to persist later. Upstream's Kafka producer catches send failures and can skip oversized messages; that behavior is a correctness improvement target, not a requirement to reproduce. [Producer source](https://github.com/umami-software/umami/blob/ec0ff50388c264ed8ce46f00967e92f7e71476ae/src/lib/kafka.ts)

**Identity, metrics, and late events.** Anonymous identity is cookieless, site scoped, keyed, and rotated daily in the website's timezone. Keep raw identity inputs transient and exclude them from logs. Explicit identify calls use the daily scope by default; do not infer persistent identity from arbitrary event properties.

Count stored events exactly. Visitor measurements represent pseudonymous identities, not verified people. For multiple identity days expose visitor-days or another precisely defined daily measure; do not relabel it as unique people across the period. Persistent identification is explicitly enabled and requires a consent signal. Person-level retention and long-term cohorts use only that population and display its coverage.

Anonymous sessions close at identity rotation. Otherwise use a configurable inactivity window, default 30 minutes. Define eligible events, exact timeout boundaries, local-midnight behavior, timezone changes, and DST handling. Store metric and session-policy versions and make recalculation explicit.

Live payloads are accepted with any event time, as upstream does (Mike, October 3, 2026). An event whose identity day key has already been destroyed is counted exactly but carries no daily identity. See [decision 0002](decisions/0002-sessions-and-events.md). Keep event time and receive time separate. Imports use a separate trusted path. Late data marks affected sessions and reports for deterministic recomputation. A query result reports freshness and calculation version rather than claiming corrected totals before correction completes.

Keep capture identifiers separate from derived sessions so analytical corrections cannot split, lose, or reorder replay chunks. Modern response shapes remain compatible while the internal identity and session algorithms change. Existing upstream tokens can be replaced at cutover; continuous old sessions are not promised.

Document formulas for pageviews, visitors, visitor-days, engagement/bounce, duration, funnels, attribution, retention, percentiles, and revenue. Observed event span is not continuous active time. Keep currencies separate unless an explicit conversion model is configured. Do not silently change saved-report meaning when a site setting changes.

**Exploration model.** Use one versioned typed state for the website, measures, dimensions, grouped predicates, relative/absolute dates, timezone, granularity, comparison, metric version, sorting, visualization, and pagination. URLs, saved explorations, board cards, exports, and query keys use that state. Resolve relative time once per request.

Support repeated conditions on the same field and grouped AND/OR expressions. Preserve event-property predicates across compatible views. Reject unsupported combinations visibly instead of dropping them. Breakdown cells offer include, exclude, and split-by actions. Browser Back undoes one deliberate investigation step. Clearing filters preserves dates, dimensions, and visualization. Resetting a view is separate.

Umami already has URL filters, segments, boards, and exports. The improvement is consistent state and richer investigation across those surfaces, not their mere existence. For example, its segment save path disables saving when event-property filters are active. [Filter bar source](https://github.com/umami-software/umami/blob/ec0ff50388c264ed8ce46f00967e92f7e71476ae/src/components/input/FilterBar.tsx)

The acceptance workflow is: choose the last 30 days, filter mobile traffic and signup events with `plan=pro`, split by country and referrer, drill into a country, compare against the previous period, save, pin to a board, and reopen the exact state.

**Frontend architecture.** Use Vue 3, TypeScript, Vue Router, Tailwind, Kit's shadcn/Reka components, and Vue Query. Reuse Folio's service boundary for behavior that benefits from independence from Vue. Do not hide a second cache inside the exploration service: Vue Query owns server-result caching, while the service owns normalized exploration state and operations.

Follow the [two data modes](contracts/architecture-patterns.md#live-snapshot-adapter). Historical reports use Vue Query keyed by complete normalized exploration state, authorization scope, and an explicit dataset watermark/refresh epoch. Cheap live resources use subscriptions and snapshot adapters. The inspected `useLiveSnapshot` refetches once per relevant event batch; it does not reconstruct arbitrary analytical results from event deltas. Do not trigger expensive historical queries for every collected event.

Cancellation prevents obsolete query results from replacing a newer exploration. Reconnect restores live snapshots and exposes stale state while doing so. Boot-time runtime configuration follows [Folio's runtime-config service](/Users/mwallner/Development/products/folio/apps/web/src/services/runtime-config.ts); installation-specific settings do not require rebuilding the web application.

**Kit is the UX contract.** Consult [Kit's frontend conventions](/Users/mwallner/Development/products/kit/CLAUDE.md), [design tokens](/Users/mwallner/Development/products/kit/docs/design-tokens.md), and [the actual stylesheet](/Users/mwallner/Development/products/kit/resources/js/assets/css/app.css) before introducing visual properties. Adopt existing components where suitable, with analytics-specific adapters replacing Laravel data dependencies.

The app header follows the supplied Clerk navigation reference: compact workspace, website, and environment context controls, slash separators, small context badges, and paired switch chevrons. Move context selection out of the sidebar. On narrow screens shorten the labels while retaining accessible names and reachable selectors. Environment context in the example is illustrative; it does not add a separate service or a new storage architecture.

The user dropdown belongs at the right edge of the app header, following [Kit's AppUserMenu](/Users/mwallner/Development/products/kit/resources/js/components/app/AppUserMenu.vue): small avatar and chevron trigger, identity block, account links, appearance choices, shortcuts, and sign out. Account navigation and the user identity block do not appear in the sidebar. Website settings remain contextual site navigation.

| Concern           | Required Kit reference                                                                                                                                                                                                                                                                       | Analytics application                                                                                                                     |
| ----------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- |
| Shell             | [AppLayout](/Users/mwallner/Development/products/kit/resources/js/layouts/AppLayout.vue), [AppSidebar](/Users/mwallner/Development/products/kit/resources/js/components/app/AppSidebar.vue), [AppHeader](/Users/mwallner/Development/products/kit/resources/js/components/app/AppHeader.vue) | Collapsible desktop sidebar, shared frame tone, context selectors, header user menu, command palette, mobile drawer and bottom navigation |
| Surface depth     | `sidebar`, `surface`, `card`, `popover` in app.css                                                                                                                                                                                                                                           | Frame, canvas, data panels, and floating controls use the correct semantic rung in both modes                                             |
| Geometry and type | app.css shell tokens, 15px base type, density settings                                                                                                                                                                                                                                       | Consistent control heights, gutters, table rows, radii, and compact density; no separate page-specific spacing system                     |
| Controls          | [Button variants](/Users/mwallner/Development/products/kit/resources/js/components/ui/button/variants.ts), [Card variants](/Users/mwallner/Development/products/kit/resources/js/components/ui/card/variants.ts)                                                                             | Compact outline/ghost controls, one clear primary action, bordered analytical panels                                                      |
| Filtering         | [TableFilter](/Users/mwallner/Development/products/kit/resources/js/components/ui/data-table/TableFilter.vue), filter chips and operator types                                                                                                                                               | Search plus staged field/operator/value selection; adapt its flat filter state to the richer analytics expression tree                    |
| Navigation state  | [useTableQueryState](/Users/mwallner/Development/products/kit/resources/js/composables/useTableQueryState.ts)                                                                                                                                                                                | URL-backed sorting/filtering, intentional history entries, reliable Back behavior, default values omitted                                 |
| Saved views       | [SavedViews](/Users/mwallner/Development/products/kit/resources/js/components/ui/data-table/SavedViews.vue)                                                                                                                                                                                  | Save the whole exploration; show current-view state; reopen without losing dates, comparison, or dimensions                               |
| Access            | AppLayout route focus and live announcements                                                                                                                                                                                                                                                 | Skip link, focus after navigation, announced state changes, visible keyboard focus, reduced-motion support                                |

Use semantic tokens rather than palette shades in components. Preserve Kit's restrained gray surfaces and one saturated accent. Use its existing typography and radius scale; do not introduce a competing visual style. Use table representations for chart data, explicit empty/error/loading states, and typed translated labels. Chart series need shared semantic tokens and distinguishable marks rather than one-off colors.

For the supplied Roadhouse overview, retain the five summary measures, hourly chart, pages/referrers, browsers, operating systems, and devices. Replace top-level tab clutter with Kit's contextual sidebar. Put global exploration controls above the measures. Make selected measures, current date, comparisons, filters, and freshness visible together. Label anonymous multi-day figures as visitor-days and duration as observed span.

The runnable [overview example](../examples/overview/index.html) demonstrates this direction. It uses Kit-derived tokens and interactions in dependency-free HTML/CSS/JavaScript so it can be reviewed directly. It is a UX reference, not the production Vue implementation. Values originate from the supplied screenshot; added series and audience rows are illustrative. Filters change exploration state without pretending to calculate new results. Save/restore is local to the example browser.

**Replay, privacy, and retention.** Recording is opt-in per website. Use strict text/input masking, sensitive-page/element exclusions, and 10% default replay sampling. The modern recorder already supports strict masking. [Recorder source](https://github.com/umami-software/umami/blob/ec0ff50388c264ed8ce46f00967e92f7e71476ae/src/recorder/index.js)

Retain raw replay for 14 days, aggregated heatmaps for 90 days, and analytical events for 12 months. Default to a 25-GiB replay cap. Enforce budgets during admission and transfer, not only periodic cleanup. If recording cannot be accepted within its cap, report that outcome while keeping ordinary analytics collection available. Record sampling applies to capture, not reporting query accuracy.

Masking happens before transmission in the supported recorder. Server settings cannot prove arbitrary clients masked data. Include that boundary in the product documentation and verify supported script behavior.

**Import boundary.** The initial importer accepts MySQL Umami 2.20.2 and the pinned PostgreSQL Umami 3.4.0 schema. This is the only v2 support in the product. Inspect columns and migration state; reject unsupported or partially migrated layouts with concrete guidance. Do not ship legacy runtime routes to make an importer work.

Import through read-only source connections and consistent snapshots into a new data directory. Preserve website UUIDs, source data, identifiers as restricted provenance, and available account credentials through tested conversion paths. Reissue logins and scoped API keys at cutover. Handle 2FA secrets explicitly.

Recompute supported sessions and metrics from timestamped events, with documented limits. Stored history lacks the inputs for reconstructing the new daily identity algorithm. Do not invent identity continuity or silently rewrite provenance. MySQL's historical visit-ID backfill does not establish genuine inactivity sessions. [MySQL schema](https://github.com/umami-software/umami/blob/v2.20.2/db/mysql/schema.prisma), [visit migration](https://github.com/umami-software/umami/blob/v2.20.2/db/mysql/migrations/05_add_visit_id/migration.sql)

Map supported segments, reports, boards, and recordings to new definitions. Inventory and preserve representations that cannot be mapped; explain each omission. Jobs must be resumable, deterministic, and reconciled by source/target manifests. Import retries cannot duplicate records. Recompute only what the available history supports, and show imported visitor limitations in reports.

**Maintenance and security.** Provide initialization, import inspection, import execution, backup, restore, retention, and diagnostics commands. Generate or require bootstrap credentials; do not ship a known default password. Scope keys to operations and sites. Reauthorize exports, recordings, shared explorations, and live subscriptions. Protect restricted identity provenance from ordinary reporting.

Back up SQLite, DuckDB, pending inboxes, batch ledger positions, schema versions, and required secrets coherently. Start with a brief maintenance pause and verified snapshot/restore rather than unsafe independent live file copies. Deletion/reset covers pending ingestion and analytical records so transfer cannot resurrect deleted data.

Budget disk for WAL, inboxes, retention/compaction, query spill, and replay. A deletion need not immediately shrink a DuckDB file. Measure maintenance at the retained-data scale. [DuckDB space reclamation](https://duckdb.org/docs/current/operations_manual/footprint_of_duckdb/reclaiming_space)

Expose accepted/rejected records, inbox age/depth, analytical watermark, visibility lag, transfer failures, query latency, memory/disk use, recording caps, import state, and maintenance outcomes. Avoid sensitive event bodies or identity inputs in operational logs.

**Release gate.** These are agreed requirements, not measured capacity claims.

| Requirement                   | Target                                                                                                              |
| ----------------------------- | ------------------------------------------------------------------------------------------------------------------- |
| Server                        | 2 vCPU, 4 GiB RAM, 250-GiB local SSD                                                                                |
| Analytical volume             | 10 million events/month                                                                                             |
| Retained analytical dataset   | Approximately 120 million primary events, plus representative properties, performance, and other associated records |
| Retention                     | 12 months analytical events; 14 days raw replay; 90 days aggregated heatmaps                                        |
| Recording                     | 25-GiB replay cap, 10% replay sampling when enabled                                                                 |
| Burst and dashboard use       | 500 events/second for 60 seconds with two dashboard users                                                           |
| Durable collection            | p95 at most 250 ms                                                                                                  |
| Analytical visibility         | p95 at most 2 seconds                                                                                               |
| Common 30-day reports         | p95 at most 2 seconds                                                                                               |
| Defined 12-month explorations | p95 at most 10 seconds                                                                                              |

Publish the query suite and workload manifest before tuning. Include cardinality, skew, properties, filtered exploration, exact distinct daily identities, funnels, eligible retention, performance, and recording load. Measure cold and warm execution separately, whole-process memory, spill space, sustained backlog, error rate, and recovery after the burst. Define supported query complexity without claiming every arbitrary query meets the target.

Test unchanged modern tracker/recorder scripts, runtime configuration, command idempotency, revisions, permissions, cursor recovery, stale browser results, batch interruption at every boundary, disk exhaustion, late events, timezone/DST boundaries, both supported import snapshots, retention, deletion, and clean-machine restore.

Analytical query correctness is not legacy metric parity. Test the agreed new formulas against deterministic datasets. Compatibility tests cover modern collection behavior. The example screen is verified separately as a visual and interaction reference.

If the gate fails, revise the engine design using measured evidence. SQLite-only or DuckDB-only may be simpler alternatives if they satisfy the same requirements. Do not silently lower retention, feature coverage, exactness, hardware constraints, or latency targets.

Execute the [milestone dependency map](milestones/README.md). Settle collection fixtures and metric definitions first. Run the storage experiment and build the Kit shell in parallel, then integrate durable collection and versioned analytics. Complete the full reporting, recorder, access, import, and maintenance work before the integrated release gate. These stages do not narrow the first-release deliverable.

## Rust performance strategy

The rewrite should remove the production Node runtime and the separate database service, bound ingestion and reporting resource use, and reduce operational work. Those are architectural opportunities, not measured savings. Rust improves control over allocation, execution, and process composition. Database layout, SQL, correction work, and report concurrency determine analytical performance.

Start with typed canonical records, batch conversion, bound SQL parameters, column projection, predicate pushdown, and measured partition pruning. Avoid parsing the same payload on every report. Store frequently queried dimensions in typed columns. Keep typed custom properties separately queryable with defined missing/null/type behavior. Decide physical partitioning, indexes, property representation, and aggregate tables from the declared workload, then retain query plans and storage measurements with the decision.

Keep one analytical writer and explicitly budget report workers, engine threads, memory, spill, inbox bytes, and replay storage together. A per-query memory limit multiplied by unbounded concurrency is not an installation limit. Admit work before allocating large buffers. Propagate cancellation through the query executor and test that an expired browser request releases engine work. Exact mergeable counts may use aggregates when filter coverage permits them; exact distinct identities and ordered funnels require evidence that an aggregate preserves the requested semantics.

Collection, transfer, corrections, realtime, and maintenance need bounded scheduling. Imports and long reports yield to live ingestion through measurable admission rules. Blocking engine calls run outside Tokio HTTP workers, with a semaphore or owned worker queue controlling total work. `spawn_blocking` alone does not impose a product resource budget. Avoid an abstraction layer that converts every event into several allocations or every database call into a new worker.

Use release builds for performance evidence. Profile a failing workload before changing code. Record CPU samples, allocations where useful, SQL plans, synchronization time, queue wait, and engine execution separately. Optimize the measured cause, then rerun the same correctness and performance case. Preserve query exactness and durability while tuning.

## Reliability strategy

Treat durable acceptance, analytical visibility, corrected metrics, and recorder availability as separate states. A collection response proves the first; an analytical watermark proves the second; a correction watermark or dirty-range indicator describes the third. Recording can reach its storage budget while ordinary events continue. The dashboard exposes these differences through Kit status and error patterns.

Batch IDs, client deduplication IDs, command idempotency keys, and import provenance solve different retry problems. Keep their scopes and retention policies explicit. Never claim exactly-once browser delivery for unchanged trackers. A durable event may have committed even if the response was lost. The batch ledger makes internal transfer replay safe, while optional client IDs make supported request retries safe.

Deletion is an ordered operation across both engines. Serialize deletion with ingestion, invalidate collection context, fence affected inbox generations, apply the analytical deletion, and complete the durable job before reporting deletion complete. A replayed inbox or a resumed import must not reintroduce deleted website data. Independent database backups and casual file replacement are incompatible with this guarantee.

## Delivery discipline

The team delivers vertical slices that can be exercised through the real generated API and Vue screen. Fixture-only screens are useful during shell development, but they do not close a reporting milestone. A milestone closes when its stated behavior, negative cases, and evidence pass. All milestones start pending. No source inspection, document, or mockup counts as a completed backend milestone.

Use [the development workflow](development-workflow.md) for shared-file ownership, change requests, and handoff reports. When a gate exposes a design problem, produce a measured decision with alternatives. Keep unrelated work moving while the affected implementation remains gated. Scope changes require Mike's decision; routine implementation choices within this contract belong to the team.

**Verification status and exclusions.** Architecture and upstream findings come from read-only source inspection. No real importer, analytics backend, durability test, or 120M-event benchmark was run. The overview is an interactive example with sample data, not a working analytical product.

Exclude Umami v2 runtime APIs, generic legacy report APIs, ClickHouse imports, additional historical source snapshots, external databases/queues, browser event databases, and multi-server operation from this release.
