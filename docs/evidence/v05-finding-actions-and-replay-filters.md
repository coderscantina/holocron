# v05: Finding actions and replay filters

Evidence for [the brief](../ux-v2/v05-finding-actions-and-replay-filters.md). This file holds the frontend part (items 4 to 6) and the acceptance table for the whole brief. The recordings change (items 1 to 3) is delivered by its own branch and fills in its rows.

## Frontend

- `useSiteFindings(site, limit)` reads `summaries.findings`, keyed `['summaries.findings', site, limit]`, and reads again on every `summary` event of `analytics:<site>` and after a reconnect.
- `components/findings/`: `FindingList` (states, banner, cards, sheet), `FindingBanner`, `FindingCard`, `FindingActions`, plus pure `text.ts` (templates per rule and version) and `actions.ts` (which actions a finding offers).
- Templates are `findings.*` keys in `en.json` and `de.json`, one message per sentence. A rule or version without a template, or values that do not match the rule, give "Something changed in {field}" with an "Open details" link to Explore with the finding's state.
- The overview mounts `FindingList` between the chart and the breakdowns. "Filter to this" copies the finding's filter into the overview state through `applySegment` and `screen.commit`, one history entry.
- `AnnotationsControl` gained an `at` prop (prefill) and a `trigger` slot.

### Decisions

- Only a dimension mover offers "Filter to this" and "Watch {n} replays". The other families filter on time windows and report conditions that mean nothing on the overview or in a replay list; they open their report (goals, funnels, performance).
- The replay count is one query: policy, then `recordings.list` with `filter`, `limit 1` and the replay page's own "1d" range, so the link lists the counted replays. It needs `site.recordings.read` and `replay_enabled`.
- A summary older than 30 minutes (`SUMMARY_STALE_MS`) replaces the whole list with a one-line notice. Before the first summary the list says findings appear after it. A server `stale` flag under 30 minutes shows the computed time and "Updating". A filtered overview adds "For all traffic".
- "Add annotation" only shows with `site.definitions.write`.

### Checks (from `apps/web`)

| Check | Result |
| --- | --- |
| `bun run typecheck`, `bun run lint` | pass |
| `bun run test` | 109 pass, 10 of them new (`components/findings/text.test.ts`) |
| `e2e/findings.spec.ts` on the packaged server, port 8094, `seed_demo` and `seed_traffic 45` | 3 pass: live overview, filter and replay link and sheet (mocked findings), empty banner (mocked) |
| `e2e/exploration.spec.ts` | 12 pass, 1 fails on the Explore page (`acceptance workflow`, first result row reads "Austria" where the test expects "AT"). The page and test do not touch findings; not checked against the base branch |

The seeded server had no summary yet, so the live test shows the "first summary" line. Rendered families come from the unit tests and the mocked answer.

### Screenshots

[v05/](v05/): `findings-{1440,390}-{dark,light}.png` (banner, two cards, "Show all 4") and `empty-{1440,390}-{dark,light}.png`, from a mocked `summaries.findings` answer.

## Acceptance

| Criterion | Status |
| --- | --- |
| Card text and numbers match the finding for each family; state link shows the same numbers | Text: unit tests per family. State link: opens the finding's own state. Not compared against the v02 server fixture |
| Replay list with a finding's filter returns exactly the matching captures, incl. a corrected session | Recordings branch |
| Filter over the condition limit is refused with the typed error and the page says why | Recordings branch (the page shows the refusal message) |
| A reader without recordings access sees no replay action | Hidden without `site.recordings.read`; not exercised with the viewer account |
| Keyboard: every action reachable and labelled with its finding | Every action has the accessible name "{action}: {fact}"; tab order is DOM order. No screen reader pass |
