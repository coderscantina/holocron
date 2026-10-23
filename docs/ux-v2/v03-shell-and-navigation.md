# v03: Shell, top bar, site menu and grouped navigation

## Outcome

The dashboard gets the v2 shell: a frame with the sidebar and a top bar, and the page as one rounded canvas inset into it, with filled controls and hairline panels. The sidebar holds the jump field and four navigation groups with attention badges and a live count. The top bar holds the workspace and site chips, a slot for the page's scope chips, and the user menu. The site menu shows each site's status and groups sites by base domain. Nothing here runs an analytical query.

## Dependencies and ownership

The layout, surface tokens, top bar, sidebar search, grouping, base-domain grouping and the "All sites" entry need nothing and can start now. Pace, finding dots and badges need v02's `summaries.forSites` and `summaries.findings`. Frontend owns all files here, and this brief owns the shell's surface and geometry tokens that every other v2 brief uses.

Owned: `apps/web/src/layouts/AppLayout.vue`, `apps/web/src/components/app/*` (`AppHeader.vue` becomes the top bar, `AppSidebar.vue`, `AppSidebarNav.vue`, `AppNavLink.vue`, `AppContextSwitcher.vue`, `AppBrand.vue`), a new `apps/web/src/components/app/AppTopBarSlot.ts` (or equivalent outlet), `apps/web/src/components/PageHeader.vue` (the page title row), the surface and geometry tokens in `apps/web/src/assets/css/app.css`, the `ui/button` and `ui/card` variants where the filled and hairline styles need them, `apps/web/src/composables/useNavigation.ts`, `apps/web/src/lib/navigation.ts`, a new `apps/web/src/lib/base-domain.ts`, a new `apps/web/src/composables/useSiteSummaries.ts`, their tests, and `nav.*`, `shell.*` and `context.*` locale keys. Read first: [design contract](design.md) Surfaces and geometry, Shell and Canvas pages; the reference [examples/v2/index.html](../../examples/v2/index.html); [Kit implementation](../kit-implementation.md) Header and navigation, Tokens and geometry; [kit-extraction-map](../contracts/kit-extraction-map.md) Environment selector.

Do not use `examples/v2/rejected/first-draft.html`; its shell was rejected.

## Current code (2026-10-15)

- Commit `0fd22b5` already moved the light canvas to white on a gray-50 frame, set the dark ladder to gray-950 frame and gray-925 canvas, shortened the header to 2.75rem, switched secondary actions to ghost buttons, and set the indigo accent. Build on it.
- `AppHeader.vue` spans the main column: brand on phones, sidebar toggle, `AppContextSwitcher`, then the palette trigger and `AppUserMenu` on the right.
- `useNavigation()` returns a workspace section (Boards) and one flat site section of 15 items, plus `settings` and phone `tabs`. `NavigationItem { id, labelKey, icon, to }` has no badge or group field.
- `AppContextSwitcher.vue` renders workspace and site dropdowns from `workspaces.list` through `useSiteContext`. Its doc comment records that there is no environment control.
- `useLiveSnapshot` in `composables/useLive.ts` and `realtime.snapshot` provide the live count.

## Work

1. Tokens: add the geometry tokens from the design contract (`--radius-control`, `--radius-panel`, `--radius-canvas`, `--shell-canvas-inset`, `--shell-control-height`, `--shell-nav-row`, `--shell-page-x`, `--table-row`, `--table-head`) and their compact-density values to `app.css`, and the fill and hairline roles for both modes. Map them to Tailwind utilities the way the existing shell tokens are mapped. No component outside the shell sets these values locally.
2. Layout: in `AppLayout.vue`, place the canvas as one panel with `--radius-canvas` inset from the frame, with the top bar on the frame above it and no full-width header border. Keep the skip link, route focus, announcements and error boundary working; the main region is the canvas.
3. Sidebar: brand with the collapse button, the jump field under it (trigger for the existing palette, filled, "Jump to anything", ⌘K / Ctrl K hint), the four groups from the design contract with sentence-case headings, filled active row, and the server card slot at the bottom. Remove the border between sidebar and canvas. The collapsed rail turns the jump field into an icon button.
4. Top bar: workspace chip, site chip, divider, an outlet where the current page mounts its scope chips (v04 fills it for explorations), user menu as a round avatar on the right. Chips use the filled control style and `--shell-control-height`. Pages without scope chips leave the outlet empty. Implement the overflow rule: scope chips collapse into one summary chip before the row wraps.
5. Page title row: turn `PageHeader.vue` into the design contract's row at the top of the canvas (name, bullet, muted context, actions, freshness note on the right, hairline below). Every page that uses `PageHeader` gets it; pages that render their own heading keep working until their brief moves them.
6. `NavigationSection` gets a translated heading per group, and the site section splits into Traffic, Behaviour, Acquisition and Experience. Keep every route and id. Phone tabs stay overview, explore, realtime; the phone drawer shows the jump field and the groups.
7. Add an optional `indicator` to `NavigationItem`: `{ kind: "count", value, label: MessageKey }` or `{ kind: "live", value }`. `AppNavLink` renders the count as a solid accent badge, the live value with a success dot, and a dot only in the collapsed rail with the count in the tooltip. Accessible names include the count.
8. Feed the Realtime live count from the existing live snapshot of the current site. While the subscription is not current, show no number.
9. Add `lib/base-domain.ts`: the last two labels of the site domain, with a short, tested exception list for two-label public suffixes (`co.uk`, `com.au`, `co.at`, `or.at`, `gv.at` and the like). Group the site menu by it; one group shows no heading. Add the "All sites in {workspace}" first row.
10. With v02: `useSiteSummaries(siteIds)` over `summaries.forSites` in Vue Query, keyed by authorization scope, refetched on the summaries topic. Render the site chip's pace, each menu row's pace text and finding dot, and the nav badges from `summaries.findings`. Stale summaries render name only.
11. Server card for install admins, from v07's status query. Until v07 lands, render nothing.

## Acceptance criteria

- The shell matches the reference's layout and surfaces at 1440 px in dark mode, and the same rungs read correctly in light mode (filled controls distinguishable from the white canvas).
- Every current route is reachable from the grouped sidebar, the collapsed rail and the phone drawer. Active state, route focus and the skip link behave as before.
- The top bar never wraps above 768 px; scope chips collapse first. Long German labels and a 60-character site name fit.
- Base-domain grouping passes focused tests for subdomains, two-label suffixes, IP-like and single-label domains, and internationalized names.
- The site menu with 40 sites in 6 groups scrolls inside the menu at 390 px and 1440 px. Keyboard selection, Escape and focus return work as today.
- Opening the site menu issues no request beyond `summaries.forSites` from cache or one refetch.
- A user with access to some sites of a workspace sees only those sites, pace and dots.
- Badges and live count have accessible names; color is never the only signal.
- Pages not yet moved to v2 still render inside the canvas without overflow or doubled headings.
- English and German keys exist together (`i18n/parity.test.ts`).

## Checks

From `apps/web`: `bun run typecheck`, `bun run lint`, `bun run test`, and the shell journeys in `e2e/shell.spec.ts` extended for the new layout, groups, badges and the menu (`bun run e2e`). Visual check at the widths, modes and densities in the design contract, against the reference.

## Handoff

Screenshots and the e2e cases in `docs/evidence/v03-shell-and-navigation.md`. v04 consumes the top-bar outlet and the tokens; v06 consumes `useSiteSummaries`; v08 consumes the jump field trigger.
