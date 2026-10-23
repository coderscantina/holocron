# Capability matrix

Milestone 01, work item 4. This matrix lists every first-release capability and maps it to the milestone that closes it, a proposed screen, proposed dashboard operations, the collection surface, a fixture, the import mapping, and the authorization capability. It also records how each upstream Umami 3.4.0 dashboard feature is covered, replaced, or excluded.

Status: proposed. Nothing here is implemented. Operation names, routes, and capability names are proposals in the briefing's convention; milestone 03 fixes the generated registry, and 13 reconciles this matrix against the shipped application.

Upstream evidence comes from read-only inspection of `.sources/umami-3.4.0` (revision `ec0ff50388c264ed8ce46f00967e92f7e71476ae`) and `.sources/umami-2.20.2`. Paths below are relative to those checkouts. `(main)` means `src/app/(main)`, `api` means `src/app/api`, `sql` means `src/queries/sql`.

## Conventions

**Milestone.** The milestone that closes the capability, with earlier enabling milestones in parentheses. Every capability is also re-verified in 13.

**Route.** Proposed Vue Router path inside the Kit shell. Site routes live under the contextual site sidebar; workspace routes under the workspace section; account routes are reached from the header user menu only; installation routes from the user menu for installation admins. The active workspace comes from the header context selector. Proposed site sidebar groups, extending the overview example:

| Sidebar group    | Entries                                                    |
| ---------------- | ---------------------------------------------------------- |
| Workspace        | Boards, Explore, Links, Pixels, Sites                      |
| Site: traffic    | Overview, Events, Sessions, Realtime, Performance          |
| Site: behavior   | Funnels, Goals, Journeys, Retention                        |
| Site: growth     | Campaigns, Attribution, Revenue                            |
| Site: audience   | Segments, Cohorts                                          |
| Site: recordings | Replays, Heatmaps                                          |
| Site footer      | Website settings (general, tracking, recording, identity, sessions, access, sharing, annotations, data) |

The example sidebar has no entries yet for journeys, campaigns, attribution, segments, cohorts, links, or pixels. Its combined "Funnels & goals" entry is split here because goals are versioned definitions shared by several reports.

**Operations.** `Q:` marks a query (`GET /api/queries/{name}`), `C:` a command (`POST /api/commands/{name}`), `T:` a subscription topic, `CLI:` an operator command. Report queries take the versioned exploration state.

**Collection surface.** Public browser surface, or `none`.

**Fixtures.** `collection: <surface>-*` refers to cases under `fixtures/collection/`. `correctness: <behavior>` refers to the small hand-audited fixture under `fixtures/correctness/`. `Q01` to `Q14` refer to the query suite in [performance validation](../performance-validation.md). `workload: <procedure>` refers to a procedure in that document run on the large-workload manifest. `import: pg`, `import: mysql`, `import: refusal` refer to `fixtures/import-schemas/{postgresql-3.4.0,mysql-2.20.2,refusal}`. `access: <file>` refers to `fixtures/access/` (`cases.json` rows `AX-G*` and `AX-T*`, `scenarios.json` scenarios `AX-S*`, `role-mapping.json`). `gaps: <file>` refers to `fixtures/gaps/`. **GAP** marks a capability with no planned fixture, with the milestone that will create it and the missing decision, listed in `fixtures/gaps/README.md`.

**Import mapping.** `mapped`, `preserved-unmapped (reason)`, or `n/a`. "Both" means PostgreSQL 3.4.0 and MySQL 2.20.2. MySQL 2.20.2 has no boards, links, pixels, annotations, API keys, 2FA, shares table, replays, or heatmaps; its website `share_id` is its only share.

**Authorization capability.** Proposed names, checked server-side against an opaque SiteScope or principal. Scoped API keys and shares grant subsets of these.

| Capability                | Grants                                                                                   |
| ------------------------- | ---------------------------------------------------------------------------------------- |
| `public`                  | Unauthenticated browser collection; validated against site/link/pixel configuration      |
| `site.read`               | Analytical reads, saved explorations, boards cards for the site                          |
| `site.export`             | Exports of site results                                                                  |
| `site.recordings.read`    | Replay list/playback and heatmaps                                                        |
| `site.definitions.write`  | Goals, segments, cohorts, annotations                                                    |
| `site.configure`          | Site settings, tracking, recording, identity and session policy                          |
| `site.share`              | Create, change, revoke shares of the site                                                |
| `site.transfer`           | Move a site between user and team ownership                                              |
| `site.delete`             | Reset and delete site data                                                               |
| `workspace.read`          | See workspace boards, links, pixels, sites                                               |
| `workspace.write`         | Create/edit boards, links, pixels in the workspace                                       |
| `team.manage`             | Team settings, members, roles, invitations                                               |
| `account.self`            | The signed-in person's own profile, security, preferences, keys                          |
| `share.viewer`            | Principal resolved from a share token; limited to the share's sites, fields, and actions |
| `install.admin`           | Installation users, teams, sites (metadata), security policy, imports, status; no site analytics unless also a workspace member ([0007](../decisions/0007-access-and-account-security.md)) |
| `operator`                | Local CLI on the host with data-directory access; not an HTTP capability                 |

## Collection

