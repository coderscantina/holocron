# v05: Finding actions and replay filters

## Outcome

The finding banner and cards render findings as facts with actions that act on the finding's own exploration state: filter the overview, open the report, add an annotation, or watch the replays that match. The replay list accepts the same filter tree as explorations, so "Watch 12 replays" lists exactly those 12.

## Dependencies and ownership

Requires v02 for findings and v04 for the mount point. Recording owns the list filter; frontend owns the banner and cards. The protocol owner merges the recordings change.

Owned: `crates/protocol/src/operations/recordings.rs` (list arguments), `apps/server/src/recordings/` (list query), new `apps/web/src/components/findings/*`, `apps/web/src/pages/ReplaysPage.vue` (filter parsing only), tests, and `findings.*` locale keys. Read first: [design contract](design.md) Findings, the reference [examples/v2/index.html](../../examples/v2/index.html), [0015](../decisions/0015-baselines-projections-and-noticed-changes.md) Noticed changes, [0011](../decisions/0011-replay-store-and-isolation.md) Capture identity.

## Current code (2026-10-15)

- `recordings.list` takes `RecordingsListArgs` with `start`, `end`, `path` (contains), `device`, `min_duration_ms`, `saved`, `session` and a cursor. There is no `Group` filter.
- `CaptureSummary` carries the session ID. Captures link to analytical sessions at read time, not by stored session ID (0011).
- `ReplaysPage.vue` keeps `range`, `device`, `saved`, `path` and `session` in its URL.
- `AnnotationsControl.vue` creates annotations through `annotations.create`.

## Work

1. Add an optional `filter: Group` to `RecordingsListArgs`, validated against the site's field catalog like an exploration filter, with the same limits. A capture matches when its analytical session, found at read time as today, has at least one event matching the filter. Return `total` for the first page so the card can say "Watch 12 replays".
2. Bound it: the session match runs through the report executor with the normal timeout and the 0004 condition limits. Refuse with the existing typed errors; never return an unfiltered list in place of a refused filter.
3. Accept the filter in `ReplaysPage.vue`'s URL as the same canonical encoding the exploration state uses, so a card can link to it. Existing simple filters keep working.
4. `components/findings/FindingBanner.vue`, `FindingCard.vue` and `FindingList.vue`: the lead finding as the full-width banner (accent sparkle, fact, truncated detail with the full text in the accessible name and a tooltip, actions as buttons on the right, one-hue gradient border), the next two as hairline cards in a 2-column row. Templates per rule family from stored values (English and German messages per rule ID and version), the window, and the actions in the design contract; the first action filled, the rest ghost. "Show all" opens a Kit Sheet.
5. Actions: "Filter to this" applies the finding's filter to the current overview state through the exploration service (one history entry). "Open report" navigates to the report route with the finding's state. "Watch {n} replays" appears only when recording is on for the site and the count is above 0. "Add annotation" opens `AnnotationsControl` prefilled with the window start.
6. Unknown rule IDs or versions render a generic "Something changed in {field}" card with the state link, never a broken template.

## Acceptance criteria

- For each rule family in the v02 fixture, the card's text and numbers match the finding and its state link shows the same numbers.
- The replay list with a finding's filter returns exactly the captures whose sessions match, checked against a hand-built fixture with matching and non-matching sessions, including a capture whose session was re-derived by a correction.
- A filter over the condition limit is refused with the typed error and the page says why.
- A user who can read the site but not recordings sees no replay action.
- Keyboard: every action reachable and labelled with the finding it belongs to.

## Checks

`cargo test -p holocron-server` for the recordings list, `cargo clippy --workspace --all-targets`, export-types `--check`; from `apps/web`: `bun run typecheck`, `bun run lint`, `bun run test`, and `e2e/recordings.spec.ts` extended for the filtered link.

## Handoff

Evidence at `docs/evidence/v05-finding-actions-and-replay-filters.md`.
