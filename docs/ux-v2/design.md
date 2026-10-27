# Dashboard v2 design contract

This document defines the screens and behavior of the v2 dashboard. It amends [Kit implementation](../kit-implementation.md); where they differ, this file wins for the screens it names, and everything it does not name stays as Kit implementation says. Formulas and data rules live in [0015](../decisions/0015-baselines-projections-and-noticed-changes.md) and the earlier decision records.

The visual reference is [examples/v2](../../examples/v2/index.html), with sample values; it shows direction, not calculations. An earlier draft is kept in [examples/v2/rejected](../../examples/v2/rejected/first-draft.html) and is not a reference (see [Rejected direction](#rejected-direction)).

## What v2 changes

The current overview lists numbers and leaves the reading to the person. v2 tells them three things before any table:

1. How the range compares with a normal day, in one sentence.
2. Where the range is heading, as a labelled estimate.
3. Which few changes explain the difference, each with an action.

The rules that keep this honest:

- Exact and estimated stay visibly apart. Measured values are solid and plain; estimates are dashed, carry the word "estimate" in their accessible name, and name their basis.
- Every sentence the product writes about the data links to the exploration state that shows it.
- What did not change is quieter than what did. Rows inside their usual range use muted text, so changes stand out without sorting.
- Status is written in words ("5 behind pace", "not enough history yet"), with color as a second signal only.

## Surfaces and geometry

v2 takes its surface treatment from the second inspiration screen (Pollinate) and keeps Holocron's gray ramp and indigo accent. It builds on the shell committed in `0fd22b5`: one inset canvas on a frame.

### Layers

| Layer | Dark | Light | Used for |
| --- | --- | --- | --- |
| Frame | `--sidebar` (gray-950) | `--sidebar` (gray-100) | Sidebar and top bar background; everything outside the canvas |
| Canvas | `--surface` (gray-925) | `--surface` (gray-50) | The page panel, inset into the frame |
| Card | `--card` (gray-900) | `--card` (white) | A panel or table on the canvas |
| Popover | `--popover` (gray-850) | `--popover` (white, with shadow) | Menus, selects, tooltips |
| Fill | `--secondary` rung, gray-900 in dark | gray-100 | Chips, search field, segmented track, table header row, buttons. Inside the frame (`data-frame`) it takes the frame's tint, `--sidebar-accent` (gray-150) |
| Fill strong | gray-875 | gray-150 | Keyboard hint, kind badge, meter track |
| Nav active | `--nav-active` (gray-850) | gray-175 | The current page in the sidebar and the account nav, with the strongest text. Hover only tints the row |
| Hairline | gray-850 | gray-150 | The edge of a card or table |
| Divider | gray-875 | gray-100 | Between rows inside a panel |
| Elevated | gray-800 | white | Raised surfaces such as the switch thumb |

- Controls are filled, not outlined. The one outlined control is the "+ Filter or segment" chip, an inset hairline with no fill, because it adds rather than shows state.
- What is closer to the viewer is lighter, in both modes: frame, canvas, card, popover. Fills sit one step lighter than the card they are on, so a control reads on the canvas and on a card alike.
- Active segments take the canvas tone (`--surface`), set into their `--fill` track with 4px of padding, a 1px hairline and a small shadow, so the selected option reads as a cut-out of the page. Every segmented control uses `segmentedTrack` and `segmentedItem` from `components/ui/segmented`, through `ChoiceGroup`, `Tabs` or a toggle group; the measures strip follows the same rule.
- A boxed block is a `Panel` (`components/ui/panel`): hairline, card tone, a 56px header with title, a muted "· meta" and controls on the right. A settings page is one document of `SettingsSection`s, not a deck of cards.
- Shadows: only the active segment's small shadow and floating layers (menus, tooltips, sheets) keep `--shadow-soft`. Nothing else casts one.
- Light mode follows the same rungs. Check it at every brief's visual acceptance; the reference shows dark only.

### Geometry

New shell tokens in `apps/web/src/assets/css/app.css`, next to the existing shell tokens. They extend Kit's radius scale rather than replace it; `--radius` stays 8px for small controls.

| Token | Value | Used for |
| --- | --- | --- |
| `--radius-control` | 10px | Chips, search field, nav rows, buttons |
| `--radius-panel` | 14px | Cards, tables, the measures strip, the finding banner |
| `--radius-canvas` | 18px | The canvas panel |
| `--shell-canvas-inset` | 10px | Gap between canvas and frame edge (top, right, bottom) |
| `--shell-control-height` | 38px | Top-bar chips |
| `--shell-nav-row` | 44px | Sidebar rows and the sidebar search field (40px in compact density) |
| `--shell-page-x` | 32px | Canvas horizontal padding (16px below 768px) |
| `--table-row` | 48px | Table body rows (42px in compact density and in small tables) |
| `--table-head` | 42px | Table header row |

- Section spacing on the canvas is 24px; inside a panel header 56px tall with 18px left padding.
- The expanded sidebar insets its rows 16px from its edge; a row has 12px of padding and 12px between icon and label. Section headings are 13px, 24px apart from the previous section and 6px above their first row.
- Type stays Kit's: 15px for navigation, 14px for table text, 15px panel titles, 17px page title, 30px headline. Numbers use tabular figures.
- The compact density setting shrinks the shell and table tokens; it does not change radii or colors.
- Icons: lucide at 16px in navigation and chips, 14px in table headers, stroke 1.75.

## Shell

### Layout

The frame holds the sidebar on the left and the top bar above the canvas. The canvas is one rounded panel inset by `--shell-canvas-inset` from the frame's top-bar row, right and bottom edges. There is no header bar spanning the whole width and no border between sidebar and canvas.

### Sidebar

Top to bottom: brand with the collapse button, the jump field, four navigation groups, and the server card at the bottom for install admins.

Four groups replace the flat list. Every current route stays; nothing moves into a tab.

| Group | Items |
| --- | --- |
| Traffic | Overview, Explore, Realtime, Sessions |
| Behaviour | Events, Funnels, Goals, Journeys, Retention |
| Acquisition | Campaigns, Attribution, Revenue |
| Experience | Performance, Replays, Heatmaps |

Workspace areas (Boards) and Site settings keep their current places.

- Group headings: sentence case, 12.5px, the faint text tone, no uppercase.
- Nav rows: icon and label, `--shell-nav-row` tall, the active row filled with the fill rung and primary text; inactive rows muted text, no fill.
- Realtime shows the current site's active count from the existing `realtime.snapshot` subscription, with a success dot. Disconnected or stale shows no number.
- A badge means "a finding here needs you", never "something exists here". Goal-pace findings badge Goals, funnel-step findings badge Funnels, performance findings badge Performance. Dimension movers badge nothing; they live on the overview. The badge is a solid accent circle with the count and an accessible name ("Goals, 1 finding").
- Collapsed rail: the jump field becomes a search icon button, badges become a dot on the icon, the tooltip carries the count.
- Phone: the drawer shows the jump field and the same groups; the bottom tabs stay overview, explore, realtime.

### Jump field

The command palette trigger moves from the header into the sidebar, under the brand: a filled field with a search icon, "Jump to anything" and a ⌘K / Ctrl K hint. It is not labelled "Ask": there is no question answering. v2 adds saved views, segments, goals, boards and the current site's findings to its results ([v08](v08-command-palette.md)). The shortcut works from anywhere, as today.

### Top bar

One row on the frame above the canvas, `--shell-control-height` controls, left to right:

1. Workspace chip: initial, name, kind badge (Team or Personal), chevron. Opens the workspace menu as today.
2. Site chip: name, then the pace from the site's summary row when it is fresh ("+12 %", success or destructive text token, accessible name "12 % above a typical Tuesday so far"), chevron. Opens the site menu below. A stale or missing summary shows the name only. The pace never comes from a client-side query.
3. A thin vertical divider.
4. Scope chips for the current exploration: range, comparison, filter and segment chips, and "+ Filter or segment" (see [Scope chips](#scope-chips)). Pages without an exploration show no scope chips.
5. The user menu at the far right, as a round avatar.

- No environment control ([kit-extraction-map](../contracts/kit-extraction-map.md) Environment selector). Staging and production are separate sites.
- Context chips and scope chips look alike on purpose but stay two groups: context changes the authorization scope, scope chips change only the exploration state.
- When the row does not fit, scope chips collapse into one "Today · vs typical · 2 filters" chip that opens a popover; context chips shorten names first, as in Kit's container-responsive header.

### Site menu

The site chip's dropdown is a short status list.

- First row: "All sites in {workspace}" with a one-line summary ("4 sites, 1 needs you"), linking to the workspace sites page.
- Sites grouped by base domain (last two labels, with a public-suffix exception list for `co.uk` and the like), group heading muted. One group means no heading.
- Each row: name, the site's label or domain on the second line, the pace text in words ("+12 % vs typical", "usual range", "Ticket bought 5 behind"), and a dot when the site has at least one finding (destructive token when any finding is a decrease, accent otherwise). A site with fewer than 4 weeks of history says "new site". A site with almost no traffic today shows its count ("31 views today") in muted text.
- Footer: when the summaries were computed ("Pace from 15:15"). If the summary step is behind, it says so.
- No live counts in the menu. A live count per site would be one live query per site on every open; the current site's live count stays in the sidebar.
- Keyboard and screen-reader behavior stays as Kit implementation's context selector rules. The row's accessible name reads name, pace and finding state in that order.

### Server card

Install admins only (`collection.status` is install-admin only and install admins read no site data, `InstallRole` in `crates/protocol/src/access.rs`). A filled block at the bottom of the sidebar above Site settings, and the same content on the admin page.

- One line of state ("Healthy", or the first problem from `/health/ready` in words), a thin disk meter and one sentence from the storage forecast ([v07](v07-operator-status-and-storage-forecast.md)).
- Links to the admin status page. Everyone else never sees it, and the sidebar does not reserve space for it.

## Canvas pages

### Page title row

Every page starts the canvas with one row, 64px tall, separated from the body by a hairline: the page name, a bullet, a muted context ("roadhouse.test", "5 of 25 goals", a report name), and on the right the freshness note ("Exact counts until 15:20", or "Data until 15:02" when analytics is stale). It replaces the page heading inside the body. Page actions (export, save, pin) sit at the right end of this row, before the freshness note.

### Scope chips

The scope chips in the top bar edit the analytical scope: range ("Today, until now 15:20"), comparison ("vs a typical Tuesday"), one chip per filter or applied segment, and "+ Filter or segment". Saved views open from the page title row's actions.

- The chips edit the one exploration state in `?s=`. They are views of it, not a second state.
- Site and workspace never appear as scope chips; they are the context chips.
- The range chip shows "until now {time}" whenever the resolved range contains the application clock's now.
- Comparison choices: none, previous period, typical weekday. Typical weekday names the weekday when the range is one day, else reads "vs typical weekdays". A choice the state cannot use (for example typical weekday on a 90-day range) is listed disabled with the reason.
- Filter editing keeps today's staged field, operator and value flow and the AND/OR group editor; the chips open it in a popover anchored to the top bar.

## Overview

Order from the top: page title row, headline, measures strip, trend chart, the findings panel, pages with goals and referrers, then countries beside one technology panel (browsers, systems and devices behind a switch), and annotations in the chart.

The Kit contract's required content stays: five summary measures, the hourly chart, pages and referrers, browsers, systems and devices. The world map was dropped on 2026-10-17: the countries table says the same in less room. The visitor-day and observed-span glossary line under the page was dropped too; only the identity coverage note remains, and only when coverage is incomplete.

### Today view

The overview has two views, picked with a segmented Today / Details control beside the title and remembered in the browser. Today is the default. Details is the page this section describes from the headline on, with its ranges, filters and panels.

Today always shows today and all traffic, read with the overview's default state for pageviews only (today by the hour against a typical weekday). It reads again when the site's summary moves.

- The left column starts with today's pageviews as a large number over "Pageviews today, until 22:15" with a short accent line, the pace verdict in one sentence ("Running 18% above a usual Sunday."), and the projected end of the day with its range. The verdict uses the site chip's rules: below 50 pageviews it says it is too early to compare, and without 4 baseline days it says there is not enough history.
- Under a hairline, the first three findings, each a link to the report it belongs to, the lead one with an accent sparkle. More findings point to Details.
- Beside it, one chart of running totals over the whole day: today as the accent line with a faint fill up to now, the usual day as a neutral line, and from now to midnight a dashed accent line in a faint band of the projection's range. The dashed line follows the usual day's shape and ends at the server's estimate, so the estimate and its range are the only projected figures. The usual day's total and the projection are labelled at the right edge; on narrow screens a small legend replaces them.
- Every hour is a column. Hover or focus shows the totals by the end of that hour against usual and the hour's own count; an hour that has begun opens Explore for today filtered to that hour, split by page. The chart is one tab stop and the arrow keys move between hours.

### Headline

A sentence built from a fixed grammar, never free text. The measure is the selected measure in the measures strip (default views).

| Situation | Sentence |
| --- | --- |
| Range contains now, comparison available, projection available | "{observed} views so far today. On pace for {projection} by midnight, {change} a typical Tuesday." |
| Same, no projection (insufficient history) | "{observed} views so far today, {change} a typical Tuesday." plus "Not enough history for an estimate yet." |
| Past range with comparison | "{value} views {range label}, {change} {comparison label}." |
| No comparison | "{value} views {range label}." |

- The first sentence uses primary text; the rest muted, with the projected number in the accent.
- `{change}`: "12 % above" or "8 % below"; within ±5 % it reads "about the same as". Zero baselines follow 0003 ("new" instead of a percentage).
- Multi-day ranges that contain now say "by the end of today", not "by midnight", and the projection covers the range total.
- The projected number has a tooltip with its band ("between 4,600 and 5,200") and basis ("estimate from the last 8 Tuesdays").
- The line under the headline names the top dimension-mover finding when it explains at least half the change: "Most of the lift is instagram.com: +214 visitor-days since 09:40." When an annotation falls in the window, the line names it: "after the annotation '33rd birthday party'". With no such finding the line reads "Everything is within its usual range" or is left out when there is no comparison.
- Each value and name in the headline is a link or button: the measure opens Explore with that state, the dimension value applies the finding's filter.

### Measures strip

The five measures (views, visitor-days, sessions, bounce rate, observed span) as one segmented control: a filled track with five cells, the selected measure raised like an active segment. Each cell shows label, value and change in words and arrow. Selecting a cell changes the headline and chart measure (one history entry). On phones the cells form a 2-column grid inside the same track. Revenue appears only when the site has revenue events, as a sixth cell; its change follows the currency rules.

### Trend chart

A hairline panel. Its header holds a segmented Hourly / Daily control on the left and the legend on the right.

- Measured buckets: solid bars in the accent series token.
- Projected buckets after now: the accent as outline with a dashed stroke and a light accent fill, labelled "Estimate" in the legend.
- Baseline: a muted line, labelled "Typical Tuesday" or "Previous period".
- A dashed vertical "Now {time}" marker.
- Annotations as they are today.
- Under a hairline: up to two consequence sentences from the projection ("Passes 4,000 around 19:30", "Busiest hour expected 20:00 to 21:00") and, right-aligned, "Exact counts up to now. The estimate comes from the last 8 Tuesdays."
- The tooltip names which kind of value it shows ("Views 212" or "Estimate 236, between 220 and 250") and the baseline.
- The accessible table alternative gains "Estimate" and "Typical" columns; estimate cells say so in text.
- Granularity follows the state; projection exists for hour and day buckets only.

### Findings

Up to three findings from the site's summary row, in rank order.

- The findings are one panel, "Noticed changes · {computed time}", with up to three rows. A row is a sparkle icon (the accent for the lead finding, muted for the others), the fact in primary text over the measured detail in muted text (each truncated, the full text in the accessible name and a tooltip), and its actions on the right. "Add annotation" is an icon button with a tooltip, so a row carries at most two text actions.
- Actions are operations on the finding's exploration state: "Filter to this" (applies its filter to the overview), "Open report" (funnel, goals, performance), "Watch {n} replays" (replays list with the same filter, only when recording is on and the count is above 0), "Add annotation" (opens the existing annotation control at the finding's start time). The first action of each finding is a filled button, the rest are ghost buttons.
- Empty: the banner reads "Nothing unusual so far today." with no border gradient. Stale: computed time and a muted "updating" note. Summary step behind by more than 30 minutes: a one-line notice instead of the banner.
- "Show all {n}" opens the full list in a sheet.

### Tables and breakdowns

Every table on the canvas follows one pattern: a hairline panel with a 56px header (title, a faint count such as "· 214 pages", and controls on the right), a filled header row with small icons and column labels, and `--table-row` rows separated by the softer divider. Body columns are separated by the same divider; the filled header row has no column dividers. Numbers right-aligned in tabular figures. `components/ui/table` draws this pattern; a hand-built table matches it.

- Every breakdown panel gets a small segmented Top / Movers switch in its header. Top is today's behavior.
- Movers sorts by absolute change against the comparison and draws a diverging bar around a center line; increase uses the success token, decrease the destructive token. Rows with an absolute change under the panel's quiet threshold (5 for counts) use faint text, keep their values, and stay in the list so the reader sees what did not move.
- Movers is available only when the state has a comparison; otherwise the switch is disabled with the reason.
- Referrers show their share as a 5-pip meter (filled pips in the accent, the rest in the dim accent) before the name, with the share in its own column. The pips round the share to fifths; the column carries the exact value.
- Browsers, systems and devices share one panel; a segmented switch in place of its title picks the breakdown.
- A country value always shows as its flag followed by its localized name (`CountryLabel`, or `CountryFlag` before a label that already names it), never as the bare ISO code.
- Include, exclude and explore actions stay on every row, in a row menu that appears on hover and focus and is always reachable by keyboard.

### Goals panel

Shown when the site has saved goals (up to 8, the `goals` report limit), using the `goals` report kind with the typical-weekday baseline. Same table pattern, rows 62px tall.

- Columns: goal name with "{completions} of {typical full day} usual by midnight" below in faint text; pace as a meter of completions against the typical full day with a dashed tick at the typical value by now; state in words on the right ("5 behind pace", "On pace for 36", "Passed the typical day at 14:05").
- Ahead and behind use the success and destructive tokens; within ±10 % of the tick reads "On pace" in muted text.
- Clicking a row opens the Goals report with the same range and comparison.

### Storage forecast

Install admins only, on the admin status page ([v07](v07-operator-status-and-storage-forecast.md)), not on the site overview.

## Realtime

`/sites/:site/realtime` has two views of the same `realtime.snapshot`, picked with a segmented Globe / Details control in the title row next to the connection status. Globe is the default; the choice is remembered in the browser. Details is the earlier page: the active count, the 30-minute chart and the table of latest events.

The globe view has a left column and the globe beside it. Below `lg` the globe is a square block above the column.

- The column starts with the active count as a large number over a success dot and "Current visitors", then 30 one-minute bars (the current minute in the accent, empty minutes as a hairline) captioned "Last 30 minutes" and the pageview count.
- Under a hairline, the 20 latest events: a success dot, the flag, the path for a pageview or the event name in the accent for a custom event, and the age ("just now", "15s ago", "1m ago") counting on the server's clock. Rows fade toward the oldest. Browser and device are in the row's tooltip.
- The globe is a dotted sphere drawn by `cobe`, gray on the canvas tone in both modes. Each place with active visitors has a pin: the round flag over a small accent dot, with a count badge from 2 visitors up. Places come from `active_locations`, one per 0.1° cell, or the country's centroid when the CDN sent no coordinates ([0017](../decisions/0017-visitor-coordinates.md)).
- Pins are buttons bound to the globe through CSS anchor positioning. Behind the globe they fade out and take no pointer; focusing one turns the globe to it. Hover or focus shows the place, its active count and latest path.
- The globe rests facing the region with the most active visitors. An event that arrives while the page is open turns it there with a soft accent halo, and the pin shows "flag, city, path" (the country name at a centroid) for 5 seconds. Dragging turns the globe. Under reduced motion it moves only when dragged or when a pin takes focus.
- Two icon buttons sit in the globe's corner: sound, off by default and remembered, which plays a soft ping per arriving pageview and two notes per custom event; and full screen for the whole view.

The globe is a shared component (`components/globe/Globe.vue`) taking neutral markers: a flag, an accessible name, two hover lines, an optional badge and an optional accent disc sized by the marker's share.

## Retention

`/sites/:site/retention` has two views of the same retention result, picked with a segmented Heatmap / Details control beside the title. Heatmap is the default; the choice is remembered in the browser. Details is the earlier page: the population notes above the cohort table with counts, averages and incomplete cells named.

The heatmap view uses the same stage as the realtime globe: a left column and the grid beside it. Below `lg` the grid sits above the column and scrolls sideways.

- The grid is the subject. One row per cohort with its start date and size, one column per period after the first visit, starting at 1: period 0 is always 100% and says nothing. Cells that have not started are left out, so the grid is a triangle. The cohort and interval controls sit in the grid's own header line.
- Cells are the accent mixed into the card in five steps of equal lightness (OKLab, so the hue never drifts), scaled to the grid's highest rate. A rate of 0 is the plain fill. The darkest step mixes in less accent in dark mode, so primary text reads on every cell. Running intervals have a dashed outline and muted text, as in the table.
- Pointing at or focusing a cell dims the other rows and draws that cohort's curve. The grid is one tab stop: arrow keys move between cells (up and down keep the column and skip empty rows), Home and End jump to the row's ends, Enter or a click selects the cell.
- The column starts with the average share that comes back one period after the first visit, in large type, and how newer cohorts compare with older ones ("−3% against earlier cohorts", green or red from 5% either way). Below it, the average curve in neutral gray with the cohort in focus over it in the accent, its running part dashed.
- Then one sentence: "Cohorts since October 12 bring back 12% more people a week later than earlier ones", or that they hold steady. The cohorts with a complete first period are split into an older and a newer half and pooled; with fewer than 20 people on a side there is no sentence.
- A selected cell names its cohort and period, "2 of 9 people came back (22%)", and links to the Sessions report for that period with the same filter. The link says it lists every visitor of the period, because there is no cohort filter yet.
- A footer line says the population: consented persistent profiles only, and that dashed cells stay out of the averages.

## Flows

Journeys, Campaigns and Attribution each have two views, picked with a segmented Flow / Details control beside the title. Flow is the default; the choice is remembered in the browser per page. Details is the earlier page, unchanged: the builder above the tables. In the flow view the builder moves onto the canvas above the diagram, so the page fits one screen like the globe.

All three draw with one dependency-free SVG flow (`components/flow`): nodes as thin bars in columns, links as curved bands between them, sized by one unit throughout. Bands are a quiet gray; the part that matters is drawn in the accent along the top of its band. Node labels sit beside their bar on a surface backdrop with the node's figure muted; a node too small for a label names itself on hover or focus. Every node is a button: choosing it focuses the flow on what passes through it (everything else dims), choosing it again or pressing Esc clears the focus. Clicking a band focuses its target. On a phone the diagram keeps a readable column width and scrolls sideways. Transitions run only under `motion-safe`.

A focused node also opens a short block in the column: its name, one line of figures, and "Show sessions" and "Open events in Explore" with the same filter plus the node's value.

### Journeys

- The diagram runs from the first step on the left to the report's length. Each step keeps its six most common values; the rest of a step is "Other". Sessions that end after a step flow into a gray "Left" node in the next column. A sequence as long as the report's length may go on, so it has no exit.
- Without a focus the most common path is in the accent. Choosing a node, a path in the list or the drop-off page shows only the sessions through it.
- The column: the share of sessions (or people) on the most common path as the large figure, the path itself in the accent, then one sentence on the biggest drop-off ("60% of sessions that reach / end there."), the page a button that focuses it. Below, the five most common paths, narrowed to those through the focus. A footnote says the flow is drawn from the top 100 paths and what share of sessions they cover.

### Campaigns

- Channel on the left, the chosen split (UTM campaign, source, medium, content, term or click ID) on the right, from an extra panel of the same exploration: channel by split value with sessions and net revenue, sorted by sessions, 100 rows. Each side keeps its seven largest values plus "Other".
- A Revenue / Sessions control in the diagram's corner sizes the flow; it shows only when there is revenue, and Revenue is then the default. Revenue is drawn in the currency with the largest amount; a footnote says so when there are others.
- Sessions without a value of the split are left out of the flow and counted in a footnote, so untagged traffic does not squeeze the campaigns.
- The column: total net revenue (or sessions) as the large figure, one sentence on the leading channel and its main value ("Paid ads brings 94% of it, most of it through spring_sale."), and the channels ranked with a thin share bar.

### Attribution

- Credited touch values on the left (top seven, "Other values", and "Direct or no outside touch"), the report's conversions on the right, sized by conversions. The revenue conversion is the accent node and shows the money.
- The page reads the same report a second time under the other model, only in the flow view. The column compares both: the headline is net revenue (or the first goal's conversions) under the chosen model, one sentence names the value whose share moves most between the models ("Organic search gets 49% of the revenue on last touch and 87% on first touch."), and that value's bands are the accent ones until something is focused.
- The list ranks values by share under the chosen model with two thin bars each, the chosen model solid and the other gray, and the other model's share in the accent where the two differ by five points or more.

## Funnels

`/sites/:site/funnels` has two views of one funnel result, Flow and Details, picked in the title row and remembered in the browser. Flow is the default. Details is the earlier step table with its drill menus.

In Flow, the step editor folds into one line ("2 steps · 1 hour window · Daily visitors") with an Edit steps button; the full editor comes back on click.

- The hero is a ribbon. Each step is a narrow column as tall as its share of step 1, labelled with its number, a short caption from its first condition ("Any page view", "/pricing") and its count. A step label opens the sessions that reached it.
- Visitors who reach a step but not the next peel off the ribbon's bottom edge and curve down into one flow per destination, stacked under the ribbon: Left the site (no further page in the session), Went back (a page the session had already shown), the three most common next pages, and Elsewhere for the rest. Each flow is labelled with its destination and count.
- One leak is in focus at a time, drawn in the accent; the rest stay neutral. The worst leak, the step losing the largest share of its visitors, starts in focus. Clicking a flow or its label focuses that step's leak.
- The rail starts with the conversion as the large number, "16,203 of 32,478 reached the last step", and the change in points against the previous period of the same length. Under a hairline, one sentence names the worst step: "Most visitors stop after step 1, Any page view: 50% of them don't go on."
- Under it the focused leak: its destinations with counts, the 20 latest sessions that stopped there, each opening the session page, and a link to all sessions that reached the step.
- Destinations come from `leaks` on the funnel result: per step, the next page view in the same session as the step event, within the funnel's read span.

### Split and step durations

Settled by Mike on 2026-10-27 ([0022](../decisions/0022-agent-report-queries.md) funnel v2).

- A "Split by" chip sits in the folded step-editor line. It opens Explore's dimension picker, limited to the fields `group_by` accepts, event and session properties included. Removing the chip removes the split.
- With a split, the overall ribbon stays the hero. Under it a panel, "By {field}", lists "All traffic" first, then the top 10 groups by visitors entered, with "Show all" expanding in place to every returned group (up to 50). The panel meta says when the server left groups out. Columns: the group, entered, reach per step as a bar with a marker, conversion, and median time to convert. The not-set group is labelled "(none)", as in Explore.
- Picking a row reads the funnel again without the split, filtered to that value (`is_not_set` for "(none)"), so ribbon, leaks, latest sessions and durations are exact for that group. The picked row has an accent edge; "All traffic" returns to the overall result.
- Step durations are exact values. The median from the previous step sits in the gap between two ribbon columns ("median 2 min 39 s"). Hover or focus opens a popover above the ribbon, never over the leak flows, with p25, median, p75 and the five bucket counts (under 1 h, 1 to 24 h, 1 to 7 d, 7 to 30 d, 30 d or more). The rail adds one line under the conversion: time to convert, median and the middle half.
- Below `md`, the median moves under each step label ("2 min 39 s after step 1") and opens the same popover on tap.
- Durations read "under 1 s" below a second, otherwise in the two largest units ("2 min 39 s", "3 d 4 h"). A step nobody reached shows no duration.
- Details shows a split funnel as one table: a row per group, "All traffic" first, per step reached and its share of step 1, then conversion and median time to convert, with the cell drill menus of the step table.
- The split is part of the funnel state, so URLs, Back and saved explorations keep it. Pin to board pins what is on screen: with a picked row, that group's filtered funnel without the split; without one, the split funnel, whose card shows the overall ribbon, "split by {field}" and the top 3 groups.
- Rejected on 2026-10-27: small multiples per group (with low conversion every mini ribbon looks the same, and 50 do not fit), the top groups as stacked bands in one ribbon (bands thinner than a pixel after step 1), and durations as a rail section (it pushes the focused leak below the fold).

## Revenue

`/sites/:site/revenue` has two views of the same state, Summary and Details, picked in the title row beside the currency switch and remembered in the browser. Summary is the default. Details is the earlier page with the measure strip, chart and breakdown. Range, filter and currency apply to both. The globe was dropped on 2026-10-26: for most shops one or two countries hold the money, so it answered a question nobody asked.

Summary has a left column and a canvas, like the other visual views, but grows with its content instead of filling one screen.

- The column starts with net revenue in the chosen currency as the large number and its change against the comparison.
- Under a hairline, gross, refunds and net: a bar of net with the refunds hatched in the destructive tone after it, then gross, refunds with their share of gross, and net per record.
- Under a hairline, 24 bars of net revenue per local hour of the site's timezone. The busiest three consecutive hours are in the accent. Each bar opens Explore narrowed to that hour.
- One sentence says when the money arrives and, when one source clearly pays best, which: "Most revenue arrives between 7:00 PM and 10:00 PM. Email pays €3.46 per visitor-day, 3 times the site's €1.15." A source is named only with at least 5% of the visitors and 1.5 times the site's rate.
- The canvas starts with the Pace panel: the running total of net revenue as a solid accent line, the comparison's running total as a gray line over the whole range, and, while the range contains now, the estimate as a dashed line with its band to the server's end-of-range projection ([0015](../decisions/0015-baselines-projections-and-noticed-changes.md) amendment of 2026-10-26). The estimate is shaped by the projected buckets and ends exactly at the projected total. End labels at the right edge name the estimate ("≈ €57.5k") and the comparison's total. Annotations mark their bucket with the accent diamond and a dashed line. One sentence above the chart: how far ahead or behind the comparison the running total is, and the estimate with its band and basis.
- Below it, "Which traffic pays": sources ranked by net revenue, the top seven and "Other". Each row has an accent bar for its share of net revenue over a gray bar for its share of visitors, on one scale across rows, then net, net per visitor and conversion (visitors with a revenue record over visitors). A source paying 1.5 times its traffic share or more has its shares in the accent, two thirds or less in the destructive tone. A segmented control picks the source: channel (default), UTM campaign, referrer, country or page, kept in the `by` URL parameter. Each named source opens Explore narrowed to it.
- Visitors here are what the range's visitor measure counts: visitor-days for any range longer than one identity day ([0003](../decisions/0003-metric-formulas.md)), and the labels say so.
- The summary's cuts are separate reads without comparison: net by currency and hour; net, records and visitors by currency and source over revenue records only; visitors by source over all traffic; and, while the range contains now, the same range under the typical-weekday comparison for the projection.

## Events board

`/sites/:site/events` has two views, Board and Details, picked beside the title and remembered in the browser. Board is the default. Details is the earlier page with its Events and Properties tabs.

- One panel, "Top events", lists the 12 custom events with the most events in range, with "12 of 31 · Last 30 days" as its meta and a fewer-to-more legend.
- Each row: the event name, a strip of its counts per bucket over the range, the count, its change against the comparison (the measures strip's tones, muted within 5%), reach (visitors who sent it over all visitors, with a thin bar), events per visitor, and the net revenue it carries.
- The strip's cells are accent squares whose opacity follows the count squared, scaled from zero to the row's own busiest bucket, so a small event's rhythm shows as clearly as a large one's and ordinary days recede. Rows share their buckets, so a day that moves several events reads down the board. Annotations put their text above the strip's column and a dashed line through every row.
- Clicking a row opens Details with that event's properties, through the `view=details` URL parameter, so Back returns to the board. A last line says how many events are not listed and opens Details.
- Below `md` a row keeps name, count and change, with the strip on its own line under them.
- The board reads the list once, all traffic once for reach, and one series per listed event.

## Sessions timeline

`/sites/:site/sessions` has two views, picked with a segmented Timeline / Details control beside the title and remembered in the browser. Timeline is the default; Details is the paged session list. A second control on the right picks the window: 1 hour, 6 hours (default) or 24 hours.

- The left column starts with the count of sessions that started in the window as a large number, then the median observed span (as the `span_median` measure reads it) and how many sessions had a custom event, marked with an accent dot. When the window has more than 300 sessions it says it shows the latest 300.
- Under a hairline, the five longest sessions: flag, entry page, an accent dot when it had a custom event, and the span. Each opens the session.
- Beside it, the sessions as people over time: one neutral line per session from its first to its last event, a small dot per pageview and an accent dot per custom event. Sessions that do not overlap share a lane, and lanes grow to fill the canvas when there are few. A single-event session is a short line so it stays clickable. Times run along the top to "Now" at the right edge. When the window holds more than 300 sessions the axis starts shortly before the earliest one shown.
- Hover or focus raises the line in the accent and shows a card: flag and country, device, start, span, pageviews and the first six events, custom events in the accent. A line opens the session detail; a small play mark after it opens the session's replay when one exists. The timeline is one tab stop: the arrow keys move through sessions in time order and Tab from a session reaches its replay.
- Data comes from `sessions.timeline` (the latest 300 sessions starting in the window with up to 60 events each, the window's count and median span), read again every minute, and from `recordings.list` for the replays of the same window when the person may read recordings.

## Session detail

`/sites/:site/sessions/:session`, opened from a row of the Sessions report. The title row names the session's start and links back to the list as it was left.

- A strip of four stats: observed span, views, events and distinct pages.
- An "Activity" panel, the page's subject: every pageview and custom event in order, with the time, a kind icon, the path or event name in primary text, the page title or path muted, and the pause since the previous event where there was one. Up to 500 events; a longer session says so.
- Beside it a "Visitor" panel with the session's stored fields: source (channel and referrer), campaign, location, device, language, screen, identity kind and the session ID with a copy button. Only fields with a value appear.
- A "Properties" panel with the latest value of each identified session property, only when there are some.
- Data comes from `sessions.get`, which reads the same session membership as the session list and every session measure.

The Sessions list itself shows start, entry page, source (referrer or channel), location, device (icon, browser, system), observed span, views and events, with a "Person" marker only on consented people's sessions. Each row opens the detail.

## Workspace sites page

The sites page has two views of the same summaries, picked with a segmented Tiles / Details control on the line that counts the sites. Tiles is the default; the choice is remembered in the browser.

- Tiles are a grid of cards in the status order below: the site name and domain, a findings dot in the accent with the count when there are findings, today's views in large type with the pace in words beside "views today", and the 24-hour sparkline across the bottom. A tile opens the site's overview.
- A site whose summary counted views in the current hour has a success dot after its name, and its border breathes softly between the hairline and a hint of the success colour. Under reduced motion the dot stays and the border holds still. The signal comes from the summary's hourly counts, not the realtime stream: one stream per site would use up the browser's connections.
- A full screen button turns the tiles into a wall display: the workspace name, the site count line and a clock on top, then the tiles filling the screen in larger type. While tiles show, the summaries are read again once a minute.
- There is no left column: sites of very different size add up to a total that means little, and the one cross-site fact, how many sites need you, already heads the page.

Details is the earlier list. "All sites in {workspace}" lists every site the person can read, in the table pattern: one row per site with today's views so far, the pace in words, a small 24-hour sparkline of today against the typical day, finding count, and the time of the summary. Sorted by findings first, then by absolute pace. A row opens that site's overview. Data comes only from the summary rows ([v06](v06-workspace-sites-page.md)).

## Setup wizard

Variant A, the guest column (Mike, 2026-10-21): the sign-in page grown into a wizard, in the guest layout without the app shell. Rejected: B, the app frame with the steps in a rail, and C, a single document page.

- One step per screen in the guest column (`max-w-sm`), with the step bar on top. Steps that show tables, progress or the report mark themselves `data-wide` and widen the column to `max-w-xl`.
- Each step opens with the page-title pattern: the question in large type, a muted second line with what it's for. One accent button moves on; secondary actions are ghost buttons below it.
- The path choice is a radio group of filled cards with an icon tile; the chosen card turns to the surface with an accent inset ring. Paths not in the build stay visible, disabled, with "Not in this build yet."
- Server check rows and import warnings are warnings, never blocks: each names the setting that fixes it, and the accent button stays.
- Destructive exits (discard, continue without import after an import) open an inline filled panel that asks for the instance name before the destructive button enables.
- Live checks (first pageview, first hit after cutover) are a filled row with a dot: pulsing accent while waiting, solid success when it arrived.

## States and copy

| State | What shows |
| --- | --- |
| Fewer than 4 baseline days | No baseline line, no projection, comparison chip reads "typical weekday (not enough history)", headline uses the no-comparison sentence plus "Not enough history for a typical day yet." |
| Range before retention start | As today: resolved bounds show `retention_start`, the chip names the clipped start. |
| Summary stale | Site chip pace hidden, site menu rows show name only, findings show their time with "updating". |
| Filtered overview | Headline and chart use the filtered state. Findings stay unfiltered and say so ("For all traffic"). |
| Shared exploration (`/share/:token`) | Shows headline, measures, chart and breakdowns of the shared state. No findings, no context chips, no site menu, no badges. |
| Analytics stale (`analytics_stale`) | Headline adds "Data until {watermark time}", the freshness note says the same, and the Now marker sits at the watermark, not the clock. |

Copy follows the existing rules: visitor-days, observed span, no "people" for anonymous figures, English and German written together with key parity.

## What v2 does not take from the inspiration screens

- Their orange accent: Holocron keeps its indigo.
- Pill shapes: controls use `--radius-control`, not fully rounded ends. Only badges and the avatar are round.
- A multi-hue gradient: the lead finding's border fades one hue into the hairline.
- An "ask" field that suggests natural-language answers.
- Badges counting things that exist rather than things that need attention.

## Rejected direction

The first draft ([examples/v2/rejected/first-draft.html](../../examples/v2/rejected/first-draft.html)) kept today's Kit shell: a header bar with breadcrumbs and the jump field across the top, a separate scope row under it, outlined controls, a bordered sidebar, three equal finding cards, and compact 32px table rows without column dividers. Rejected by Mike on 2026-10-15 in favor of the surface treatment above. Its content decisions (headline, findings, movers, goal pace, site menu) carry over unchanged; only the shell and surface treatment were rejected. Do not use it as a visual reference.

## Visual acceptance

As in Kit implementation Visual and interaction acceptance, plus:

- 1440, 1024, 768 and 390 px, light and dark, with the longest German strings for the headline grammar and the top bar.
- The headline wraps to at most three lines at 1024 px.
- The top bar collapses scope chips before it wraps; it never takes two rows above 768 px.
- Chart, estimate and baseline remain distinguishable in grayscale.
- Filled controls stay distinguishable from the canvas in light mode (fill against white) and dark mode (gray-900 against gray-925 and gray-950).
- The site menu with 40 sites in 6 base domains scrolls inside the menu, not the page.