| ID | Capability | Upstream 3.4.0 | Milestone | Screen/route | Operations | Collection surface | Fixture | Import mapping | Authorization |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| CAP-COLLECT-01 | Serve pinned tracker script and configured aliases; tracking snippet | `src/tracker/index.ts`; `(main)/websites/[websiteId]/settings/WebsiteTrackingCode.tsx` | 04 (01) | `/sites/:siteId/settings/tracking` | Q: `sites.trackingCode` | Tracker script and aliases | collection: script-* | n/a | `public` (script); `site.configure` (snippet) |
| CAP-COLLECT-02 | Pageviews: auto and SPA navigation, `data-exclude-search`, `data-exclude-hash`, `data-tag`, hostname, `data-domains`, `data-do-not-track`, `data-before-send` | `api/send/route.ts`; `src/tracker/index.ts` | 04 | none | none | `POST /api/send` | collection: send-pageview-* | n/a (history in CAP-IMPORT-02) | `public` |
| CAP-COLLECT-03 | Custom events with typed properties (`track()`, `data-umami-event-*`) | `api/send/route.ts`; `event_data` | 04 (typed semantics 05) | none | none | `POST /api/send` | collection: send-event-*; correctness: typed properties | n/a | `public` |
| CAP-COLLECT-04 | Identify and session properties (`identify(id, data)`, `data-distinct-id`) | `api/send/route.ts`; `session_data`, `session_link` | 04 (derivation 05) | none | none | `POST /api/send` | collection: send-identify-*; correctness: anonymous and identified populations | n/a | `public` |
| CAP-COLLECT-05 | Consent signal for persistent identity, with revocation | new | 05 (transport settled in 01) | none | none | `POST /api/send` extension, or trusted integration | collection: send-identify-* consent cases (depend on 01 item 3); correctness: consent absence and revocation | n/a | `public` |
| CAP-COLLECT-06 | Web performance payloads (LCP, INP, CLS, FCP, TTFB; `data-performance`) | `api/send/route.ts`; `website_event` vitals columns | 04 | none | none | `POST /api/send` | collection: send-performance-* | n/a | `public` |
| CAP-COLLECT-07 | Revenue-bearing events (amount and currency) | `api/send/route.ts`; `revenue` | 04 (formula 08) | none | none | `POST /api/send` | collection: send-event-* revenue cases; correctness: revenue currencies and refunds | n/a | `public` |
| CAP-COLLECT-08 | Batch with per-item results, ordering, limits | `api/batch/route.ts` | 04 | none | none | `POST /api/batch` | collection: batch-* | n/a | `public` |
| CAP-COLLECT-09 | Session cache token handshake and compatible cache responses | `api/send/route.ts` | 04 | none | none | `POST /api/send`, `POST /api/batch` | collection: send-* cache cases | n/a | `public` |
| CAP-COLLECT-10 | Tracked link redirect collection | `src/app/(collect)/q/[slug]/route.ts` | 04 | none | none | `GET /q/:slug` | collection: link-* | n/a | `public` |
| CAP-COLLECT-11 | Tracking pixel collection | `src/app/(collect)/p/[slug]/route.ts` | 04 | none | none | `GET /p/:slug` | collection: pixel-* | n/a | `public` |
| CAP-COLLECT-12 | Recorder script and configuration handshake | `src/recorder/index.js`; `api/websites/[websiteId]/recorder/route.ts` | 09 (04 interface) | none | none | Recorder script, recorder configuration | collection: recorder-config-* | n/a | `public` |
| CAP-COLLECT-13 | Replay fragment ingest: ordering, duplicates, gaps, final chunk | `api/record/route.ts`; `session_replay` | 09 | none | none | `POST /api/record` | collection: record-replay-* | n/a | `public` |
| CAP-COLLECT-14 | Heatmap interaction ingest (click, scroll) | `api/record/route.ts`; `heatmap_event` | 09 | none | none | `POST /api/record` | collection: record-heatmap-* | n/a | `public` |
| CAP-COLLECT-15 | Bot suppression and distinct accepted/suppressed/refused outcomes | `api/send/route.ts` (`isbot`, `DISABLE_BOT_CHECK`) | 04 | none | none | All collection surfaces | collection: negative case per surface | n/a | `public` |
| CAP-COLLECT-16 | CORS, origin policy, trusted proxy; dashboard mutation protection kept separate | `api/send/route.ts` headers | 04 (03 dashboard side) | none | none | All collection surfaces | collection: CORS and proxy cases | n/a | `public` |
| CAP-COLLECT-17 | Durable acknowledgement, bounded admission, explicit backpressure | new (replaces `src/lib/kafka.ts` behavior) | 04 (02 gate) | none | none | All collection surfaces | workload: burst, transfer pause, ENOSPC, failpoints | n/a | `public` |
| CAP-COLLECT-18 | Optional client event-ID deduplication | new | 04 | none | none | `POST /api/send`, `POST /api/batch` | correctness: duplicate retries | n/a | `public` |
| CAP-COLLECT-19 | Late and early events accepted with any timestamp, separate receive time, keyless events for destroyed day keys ([0002](../decisions/0002-sessions-and-events.md), [0001](../decisions/0001-identity-and-consent.md)) | new | 05 (04 admission) | none | none | `POST /api/send`, `POST /api/batch` | collection: send-* timestamp cases; correctness: late events | n/a | `public` |

## Traffic

| ID | Capability | Upstream 3.4.0 | Milestone | Screen/route | Operations | Collection surface | Fixture | Import mapping | Authorization |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| CAP-TRAFFIC-01 | Overview: five measures (views, sessions, visitor-days, bounce, observed span), series, comparison | `(main)/websites/[websiteId]/page.tsx`; `api/websites/[websiteId]/stats`, `pageviews`; `sql/getWebsiteStats.ts`, `sql/pageviews` | 06 (05) | `/sites/:siteId` | Q: `analytics.explore` | none | Q01; correctness: visitor-days, bounce, observed span, midnight and DST | n/a (history in CAP-IMPORT-02) | `site.read` |
| CAP-TRAFFIC-02 | Page breakdowns: path, entry, exit, title, hostname, query, referrer | `api/websites/[websiteId]/metrics`, `metrics/expanded`; `sql/pageviews` | 06 | `/sites/:siteId` panels, `/sites/:siteId/explore` | Q: `analytics.explore` | none | Q02 | n/a | `site.read` |
| CAP-TRAFFIC-03 | Audience breakdowns: browser, OS, device, screen, language, country, region, city | `api/websites/[websiteId]/metrics`; `sql/sessions` | 06 | `/sites/:siteId` panels | Q: `analytics.explore` | none | Q01, Q02; correctness: two-website totals | n/a | `site.read` |
| CAP-TRAFFIC-04 | Traffic channels (source grouping) | `sql/getChannelMetrics.ts`, `sql/getChannelExpandedMetrics.ts` | 06 | `/sites/:siteId` panel | Q: `analytics.explore` (channel dimension) | none | gaps: channel-classification.json ([0003](../decisions/0003-metric-formulas.md) Channels) | n/a | `site.read` |
| CAP-TRAFFIC-05 | Weekly day/hour traffic grid | `api/websites/[websiteId]/sessions/weekly`; `sql/getWeeklyTraffic.ts` | 06 | `/sites/:siteId/sessions` panel | Q: `analytics.explore` (weekday, hour dimensions) | none | Q01 hour series (partial); gaps: weekday-grid.json ([0004](../decisions/0004-exploration-state-and-limits.md) Weekday and hour) | n/a | `site.read` |
| CAP-TRAFFIC-06 | Events report: names, series, counts | `(main)/websites/[websiteId]/events`; `api/websites/[websiteId]/events`, `events/series`, `events/stats`; `sql/events` | 06 | `/sites/:siteId/events` | Q: `analytics.explore` | none | Q12; correctness: typed properties | n/a | `site.read` |
| CAP-TRAFFIC-07 | Event and session property analysis: values, numeric stats, date/array series, pivot | `api/websites/[websiteId]/event-data/*`, `event-data-pivot/*`, `session-data/*`, `session-data-pivot` | 06 | `/sites/:siteId/events` properties view, `/sites/:siteId/explore` | Q: `analytics.explore` | none | Q12; correctness: typed properties, mixed types | n/a | `site.read` |
| CAP-TRAFFIC-08 | Realtime: recent identities, live activity, stale and resync states | `(main)/websites/[websiteId]/realtime`; `api/realtime/[websiteId]`, `api/websites/[websiteId]/active`; `sql/getRealtimeData.ts`, `sql/getActiveVisitors.ts` | 06 | `/sites/:siteId/realtime` | Q: `realtime.snapshot`; T: `realtime.site` | none | Q14; gaps: realtime-window.json ([0003](../decisions/0003-metric-formulas.md) Realtime) | n/a | `site.read` |
| CAP-TRAFFIC-09 | Sessions list, session detail, activity sequence, session properties | `(main)/websites/[websiteId]/sessions`, `sessions/[sessionId]`; `api/websites/[websiteId]/sessions/*` | 06 (05) | `/sites/:siteId/sessions`, `/sites/:siteId/sessions/:sessionId` | Q: `sessions.list`, `sessions.get` | none | Q05; correctness: inactivity boundaries, one-event sessions | n/a | `site.read` |
| CAP-TRAFFIC-10 | Site list with summary sparklines | `(main)/websites/page.tsx`; `api/websites/charts`; `sql/getWebsiteListCharts.ts` | 06 (03) | `/sites` | Q: `sites.list`, `analytics.siteSummaries` | none | correctness: two-website totals | n/a | `workspace.read` |
| CAP-TRAFFIC-11 | Freshness, watermark, correction and metric-version display | new | 06 (05) | All report headers | Q: `analytics.status`; T: `analytics.watermark` | none | Q14; correctness: late events | n/a | `site.read` |
| CAP-TRAFFIC-12 | Tracked link management | `(main)/links/*`; `api/links`, `api/links/[linkId]`, `api/teams/[teamId]/links`; `link` | 06 (04) | `/workspaces/:workspaceId/links` | Q: `links.list`; C: `links.create`, `links.update`, `links.delete` ([0020](../decisions/0020-links-and-pixels-management.md)) | Configures `GET /q/:slug` | collection: link-* | PG: mapped (ID, slug, target URL); MySQL: n/a | `site.read`, `workspace.write` (create), `site.configure` (update), `site.delete` (delete) |
| CAP-TRAFFIC-13 | Tracked link analytics | `(main)/links/[linkId]/page.tsx`; `api/links/charts` | 06 | `/workspaces/:workspaceId/links/:linkId` | Q: `links.explore` | none | collection: link-*; gaps: link-pixel-totals.json ([0003](../decisions/0003-metric-formulas.md) Links and pixels) | PG: link events mapped with history | `workspace.read` |
| CAP-TRAFFIC-14 | Pixel management | `(main)/pixels/*`; `api/pixels`, `api/pixels/[pixelId]`, `api/teams/[teamId]/pixels`; `pixel` | 06 (04) | `/workspaces/:workspaceId/links` | Q: `pixels.list`; C: `pixels.create`, `pixels.update`, `pixels.delete` ([0020](../decisions/0020-links-and-pixels-management.md)) | Configures `GET /p/:slug` | collection: pixel-* | PG: mapped (ID, slug); MySQL: n/a | `site.read`, `workspace.write` (create), `site.configure` (update), `site.delete` (delete) |
| CAP-TRAFFIC-15 | Pixel analytics | `(main)/pixels/[pixelId]/page.tsx`; `api/pixels/charts` | 06 | `/workspaces/:workspaceId/pixels/:pixelId` | Q: `pixels.explore` | none | collection: pixel-*; gaps: link-pixel-totals.json ([0003](../decisions/0003-metric-formulas.md) Links and pixels) | PG: pixel events mapped with history | `workspace.read` |

