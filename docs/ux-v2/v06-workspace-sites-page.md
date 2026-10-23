# v06: Workspace sites page

## Outcome

A team with several sites sees which ones moved today on one page, without switching between them. The site menu's "All sites" entry opens it.

## Dependencies and ownership

Requires v02 (`summaries.forSites`) and v03 (`useSiteSummaries`, base-domain grouping). Frontend owns it.

Owned: `apps/web/src/pages/workspace/WorkspaceSites.vue` and a new `SiteStatusRow.vue` beside it, the route entry if the section needs its own path, tests, and `workspace.sites.*` locale keys. Read first: [design contract](design.md) Workspace sites page.

## Current code (2026-10-15)

- `WorkspaceSites.vue` exists under `/workspaces/:workspaceId` and lists the workspace's sites for management. Check what it shows and who sees it before changing it.
- `workspaces.list` returns sites with id, name and domain; no analytics.

## Work

1. Add a status view of the sites list: today's views so far, pace in words, finding count, summary time, and a 24-hour sparkline of today against the typical day. The sparkline uses the 24 hourly values in v02's summary row, never a per-site query.
2. Sort by finding count, then by absolute pace. Group by base domain only when there are more than 8 sites.
3. A row opens the site's overview with the default state. Management actions stay where they are today and keep their permission checks.
4. Stale and missing summaries render the name and "No status yet" or the summary time.
5. Phone: rows stack name, pace and findings; the sparkline hides below 480 px.

## Acceptance criteria

- One request (`summaries.forSites`) loads the page for 40 sites. No per-site analytical query.
- Only sites the person can read appear; counts in "4 sites, 1 needs you" match the list.
- Sorting matches the fixture with ties broken by site name.
- Keyboard and screen reader: each row has one accessible name with name, pace and findings.

## Checks

From `apps/web`: `bun run typecheck`, `bun run lint`, `bun run test`, an e2e case in `e2e/shell.spec.ts` or a new spec for the page. Visual check at 1440 and 390 px in both modes.

## Handoff

Evidence at `docs/evidence/v06-workspace-sites-page.md`.
