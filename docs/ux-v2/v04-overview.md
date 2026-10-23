# v04: Overview v2

## Outcome

The site overview opens with a sentence that states the range's total, its comparison and, when the range contains now, a labelled projection. Under it: the five measures as a segmented strip, a trend chart with measured, projected and baseline values and a Now marker, the finding banner and cards, breakdown tables with a Movers mode, and a goals panel with pace. The scope chips sit in the shell's top bar.

## Dependencies and ownership

Requires v01 for the wire types and v03 for the shell, tokens, top-bar outlet and page title row. Findings need v02 and the components from v05; until then the section renders nothing. Frontend owns all files here.

Owned: `apps/web/src/pages/overview/*`, `apps/web/src/components/exploration/TrendChart.vue`, `MeasureCards.vue`, `ChangeBadge.vue`, `ExplorationControls.vue`, `ExplorationHeader.vue`, `RangeMenu.vue`, new `apps/web/src/components/exploration/Headline.vue` and `GoalPace.vue`, `apps/web/src/services/exploration.ts` (default state and compare operations only), tests, and `overview.*` and `exploration.*` locale keys. Read first: [design contract](design.md) Surfaces and geometry, Canvas pages and Overview; the reference [examples/v2/index.html](../../examples/v2/index.html) (not the rejected draft); [0015](../decisions/0015-baselines-projections-and-noticed-changes.md), [Kit implementation](../kit-implementation.md) Exploration and table state, and Queries, live state and freshness.

## Current code (2026-10-15)

- `OverviewView.vue` makes one `analytics.explore` call through `useExplorationScreen` with six panels (path, referrer_domain, browser, os, device, country) and renders `ExplorationHeader`, `MeasureCards`, `TrendChart`, `AnnotationsControl`, `BreakdownPanel` and `WorldMap`.
- `defaultState()` in `services/exploration.ts`: five measures (pageviews, sessions, visitor_days, bounce_rate, span_mean), last 7 days, day granularity, `previous_period`, bar chart.
- `TrendChart.vue` is custom SVG with at most two measures, a hover tooltip and an accessible table.
- State lives in `?s=` as canonical base64url JSON.

## Work

1. Scope chips: turn `ExplorationControls` into the chips of the design contract (range with "until now", comparison, one chip per filter or segment, "+ Filter or segment") and mount them in v03's top-bar outlet. Saved views, export, refresh and pin move to the page title row's actions. It keeps calling the exploration service's existing operations. Add the comparison chip's third option and its disabled-with-reason states from the generated limits. The filter editor keeps its staged flow and opens in a popover from the chips.
2. Default state: today with hour granularity and `typical_weekday` (0015 decision 4). Existing URLs and saved views keep their state.
3. `Headline.vue`: render the grammar table of the design contract from result values only, through typed i18n templates in English and German. Every number and name in it is a link or button to the state that shows it. No string concatenation of translated fragments; each sentence is one message with placeholders.
4. The "why" line: the top dimension-mover finding that explains at least half the change, and an annotation from `annotations.list` inside its window when one exists. Without v02 the line is left out.
5. Measures strip: `MeasureCards` becomes the segmented strip of the design contract: a filled track, the selected measure raised, label, value and change in words and arrow per cell, revenue as a sixth cell only when the site has revenue. Selecting a cell sets the headline and chart measure. Phones get a 2-column grid in the same track.
6. `TrendChart`: a hairline panel with a segmented Hourly / Daily control and the legend in its header; measured bars, projected bars (outline, dashed, light fill), baseline line, Now marker at the watermark when analytics is stale and otherwise at the clock, consequence sentences under the chart, and the right-aligned exactness note. The tooltip and the accessible table name estimates as estimates. Add central series tokens for projected and baseline to `assets/css/app.css` next to the existing series tokens.
7. Breakdown panels: `BreakdownPanel` follows the table pattern (56px header with title, faint count and a small segmented Top / Movers switch; filled header row with icons; `--table-row` rows; column dividers; row actions in a menu on hover and focus). Movers needs the panel to request comparison rows; extend the panel spec rather than issuing a second request. Muted rows under the quiet threshold, diverging bars, include/exclude/explore actions unchanged. Referrers show the 5-pip share meter and a share column. Browsers, systems and devices become three small tables in one row; countries with the map stay below.
8. `GoalPace.vue`: when the site has active goals, one `goals` report request with `typical_weekday` for up to 8 goals; rows per the design contract, in the table pattern. It is a second request; run it in parallel and keep it out of the main snapshot's loading state.
9. Mount v05's finding banner and cards between the chart and the breakdowns.
10. States from the design contract's table, each with a focused test or e2e case: insufficient history, stale summary, filtered overview, shared exploration, analytics stale, retention-clipped range.

## Acceptance criteria

- Against the v01 fixture with the clock pinned, the headline, measures, chart, movers and goal rows show the fixture's values, and the projection carries its band and basis in the tooltip and the table.
- Changing range, comparison or filter produces one history entry and one `analytics.explore` request (plus the goals request when goals exist). Back restores the previous state without a refetch from duplicate watchers.
- A past range shows no projection, no Now marker, and the past-range sentence.
- Movers are disabled with a reason when `compare` is `none`.
- The accessible table alternative agrees with the chart, including estimate labels.
- Headline wraps to at most three lines at 1024 px with German strings. No horizontal page overflow at 390 px.
- Light and dark, grayscale: measured, projected and baseline are distinguishable.
- Shared explorations render without findings, context chips, badges or site menu.
- The page matches the reference's surfaces and spacing at 1440 px in dark mode, and the same rungs read correctly in light mode.

## Checks

From `apps/web`: `bun run typecheck`, `bun run lint`, `bun run test` (headline grammar, state defaults, movers threshold), `bun run e2e` with `e2e/exploration.spec.ts` extended for the overview journeys. Visual check at the widths and modes in the design contract.

## Handoff

Screenshots and cases in `docs/evidence/v04-overview.md`. v09 runs the integrated journeys.