## Exploration

| ID | Capability | Upstream 3.4.0 | Milestone | Screen/route | Operations | Collection surface | Fixture | Import mapping | Authorization |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| CAP-EXPLORE-01 | Versioned typed exploration state: dates, timezone, granularity, measures, dimensions, sort, pagination, visualization; URL roundtrip and Back | URL filters, `src/components/input/FilterBar.tsx`; `api/websites/[websiteId]/daterange` | 06 (03) | `/sites/:siteId/explore`, every report route | Q: `analytics.explore` | none | Q03 | n/a | `site.read` |
| CAP-EXPLORE-02 | Grouped AND/OR predicates, repeated same-field conditions, typed property predicates, missing values | `FilterBar.tsx` (flat filters) | 06 | Filter bar on every report | Q: `analytics.explore` | none | Q04, Q12; correctness: typed properties | n/a | `site.read` |
| CAP-EXPLORE-03 | Authorized field catalog and value suggestions | `api/websites/[websiteId]/values`, `event-data/fields`, `event-data/values`; `sql/getValues.ts` | 06 | Filter bar | Q: `fields.catalog`, `fields.values` | none | Q12 | n/a | `site.read` |
| CAP-EXPLORE-04 | Include, exclude, split-by drill-downs from breakdown cells | `(main)/websites/[websiteId]/(reports)/breakdown`; `api/websites/[websiteId]/breakdown`; `sql/breakdown` | 06 | Breakdown tables | Q: `analytics.explore` | none | Q03 | PG `report` type `breakdown`, MySQL type `insights`: mapped to explorations where fields map, else preserved-unmapped (unsupported field) | `site.read` |
| CAP-EXPLORE-05 | Period and segment comparison | `(main)/websites/[websiteId]/compare` | 06 | Comparison control on every report | Q: `analytics.explore` (comparison) | none | Q01, Q03 | n/a | `site.read` |
| CAP-EXPLORE-06 | Saved explorations (all report kinds), rename, delete, reopen exact state | `report` table; `api/websites/[websiteId]/funnels`, `goals`; `src/app/(compat)` report saves | 06 (07, 08 add report kinds) | Saved views menu; `/sites/:siteId/explore?view=:id` | Q: `explorations.list`, `explorations.get`; C: `explorations.save`, `explorations.rename`, `explorations.delete` | none | Q14 | Both: `report` rows mapped by type (see CAP-IMPORT-04) | `site.read` (own), `site.definitions.write` (shared to site) |
| CAP-EXPLORE-07 | Segments (named predicate sets, including event-property predicates) | `(main)/websites/[websiteId]/segments`; `api/websites/[websiteId]/segments`; `segment` type `segment` | 06 | `/sites/:siteId/segments` | Q: `segments.list`, `segments.get`; C: `segments.save`, `segments.delete` | none | Q04; gaps: segment-definitions.json ([0004](../decisions/0004-exploration-state-and-limits.md) Segments) | Both: mapped where filters map, else preserved-unmapped (unsupported operator or field) | `site.read`, `site.definitions.write` |
| CAP-EXPLORE-08 | Exact exports of the current exploration | `api/websites/[websiteId]/export` (fixed CSV zip) | 06 (07, 08 for their report kinds) | Export action on every report | Q: `analytics.export` | none | Q03 export of the same state | n/a | `site.export` |
| CAP-EXPLORE-09 | Declared query limits; unsupported combinations rejected visibly | new | 06 (01 limits) | Every report, typed error state | Typed error on all report queries | none | Q04 complexity envelope | n/a | `site.read` |

## Behavioral

