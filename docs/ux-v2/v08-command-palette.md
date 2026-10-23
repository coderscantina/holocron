# v08: Command palette results

## Outcome

The jump field in the sidebar finds saved views, segments, goals, boards and, once v02 lands, the current site's findings, besides the navigation, sites and actions it finds today.

## Dependencies and ownership

Ready now; findings need v02. v03 places the trigger in the sidebar; this brief owns what the palette finds. Frontend owns it.

Owned: `apps/web/src/components/CommandPalette.vue`, `apps/web/src/composables/useCommandPalette.ts`, tests, and `commandPalette.*` locale keys. Read first: [design contract](design.md) Jump field.

## Current code (2026-10-15)

- `useCommandPalette.ts` holds open state. `CommandPalette.vue` lists navigation items, every site across workspaces, and actions (theme, sidebar, shortcuts, sign out).
- Saved explorations, segments, goals and boards have list queries used by their own screens (`SavedViews.vue`, `SegmentsMenu.vue`, `goals.list`, `BoardsPage.vue`).

## Work

1. Add result groups: saved views (personal), segments and goals of the current site, boards of the current workspace. Load each list through its existing Vue Query key when the palette opens, so screens and palette share cache.
2. Selecting a saved view or board opens it. Selecting a segment applies it to the current exploration through the exploration service. Selecting a goal opens the Goals report with it.
3. With v02: a "Findings" group for the current site, opening the finding's state.
4. Label the trigger "Jump to anything" in English and the German equivalent, with the ⌘K / Ctrl K hint. v03 renders it; this brief supplies the keys.
5. Results stay grouped and capped (8 per group, "Show all" per group) so the list stays scannable.

## Acceptance criteria

- Typing a saved view's name and pressing Enter opens it with its exact state.
- Opening the palette issues no request for lists already in cache.
- A segment applied from the palette adds its conditions with fresh predicate IDs and one history entry, as `SegmentsMenu` does.
- Keyboard-only use works: arrows across groups, Enter, Escape returns focus to the trigger.

## Checks

From `apps/web`: `bun run typecheck`, `bun run lint`, `bun run test`, an e2e case for the saved-view jump.

## Handoff

Evidence at `docs/evidence/v08-command-palette.md`.
