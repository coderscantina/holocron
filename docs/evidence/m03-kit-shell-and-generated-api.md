# Milestone 03 evidence: Kit shell and generated API

Status on 2026-10-03: complete. Acceptance criteria have evidence and the work is merged with milestone 02. Built on macOS (Darwin 27, arm64) with Rust 1.99.0, Bun 1.4.2 and Playwright Chromium 1.63.0.

The overview shows fixture data under a visible label. No analytics are calculated; that is milestone 06. Site settings read and write real data through the generated API.

## Acceptance criteria

| Criterion | Result | Evidence |
| --- | --- | --- |
| The packaged Rust server serves the Vue shell without a production Node process | Passed | `apps/server/scripts/package.sh` builds `holocron` plus `web/`. The browser journeys ran against that package started with `PATH=/usr/bin:/bin`; the server process had no child processes. |
| Generated schema regeneration produces no unexplained drift; dispatch, types, errors and API docs agree | Passed | `export-types -- --check` reports 34 files up to date. A hand edit to a generated file makes it exit 1 (`changed: SitesGetArgs.ts`). Dispatch is generated from the same registry, so a handler that does not match its declared types does not compile. |
| Site access is enforced server-side, including a guessed inaccessible ID | Passed | `hidden_sites_answer_exactly_like_missing_ones`: a guessed ID, a site in another workspace, a site after removal and an install admin without membership all get byte-identical 404 bodies for `sites.get` and `sites.update`. 9 rows of `fixtures/access/cases.json` pass. Browser: `site-unavailable-dark.png`. |
| Header/menu behavior, light/dark, phone navigation, keyboard focus and translated copy meet the Kit contract | Passed, with the gaps listed below | 9 browser journeys, 32 screenshots in [m03/](m03/) |
| Component reuse is recorded in an extraction map with replaced dependencies | Passed | [Kit extraction map](../contracts/kit-extraction-map.md) |
| A runtime-config change takes effect without rebuilding Vue | Passed | The same package restarted with `HOLOCRON_INSTANCE_NAME="Roadhouse Analytics"`. `index.html` hash unchanged (`6fc2466d…`), `/config.json` returned the new name, and the journey asserted the sidebar brand and document title (`runtime-config-instance-name-light.png`). |

## What was built

- **Protocol** (`crates/protocol`): IDs, roles and capabilities from the role mapping, 31 error codes, the envelope, limits, runtime config, and a `domain!` registry with `auth.login`, `auth.logout`, `auth.session`, `sites.get`, `sites.update`, `workspaces.list`. TypeScript and OpenAPI 3.1 export with a drift check. See [0009](../decisions/0009-protocol-envelope.md).
- **Server** (`apps/server`): axum 0.8 router, single-writer and read-pool SQLite control database (WAL, `synchronous=FULL`, foreign keys, 5 s busy timeout), migrations for users, sessions, workspaces, memberships, sites, topic events and idempotency keys. Session cookies with hashed IDs, argon2 passwords, login rate limit, opaque `SiteScope`, origin and request-marker check on commands, idempotent commands with reauthorized replay, expected revisions, typed API 404s, SPA hosting with `/config.json`, immutable hashed assets, revalidated HTML and config, `nosniff`, `X-Frame-Options: DENY`. `holocron init` follows the 0006 command and exit codes; `holocron serve` refuses an uninitialized directory with exit 3.
- **Web** (`apps/web`): Vite, Vue 3, Tailwind 4, Vue Router, Vue Query, VueUse and reka-ui at Kit's versions. Adapted Kit shell, context switcher, user menu, command palette, shortcuts dialog, mobile drawer and tab bar. English and German catalogs. Pages: sign-in, overview (fixture data), website settings (real), account, pending sections naming their milestone, empty workspace, not found.

## Checks run

| Check | Command | Outcome |
| --- | --- | --- |
| Rust format | `cargo fmt --all --check` | Clean |
| Rust lint | `cargo clippy --workspace --all-targets` | 0 warnings |
| Protocol tests | `cargo test -p holocron-protocol` | 3 passed: deterministic output, unique names, no drift |
| Server tests | `cargo test -p holocron-server` | 19 passed: 6 access, 8 command, 4 web, 1 fingerprint unit test |
| Drift check | `cargo run -p holocron-protocol --bin export-types -- --check` | Up to date; exits 1 after a hand edit |
| Typecheck | `cd apps/web && bun run typecheck` | Clean |
| Lint | `cd apps/web && bun run lint` | Clean; a bare `fetch` outside the transport is an error |
| Unit tests | `cd apps/web && bun run test` | 8 passed: 6 client, 2 locale parity |
| Build | `cd apps/web && bun run build` | Passes; warns about the 612 KB (97 KB gzipped) icon chunk |
| Browser journeys | `cd apps/web && HOLOCRON_E2E_PASSWORD=… HOLOCRON_E2E_VIEWER_PASSWORD=… bun run e2e` | 9 passed, 1 skipped by design (the runtime-config journey runs only with `HOLOCRON_E2E_INSTANCE_NAME`, then passed) |