| ID | Capability | Upstream 3.4.0 | Milestone | Screen/route | Operations | Collection surface | Fixture | Import mapping | Authorization |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| CAP-BEHAV-01 | Goals: versioned definitions, conversion, counting scope | `(main)/websites/[websiteId]/(reports)/goals`; `api/websites/[websiteId]/goals/*`; `sql/goals` | 07 | `/sites/:siteId/goals` | Q: `goals.list`, `goals.get`, `analytics.explore` (`report.kind = goals`); C: `goals.save` (new version), `goals.archive`; C: `explorations.save` | none | Q06; correctness: `goals.json` (session once and every, person scope with consented coverage); API: `goal_edits_are_versions_and_saved_reports_keep_theirs` | PG `report` type `goal`, MySQL type `goals`: mapped (one definition per goal) | `site.read`, `site.definitions.write` |
| CAP-BEHAV-02 | Funnels: ordered steps, window, repeats, ties | `(main)/websites/[websiteId]/(reports)/funnels`; `api/websites/[websiteId]/funnels/*`; `sql/funnels` | 07 (02 probe) | `/sites/:siteId/funnels` | Q: `analytics.explore` (`report.kind = funnel`); C: `explorations.save` | none | Q06 and Q06 worst cases; correctness: `funnels.json` (step order, repeated steps, equal timestamps, half-open window, midnight, person scope); API: `a_funnel_keeps_its_state_through_save_pin_and_export` | Both: type `funnel` mapped | `site.read` |
| CAP-BEHAV-03 | Journeys: entry/exit populations, depth, loop compression | `(main)/websites/[websiteId]/(reports)/journeys`; `api/websites/[websiteId]/journeys`; `sql/journeys` | 07 | `/sites/:siteId/journeys` | Q: `analytics.explore` (`report.kind = journey`); C: `explorations.save` | none | Q07 and Q07 length-7 cases; correctness: `journeys.json` (loop compression before the step limit, prefix order) | Both: type `journey` mapped | `site.read` |
| CAP-BEHAV-04 | Campaigns: UTM fields and click IDs, campaign breakdown | `(main)/websites/[websiteId]/(reports)/utm`; `api/websites/[websiteId]/utm/metrics`; `sql/utm` | 07 | `/sites/:siteId/campaigns` | Q: `analytics.explore` (UTM and `click_id` dimensions) | none | Q08; e2e: `behavioral.spec.ts` campaigns; gaps: campaign-normalization.json ([0003](../decisions/0003-metric-formulas.md) UTM values) | Both: type `utm` mapped | `site.read` |
| CAP-BEHAV-05 | Attribution: named rule, lookback, direct/internal handling | `(main)/websites/[websiteId]/(reports)/attribution`; `api/websites/[websiteId]/attribution`; `sql/attribution` | 07 | `/sites/:siteId/attribution` | Q: `analytics.explore` (`report.kind = attribution`); C: `explorations.save` | none | Q08 and Q08 referrer cases; correctness: `attribution.json` (last and first touch, once and every, internal and direct, midnight, revenue per currency) | Both: type `attribution` mapped where the model maps, else preserved-unmapped (unsupported model) | `site.read` |
| CAP-BEHAV-06 | Drill from a report result into matching sessions or events | new | 07 | Report result tables | Q: `analytics.explore` (`report.kind = session_list`, and plain states with derived predicates) | none | correctness: `session_list_pages_the_sessions_of_the_range`; e2e: funnel step to sessions | n/a | `site.read` |

## Identified analytics

| ID | Capability | Upstream 3.4.0 | Milestone | Screen/route | Operations | Collection surface | Fixture | Import mapping | Authorization |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| CAP-IDENT-01 | Daily site-scoped keyed anonymous identity; visitor-days labels | replaces upstream session hashing (`api/send/route.ts`) | 05 | Labels on all reports | none (policy) | `POST /api/send` | correctness: midnight and DST, visitor-days | preserved-unmapped (source session IDs kept as restricted provenance; cannot rebuild daily identity) | n/a |
| CAP-IDENT-02 | Session policy: inactivity window, policy versions, explicit recalculation, timezone change | replaces upstream `visit_id` | 05 | `/sites/:siteId/settings/sessions` | C: `sites.updateSessionPolicy`, `recalculations.start`; Q: `recalculations.status` | none | Q05; correctness: inactivity boundaries, timezone change | preserved-unmapped (historical visit IDs do not prove inactivity sessions; recomputed) | `site.configure` |
| CAP-IDENT-03 | Persistent identified analytics, opt-in per site, consent required | new | 05 (10 capability) | `/sites/:siteId/settings/identity` | Q: `sites.identityPolicy`; C: `sites.updateIdentityPolicy` | Consent signal (CAP-COLLECT-05) | correctness: consent population | n/a | `site.configure` |
| CAP-IDENT-04 | Retention for consented persistent identities only. Built in 08, [evidence](../evidence/m08-retention-revenue-and-performance.md) | `(main)/websites/[websiteId]/(reports)/retention`; `api/websites/[websiteId]/retention`; `sql/retention` | 08 | `/sites/:siteId/retention` | Q: `analytics.explore` with `report.kind = "retention"`, `analytics.export`; C: `explorations.save`, `boards.pin` | none | Q09; correctness: `retention.json` (`retention_matches_the_fixture`) | Both: type `retention` mapped as definition; results limited to imported-identity population and labeled | `site.read` |
| CAP-IDENT-05 | Cohorts: entry/activity predicates, intervals, versioned definitions. Built in 08 as the retention report: cohort predicates are its `filter`, the definition is versioned by the report's `v`, and definitions are saved views, [evidence](../evidence/m08-retention-revenue-and-performance.md) | `(main)/websites/[websiteId]/cohorts`; `segment` type `cohort` | 08 | `/sites/:siteId/retention` (no separate cohorts screen) | Q: `analytics.explore`, `explorations.list`, `explorations.get`; C: `explorations.save`, `explorations.delete` | none | Q09; correctness: `retention.json` | Both: `segment` type `cohort` mapped to a retention state's `filter` where predicates map, else preserved-unmapped | `site.read`, `site.definitions.write` |
| CAP-IDENT-06 | Population and coverage labels (anonymous, identified, eligible, imported) that survive save, share, export. Built in 08 for retention (coverage per cohort and in total, retained-history start, completeness watermark), [evidence](../evidence/m08-retention-revenue-and-performance.md); imported population waits for 11 | new | 08 (05) | Retention page, board card, export | `RetentionResult.coverage`, `history_start`, `watermark`; `Cut.identity_coverage` on every exploration | none | correctness: `retention.json` coverage; API: `reports_keep_their_labels_through_save_pin_export_and_reopen` | n/a | `site.read` |

## Revenue and performance

