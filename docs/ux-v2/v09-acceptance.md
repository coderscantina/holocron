# v09: v2 acceptance

## Outcome

The v2 dashboard works end to end against the Rust server on seeded data, keeps every milestone 06 to 10 behavior, and holds the release-gate latency targets with the background summary step running.

## Dependencies and ownership

Requires v01 to v08. Integration owns it.

Owned: new e2e specs under `apps/web/e2e/`, workload additions under `crates/workload` agreed with the storage owner, and `docs/evidence/v09-acceptance.md`.

## Work

1. Extend the seed (`cargo run -p holocron-server --example seed_demo`) with: 10 weeks of hourly traffic for one site with a spike day, a 3-week-old site, a second workspace with 6 sites over 3 base domains, saved goals and a saved funnel, and recordings for the filtered-replay case. The journeys need a fixed time: add a development-only way to start the packaged server with `Clock::frozen` (`apps/server/src/clock.rs`) that a release build refuses, and agree it with the integration owner before adding a flag.
2. Journeys, each one spec:
   - Open the overview at a pinned Tuesday 15:20: headline, projection band, baseline, Now marker, finding banner and two cards, movers, goal rows.
   - Follow "Filter to this", then Back: one history step each, state restored without a refetch.
   - Follow "Watch {n} replays": the list shows exactly n captures.
   - Switch site through the menu: pace and dots shown, no other scope's data visible during the switch, cancelled requests do not land.
   - The briefing's acceptance workflow (last 30 days, mobile, signup with plan=pro, split by country and referrer, drill-down, previous period, save, pin, reopen) still passes unchanged.
   - Install admin sees the server card and status page; a workspace owner without install admin does not.
   - Shared exploration renders without findings, badges or site menu.
3. Visual acceptance per the design contract and Kit implementation, compared with [examples/v2/index.html](../../examples/v2/index.html) (not the rejected draft): 1440, 1024, 768 and 390 px, light and dark, German strings, collapsed rail, zoom and reflow, automated accessibility pass.
4. Workload: the declared suite with two users plus the summary step over the 124M-record fixture, and the 500 events/s burst. Report the summary step's engine time share and whether every target holds.

## Acceptance criteria

- Every journey passes against the packaged server.
- The pre-existing e2e specs pass unchanged, or each change to them is explained by a design-contract rule.
- Latency, collection and visibility targets of the release gate hold with the summary step running, or the report names the failing case with measured cause and alternatives, per the development workflow's failing-gate rule.
- No screen shows an estimate without its label, and no finding claims a cause.

## Checks

`bun run typecheck`, `bun run lint`, `bun run test`, `bun run e2e` in `apps/web`; `cargo test --workspace`; export-types `--check`; the workload runs with their commands recorded.

## Handoff

Evidence at `docs/evidence/v09-acceptance.md` with screenshots, journey results and workload numbers. Set the v2 index statuses from it.
