# v03 evidence: shell, top bar, site menu and grouped navigation

Status: in progress. Work items 1 to 9 and 11 are done. Item 10 is done, including finding dots and badges.

## Acceptance criteria

| Criterion | Result | Evidence |
| --- | --- | --- |
| Shell matches the reference at 1440 px dark; light mode reads correctly | Passed by eye | `v03/shell-overview-{1440,1024,768,390}-{dark,light}.png`, `v03/nav-sidebar-1440-dark.png`. Filled controls stay distinguishable on the white canvas. |
| Every route reachable from the grouped sidebar, collapsed rail and phone drawer; active state, route focus and skip link as before | Passed | `e2e/shell.spec.ts`; `v03/nav-rail-1440-light.png`, `v03/nav-drawer-390-{dark,light}.png`. |
| Top bar never wraps above 768 px; scope chips collapse first; long German labels and a 60-character site name fit | Partly | The bar height is asserted at 1440, 1024 and 768 px. No page mounts scope chips yet, so the collapse rule was checked only in a temporary harness. v04 adds the first chips. German: `v03/overview-de-{1440,390}-dark.png`. |
| Base-domain grouping tests | Passed | `lib/base-domain.test.ts`: subdomains, two-label suffixes, IP-like, single-label and internationalized names. |
| Site menu with 40 sites in 6 groups scrolls inside the menu at 390 and 1440 px; keyboard, Escape and focus return | Passed | `e2e/shell.spec.ts` with 40 mocked sites; `v03/nav-site-menu-{1440,390}-{dark,light}.png`. |
| Opening the site menu issues no request beyond `summaries.forSites` | Passed | `e2e/shell.spec.ts` pace test: no query besides `summaries.forSites`, at most one refetch. Finding dots read `Summary.finding_count` from that answer, so they add none. |
| Users see only the sites they can access, pace and dots | Passed | Sites come from `workspaces.list`; pace and dots come from the summaries of those sites only. |
| Badges and live count have accessible names; color is never the only signal | Passed | Live count: success dot plus number, shown only while the subscription is current. Finding badges name themselves ("Goals, 2 findings"), the rail shows a dot with that name in its tooltip, and a menu row with findings adds "N findings" for screen readers. `e2e/shell.spec.ts` "findings badge the report items…" with mocked `summaries.findings` and `summaries.forSites`; `v03/findings-nav-badges-1440-dark.png`, `v03/findings-nav-rail-dot-1440-dark.png`, `v03/findings-site-menu-dot-1440-dark.png`. |
| Pages not yet moved to v2 render inside the canvas without overflow or doubled headings | Passed | `e2e/shell.spec.ts` overflow checks. |
| English and German keys exist together | Passed | `i18n/parity.test.ts`. |

## Commands

```
cd apps/web && bun run typecheck && bun run lint && bun run test
HOLOCRON_E2E_URL=… HOLOCRON_E2E_PASSWORD=… HOLOCRON_E2E_VIEWER_PASSWORD=… npx playwright test e2e/shell.spec.ts
```

`shell.spec.ts` passed 16, skipped 1 by design (the runtime-config journey) against packaged builds of `v2/v03-a` and `v2/v03-b` seeded with `seed_demo`.

## Findings in the shell

- Goal-pace findings badge Goals, funnel-step findings Funnels, performance findings Performance. Dimension movers badge nothing, as the design says. A stale answer badges nothing. One shared `useSiteFindings` query (`useCurrentFindings`) feeds the badges and the palette.
- The menu dot reads `finding_count` of a fresh summary. It is always the accent: the design asks for the destructive token when a finding is a decrease, but `Summary` carries no direction.
- The site chip's accessible name is stable ("Website: {name}. Switch website"); its pace in words is the accessible description.

## Interfaces for later briefs

- Scope chips: `<TopBarScope summary="…">chips</TopBarScope>` teleports into the top bar; state in `components/app/AppTopBarSlot.ts`. Chips use `Button variant="fill" size="control"` or `h-shell-control rounded-control bg-fill`.
- `NavigationItem.indicator`: `{ kind: 'count'; value; label: MessageKey }` or `{ kind: 'live'; value }`; `nav.indicator.findings` is ready.
- `baseDomain(domain)` and `groupByBaseDomain(sites)` in `lib/base-domain.ts`.
- `AppServerCard.vue` in `AppSidebarNav.vue` renders v07's `ServerCard` for install admins.

## Open items

- Default density stays compact (existing `appConfig`); screenshots at default use 34px controls and an 8px inset, the e2e forces comfortable to check the contract values.
- "All sites in {workspace}" links to the existing workspace page; v06 repoints it.
- `AccountPage.vue` keeps a sticky offset that is now too large, because the header sits outside the scroll area.
- `usePageMeta` loops when a parent page and its child both call it (found by v07).