| ID | Capability | Upstream 3.4.0 | Milestone | Screen/route | Operations | Collection surface | Fixture | Import mapping | Authorization |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| CAP-REVPERF-01 | Revenue: currency-separated totals and series, orders, refunds, exact decimals. Built in 08, [evidence](../evidence/m08-retention-revenue-and-performance.md) | `(main)/websites/[websiteId]/(reports)/revenue`; `api/websites/[websiteId]/revenue/chart`, `metrics`, `stats`; `sql/revenue` | 08 | `/sites/:siteId/revenue` | Q: `analytics.explore` with `revenue_net`, `revenue_gross`, `revenue_refunds`, `revenue_count` and the `currency` dimension | none | Q10; correctness: `revenue.json` (`revenue_matches_the_fixture`) | Both: `revenue` rows mapped; type `revenue` reports mapped | `site.read` |
| CAP-REVPERF-02 | Revenue by session, campaign, and attribution rule. Built in 08: revenue by any session field (`utm_campaign`, `channel`, `referrer_domain`, country, device), [evidence](../evidence/m08-retention-revenue-and-performance.md); by attribution rule is 07's revenue conversion | `api/websites/[websiteId]/revenue/sessions` | 08 (07) | `/sites/:siteId/revenue`, `/sites/:siteId/attribution` | Q: `analytics.explore` (revenue measures with session dimensions), `analytics.attribution` | none | Q08, Q10 | n/a | `site.read` |
| CAP-REVPERF-03 | Performance: named vitals, units, p50/p75/p95 per dimension, missing values. Built in 08, [evidence](../evidence/m08-retention-revenue-and-performance.md) | `(main)/websites/[websiteId]/(reports)/performance`; `api/websites/[websiteId]/performance/*`; `sql/performance` | 08 | `/sites/:siteId/performance` | Q: `analytics.explore` with `p50`, `p75`, `p95`, a `perf_metric` condition and the `perf_metric` dimension | none | Q11; correctness: `percentiles.json` (`percentiles_match_the_fixture`) | PG: vitals columns and type `performance` reports mapped; MySQL: n/a (no vitals) | `site.read` |

## Replay and heatmaps

| ID | Capability | Upstream 3.4.0 | Milestone | Screen/route | Operations | Collection surface | Fixture | Import mapping | Authorization |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| CAP-RECORD-01 | Recording policy per site: off by default, strict masking, page/element exclusions, 10% replay sampling, heatmap enable | `(main)/websites/[websiteId]/settings/WebsiteReplaySettings.tsx`; `website.replay_config`; `src/lib/recorder.ts` (defaults 15%, moderate masking) | 09 | `/sites/:siteId/settings/recording` | Q: `recordings.policy`; C: `recordings.updatePolicy` | Recorder configuration | collection: recorder-config-* | PG: mapped, tightened to strict masking (deviation recorded); MySQL: n/a | `site.configure` |
| CAP-RECORD-02 | Replay list and filtering, linked from sessions and reports | `(main)/websites/[websiteId]/replays`; `api/websites/[websiteId]/replays`, `sessions/[sessionId]/replays` | 09 | `/sites/:siteId/replays` | Q: `recordings.list` | none | Q13 | PG: `session_replay` within 14 days mapped; older preserved-unmapped (outside retention) | `site.recordings.read` |
| CAP-RECORD-03 | Isolated replay playback with controls and accessible event timeline | `(main)/websites/[websiteId]/replays/[replayId]`, `@modal/(.)replays/[sessionId]`; `api/websites/[websiteId]/replays/[replayId]` | 09 | `/sites/:siteId/replays/:captureId` | Q: `recordings.get` (bounded chunks) | none | Q13; collection: record-replay-* ordering and duplicates; replay-hostile: 10 cases in `fixtures/replay-hostile/`; format and isolation in [0011](../decisions/0011-replay-store-and-isolation.md) | PG: chunks mapped with capture IDs and order | `site.recordings.read` |
| CAP-RECORD-04 | Saved (bookmarked) replays | `api/websites/[websiteId]/replays/saved/*`; `session_replay_saved`; `ReplaySaveForm.tsx` | 09 | `/sites/:siteId/replays?saved=1` | C: `recordings.save`, `recordings.unsave` | none | gaps: saved-replays.json (Confirmed, [0006](../decisions/0006-retention-clock-and-operations.md) Saved replays) | PG: mapped; exempt from the 14-day removal, counted in the 25 GiB cap and the 5 GiB saved sub-cap (Confirmed, [0006](../decisions/0006-retention-clock-and-operations.md)) | `site.recordings.read` |
| CAP-RECORD-05 | Heatmaps: click and scroll cells by page, version, viewport | `(main)/websites/[websiteId]/(reports)/heatmaps`; `api/websites/[websiteId]/heatmaps`; `sql/heatmap` | 09 | `/sites/:siteId/heatmaps` | Q: `heatmaps.cells` | none | Q13; collection: record-heatmap-* | PG: `heatmap_event` within 90 days aggregated; type `heatmap` reports mapped; MySQL: n/a | `site.recordings.read` |
| CAP-RECORD-06 | Recording cap (25 GiB), 14-day replay and 90-day heatmap retention, budget status | new | 09 (12 maintenance) | `/sites/:siteId/settings/recording`, `/admin/status` | Q: `recordings.budget` | Recorder outcomes at cap | workload: recording cap test, retention test | n/a | `site.configure`, `install.admin` |

## Sharing, boards, and annotations

| ID | Capability | Upstream 3.4.0 | Milestone | Screen/route | Operations | Collection surface | Fixture | Import mapping | Authorization |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| CAP-SHARE-01 | Boards: create, edit layout, cards from saved explorations, local overrides, clone, keyboard ordering | `(main)/boards/*`; `api/boards`, `api/boards/[boardId]`, `boards/[boardId]/clone`, `api/teams/[teamId]/boards`; `board` types `mixed`, `website`, `link`, `pixel` | 06 (07, 08, 09 add card kinds) | `/boards`, `/boards/:boardId`, `/boards/:boardId/edit` | Q: `boards.list`, `boards.get`; C: `boards.create`, `boards.update`, `boards.clone`, `boards.delete` | none | Q14 | PG: mapped where cards reference mapped sites/reports, else card preserved-unmapped (unsupported component); MySQL: n/a | `workspace.read`, `workspace.write` |
| CAP-SHARE-02 | Personal home dashboard | `(main)/dashboard`, `dashboard/edit`; `api/dashboard`; `board` type `dashboard` | 06 | `/` | Q: `boards.home`; C: `boards.update` | none | Q14 | PG: mapped to personal home board; MySQL: n/a | `account.self` |
| CAP-SHARE-03 | Annotations on charts, list and edit | `api/websites/[websiteId]/annotations/*`; `annotation` | 06 | Chart overlay; `/sites/:siteId/settings/annotations` | Q: `annotations.list`; C: `annotations.create`, `annotations.update`, `annotations.delete` | none | Q14 | PG: mapped; MySQL: n/a | `site.read`, `site.definitions.write` |
| CAP-SHARE-04 | Restricted shared explorations: sites, measures/fields, actions, lifetime; revocation | `share` table; `api/share`, `api/share/[slug]`, `api/share/id/[shareId]`, `api/websites/[websiteId]/shares`; `src/app/share/[slug]`; section toggles in `src/permissions/share.ts` | 10 (06) | `/sites/:siteId/settings/sharing`; viewer `/share/:token` | Q: `sharing.list`, `sharing.resolve`; C: `sharing.create`, `sharing.update`, `sharing.revoke` | none | Q14 access-scoped snapshot; access: AX-T* share rows (comparison window and recordings grant per [0007](../decisions/0007-access-and-account-security.md)), AX-S010, AX-S015 | PG `share` rows and MySQL `website.share_id`: mapping to restricted shares needs a decision; until then preserved-unmapped (broader scope than new shares) | `site.share`; viewer `share.viewer` |
| CAP-SHARE-05 | Shares of boards, links, and pixels | `api/boards/[boardId]/shares`, `api/links/[linkId]/shares`, `api/pixels/[pixelId]/shares` | 10 | Share dialog on board, link, pixel | Same `sharing.*` operations with entity kind | none | access: board shares (`p_share_board` rows); **GAP** for link and pixel shares until the `sharing.*` entity kinds are fixed | PG: as CAP-SHARE-04 | `workspace.write`; viewer `share.viewer` |
| CAP-SHARE-06 | Share and live delivery reauthorized on every read, export, subscription | `src/lib/auth.ts` share token | 10 | Viewer `/share/:token` | Q: `sharing.resolve`; T: scoped topics | none | Q14; access: AX-S010, AX-S013 to AX-S016 | n/a | `share.viewer` |

