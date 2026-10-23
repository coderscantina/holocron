# v06: Workspace sites page

Status view of the workspace sites list at `/workspaces/:workspaceId` (the route the site menu's "All sites" row already opens, so no new path and no change to `AppContextSwitcher.vue`).

## Acceptance

| Criterion | Result | Evidence |
| --- | --- | --- |
| One `summaries.forSites` request loads the page, no per-site analytical query | Pass | `e2e/workspace.spec.ts` counts requests after a reload: one `summaries.forSites`, no `analytics.*`. Seed has 3 sites here; the 40-site case is the chunking test in `api/resources/summaries.test.ts` (100 per call), not exercised in the browser |
| Only readable sites appear, "N sites, M need you" matches the list | Pass | List comes from `workspaces.list`; count line uses the same array and `needingAttention` (`site-status.test.ts`) |
| Sorting: findings, then absolute pace, ties by name | Pass | `sortSites` test with ties and stale/missing sites last |
| Group by base domain only above 8 sites | Pass | `arrangeSites` tests at 8 and 9 |
| One accessible name per row with name, pace, findings | Pass | e2e asserts the link's accessible name; sparkline text is its description |
| Stale and missing summaries | Pass | Missing: "No status yet". Stale: "Status from {time}". Both drop pace, findings and sparkline. Unit-level via `sortSites`; stale and missing rows were not seen in the browser (all seeded summaries were fresh) |
| Row opens the overview with the default state | Pass | e2e clicks a row and lands on `/sites/:id` without query |
| Management actions and permission checks unchanged | Pass | "Add site" and its dialog keep the `workspace.write` check |
| Phone stacks name, pace and findings; sparkline hidden below 480 px | Pass | e2e asserts the svg is hidden at 390 and no horizontal overflow; screenshots |
| Sparkline: measured solid, typical dotted line, text equivalent | Pass | Screenshots; `sparkline` tests |

## Commands

From `apps/web`: `bun run typecheck`, `bun run lint`, `bun run test` (98 tests, 20 files), and
`HOLOCRON_E2E_URL=http://127.0.0.1:8094 HOLOCRON_E2E_PASSWORD=… HOLOCRON_E2E_SHOTS=../../docs/evidence/v06 bunx playwright test e2e/workspace.spec.ts` against the packaged server with `seed_demo` and `seed_traffic 45` (1 passed).

## Screenshots

[v06/](v06/): `workspace-sites-{1440,390}-{light,dark}.png`. Dark at 1440 and light at 390 were captured but only light 1440 and dark 390 were inspected by eye.

## Not verified

- German strings in the browser (key parity test passes).
- More than 8 sites and the grouped layout in the browser; covered by unit tests only.
- Findings dot with real counts: `finding_count` is 0 in the seed.

## Notes

- `workspace.sites` was a string key; it is now an object and the old label is `workspace.sites.title` (one line in `WorkspacePage.vue`).
- The pace wording is the `context.pace.*` keys, duplicated as a small switch in `SiteStatusRow.vue`.
