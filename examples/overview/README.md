# Roadhouse overview reference

An interactive screen based on the supplied Umami overview, using Kit's surface tokens, shell, control sizing, filter chips, saved-view behavior, and navigation patterns.

The app header uses the supplied Clerk-style workspace / website / environment context selectors. The right-aligned user dropdown follows Kit's AppUserMenu; user identity and account navigation are absent from the sidebar. Dropdown selections are local reference-screen interactions.

Open `index.html` directly, or serve the workspace and navigate to `/examples/overview/`. No installation or build is required. The production dashboard specified in [the briefing](../../docs/technical-briefing.md) uses Vue; this example is dependency-free HTML/CSS/JavaScript.

The example supports URL-backed filter state, row drill-down, browser Back, comparison and measure selection, saved explorations in browser storage, search, CSV export, theme switching, and responsive navigation. Other navigation entries open an explanatory dialog rather than representing implemented product pages.

Values are sample data derived from the supplied screenshot. Additional series and audience rows are illustrative. Filters and dates demonstrate state changes without claiming to recalculate analytics.

Primary Kit references:

- `/Users/mwallner/Development/products/kit/resources/js/assets/css/app.css`
- `/Users/mwallner/Development/products/kit/docs/design-tokens.md`
- `/Users/mwallner/Development/products/kit/resources/js/layouts/AppLayout.vue`
- `/Users/mwallner/Development/products/kit/resources/js/components/app/AppSidebar.vue`
- `/Users/mwallner/Development/products/kit/resources/js/components/app/AppHeader.vue`
- `/Users/mwallner/Development/products/kit/resources/js/components/app/AppUserMenu.vue`
- `/Users/mwallner/Development/products/kit/resources/js/components/ui/data-table/TableFilter.vue`
- `/Users/mwallner/Development/products/kit/resources/js/composables/useTableQueryState.ts`

The screenshot labels are adapted to the briefing's new metric definitions. Historical figures have not been recalculated under those definitions.