## Access and accounts

| ID | Capability | Upstream 3.4.0 | Milestone | Screen/route | Operations | Collection surface | Fixture | Import mapping | Authorization |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| CAP-ACCESS-01 | Bootstrap admin without a known default password | replaces upstream seeded default admin | 10 (03, 12 init) | First-run sign-in | CLI: `holocron init` | none | access: AX-S023 (no known default credentials); gaps: init-outcomes.json ([0006](../decisions/0006-retention-clock-and-operations.md)) | n/a | `operator` |
| CAP-ACCESS-02 | Sign in, sign out, session lifetime and revocation, cache clearing | `src/app/login`, `src/app/logout`; `api/auth/login`, `logout`, `verify` | 10 (03) | `/login` | C: `auth.login`, `auth.logout`, `auth.revokeSessions`; Q: `auth.session` | none | access: AX-S017, AX-S018, AX-S021, AX-S022, `auth.reauthenticate` rows | Both: logins reissued at cutover (sessions not carried) | anonymous, `account.self` |
| CAP-ACCESS-03 | 2FA: enroll, confirm, challenge, recovery codes, disable, reenroll | `api/2fa/*`; `src/app/login/two-factor`; `(main)/settings/security`; `two_factor_*` tables | 10 | `/account/security`, `/login/two-factor` | Q: `twoFactor.status`; C: `twoFactor.begin`, `twoFactor.confirm`, `twoFactor.disable`, `twoFactor.regenerateRecoveryCodes`, `auth.verifySecondFactor` | none | access: AX-S017, AX-S019, AX-S020, AX-S027, reauthentication rows for `twoFactor.disable` and `regenerateRecoveryCodes` ([0007](../decisions/0007-access-and-account-security.md)); reenroll row not yet written | PG: secret and backup codes handled by an explicit tested path; used-OTP and rate-limit rows preserved-unmapped (transient); MySQL: n/a | `account.self` |
| CAP-ACCESS-04 | 2FA requirement policy: installation, team, user | `(main)/admin/security`; `api/admin/2fa/global`, `admin/teams/[teamId]/2fa`, `admin/users/[userId]/2fa`; `app_setting`; `two_factor_required` flags | 10 | `/admin/security`, `/workspace/settings` | Q: `security.policy`; C: `security.updatePolicy` | none | access: AX-S020, AX-S027, `security.updatePolicy` rows ([0007](../decisions/0007-access-and-account-security.md) Required 2FA without enrollment) | PG: mapped; MySQL: n/a | `install.admin`, `team.manage` |
| CAP-ACCESS-05 | Profile and password change | `(main)/settings/profile`; `api/me`, `api/me/password` | 10 | `/account/profile` (user menu) | Q: `account.get`; C: `account.update`, `account.changePassword` | none | access: AX-S021 (password change with reauthentication, [0007](../decisions/0007-access-and-account-security.md)); profile fields have no cross-principal rows | Both: users mapped; bcrypt hashes through tested conversion | `account.self` |
| CAP-ACCESS-06 | Preferences: timezone, default date range, language, theme | `(main)/settings/preferences` (browser storage) | 10 (03 appearance, locale) | `/account/preferences`, user menu | Q: `account.preferences`; C: `account.updatePreferences` | none | access: account.self boundary only (`apiKeys.create` grid rows); no other protected surface | n/a (upstream keeps them in browser storage) | `account.self` |
| CAP-ACCESS-07 | Scoped API keys: operation and site grants, one-time disclosure, revocation | `(main)/settings/api-keys`; `api/me/api-keys/*`; `api_key` (user-wide) | 10 | `/account/api-keys` | Q: `apiKeys.list`; C: `apiKeys.create`, `apiKeys.revoke` | none | access: `apiKeys.create` grid rows, key ceiling rows, AX-S006, AX-S009, AX-S012, AX-S014 | PG: preserved-unmapped (reissued at cutover; name and prefix inventoried); MySQL: n/a | `account.self` |
| CAP-ACCESS-08 | Teams, membership, invitations, join by access code | `(main)/teams`, `(main)/settings/teams/*`; `api/teams/*`, `api/teams/join`; `team.access_code` | 10 | `/workspace/members`, `/workspace/settings`, `/teams/join` | Q: `teams.list`, `teams.get`; C: `teams.create`, `teams.update`, `teams.delete`, `teams.invite`, `teams.join`, `members.remove` | none | access: `teams.invite`, `members.updateRole`, `members.remove`, `teams.delete` rows; join by access code not yet written | Both: `team`, `team_user` mapped | `account.self`, `team.manage` |
| CAP-ACCESS-09 | Roles and site capability checks, guessed-ID refusal | `src/lib/constants.ts` `ROLES`, `ROLE_PERMISSIONS`; `src/permissions/*` | 10 (03 core) | `/sites/:siteId/settings/access`, `/workspace/members` | Q: `siteRoles.list`; C: `siteRoles.update`, `members.updateRole` | none | access: role-mapping.json, `cases.json` grid (22 operations x 12 principals); M10 checks that every registry operation has rows | Both: roles mapped to capability sets | `team.manage`, `site.configure` |
| CAP-ACCESS-10 | Site management: create, edit name/domain/timezone, transfer between user and team | `(main)/settings/websites/*`, `(main)/websites/[websiteId]/settings`; `api/websites`, `api/websites/[websiteId]`, `transfer` | 10 (03 read and update) | `/sites`, `/sites/:siteId/settings/general` | Q: `sites.list`, `sites.get`; C: `sites.create`, `sites.update`, `sites.transfer` | Configures site collection | access: `sites.update`, `sites.delete`, `sites.reset`, `sites.transfer` rows (member loses delete and reset, [0007](../decisions/0007-access-and-account-security.md)); `sites.create` not yet written | Both: mapped, website UUIDs preserved | `workspace.write`, `site.configure`, `site.transfer` |
| CAP-ACCESS-11 | Installation administration: users, teams, sites | `(main)/admin/users/*`, `admin/teams/*`, `admin/websites/*`; `api/admin/*`, `api/users/*` | 10 | `/admin/users`, `/admin/teams`, `/admin/sites` | Q: `users.list`, `users.get`; C: `users.create`, `users.update`, `users.delete` | none | access: `users.create` and `users.delete` rows, AX-S026 ([0007](../decisions/0007-access-and-account-security.md) User erasure scope); `users.update` not yet written | Both: mapped | `install.admin` |
| CAP-ACCESS-12 | Header workspace and site context switching from server-authorized options | `api/me/teams`, `api/me/websites` | 10 (03) | App header selectors | Q: `workspaces.list`, `sites.list` | none | access: AX-S011 (`workspaces.list` after removal); `sites.list` rows not yet written | n/a | `account.self` |

