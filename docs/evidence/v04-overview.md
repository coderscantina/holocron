# v04: Overview v2

Evidence for [the brief](../ux-v2/v04-overview.md). Items 1 to 3 and 5 to 9 landed in earlier v04 branches (scope chips, default state, headline, measures strip, trend chart, breakdowns, goal pace, findings mount). This file adds item 4, the line under the headline, and holds the acceptance table for the whole brief.

## The line under the headline (item 4)

- `why()` in `apps/web/src/pages/overview/headline.ts` picks the first dimension-mover finding in rank order whose `share` is at least 0.5. `share` is stored as 0 to 1 (`Finding.share`); the server's own floor is `FINDING_MOVER_SHARE_PERCENT` = 25 %, so the line names only movers at twice that.
- Direction ("lift" or "drop") comes from the sign of `today − typical`, the change is in visitor-days, and "since {time}" is `window_start` in the state's timezone, which is the site's on the default overview.
- An annotation from `annotations.list` with `window_start ≤ at ≤ computed_at` adds the annotation sentence; with several, the earliest. The annotated sentences are their own messages, not appended fragments.
- No qualifying mover reads "Everything is within its usual range." The line is left out without a comparison, while the summary is stale or more than 30 minutes behind (`listState`), and for any range other than today, because findings measure today so far against a typical weekday.
- `Headline.vue` renders it muted under the headline. The dimension value is a button that commits the finding's filter through the overview's `filterTo` (`applySegment` and `screen.commit`, one history entry), the same path as "Filter to this". The annotation name is plain text: annotations have no page of their own to link to.
- The overview passes findings only when unfiltered. It reads the same `['summaries.findings', site, 20]` cache entry `FindingList` keeps live, and the annotations the chart already loaded, so the line adds no request and no stream.
- Messages: `overview.headline.why.{lift,drop,liftAfter,dropAfter,usual,filter}` in `en.json` and `de.json`.

## Checks (from `apps/web`)

| Check | Result |
| --- | --- |
| `bun run typecheck`, `bun run lint` | pass |
| `bun run test` | 116 pass, 7 new in `pages/overview/headline.test.ts` (lift, drop, share under half, annotation inside and before the window, stale, no comparison; English and German) |
| `e2e/findings.spec.ts` | 4 pass, incl. the new "the line under the headline names the mover; its value adds the filter with one history entry" (mocked findings) |
| `e2e/exploration.spec.ts` | 9 pass, 1 known failure ("acceptance workflow": expects "AT", the row reads "Austria"), 1 skipped (revoked site, needs `HOLOCRON_E2E_DB`) |

Server: packaged from this branch, port 8096, `seed_demo` and `seed_traffic 45`.

## Screenshots

All in [v04/](v04/).

- Overview: `overview-{1440,1024,768,390}-{dark,light}.png`, `overview-default-1440-dark.png`, `overview-filtered-dark.png`, `overview-compare-blocked-dark.png`, `explore-390-{dark,light}.png`, `filter-group-dark.png`.
- Chart: `chart-overview-{1440,390}-{dark,light}.png`, `chart-panel-{1440,390}-{dark,light}.png`.
- Tables: `tables-movers-{1440,390}-{dark,light}.png`, `tables-top-no-comparison-1440-dark.png`, `tables-goals-1440-dark.png`.
- Line under the headline, mocked findings: `why-{1440,390}-{dark,light}.png`.

## Acceptance

| Criterion | Result | Evidence |
| --- | --- | --- |
| Against the v01 fixture with the clock pinned, headline, measures, chart, movers and goal rows show the fixture's values; the projection carries band and basis in tooltip and table | Partly. Grammar, estimate text, movers and goal pace are unit-tested on fixed inputs; the e2e journeys run on seeded traffic and live time, not the v01 fixture with a pinned clock | `headline.test.ts`, `trend.test.ts`, `breakdown.test.ts`, `goal-pace.test.ts`; e2e "the trend chart labels the estimate, the typical day and now" |
| Range, comparison or filter change: one history entry and one `analytics.explore` request (plus goals); Back restores without a refetch | Pass | e2e "the overview opens on today against a typical weekday; chips and measures each take one step", "goal pace: one goals request beside the snapshot" |
| A past range shows no projection, no Now marker, the past-range sentence | Pass | e2e "the trend chart labels the estimate…" (Daily, past range: no Now, no Estimate); `trend.test.ts` "is absent for a range that ended before now"; `headline.test.ts` "compares a past range" |
| Movers disabled with a reason when `compare` is `none` | Pass | e2e "breakdowns switch between Top and Movers within the one snapshot"; `tables-top-no-comparison-1440-dark.png` |
| Accessible table agrees with the chart, incl. estimate labels | Pass | e2e "the trend chart labels the estimate…" (table columns, estimate cells, caption) |
| Headline wraps to at most three lines at 1024 px in German; no horizontal overflow at 390 px | Overflow: pass (e2e overflow checks at 390). German wrap at 1024: not measured | e2e "the overview shows real figures at every width in both modes"; `overview-1024-*.png` are English |
| Light and dark, grayscale: measured, projected and baseline distinguishable | Not verified in grayscale; colour captures only | `chart-panel-*.png` |
| Shared explorations render without findings, context chips, badges or site menu | Not covered by a v04 test | `e2e/access.spec.ts` opens a share; it does not assert the v2 absences |
| Page matches the reference's surfaces and spacing at 1440 dark, and light | Captured, compared by eye only | `overview-1440-{dark,light}.png` |
| Line under the headline names the top mover over half the change, the annotation inside its window, "usual range" otherwise, nothing without a comparison or when stale | Pass | `headline.test.ts` "why line"; e2e "the line under the headline names the mover; its value adds the filter with one history entry"; `why-*.png` |
| Dimension value applies the finding's filter, one history entry; filtered overview leaves the line out | Pass | same e2e case |

### States (item 10)

| State | Evidence |
| --- | --- |
| Insufficient history | `headline.test.ts` "reads as uncompared when a typical weekday lacks history", "says why there is no estimate when history is short"; `goal-pace.test.ts` "says when there is not enough history" |
| Stale summary | `headline.test.ts` "says nothing from stale findings"; findings notice from v05 |
| Filtered overview | e2e "the overview opens on today…" (`overview-filtered-dark.png`); e2e "Filter to this adds one history entry" ("For all traffic") |
| Shared exploration | No focused case |
| Analytics stale | `trend.test.ts` "sits at the watermark while analytics is stale", "reads a stale freshness as "Data until" the watermark" |
| Retention-clipped range | No focused case; `FreshnessNote.vue` shows `retention_start` |

## Not verified

- v01 fixture with a pinned clock in the browser.
- German headline wrap at 1024 px, grayscale check, shared exploration and retention-clipped range cases.
- The line on live findings: seeded traffic did not produce a qualifying mover, so the browser case uses a mocked `summaries.findings` answer.
