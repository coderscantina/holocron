# v09: v2 acceptance

Evidence for [the brief](../ux-v2/v09-acceptance.md).

## Fixture

Work item 1: the seeded data and the fixed time every journey runs against.

### Pinned instant

`2026-10-13T15:20:00+02:00`, Tuesday 15:20 in Vienna (`1791897600000`, the same `TUESDAY_1520` as `tests/explorations.rs`). The seeded range runs from 2026-08-05 to now with no daylight saving change in it. Retention, the 14-day replay window and the 8-week baseline are all counted from this instant, so every seeded record stays inside retention.

### Frozen clock

`holocron serve --frozen-clock <RFC 3339>` (or `HOLOCRON_FROZEN_CLOCK`) starts the server on `Clock::frozen` at that instant. The flag only works in a build with the `dev-clock` cargo feature. Other builds exit with code 2: "this build does not support a frozen clock". `apps/server/scripts/package.sh` builds without features unless `HOLOCRON_PACKAGE_FEATURES` names some. Startup logs a warning, and health and operations status report `clock: "test"`.

A frozen clock is the right fit here. A clock that keeps ticking would move today's typical-so-far, the projection and the Now marker every minute, so exact assertions would drift. What a frozen clock means for the journeys:

- Login sessions, the login limit and session expiry use system time, so sign-in behaves normally.
- The first summary round runs 60 s after start and summarizes every site. After that, the 10-minute cadence never passes, so findings stay fixed and never go stale. Events collected during a run don't change them.
- The browser has its own clock. The replay page's "1d" range and share expiry use `Date.now()`, so journeys pin the page to the same instant (Playwright `page.clock.setFixedTime` or `page.clock.install({ time })`).
- Events collected live are received at the pinned instant. A spec that records replays live and lists them by the browser's real time needs the browser clock pinned too, or a server on the system clock.

### Commands

From the repository root:

```sh
NOW=2026-10-13T15:20:00+02:00
HOLOCRON_PACKAGE_FEATURES=dev-clock apps/server/scripts/package.sh
H=target/package/holocron/holocron
$H init --data-dir DATA --admin-email mike@example.test --admin-password-file PW
cargo run -p holocron-server --example seed_demo -- DATA mike@example.test VIEWER_PASSWORD
cargo run -p holocron-server --example seed_traffic -- DATA 45 --now $NOW
cargo run -p holocron-server --example seed_commerce -- DATA 45 --now $NOW
cargo run -p holocron-server --example seed_acceptance -- DATA mike@example.test --now $NOW
$H serve --data-dir DATA --bind 127.0.0.1:8098 --frozen-clock $NOW
# wait 60 s for the first summary round
```

`seed_traffic` and `seed_commerce` take `--now` as well. Without it they seed relative to the system time, as before. `seed_acceptance` runs last, so `seed_traffic` doesn't add its random traffic to the Lagoon sites.

### What `seed_acceptance` adds

A team workspace "Lagoon", owned by the admin, with six sites over three base domains, all in `Europe/Vienna`:

| Site | Domain | History | Weekday visitors |
| --- | --- | --- | --- |
| Lagoon | `lagoon.test` | 70 days, spike on 2026-09-22 | 900 |
| Lagoon shop | `shop.lagoon.test` | 70 days | 250 |
| Lagoon blog | `blog.lagoon.test` | 70 days | 120 |
| Lagoon Festival | `lagoon-festival.test` | 70 days | 300 |
| Festival tickets | `tickets.lagoon-festival.test` | 21 days | 150 |
| Lagoon Deutschland | `lagoon-de.test` | 70 days | 90 |

Fridays get 0.9 times the weekday visitors and weekends 0.6 times. On `lagoon.test`, 2026-09-22 (the Tuesday three weeks back) has 3.5 times the visitors. Today a newsletter adds 15 visitors per hour from 08:00 on, coming from `news.example.net` and landing on `/de/newsletter/herbst`. Half of them sign up, against one in six of the other visitors.

`lagoon.test` also has:

- The goals "Signup" (`event_name = signup`) and "Checkout" (`path = /de/checkout`), session scope, counted once.
- The board "Lagoon" with one card, "Events to checkout": a daily funnel from `/de/events` to `/de/checkout` over 7 days.
- Replay on, with 17 replays from today: every tenth newsletter visitor, starting with the fourth, gives 11. The other 6 come from visitors who neither came from the newsletter nor saw its page.

Site, goal and board IDs are new on every run. Journeys find them by domain or name.

### Expected values at the pinned instant

Each value was read from the API after the first summary round, on two separate seeded data directories. Both runs gave identical results.

`lagoon.test` overview (`summaries.forSites`, the same state the overview opens):

| Measure | Today so far | Typical weekday | Change | Projection (low to high) | Basis days |
| --- | --- | --- | --- | --- | --- |
| Pageviews | 1,420 | 1,087 | +30.63 % | 2,951.04 (2,907.87 to 2,966.22) | 8 |
| Visitor-days | 573 | 436 | | 1,179.79 (1,167.79 to 1,186.16) | 8 |

`summaries.findings` for `lagoon.test`, 3 findings, in rank order:

| Rank | Rule | Values |
| --- | --- | --- |
| 1 | `goal_pace` | Signup: 133 today, 69 typical, +92.75 % |
| 2 | `dimension_mover` | `referrer_domain = news.example.net`: 125 visitor-days, 14 typical, ratio 8.9286 |
| 3 | `dimension_mover` | `path = /de/newsletter/herbst`: 127 visitor-days, 21 typical, ratio 6.0476 |

Both movers have `site_today` 573 and `site_typical` 436. No funnel step or performance finding fires.

Goal rows (goals report, typical weekday): Signup 133 against 69. Checkout 108 against 91, +18.68 %, under the 30 % threshold, so no finding.

Replays (`recordings.list`, today from 2026-10-12 15:20 to 15:20):

| Filter | Total |
| --- | --- |
| `referrer_domain = news.example.net` (mover 2) | 11 |
| `path = /de/newsletter/herbst` (mover 3) | 11 |
| none | 17 |

The spike: `lagoon.test` had 7,603 pageviews on 2026-09-22. Every other whole Tuesday since 2026-08-11 had 2,167 to 2,355. 2026-09-22 is one of today's 8 baseline days, and the median leaves it out.

`tickets.lagoon-festival.test`: `insufficient_history` true, with 167 pageviews and 70 visitor-days today, no typical value and no projection. It has two baseline Tuesdays and needs four.

The other Lagoon sites have a projection with 8 basis days and no findings. The Roadhouse and personal sites have 6 basis days, from 45 days of `seed_traffic`. Their numbers aren't pinned, because `seed_traffic` orders sites by random ID and gives the first one most of the traffic.

### Checks

| Check | Result |
| --- | --- |
| `cargo test -p holocron-server` (no feature): `a_build_without_dev_clock_refuses_a_frozen_clock` | pass |
| `cargo test -p holocron-server --features dev-clock --test explorations`: `a_frozen_clock_resolves_today_from_its_instant` | pass |
| Fixture against a debug `dev-clock` build, two runs | values above, identical in both |

The packaged release build with `HOLOCRON_PACKAGE_FEATURES=dev-clock` wasn't built for this check. The fixture was verified on the debug binary with the same feature.

## Journeys