## Import

| ID | Capability | Upstream 3.4.0 | Milestone | Screen/route | Operations | Collection surface | Fixture | Import mapping | Authorization |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| CAP-IMPORT-01 | Read-only source inspection with migration signatures; refusal with guidance | new; `prisma/migrations` (3.4.0), `db/mysql/migrations` (2.20.2) | 11 (01 fixtures) | `/admin/imports` | CLI: `holocron import inspect`; C: `imports.inspect` | none | import: pg, mysql, refusal | n/a | `operator`, `install.admin` |
| CAP-IMPORT-02 | Event history: events, properties, session data, revenue, vitals, link/pixel events | `website_event`, `event_data`, `session_data`, `session`, `revenue` | 11 (04, 05, 08) | `/admin/imports/:importId` | C: `imports.start`, `imports.resume`, `imports.cancel`; Q: `imports.status`; T: `imports.progress` | none (trusted offline path) | import: pg, mysql; correctness: same facts through importer | mapped; source session/visit IDs preserved-unmapped as restricted provenance | `operator`, `install.admin` |
| CAP-IMPORT-03 | Accounts, teams, roles, credentials, 2FA conversion | `user`, `team`, `team_user`, `two_factor_*`, `api_key` | 11 (10) | `/admin/imports/:importId` | Q: `imports.inventory` | none | import: pg, mysql | see CAP-ACCESS-02 to 11 | `operator`, `install.admin` |
| CAP-IMPORT-04 | Saved objects: reports by type, segments, cohorts, boards, annotations, links, pixels, shares, saved replays, recorder config | `report`, `segment`, `board`, `annotation`, `link`, `pixel`, `share`, `session_replay_saved`, `website.replay_config` | 11 (06 to 10) | `/admin/imports/:importId` | Q: `imports.inventory` | none | import: pg, mysql | see the saved-object table below | `operator`, `install.admin` |
| CAP-IMPORT-05 | Recordings and heatmaps within retention | `session_replay`, `heatmap_event` | 11 (09) | `/admin/imports/:importId` | Q: `imports.status` | none | import: pg | see CAP-RECORD-02, CAP-RECORD-05 | `operator`, `install.admin` |
| CAP-IMPORT-06 | Unmapped inventory: preserved source representation, reason, count | new | 11 | `/admin/imports/:importId/unmapped` | Q: `imports.inventory` | none | import: pg, mysql | preserved-unmapped by definition | `install.admin` |
| CAP-IMPORT-07 | Deterministic provenance, resumable checkpoints, reconciliation manifests | new | 11 | `/admin/imports/:importId` | Q: `imports.reconciliation` | none | import: pg, mysql (interrupt and resume cases) | n/a | `install.admin` |
| CAP-IMPORT-08 | Imported limitation labels on reports | new | 11 (05, 08) | Report headers | Fields on report results | none | correctness: importer differences from unavailable history | n/a | `site.read` |
| CAP-IMPORT-09 | Cutover and rollback procedure | new | 11 | Runbook | CLI: `holocron import run` | none | import: pg, mysql; **GAP, deferred to 11**: rehearsal needs importer output from 04 and 05 (sequencing, no missing decision) | n/a | `operator` |

### Saved-object import mapping

| Source object | PostgreSQL 3.4.0 | MySQL 2.20.2 | Target |
| --- | --- | --- | --- |
| `report` type `funnel` | mapped | mapped | Funnel exploration |
| `report` type `goal` / `goals` | mapped | mapped (one definition per goal entry) | Goal definitions |
| `report` type `journey` | mapped | mapped | Journey exploration |
| `report` type `retention` | mapped, identity-limited | mapped, identity-limited | Retention exploration |
| `report` type `utm` | mapped | mapped | Campaign exploration |
| `report` type `attribution` | mapped where model maps | mapped where model maps | Attribution exploration |
| `report` type `revenue` | mapped | mapped | Revenue exploration |
| `report` type `breakdown` / `insights` | mapped where fields map | mapped where fields map | Exploration with split-by |
| `report` type `performance` | mapped | n/a | Performance exploration |
| `report` type `heatmap` | mapped | n/a | Heatmap view |
| `segment` type `segment` | mapped where filters map | mapped where filters map | Segment |
| `segment` type `cohort` | mapped where predicates map | mapped where predicates map | Cohort definition (results need consented population) |
| `board` | mapped; unsupported cards preserved-unmapped | n/a | Board |
| `board` type `dashboard` | mapped | n/a | Personal home board |
| `annotation` | mapped | n/a | Annotation |
| `link`, `pixel` | mapped | n/a | Link, pixel |
| `share`, `website.share_id` | mapped | mapped (`share_id`) | Restricted share at the same URL |
| `session_replay_saved` | mapped | n/a | Saved replay |
| `website.replay_config` | mapped, strict masking enforced | n/a | Recording policy |
| `api_key` | preserved-unmapped (reissued at cutover) | n/a | none |
| `two_factor_otp_used`, `two_factor_rate_limit` | preserved-unmapped (transient) | n/a | none |
| `app_setting` | mapped for 2FA policy; other keys preserved-unmapped | n/a | Security policy |
| `_prisma_migrations` | inspected only | inspected only | Signature check |

## Operations

