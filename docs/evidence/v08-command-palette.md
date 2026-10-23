# v08 evidence: command palette results

Status: in progress. Work items 1 to 5 are done.

## Acceptance criteria

| Criterion | Result | Evidence |
| --- | --- | --- |
| Typing a saved view's name and pressing Enter opens it with its exact state | Passed | `e2e/exploration.spec.ts` "the command palette jumps to a saved view with the keyboard alone": after the jump, the "Saved view: {name}" button is active, which it is only when the page state equals the saved one. |
| Opening the palette issues no request for lists already in cache | Passed by construction | Each group mounts only while the palette is open and reads `useSavedExplorations`, `useSegments`, `useGoals`, `useBoards` and `useSite` with the screens' own Vue Query keys (stale time 5 minutes). Not asserted by a request-count test. |
| A segment from the palette adds its conditions with fresh predicate IDs and one history entry | Passed by construction | The palette calls the same `applySegment` as `SegmentsMenu` and makes one `router.push`. No dedicated test. |
| Keyboard-only use: arrows across groups, Enter, Escape returns focus | Passed | Same e2e case. Escape returns focus to the jump field since v03-b (`e2e/shell.spec.ts`). |
| A "Findings" group lists the current site's findings and opens the one chosen on its report | Passed | Each row is the finding's fact sentence; selecting it opens its report with the finding's state (Explore for movers and unknown rules), the same target as the card's "Open report" (`openFinding` in `findings/actions.ts`, tested in `findings/text.test.ts`). `e2e/shell.spec.ts` "the palette lists the site findings…" with mocked `summaries.findings`. No group while the answer is stale. |

Groups are capped at 8 with "Show all N"; the search runs over the whole list before the cap (`composables/useCommandPalette.test.ts`).

## Commands

```
cd apps/web && bun run typecheck && bun run lint && bun run test
HOLOCRON_E2E_PASSWORD=… HOLOCRON_E2E_VIEWER_PASSWORD=… npx playwright test e2e/exploration.spec.ts -g "command palette jumps"
```

The e2e ran against the packaged `v2/integration` build seeded with `seed_demo`, as in [m03](m03-kit-shell-and-generated-api.md#browser-setup).

## Open items

- The input placeholder still reads "Search pages, websites and actions"; `shell.spec.ts` matches it.
- A segment that would exceed the URL limit does nothing from the palette, with no notice.