Work item 2: one spec per journey in `apps/web/e2e/v2-*.spec.ts`, with shared helpers in `e2e/v2-fixture.ts`. Every page pins its clock to the fixture instant and runs in `Europe/Vienna`. Each spec's `beforeAll` polls `summaries.findings` for `lagoon.test` until the first summary round has landed.

Run on 2026-10-15 against the packaged `dev-clock` release build (`HOLOCRON_PACKAGE_FEATURES=dev-clock apps/server/scripts/package.sh`), on a fresh data dir seeded as in [Commands](#commands), served with `--frozen-clock 2026-10-13T15:20:00+02:00` on port 8100.

| Journey | Spec | Result | What it asserts |
| --- | --- | --- | --- |
| 1. Overview at Tuesday 15:20 | `v2-overview.spec.ts` | pass, both runs | Headline "1,420 views so far today. On pace for 2,951 by midnight, 31% above a typical Tuesday."; the estimate is named "Estimate: 2,951. Between 2,908 and 2,966. Estimate from the last 8 Tuesdays." and shows the same text in its tooltip; mover line "+111 visitor-days"; legend "Estimate" and "Typical Tuesday"; "Now" at 15:20; chart tooltip with estimate, basis and baseline; banner Signup +93 % (133 vs 69), then cards `news.example.net` +793 % (125 vs 14) and `/de/newsletter/herbst` +505 % (127 vs 21), each with "Watch 11 replays"; Movers rows led by both values; goal rows Signup 133/69 and Checkout 108/91 |
| 1b. Site without history | `v2-overview.spec.ts` | expected failure (`test.fail`), see bugs | `tickets.lagoon-festival.test` shows "167 views today." and no estimate, but not the "Not enough history" note |
| 2. Filter to this, then Back | `v2-filter-back.spec.ts` | pass, both runs | Filter adds one history entry and refetches; findings stay unfiltered with "For all traffic"; Back restores the exact headline with 0 new `analytics.explore` requests |
| 3. Watch {n} replays | `v2-replays.spec.ts` | pass, both runs | Each mover's link lists exactly 11 rows; the unfiltered 1-day list has 17 |
| 4. Site switch | `v2-site-switch.spec.ts` | pass, both runs | Chip "+31 %" described as "31 % above a typical Tuesday so far"; menu groups by base domain, Lagoon row "+31 % vs typical" with "3 findings" (the only dot), tickets "new site", footer "Pace from 3:20 PM". Every shop read is held 3 s, the person moves on to the festival before the answers arrive, and a MutationObserver log of the headline shows only the festival's number after the first click |
| 5. Briefing acceptance workflow | `exploration.spec.ts` "acceptance workflow…", unchanged | pass | Run on its own (`-g "acceptance workflow"`) and in both full-suite runs |
| 6. Install admin vs owner | `v2-install-admin.spec.ts` | pass, both runs | Admin: server card in the sidebar, link to `/admin/status` with the storage forecast. Owner: created per run via `users.create` (`install_role: user`), signs in, creates a team and site with `teams.create` and `sites.create`. Gets no card, "Page not found" on `/admin/status`, and the browser sends no `operator.status` request |
| 7. Shared exploration | `v2-share.spec.ts` | pass, both runs | Share created in Website settings > Sharing on `lagoon.test`; the anonymous viewer gets measures and a table, but no findings region, finding text, context chips, sidebar, pace or finding counts, and sends no `summaries.*` request |

Two checks run wherever they apply. `expectNoCause` rejects causal wording ("because", "due to", "driven by" and similar) in the findings region. `expectEstimatesLabelled` requires every projected number in the headline, chart table and goal rows to carry its "Estimate" label.

### Full suite

| Run | Server | Result |
| --- | --- | --- |
| Frozen 1 | `--frozen-clock`, acceptance fixture | 54 passed, 9 failed, 1 skipped, 13 did not run (serial `access.spec.ts` stops after its first failure) |
| Frozen 2 | same server and data | 53 passed, 9 failed, 1 skipped, 14 did not run |
| Normal | same binary without `--frozen-clock`, fresh data seeded on system time (no `--now`), pre-existing specs only | 67 passed, 1 failed, 1 skipped (`shell.spec.ts` installation name, skipped by design) |

All v2 journeys pass in both frozen runs. These pre-existing specs fail only under the frozen clock and pass on the normal server. The specs were not changed:

- `exploration.spec.ts` overview defaults (142), trend chart (203), goal pace (340), and `findings.spec.ts` mover line (180). The UI reads "now" from the browser, which runs on real time two days after the server's today. The range no longer contains now, so "so far", the Now marker, "usual by now" and the mover line don't appear.
- `recordings.spec.ts` filter link (246) in run 1, recorder masking (131) in run 2. Replays recorded live arrive at the pinned instant but are listed by the browser's real time, as noted under [Frozen clock](#frozen-clock).
- `access.spec.ts` API keys (244) in run 1, 2FA (294) in run 2. Codes and the password confirmation are checked against the frozen instant, while the spec computes TOTP from real time.
- `retention-revenue.spec.ts` revenue (168). It waits for the collapsed "Scope:" chip, but the scope chips don't collapse with the range labels of a frozen today.
- `shell.spec.ts` session policies (440). The second "Saved." after a recalculation stays empty. Cause not isolated.

Under both clocks, `shell.spec.ts` "overview, context switching and menus" (96) fails because of the fixture, not the clock. `seed_acceptance` adds "Lagoon shop", so typing "shop" in the palette highlights it before "Roadhouse shop". The spec passes on `seed_demo`, `seed_traffic` and `seed_commerce` alone (checked on a third data dir).

### Bugs found

- Fixed: the headline dropped the "Not enough history for a typical day yet." note because `compare()` left `Comparison.reason` empty when the baseline was `insufficient_history`. It now sets the reason, like `zero_baseline`, and `v2-overview.spec.ts` passes without `test.fail()`.
- The projection tooltip's `role="tooltip"` element is `aria-hidden`. The button's accessible name carries the same text, so screen readers lose nothing.

### Commands

The pinned journeys (`HOLOCRON_E2E_PINNED`, `e2e/v2-*`) were retired on 2026-10-21 because the regular suite covers the v2 screens. The commands below are kept as run.

```sh
cd apps/web
export HOLOCRON_E2E_URL=http://127.0.0.1:8100 HOLOCRON_E2E_PASSWORD=… HOLOCRON_E2E_VIEWER_PASSWORD=… HOLOCRON_E2E_DB=DATA/control.sqlite
HOLOCRON_E2E_PINNED=1 bunx playwright test                              # journeys 1–4, 6, 7 and the visual and a11y specs, pinned fixture only
bunx playwright test e2e/exploration.spec.ts -g "acceptance workflow"  # journey 5, normal-clock fixture
bunx playwright test                                                    # every other spec, normal-clock fixture
bun run typecheck && bun run lint && bun run test                       # clean, 119 unit tests
```

## Visual and accessibility

Work item 3, on 2026-10-15. Packaged `dev-clock` release build on port 8101 with the fixture above, browser clock pinned to the same instant (`context.clock.setFixedTime`). Runs started after `summaries.findings` for `lagoon.test` returned 3 findings, about 55 s after start.

```sh
cd apps/web
HOLOCRON_E2E_URL=http://127.0.0.1:8101 HOLOCRON_E2E_PASSWORD=… HOLOCRON_E2E_SHOTS=$PWD/../../docs/evidence/v09 \
  bunx playwright test e2e/v2-visual.spec.ts e2e/v2-a11y.spec.ts
```

Result: 9 of 9 pass (re-run after the polish pass: 17 of 17 v2 specs). `bun run typecheck`, `bun run lint` and `bun run test` (119 tests) pass.

### Screenshots

Under [v09/](v09/), full length (from 1024 px the viewport grows by the canvas's scroll height for the capture). Pages: `overview` (`lagoon.test`), `explore`, `goals`, `replays`, `workspace-sites` (Lagoon), `admin-status`.

| Case | Files |
| --- | --- |
| 1440, 1024, 768, 390 px, light and dark | `{page}-{width}-{light,dark}.png` |
| German, 1440 and 390 px, dark | `{page}-de-{width}-dark.png` |
| Collapsed rail, 1440 px | `overview-rail-1440-{light,dark}.png` |
| 200 % zoom (720 × 450 at device scale 2, the 1440 layout zoomed) | `{page}-zoom200-light.png` |
| 320 px reflow (WCAG 1.4.10) | `{page}-reflow320-light.png` |
| Reference `examples/v2/index.html` | `reference-1440-dark.png` |
| axe results, one entry per node | `axe.json` |

Every case asserts no horizontal overflow: the document doesn't scroll sideways and no visible element in `#main-content` reaches past the viewport, except content of an inner horizontal scroller that itself fits (wide data tables).

### Overview against the reference

Compared by eye at 1440 px, `overview-1440-dark.png` against `reference-1440-dark.png`. The reference shows dark only; `overview-1440-light.png` follows the light rungs of the token table (white canvas, gray-100 fills, raised active segment, hairline panels) and shows no conflicting surface tones.

Fixed in this change:

| Component | Difference | Rule |
| --- | --- | --- |
| Finding banner | At 1024 and 768 px the banner grew past the canvas (897 px wide in a 742 px canvas) and the actions were clipped, because the grid item kept `min-width: auto` | Findings: one line, truncated, full text in the name and tooltip; Visual acceptance: no clipped controls |
| Server card | `bg-secondary` (gray-800 in dark) instead of the fill rung, so muted text was 4.0:1 | Layers: fill is gray-900; reference `.server` uses `--fill`, meter track fill strong |
| Breakdown panel header | At 320 px the shrink-0 title pushed the split button out of the panel | WCAG 1.4.10 |

Fixed afterwards (polish pass, screenshots replaced):

| Component | Fix |
| --- | --- |
| Page title row | Freshness note on the right, "Exact counts until 3:20 PM" or "Data until {watermark}" while analytics is stale, from `chartClock`, on the overview and every exploration page. The facts row (range, timezone, "All figures corrected", metric version) stays, it states different facts and isn't a duplicate |
| Measures strip | Exact values ("1,420"), five cells in one row from 640 px, two columns on phones only |
| Server card | "This server" line above the state |
| Small breakdown tables | Auto-fit grid (2 + 1 at 1024 px, 3 from about 880 px). Panel headers drop the row count below 448 px of panel width, so titles never truncate |

Not a defect: the second cell reads "Visitors" on a one-day range and "Visitor-days" on longer ones. Decision 0003 shows "visitor-days" for any range not inside one identity day, so the label follows `visitor_label` and wins over the reference's fixed name.

Left as follow-ups:

| Component | Difference | Rule |
| --- | --- | --- |
| Measures strip | The change reads "31% above" in primary text, not in the success or destructive token | Measures strip |
| Server card | The disk meter is almost invisible at the fixture's low usage | Server card |
| Systems panel | Titled "Operating systems", reference "Systems" | Overview, Kit required content |
| Times | The headline line and chart axis use the browser's 12-hour clock ("since 12:00 AM", "03:00 PM") in English; the reference uses 24-hour. It follows the locale, not a defect, but "since 12:00 AM" for a mover that started at 08:00 reads oddly | Headline |

Matches the contract: one inset canvas on the frame, grouped navigation with the Goals badge only (the fixture has a goal-pace finding and no funnel or performance finding), context and scope chips in one top-bar row, headline with the accent estimate and the mover line, Hourly / Daily control with Estimate and Typical Tuesday in the legend, Now marker, consequence sentences, banner with the gradient border and two cards, Top / Movers on every breakdown, referrer pips, goals with dashed ticks and states in words. German fits at 1440 and 390 px; at 390 px column headers and goal sub-lines truncate.

### Accessibility

axe-core 4.14.0 through `@axe-core/playwright` 4.14.0, tags `wcag2a`, `wcag2aa`, `wcag21a`, `wcag21aa`, on the six pages in light and dark (12 runs). Counts are nodes.

| Rule | Before | After |
| --- | --- | --- |
| `color-contrast` (1.4.3) | 15 | 4 |
| `label-content-name-mismatch` (2.5.3) | 52 | 10 |
| Total | 67 | 14 |

Contrast fixes:

- Light `--success` from green-600 to green-700: success text on the white canvas went from 3.21:1 to 4.94:1 (workspace sites pace, goal states, movers). Dark mode is unchanged.
- Light `--warning-background` from yellow-600 to yellow-700: the "Test clock" badge on the admin page went from 2.84:1 to 4.76:1.
- Server card on the fill rung (above): 4.0:1 to 5.55:1, in dark on every page.

Left, 4 nodes: the site chip's pace in light mode, success on the gray-100 fill, 4.04:1 (overview, explore, goals, replays). Passing needs green-800 (5.8:1); red-600 on the same fill is 3.9:1, so a decrease fails the same way. That's a palette decision, so it isn't changed here.

`label-content-name-mismatch` is down from 26 to 5 nodes per mode. The 5 left are false positives: the range chip ("Date range: Today, until now, 3:20 PM"), and the replays "Range" and "Device" filters. Vue drops the whitespace between the label and value spans, so axe reads "RangeLast 7 days"; the visible words are in the name.

Fixed, 21 nodes per mode: every name now starts with its visible text, then adds context.

| Element | Name |
| --- | --- |
| Workspace chip | "Lagoon Team. Switch workspace" (the avatar letter is drawn from a data attribute, so it isn't text) |
| Site chip | "Lagoon +31 %. Switch website", the pace in words stays the description |
| Account menu | "MW. Account menu" |
| Workspace site rows | No `aria-label`: the link's own text (name, domain, views, pace, findings, time) is its name, the sparkline text stays the description |

German follows the same shape ("Lagoon Team. Arbeitsbereich wechseln", "MW. Kontomenü"). The journeys and `e2e/v2-a11y.spec.ts` match the new names, and the spec's exception list holds only the 5 false positives per mode and the open contrast entry.

### Keyboard

From the top of the overview, 80 Tab presses (dark). The spec asserts this order: skip link, jump field, Overview in the navigation, workspace chip, site chip, the Hourly control, then the first "Filter to this". It also asserts that every stop draws an outline or ring on itself or a close ancestor (the server card rings its section on `focus-within`).

Before the fix, 23 stops showed no focus: every dropdown trigger (workspace, site, range, comparison, account, saved views, export, the breakdown row menus). The shared `DropdownMenuTrigger` added `outline-none`. In Tailwind 4 that sets `--tw-outline-style: none`, which `focus-visible:outline-2` then uses, so the ring never drew. The trigger now carries the project's `focus-visible` ring instead.

### Estimate labels

`e2e/v2-a11y.spec.ts` asserts on the overview that the headline's projected number (2,951) is a control named "Estimate: 2,951. Between 2,908 and 2,966. …". It also checks that every text on the page showing a projected pageview or visitor-day total sits in an element whose name or text says "estimate". The same check covers each goal row's projection ("On pace for 209", "On pace for 265"). It passes. The chart's projected buckets are labelled "Estimate" in the legend, the tooltip and the table alternative.

## Workload

Work item 4 of the [brief](../ux-v2/v09-acceptance.md): the declared 30-day suite with two report users while the summary step runs, over the 124M-record fixture, and the 500 events/s burst.

Result: **the latency target fails at full scale, with and without the summary step.** 11 to 16 cases, all but one on site A, miss 2 s p95 with no summary step at all. The step itself holds every other target at its real cadence: it adds no measurable queue wait, takes 3 to 4% of report-lane engine time, and summarizes all 12 sites in 17 to 30 s. Run back to back, without the 10-minute cadence, it adds queue waits up to 3.7 s and fails another 3 to 13 cases. The burst passes: acknowledgement p95 16 ms, visibility p95 173 ms. At 10% every target holds.

| Target | 10% | Full scale |
| --- | --- | --- |
| 30-day p95 ≤ 2 s, two users, `previous_period` | Pass, with and without the step | **Fail**: 16 of 38 without the step, 7 at the step's cadence, 19 back to back |
| 30-day p95 ≤ 2 s, two users, `typical_weekday` | Pass, with and without the step | **Fail**: 12 of 31 without the step, 12 at the cadence, 25 back to back |
| Every site summarized within 10 minutes while two users run the suite | Pass: 7 to 32 s per pass | Pass: 17 to 48 s per pass, 0 timeouts, 0 failures |
| Burst acknowledgement p95 ≤ 250 ms, two users and the step running | Pass: 22 ms | Pass: 16 ms |
| Burst visibility p95 ≤ 2 s, two users and the step running | Pass: 120 ms | Pass: 173 ms |

The failing gate follows the [failing-gate rule](../development-workflow.md#decisions-before-dependent-work): [Failing cases](#failing-cases) names the cases, the measured cause and alternatives that keep results exact. Nothing was relaxed.

### Harness

`holocron-gate` opens only the analytical store, so it cannot run `summaries::round`, which needs an `AppState` with control storage and sites ([v02](v02-site-summaries-and-findings.md)). The bridge is two examples, outside the release binary:

- `apps/server/examples/summary_load.rs` opens a gate data directory as the server does (`Db::open` adds `control.sqlite` beside `analytics.duckdb`) and registers the fixture's 12 sites under their analytics IDs, with the session policy `holocron-gate sessions` recorded, so mirroring it marks nothing dirty. Each site gets one goal (signup with plan pro) and one daily funnel pinned to a board (the `Q06-30d-A-vienna` steps), so all four rule families read. Two report users then run the declared `analytics.explore` cases through `explore_bench::measure` with the contract's 500 ms ± 50 ms think time, 20 samples per case, while the summary step runs in passes: each pass starts a fresh `Schedule`, so every site is due, and repeats `summaries::round` until every site is written. `--every-s 0` runs passes back to back, the upper bound; `--every-s 600` starts one every 10 minutes, the step's cadence (0015 Where it runs).
- `apps/server/examples/collect_burst.rs --gate GATE` runs the same users and back-to-back passes beside the burst, on a new site in the gate directory, with the clock frozen. The visibility probe now reads only rows with a `seq` above the store's maximum at start, so neither the fixture nor an earlier burst is scanned or matched.
- Shared code is `apps/server/examples/gate/mod.rs`. `holocron-workload` is a dev-dependency of `holocron-server`. `explore_bench::{cases, measure}` and `bench::{stats, write_lines}` became public, and each sample now carries `queue_ms` and `execution_ms` next to `ms`.

Report latency is queue wait plus execution, as before. The step's **engine share** is its time on the report lane over all report-lane time: the sum of every round's duration (its reads, including yielded and timed-out ones, plus its small SQLite reads, so slightly overstated) over that sum plus the users' read execution. A site's run time is the gap between consecutive summary writes, or from the round's start. The step's clock is `--now`, 2026-09-30T13:20Z (15:20 in Vienna, a Wednesday) at full scale and 2026-09-03T13:20Z at 10%, where only the first three days of each month hold events. Users resolve ranges at the gate's measurement clock, 2026-10-01T00:00Z.

### Commands

Release builds, `CFLAGS=-g0 CXXFLAGS=-g0`, from the repository root at df68555 plus this change. `GATE` and `OUT` lay under the worktree's ignored `.bench/` and were deleted afterwards.

```
cargo build --release -p holocron-workload
cargo build --release -p holocron-server --example summary_load --example collect_burst
B=target/release/holocron-gate
S=target/release/examples/summary_load
HOLOCRON_DISK_RESERVE_MIB=256 $B load --data GATE --groups-per-month 1000000   # 100000 for 10%
HOLOCRON_DISK_RESERVE_MIB=256 $B sessions --data GATE
# per comparison: alone, back to back, at the cadence
HOLOCRON_DISK_RESERVE_MIB=256 $S --data GATE --out OUT --observations 20 --contains=-30d- --compare previous_period --no-summaries
HOLOCRON_DISK_RESERVE_MIB=256 $S --data GATE --out OUT --observations 20 --contains=-30d- --compare previous_period
HOLOCRON_DISK_RESERVE_MIB=256 $S --data GATE --out OUT --observations 20 --contains=-30d- --compare previous_period --every-s 600
#   the same with --compare typical_weekday; at 10% add --now 2026-09-03T13:20:00Z
# reference: the gate's own explore on the same data
HOLOCRON_DISK_RESERVE_MIB=256 HOLOCRON_QUERY_TIMEOUT_S=900 $B explore --data GATE --out OUT --observations 20 --contains=-30d- --compare previous_period
# burst beside two users and the step, last because it adds a site
HOLOCRON_DISK_RESERVE_MIB=256 target/release/examples/collect_burst 500 60 --gate GATE   # 10%: --now 2026-09-03T13:20:00Z
```

### Machine

Apple M1 Pro, 10 cores, 32 GiB, macOS (Darwin 27.0.0), APFS SSD, not the reference host. Two other agents built Rust in parallel. Load averages (1/5/15 min) at the start and end of each run:

| Run | 10% | Full scale |
| --- | --- | --- |
| `previous_period` alone | 11 / 19 / 29 to 75 / 44 / 37 | 4 / 7 / 9 to 5 / 14 / 14 |
| `previous_period` back to back | 75 / 44 / 37 to 51 / 62 / 50 | 5 / 14 / 14 to 5 / 6 / 9 |
| `typical_weekday` alone | 51 / 62 / 50 to 11 / 36 / 42 | 5 / 6 / 9 to 5 / 5 / 6 |
| `typical_weekday` back to back | 11 / 36 / 42 to 6 / 19 / 33 | 5 / 5 / 6 to 5 / 12 / 11 |
| `previous_period` at the cadence | | 6 / 7 / 9 to 3 / 4 / 6 |
| `typical_weekday` at the cadence | | 3 / 4 / 6 to 3 / 3 / 4 |
| burst | 4 / 17 / 31 to 6 / 15 / 29 | 3 / 3 / 4 to 4 / 3 / 4 |

The 10% runs overlapped this work's own full-scale load (admission until 18:13). Run-to-run noise is large: at full scale `previous_period` at the cadence came out faster than alone on most cases, though the step only adds work. A difference under about 40% between two runs here is not a signal.

### 10%

12.4M records, 1.2M sessions. p95 in ms, 20 samples per case, two users, the step back to back. Every case passes 2 s. `typical_weekday` refuses Q10, Q11 and the plain Q12 cases, as in [v01](v01-baseline-and-projection.md#workload).

| Case | `previous_period` alone | with step | `typical_weekday` alone | with step |
| --- | --- | --- | --- | --- |
| Q01-30d-A-vienna | 315 | 911 | 219 | 230 |
| Q01-30d-D-utc | 248 | 1,021 | 144 | 90 |
| Q02-30d-A-vienna | 156 | 697 | 152 | 157 |
| Q02-30d-A-vienna-page-99 | 161 | 815 | 167 | 109 |
| Q02-30d-A-vienna-tail | 170 | 659 | 105 | 260 |
| Q02-30d-D-utc | 72 | 458 | 79 | 236 |
| Q03-30d-A-vienna | 373 | 559 | 365 | 201 |
| Q03-30d-A-vienna-drill-at | 266 | 769 | 187 | 338 |
| Q03-30d-D-utc | 102 | 255 | 125 | 116 |
| Q04-30d-A-vienna | 732 | 970 | 520 | 274 |
| Q04-30d-A-vienna-broad | 837 | 900 | 772 | 315 |
| Q04-30d-A-vienna-selective | 336 | 425 | 144 | 93 |
| Q04-30d-D-utc | 191 | 722 | 197 | 243 |
| Q05-30d-A-vienna | 252 | 825 | 148 | 123 |
| Q05-30d-D-utc | 100 | 746 | 61 | 135 |
| Q06-30d-A-vienna | 187 | 1,075 | 186 | 133 |
| Q06-30d-A-vienna-worst | 338 | 1,299 | 266 | 144 |
| Q06-30d-D-utc | 107 | 775 | 93 | 90 |
| Q07-30d-A-vienna | 607 | 1,493 | 528 | 283 |
| Q07-30d-A-vienna-events-7 | 541 | 1,038 | 342 | 255 |
| Q07-30d-D-utc | 135 | 387 | 77 | 127 |
| Q08-30d-A-vienna | 986 | 1,433 | 653 | 269 |
| Q08-30d-A-vienna-referrer | 955 | 1,366 | 681 | 321 |
| Q08-30d-D-utc | 381 | 508 | 150 | 137 |
| Q09-30d-A-vienna | 494 | 949 | 273 | 227 |
| Q09-30d-D-utc | 226 | 702 | 130 | 208 |
| Q10-30d-A-vienna | 103 | 578 | refused | refused |
| Q10-30d-A-vienna-breakdown | 219 | 660 | refused | refused |
| Q10-30d-D-utc | 71 | 504 | refused | refused |
| Q11-30d-A-vienna | 417 | 693 | refused | refused |
| Q11-30d-D-utc | 83 | 121 | refused | refused |
| Q12-30d-A-vienna | 263 | 550 | refused | refused |
| Q12-30d-A-vienna-broad | 624 | 756 | 466 | 251 |
| Q12-30d-A-vienna-pivot | 982 | 1,128 | 1,381 | 486 |
| Q12-30d-A-vienna-selective | 73 | 305 | 59 | 46 |
| Q12-30d-A-vienna-session | 1,175 | 1,412 | 802 | 442 |
| Q12-30d-A-vienna-tail | 324 | 586 | 341 | 218 |
| Q12-30d-D-utc | 148 | 733 | refused | refused |

| Run | Suite | Passes over 12 sites | Site run p50 / p95 | Rounds, skips, timeouts | Engine share |
| --- | --- | --- | --- | --- | --- |
| `previous_period` back to back | 363 s | 14, 18 to 32 s | 1,258 / 5,150 ms | 284, 1,787, 0 | 54% |
| `typical_weekday` back to back | 239 s | 30, 7.1 to 8.5 s | 446 / 1,126 ms | 102, 433, 0 | 73% |

Back to back the step fills the lane whenever both users think, so its share says how much idle time there was, not what the step needs. At its cadence, one pass per 10 minutes, the same numbers give 3 to 5%.

### Full scale

124M records, 12M sessions, 15 GB. p95 in ms, 20 samples per case, two users; **bold** misses 2 s. The first column is `holocron-gate explore` (two users without think time) as a reference for the harness.

| Case | gate `explore`, `previous_period` | `previous_period` alone | back to back | at the cadence | `typical_weekday` alone | back to back | at the cadence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Q01-30d-A-vienna | 1,047 | 1,104 | **2,739** | 625 | 1,221 | **2,305** | 1,523 |
| Q01-30d-D-utc | 599 | 694 | 1,618 | 454 | 730 | **2,183** | 744 |
| Q02-30d-A-vienna | 672 | 900 | **2,284** | 566 | 1,010 | 1,758 | 1,140 |
| Q02-30d-A-vienna-page-99 | 713 | 1,104 | 1,925 | 607 | 1,216 | **4,022** | 1,051 |
| Q02-30d-A-vienna-tail | 731 | 854 | 1,818 | 794 | 1,250 | 1,925 | 1,472 |
| Q02-30d-D-utc | 479 | 678 | **3,019** | 435 | 536 | **2,985** | 681 |
| Q03-30d-A-vienna | 1,346 | **2,495** | 1,904 | 1,297 | **2,409** | **3,718** | **2,025** |
| Q03-30d-A-vienna-drill-at | 1,158 | 1,683 | **2,313** | 1,055 | 1,733 | **3,687** | 1,994 |
| Q03-30d-D-utc | 804 | 1,292 | 1,759 | 899 | 1,398 | **3,178** | 1,150 |
| Q04-30d-A-vienna | **2,451** | **3,075** | **2,555** | 1,591 | **2,833** | **4,981** | **2,935** |
| Q04-30d-A-vienna-broad | **3,061** | **3,985** | **2,665** | **2,353** | **5,709** | **10,813** | **4,224** |
| Q04-30d-A-vienna-selective | **2,215** | **2,752** | **2,163** | 1,593 | **2,415** | **3,620** | **2,002** |
| Q04-30d-D-utc | 1,231 | 1,618 | 1,721 | 744 | 1,596 | **2,502** | 1,475 |
| Q05-30d-A-vienna | 808 | 1,568 | 1,967 | 630 | 825 | **3,181** | 771 |
| Q05-30d-D-utc | 540 | 877 | **2,039** | 406 | 476 | 1,468 | 505 |
| Q06-30d-A-vienna | 1,221 | 1,947 | **2,837** | 996 | 1,265 | **2,619** | 953 |
| Q06-30d-A-vienna-worst | 1,399 | **2,220** | 1,870 | 1,326 | 1,504 | **2,460** | 1,263 |
| Q06-30d-D-utc | 669 | 1,390 | **2,743** | 576 | 691 | 1,743 | 770 |
| Q07-30d-A-vienna | **3,080** | **4,011** | **3,684** | **2,757** | **3,342** | **4,845** | **2,499** |
| Q07-30d-A-vienna-events-7 | **2,642** | **4,564** | **3,639** | **2,130** | **2,247** | **3,955** | **2,283** |
| Q07-30d-D-utc | 770 | 866 | 1,065 | 629 | 788 | 1,743 | 515 |
| Q08-30d-A-vienna | **3,834** | **5,117** | **3,645** | **3,170** | **3,363** | **5,189** | **3,156** |
| Q08-30d-A-vienna-referrer | **4,765** | **6,710** | **4,300** | **3,593** | **3,466** | **4,954** | **3,172** |
| Q08-30d-D-utc | 984 | 1,552 | 1,682 | 810 | 802 | **2,685** | 936 |
| Q09-30d-A-vienna | **2,409** | **3,242** | **2,914** | 1,875 | 1,747 | **4,113** | 1,827 |
| Q09-30d-D-utc | 1,054 | **2,002** | 1,713 | 800 | 1,000 | **3,607** | 1,026 |
| Q10-30d-A-vienna | 623 | 1,034 | 1,568 | 544 | refused | refused | refused |
| Q10-30d-A-vienna-breakdown | 1,085 | 1,453 | 1,742 | 801 | refused | refused | refused |
| Q10-30d-D-utc | 624 | 897 | 1,766 | 544 | refused | refused | refused |
| Q11-30d-A-vienna | 1,573 | **2,166** | **2,849** | 1,320 | refused | refused | refused |
| Q11-30d-D-utc | 431 | 570 | 1,040 | 416 | refused | refused | refused |
| Q12-30d-A-vienna | 951 | 1,093 | 1,215 | 693 | refused | refused | refused |
| Q12-30d-A-vienna-broad | **2,177** | **3,332** | **2,513** | 1,883 | **3,187** | **4,890** | **2,773** |
| Q12-30d-A-vienna-pivot | **4,034** | **5,305** | **3,687** | **3,241** | **7,406** | **11,820** | **6,422** |
| Q12-30d-A-vienna-selective | 456 | 487 | 762 | 360 | 519 | 918 | 399 |
| Q12-30d-A-vienna-session | **2,772** | **3,319** | **3,974** | **2,013** | **3,964** | **6,376** | **3,680** |
| Q12-30d-A-vienna-tail | 1,646 | **2,162** | 1,757 | 1,327 | **3,230** | **5,025** | **3,447** |
| Q12-30d-D-utc | 864 | 1,143 | 1,369 | 541 | refused | refused | refused |

| Run | Suite | Failing | Passes over 12 sites | Site run p50 / p95 | Rounds, skips, timeouts | Engine share |
| --- | --- | --- | --- | --- | --- | --- |
| `previous_period` alone | 728 s | 16 | | | | |
| `previous_period` back to back | 746 s | 19 | 26, 15 to 51 s | 906 / 6,318 ms | 1,854, 11,993, 0 | 39% |
| `previous_period` at the cadence | 604 s | 7 | 1, 30 s | 885 / 9,659 ms | 62, 417, 0 | 3.9% |
| `typical_weekday` alone | 745 s | 12 | | | | |
| `typical_weekday` back to back | 928 s | 25 | 30, 18 to 48 s | 839 / 5,811 ms | 3,497, 23,062, 0 | 32% |
| `typical_weekday` at the cadence | 714 s | 12 | 2, 17 and 26 s | 778 / 5,524 ms | 130, 741, 0 | 2.8% |
| gate `explore`, `previous_period` | 475 s | 11 | | | | |

The slow site runs are site A: a site's pace exploration and dimension movers read its whole day plus 8 baseline days.

`typical_weekday` against `previous_period` at full scale, median execution per case, alone: 1.0 to 2.1 times, median 1.1. Q05 to Q09 are reports that do not read baselines and stay within noise (0.8 to 1.1). The cases that read baselines cost 1.1 to 1.5 times, and the filtered Q04-broad and Q12 pivot, session and tail about 2 times (Q12 pivot 3,485 to 6,786 ms). At 10% these ratios were hidden: the partial load leaves most of the 79 baseline days empty.

### Burst

500 events/s for 60 s over HTTP into the in-process server on the gate directory, two report users and the step back to back, the clock frozen at `--now`. All 30,000 events answered 200 and became visible.

| Load | Ack p50 / p95 / p99 / max (ms) | Ack to visible p50 / p95 / p99 / max (ms) | Send to visible p95 (ms) | Probe mean | Inbox peak | Peak RSS | Step during the burst |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 10% | 11.9 / 22.1 / 32.5 / 73.2 | 72 / 120 / 129 / 302 | 134 | 1.7 ms | 100 records | 1.1 GiB | 6 passes, 8.5 to 9.2 s, share 76% |
| Full scale | 10.0 / 16.2 / 20.1 / 36.7 | 80 / 173 / 242 / 352 | 182 | 4.8 ms | 105 records | 2.4 GiB | 1 pass of 40 s, 25 sites written, share 40% |

Collection and visibility do not use the report lane, so the step does not touch them. The report users got only 1 or 2 samples per case in the 85 s, so their latencies during the burst are no p95. One run each, not the three the contract asks for, and 30,000 events, not 100,000.

### Failing cases

**Without the summary step**, at full scale, these cases miss 2 s in at least two of the three `previous_period` runs that do not run the step back to back (alone, at the cadence, gate `explore`), and in both `typical_weekday` runs (alone, at the cadence):

- `previous_period`: Q04-30d-A-vienna, -broad, -selective; Q07-30d-A-vienna, -events-7; Q08-30d-A-vienna, -referrer; Q09-30d-A-vienna; Q12-30d-A-vienna-broad, -pivot, -session.
- `typical_weekday`: Q03-30d-A-vienna; the three Q04 site A cases; both Q07 and Q08 site A cases; Q12-30d-A-vienna-broad, -pivot, -session, -tail.

Measured cause: execution, not queueing. In the runs alone, queue wait p95 is 0 to 9 ms for every failing case; execution p50 is 1.5 to 3.9 s with `previous_period` and up to 6.8 s with `typical_weekday` (Q12 pivot). The plans (`holocron-gate explore --plans`) show each of them materializing site A's events of the range from `events` with a sequential scan, an estimated 9.2 to 9.5 million rows per 30-day window, joined to `session_members` (role 1), `sessions` and the property partitions; the previous period scans a second window, and `typical_weekday` scans the 79-day baseline window in three 31-day chunks. Site A holds 57M events a year, and the load leaves `events` in arrival order, so a 30-day site A read cannot skip other sites' row groups. Every one of them is site A. The only site D miss, Q09-30d-D-utc at 2,002 ms in one run, passes in the other two.

This is worse than milestone 06 on the same laptop ([m06 Full scale](m06-member-order-and-property-partitions.md#full-scale)), where only the two 30-day Q04 site A cases missed (2.2 s) and Q12-30d-A-vienna-pivot took 1,628 ms; the gate's own `explore` now takes 4,034 ms for it. I did not find the cause. Candidates are the changes to the analytical side since 2026-10-05 (98f30ec rebuilds only the sessions a correction can change, which changes how `sessions` lays out `session_members`; a1a3d03 clusters closed months by site, which the fresh gate load never runs), or the machine. A bisect over those commits on one full-scale directory would settle it. Settled in [Gate diagnosis](#gate-diagnosis) below: the m06 numbers are for the declared comparison, not a regression, and clustering changes little.

**With the summary step back to back**, queue waits appear: queue p95 0.4 to 3.7 s on the failing cases (Q02-30d-A-vienna-page-99 waits 3,325 ms with `typical_weekday`, which takes 1.2 s to execute), and cases that pass alone fail, among them Q01, Q02, Q05 and Q06 on both sites. Cause: the step checks `reports_idle` only before each read; a read that has started keeps its worker until it ends, and a site A run of several reads takes up to 13 s. When both users send while it runs, one of them waits. At the 10-minute cadence the step runs 3 to 4% of the time and the queue p95 stays at 0 to 314 ms, so the cadence, not yielding, is what keeps it out of the way.

Alternatives that keep results exact:

1. Site A reads: start from the site's own rows. Run compaction's clustering by site on the gate directory before measuring (production does it in the background, 0014), or a site-ordered layout for `events` and `session_members`, so zone maps skip other sites. Measure before and after; m06 lists the planner designs deferred to milestone 13 for the same misses.
2. Bisect the regression against milestone 06 before tuning anything new.
3. Summary step: cancel its in-flight read when a report starts waiting. The read lane already cancels a read whose future is dropped (`CancelOnDrop`), so the step can race its read against a "report waiting" signal and retry the site next round. This bounds a report's wait by the cancellation latency instead of the summary read.
4. Summary step: give it its own worker outside the two report workers, with its own thread and memory budget. Reports then never wait for it, at the cost of CPU shared with them.
5. The 0015 fallback (performance and funnel step hourly) shortens a site run but does not stop a running read from holding a worker; on its own it does not fix the back-to-back queueing.

### Not verified

- The reference host. Every number is from a laptop shared with other builds.
- Three independent runs per configuration, 200 samples per case and 100,000 burst events. Each configuration ran once with 20 samples per case.
- The 12-month cases (10 s target) and the contract's live-snapshot refresh, saves and Q13 requests. The suite here is the declared 30-day `analytics.explore` cases, as in v01.
- The step at its cadence with ingestion moving the watermark: the cadence runs force a full pass every 10 minutes instead of using the schedule's own due rule.

### Gate diagnosis

2026-10-15, after the runs above, on the same laptop with nothing else running (load averages 4 to 6 throughout, against 11 to 75 during the 10% runs). Source: cb933fa plus this change, in a worktree. One fresh full-scale directory: `load` admitted 124M records in 1,812 s and closed at 2,780 s, `sessions` derived 12M sessions in 315 s.

Hypotheses tested, in order:

1. Production clusters closed months by site in the background (a1a3d03, [0014 Compaction](../decisions/0014-operations-and-recovery.md#compaction)); the gate's `load` leaves `events` in arrival order and nothing clusters it before measuring, so a site A read scans every site's row groups.
2. 98f30ec changed how `sessions` lays out `session_members`.
3. The planner regressed since milestone 06.

**Result: no regression, and clustering does not close the gap.** The milestone 06 numbers are for the cases as declared, and every Q04 and Q12 case declares `compare: none`. The runs above forced `--compare previous_period` (a second window) and `typical_weekday` (the range plus the 79-day baseline in three 31-day chunks, four materialized windows) on every state without a report, which milestone 06 never measured, and compared them to its one-window numbers. Measured as declared, on the fresh unclustered directory at this commit, Q04-30d-A-vienna is p95 1,181 ms against m06's 2,220 and the pivot 1,926 and 2,047 ms in two runs against m06's 1,628, under the 40% this laptop's run-to-run noise allows. So there was nothing to bisect, and the bisect was not run.

Clustering ran on the directory through the production path (`Storage::compact`, 250,000 events per step, every month that closed a day before the wall clock, without the server's pauses): 120M events moved in 2,584 s, the file grew from 12 to 15 GB and stayed there. What it changed, site A's September window (2026-08-31T22:00Z to 2026-09-30T22:00Z), 2 threads and 1 GiB as in every measured run:

| Measure | Before | After |
| --- | --- | --- |
| Row groups of `events` (`pragma_storage_info`) | 1,236 | 979 |
| Row groups whose `event_time` zone map covers the window | 107 | 94 |
| Of those, whose `site` zone map admits site A / holds site A alone | 107 / 0 | 45 / 39 |
| `events` scan of the window, site A, kinds 1 and 2 (`EXPLAIN ANALYZE`, single user) | 4,750,000 rows, 0.09 s | 4,750,000 rows, 0.04 s |
| `session_members` scan of the window, site A, role 1 | 4,750,000 rows, 0.03 s | 4,750,000 rows, 0.06 s |
| Pivot's first statement, `CREATE TEMP TABLE x_ev` with its bindings as literals | 0.85 s | 0.60 s |
| of which the temp table write / the two property joins / the `events` scan | 0.68 / 0.30 + 0.29 / 0.12 s | 0.59 / 0.19 + 0.22 / 0.01 s |

The planner's estimate of 9.2M rows per window is twice the 4.75M it reads. The `session_members` read is already in time order on the fresh load: correction appends each step's members in (site, event time) order, and 98f30ec only narrowed which sessions a step rewrites, so hypothesis 2 is out. The scan was a seventh of the pivot's first statement before clustering; the rest is writing 4.75M rows with 19 columns to the temp table and joining two property partitions, and the second statement groups them. Each comparison window repeats that, so `previous_period` costs about twice the declared case and `typical_weekday` three to four times, and clustering trims each window by about a tenth.

p50 / p95 in ms, two users, 20 samples per case, `holocron-gate explore`; **bold** misses 2 s:

| Case | Compare | Before p50 / p95 | After p50 / p95 |
| --- | --- | --- | --- |
| Q12-30d-A-vienna-pivot | declared (`none`) | 1,792 / **2,047** | 1,579 / 1,821 |
| Q12-30d-A-vienna-pivot | `previous_period` | 3,651 / **4,491** | 3,909 / **4,209** |
| Q12-30d-A-vienna-pivot | `typical_weekday` | 7,948 / **10,090** | 7,485 / **7,891** |
| Q04-30d-A-vienna | declared (`none`) | 1,057 / 1,181 | 830 / 944 |
| Q04-30d-A-vienna-broad | declared (`none`) | 1,201 / 1,390 | 1,176 / 1,315 |
| Q04-30d-A-vienna-selective | declared (`none`) | 1,031 / 1,184 | 790 / 853 |
| Q04-30d-A-vienna | `previous_period` | 1,797 / **2,004** | 1,455 / 1,593 |
| Q04-30d-A-vienna-broad | `previous_period` | 2,259 / **2,406** | 2,250 / **2,364** |
| Q04-30d-A-vienna-selective | `previous_period` | 1,747 / 1,822 | 1,317 / 1,472 |
| Q04-30d-A-vienna | `typical_weekday` | 3,101 / **3,239** | 2,375 / **2,796** |
| Q04-30d-A-vienna-broad | `typical_weekday` | 4,649 / **5,027** | 4,919 / **5,208** |
| Q04-30d-A-vienna-selective | `typical_weekday` | 1,964 / **2,370** | 1,480 / 1,882 |

The declared 30-day suite, p95 in ms, before and after clustering:

| Case | `previous_period` before | after | `typical_weekday` before | after |
| --- | --- | --- | --- | --- |
| Q01-30d-A-vienna | 750 | 616 | 1,696 | 1,595 |
| Q01-30d-D-utc | 437 | 385 | 715 | 559 |
| Q02-30d-A-vienna | 634 | 648 | 1,179 | 859 |
| Q02-30d-A-vienna-page-99 | 649 | 598 | 975 | 992 |
| Q02-30d-A-vienna-tail | 716 | 558 | 1,293 | 996 |
| Q02-30d-D-utc | 509 | 346 | 757 | 442 |
| Q03-30d-A-vienna | 1,261 | 1,004 | **2,397** | 1,774 |
| Q03-30d-A-vienna-drill-at | 1,101 | 1,070 | **2,446** | 1,552 |
| Q03-30d-D-utc | 800 | 374 | 1,284 | 559 |
| Q04-30d-A-vienna | **2,227** | 1,820 | **3,370** | **3,028** |
| Q04-30d-A-vienna-broad | **2,736** | **2,670** | **5,562** | **6,812** |
| Q04-30d-A-vienna-selective | **2,136** | 1,719 | **2,364** | **2,141** |
| Q04-30d-D-utc | 1,066 | 544 | 1,520 | 639 |
| Q05-30d-A-vienna | 895 | 778 | 915 | 903 |
| Q05-30d-D-utc | 507 | 500 | 466 | 527 |
| Q06-30d-A-vienna | 1,042 | 848 | 1,152 | 1,036 |
| Q06-30d-A-vienna-worst | 1,365 | 1,057 | 1,446 | 1,283 |
| Q06-30d-D-utc | 685 | 414 | 744 | 487 |
| Q07-30d-A-vienna | **2,730** | **2,710** | **2,826** | **2,679** |
| Q07-30d-A-vienna-events-7 | **2,105** | **2,024** | **2,640** | **2,431** |
| Q07-30d-D-utc | 658 | 485 | 635 | 507 |
| Q08-30d-A-vienna | **3,789** | **3,723** | **3,622** | **3,309** |
| Q08-30d-A-vienna-referrer | **3,970** | **4,903** | **3,502** | **3,981** |
| Q08-30d-D-utc | 863 | 526 | 1,000 | 518 |
| Q09-30d-A-vienna | **2,115** | **2,393** | 1,967 | 1,684 |
| Q09-30d-D-utc | 997 | 536 | 1,307 | 477 |
| Q10-30d-A-vienna | 604 | 525 | refused | refused |
| Q10-30d-A-vienna-breakdown | 907 | 904 | refused | refused |
| Q10-30d-D-utc | 452 | 318 | refused | refused |
| Q11-30d-A-vienna | 1,312 | 1,367 | refused | refused |
| Q11-30d-D-utc | 412 | 361 | refused | refused |
| Q12-30d-A-vienna | 831 | 650 | refused | refused |
| Q12-30d-A-vienna-broad | 1,978 | 1,827 | **3,396** | **2,752** |
| Q12-30d-A-vienna-pivot | **3,544** | **3,346** | **7,440** | **7,542** |
| Q12-30d-A-vienna-selective | 455 | 336 | 427 | 374 |
| Q12-30d-A-vienna-session | **2,689** | **2,447** | **4,585** | **4,282** |
| Q12-30d-A-vienna-tail | 1,628 | 1,612 | **4,271** | **2,969** |
| Q12-30d-D-utc | 662 | 411 | refused | refused |

| Run | Suite | Failing |
| --- | --- | --- |
| `previous_period` before | 458 s | 10: the three Q04, both Q07 and Q08, Q09 site A cases, Q12 pivot and session |
| `previous_period` after | 406 s | 8: Q04-broad, both Q07 and Q08, Q09 site A, Q12 pivot and session |
| `typical_weekday` before | 585 s | 13: both Q03, the three Q04, both Q07 and Q08 site A cases, Q12 broad, pivot, session and tail |
| `typical_weekday` after | 511 s | 11: the three Q04, both Q07 and Q08 site A cases, Q12 broad, pivot, session and tail |

Clustering helps the small sites most (site D cases run about twice as fast, with half the suite's row groups gone) and recovers Q04-30d-A-vienna and -selective under `previous_period` and both Q03 cases under `typical_weekday`. The Q07, Q08 and Q09 site A reports take no comparison at all; they miss in both modes at 2.0 to 4.9 s, were added in milestone 07, and have no milestone 06 number to regress from.

Per target, after the change (the gate clusters like production before measuring; the other targets were not rerun):

| Target | Full scale |
| --- | --- |
| 30-day p95 ≤ 2 s, two users, `previous_period` | **Fail**: 8 of 38 |
| 30-day p95 ≤ 2 s, two users, `typical_weekday` | **Fail**: 11 of 31 |

What changed: `holocron-gate compact` runs `Storage::compact` step after step until every month closed at `--now` (default the wall clock) is clustered, so a gate run can measure the layout production has. The target itself needs planner work, not layout. Exact alternatives, most valuable first:

1. Aggregate without the temp table where one cut needs no session measures: the write is two thirds of the pivot's first statement, and the comparison windows repeat it. Milestone 06 lists it as open item 3.
2. `typical_weekday` reads 79 baseline days of events (three windows) to get one per-day aggregate per baseline day; a per-chunk aggregate in the reading statement avoids writing 12.5M rows per request.
3. The site A reports (Q07, Q08, Q09): the property-driven and session-first plans deferred to milestone 13 in milestone 06.

Commands, release build with `CFLAGS=-g0 CXXFLAGS=-g0`; `GATE` and the outputs under the ignored `.bench/` and deleted afterwards:

```
cargo build --release -p holocron-workload
B=target/release/holocron-gate
HOLOCRON_DISK_RESERVE_MIB=15360 $B load --data GATE --groups-per-month 1000000   # 15 GB guard
HOLOCRON_DISK_RESERVE_MIB=15360 $B sessions --data GATE
export HOLOCRON_DISK_RESERVE_MIB=256 HOLOCRON_QUERY_TIMEOUT_S=900
# before: the single cases as declared and in each comparison mode, then the suite
$B explore --data GATE --out OUT --observations 20 --only Q12-30d-A-vienna-pivot --plans
$B explore --data GATE --out OUT --observations 20 --only Q12-30d-A-vienna-pivot --plans --compare previous_period
$B explore --data GATE --out OUT --observations 20 --only Q12-30d-A-vienna-pivot --plans --compare typical_weekday
#   the same with --only Q04-30d-A-vienna, which also matches -broad and -selective
$B explore --data GATE --out OUT --observations 20 --contains=-30d- --compare previous_period
$B explore --data GATE --out OUT --observations 20 --contains=-30d- --compare typical_weekday
$B compact --data GATE
# after: the same explore runs
```

The zone-map counts come from `pragma_storage_info('events')` (`Min`/`Max` of the `site` and `event_time` segment stats per row group); the scans and the pivot statement ran as `EXPLAIN ANALYZE` on the live lane, the pivot's text taken from `--plans` with its bindings as literals. They ran through a scratch SQL subcommand of the gate that was not kept.