| ID | Capability | Upstream 3.4.0 | Milestone | Screen/route | Operations | Collection surface | Fixture | Import mapping | Authorization |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| CAP-OPS-01 | Initialization of a new data directory | new | 12 (03) | none | CLI: `holocron init` | none | gaps: init-outcomes.json ([0006](../decisions/0006-retention-clock-and-operations.md) `holocron init` outcomes); access side AX-S023 | n/a | `operator` |
| CAP-OPS-02 | Liveness, collection readiness, runtime configuration | `api/heartbeat`, `api/config` | 12 (03 config, 04 readiness) | none | Health endpoints; runtime configuration endpoint | none | workload: ENOSPC and read-only storage cases (health distinction) | n/a | `public` (liveness), deployment |
| CAP-OPS-03 | Operational status: accepted/refused counts, inbox age/depth, watermark, lag, recording caps, import and maintenance state | new | 12 (04, 05, 09) | `/admin/status` | Q: `operations.status`; T: `operations.status` | none | workload: burst, soak, transfer pause | n/a | `install.admin` |
| CAP-OPS-04 | Coordinated backup and clean-machine restore | new | 12 | none | CLI: `holocron backup`, `holocron restore` | none | workload: backup and restore procedure | n/a | `operator` |
| CAP-OPS-05 | Retention maintenance: events 12 months, replay 14 days, heatmaps 90 days | new | 12 (09) | `/admin/status` | CLI: `holocron retention run`; Q: `jobs.status` | none | workload: retention test | n/a | `operator`, `install.admin` |
| CAP-OPS-06 | Site data reset and deletion across pending and applied records | `(main)/websites/[websiteId]/settings/WebsiteResetForm.tsx`, `WebsiteDeleteForm.tsx`; `api/websites/[websiteId]/reset` | 12 (10 screen) | `/sites/:siteId/settings/data` | C: `sites.reset`, `sites.delete`; Q: `jobs.status` | Fences site collection | workload: deletion and reset tests | n/a | `site.delete` |
| CAP-OPS-07 | User erasure | `api/users/[userId]` delete | 12 (10) | `/admin/users/:userId` | C: `users.delete`; Q: `jobs.status` | none | gaps: user-erasure.json ([0007](../decisions/0007-access-and-account-security.md) User erasure scope); deletion tests cover sites | n/a | `install.admin` |
| CAP-OPS-08 | Graceful shutdown and restart reconciliation | new | 12 (04) | none | Process signal handling | Admission stops | workload: transfer failpoints | n/a | `operator` |
| CAP-OPS-09 | Schema upgrade with pre-upgrade backup and recoverable failure | new | 12 | none | CLI: `holocron upgrade` | none | gaps: schema-upgrade.json ([0006](../decisions/0006-retention-clock-and-operations.md) Schema versions) | n/a | `operator` |
| CAP-OPS-10 | Scrubbed diagnostics bundle | new | 12 | none | CLI: `holocron diagnostics` | none | gaps: diagnostics-canaries.json (planted-canary check; the bundle format is left to 12) | n/a | `operator` |

## Upstream features not carried over

Each row is deliberately excluded or replaced, with the briefing sentence that decides it.

| Upstream feature | Source | Status | Briefing sentence |
| --- | --- | --- | --- |
| Legacy report API | `src/app/(compat)/compat/api/reports/*`, `compat/api/websites/[websiteId]/reports` | Excluded | "Exclude Umami v2 runtime APIs, generic legacy report APIs, ClickHouse imports, additional historical source snapshots, external databases/queues, browser event databases, and multi-server operation from this release." |
| Upstream REST dashboard API | `src/app/api/*` except collection routes | Replaced by generated command/query API | "Do not implement v2 tracker compatibility or legacy dashboard/report endpoints. The new dashboard uses a generated command/query API." |
| v2 tracker protocol | 2.20.2 tracker | Excluded | "Do not implement v2 tracker compatibility or legacy dashboard/report endpoints." |
| ClickHouse, Kafka, Redis backends | `src/lib/kafka.ts`, `src/lib/redis.ts`, ClickHouse queries | Excluded | "No production Node runtime, external database, external queue, or additional application service is required." and the exclusion sentence above |
| Fixed-format CSV export | `api/websites/[websiteId]/export` | Replaced by exact exports of the exploration state (CAP-EXPLORE-08) | "URLs, saved explorations, board cards, exports, and query keys use that state." |
| Anonymous cross-day retention and cohorts | `sql/retention`, `segment` type `cohort` | Replaced by consented identified retention (CAP-IDENT-04, 05) | "Persistent person-level retention and cohorts are available only for explicitly enabled, consented identified analytics." |
| Recorder defaults 15% sampling, moderate masking | `api/websites/[websiteId]/recorder/route.ts` | Replaced by 10% and strict masking (CAP-RECORD-01) | "Use strict text/input masking, sensitive-page/element exclusions, and 10% default replay sampling." |
| Upstream session hashing and continuous sessions | `api/send/route.ts` | Replaced (CAP-IDENT-01, 02) | "Existing upstream tokens can be replaced at cutover; continuous old sessions are not promised." |
| Seeded default admin password | upstream seed | Replaced (CAP-ACCESS-01) | "Generate or require bootstrap credentials; do not ship a known default password." |
| Carried-over logins and API key secrets | `api_key`, sessions | Reissued (CAP-ACCESS-02, 07) | "Reissue logins and scoped API keys at cutover." |

## Coverage summary

**Count per closing milestone** (101 capabilities):

| Milestone | Count | Capabilities |
| --- | --- | --- |
| 01 to 03 | 0 | Enabling only (contracts, storage gate, shell); none closes a capability |
| 04 | 14 | COLLECT-01 to 04, 06 to 11, 15 to 18 |
| 05 | 5 | COLLECT-05, COLLECT-19, IDENT-01 to 03 |
| 06 | 27 | TRAFFIC-01 to 15, EXPLORE-01 to 09, SHARE-01 to 03 |
| 07 | 6 | BEHAV-01 to 06 |
| 08 | 6 | IDENT-04 to 06, REVPERF-01 to 03 |
| 09 | 9 | COLLECT-12 to 14, RECORD-01 to 06 |
| 10 | 15 | SHARE-04 to 06, ACCESS-01 to 12 |
| 11 | 9 | IMPORT-01 to 09 |
| 12 | 10 | OPS-01 to 10 |
| 13 | 0 | Re-verifies all; reconciles this matrix |

**Gaps.** Every capability has a closing milestone. Fixtures under `fixtures/access/` and `fixtures/gaps/` now cover channel classification, weekday grid, realtime window, link and pixel totals, segment definitions, campaign normalization, initialization, user erasure, schema upgrade, saved replays, diagnostics canaries, and the access and sharing capabilities SHARE-04 to 06 and ACCESS-01 to 12. Their rules come from decisions 0001, 0002, 0003, 0004, 0006 and 0007. Access rows are partial where the table says so (`sites.create`, `users.update`, 2FA reenroll, team join, link and pixel shares).

Still without a fixture:

- Replay hostile content (RECORD-03): the replay format is left to milestone 09.
- Cutover rehearsal (IMPORT-09): needs importer output from 04 and 05.
- Consent wire cases (COLLECT-05) belong with the collection fixtures.

Fixture rows carry a `status` naming the record that settles them, or `proposed (milestone 01)`. Remaining unnamed error codes are listed in each file's `open` list.

**Upstream features whose scope the briefing does not settle.** Decided by Mike on 2026-10-04:

| Upstream feature | Source | Decision |
| --- | --- | --- |
| MCP server endpoint | `src/app/mcp/route.ts` (`@umami/mcp`) | Deferred; it would sit on the generated API with scoped keys if wanted |
| SSO token handoff | `src/app/sso`, `api/auth/sso` (requires Redis) | Excluded; depends on Redis, and 10 forbids unexpected external auth dependencies |
| Cloud subscription status | `api/auth/subscription`, `src/lib/subscription.ts` | Excluded; Umami Cloud billing, not self-hosted behavior |
| Telemetry script | `api/scripts/telemetry` | Excluded |
| Tracker test console | `(main)/console/[websiteId]` | Excluded |
| Public share slug continuity at cutover | `share.slug`, `website.share_id` | Imported public share URLs keep working as restricted shares; milestone 11 maps them |