Use a dedicated `CARGO_TARGET_DIR` per worktree when two agents build at once.

### Browser setup

```sh
apps/server/scripts/package.sh
target/package/holocron/holocron init --data-dir <dir> --admin-email mike@example.test --admin-password-file <file>
cargo run --release -p holocron-server --example seed_demo -- <dir> mike@example.test <viewer-password>
target/package/holocron/holocron serve --data-dir <dir> --bind 127.0.0.1:8093
cd apps/web && HOLOCRON_E2E_PASSWORD=… HOLOCRON_E2E_VIEWER_PASSWORD=… HOLOCRON_E2E_SHOTS=../../docs/evidence/m03 bun run e2e
```

`seed_demo` adds a Roadhouse team workspace with three sites (one with a 58-character name) and a personal workspace with one site. Init's own "Default" workspace stays empty and covers the empty-workspace case.

## Failures found and fixed

- First journey runs failed on the server's login limit (10 per address per minute). The limit was right; the journeys now sign in through the form once per account and reuse the session.
- The hourly chart overflowed at 390 px: 24 nowrap axis labels set a minimum width. Labels are now positioned absolutely. The overflow check now also looks for elements past the viewport inside `main`, because `overflow-x: clip` hid it from a page-level check.
- Command palette: typing a search under a resting pointer dropped the highlight, so Enter did nothing. Fixed in the adapted `Command.vue`.
- Muted secondary text (workspace role, site domain, workspace name in the palette) was unreadable on the accent highlight. It now takes the accent foreground at 80%.
- Overview shares were computed from the listed rows only and disagreed with the reference. The fixture now carries the reference shares.
- Screenshots were taken mid-animation. Journeys now wait for animations to finish first.
- Server logs carried ANSI codes when not on a terminal.
- After the merge with milestone 02, `holocron-workload` enabled `serde_json` `preserve_order` in workspace builds and reordered `openapi.json`, so the drift test failed. The protocol crate now enables the feature itself and the contract was regenerated; its content is unchanged.

## Screenshots

In [m03/](m03/), all viewport captures: overview at 1440, 1024, 768 and 390 px in light and dark; German overview at 1440 and 390 px with the 58-character site name; user menu (desktop, phone, German); workspace and site selectors; command palette; collapsed rail; mobile drawer and settings in both modes; skip link and focus ring; sign-in error in both modes; settings field error, saved, stale revision and viewer read-only; guessed site ID; runtime instance name.

## Not verified

- **Zoom/reflow and compact density.** Not captured. Density is a single attribute and was not switched.
- **Automated accessibility pass.** No axe run; keyboard and focus checks only.
- **Long workspace names.** Long site names were tested; long workspace names were not.
- **Access fixture rows for API keys, shares, 2FA and reauthentication.** 402 rows are pending until their operations exist (milestone 10). None were edited.
- **Live subscriptions.** Topics and cursors exist; the WebSocket endpoint is not built (no live resource needs it yet).
- **Exploration state, URL encoding, saved views, tables.** Milestone 06.
- **Argon2 inside the write transaction.** `auth.login` verifies the password while holding the write lock (not measured). Acceptable now; worth moving before the 500 events/s gate shares the writer.
- **Windows builds.** Not built. Linux arm64 (Debian bookworm, `rust:1.99-bookworm` in OrbStack, 4 GiB) builds the release server in 10 min and passes the whole workspace suite on 2026-10-06 with `CARGO_BUILD_JOBS=1`; with more parallel jobs the debug test binaries ran out of memory while linking. Run as a non-root user: as root, `a_failed_upgrade_is_refused_by_serve_and_resumes` fails because root writes the read-only analytics file the test relies on. Linux x86_64 was not built.

## Decisions

- Environment selector omitted; the workspace kind badge is the only badge (extraction map).
- Control database file is `control.sqlite` in the data directory, named alongside milestone 02's files in [0008](../decisions/0008-storage-engine-and-budgets.md).
- `serve` on a missing or half-initialized directory exits 3 with `not_initialized` or `init_incomplete`. 0006 left this open for milestone 12.
- `holocron init` also creates a "Default" team workspace owned by the first admin, so the admin has a place for sites. There is no `sites.create` operation yet.
- Unadopted collection proposals from milestone 01 (invalid `eventId` response, identify on link and pixel sources, replay-budget body, collect-alias serving) are deferred to milestone 04. They belong to the collection surface, which keeps upstream's wire format and does not use this envelope.

## Handoff

Milestone 04 adds collection routes to the API router outside `/api/commands` and `/api/queries`, with its own CORS and without the origin check. Ingestion state belongs in its own migrations; `AppState` gains the inbox and storage handles from milestone 02. Milestones 06 to 10 add operations to `crates/protocol/src/operations`, implement the generated trait on `AppState`, take a `SiteScope` from `ctx.site_scope`, and regenerate.
