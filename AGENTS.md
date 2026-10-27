# Holocron agent guide

Holocron is a self-hosted web analytics server in Rust with a Vue dashboard. It replaces Umami, imports its database, and serves many teams and sites from one process and one data directory.

## Start here

Read [docs/STATUS.md](docs/STATUS.md) for where the work stands, then [docs/README.md](docs/README.md) and its reading order. The technical briefing owns scope, the milestone index owns sequence and open decisions, the development workflow owns how you take, change and hand back work, and [docs/ux-v2/design.md](docs/ux-v2/design.md) owns the dashboard's visual rules.

## Rules that apply to every task

- Work from an assigned milestone, brief or request. State the files you own, the interface you consume, the behavior you add and the check that proves it before editing.
- Stay inside your file ownership. Send changes to shared files (protocol registry, migrations, tokens, locale catalogs) to their owner as a request.
- Make targeted edits. Generated files (`apps/web/src/generated/*.ts`, `crates/protocol/openapi.json`) are regenerated with `cargo run -p holocron-protocol --bin export-types`, never edited.
- Scope changes, relaxed targets and anything touching privacy or secrets go to Mike. Routine choices inside the contract are yours. A decision record says Adopted only after Mike answered.
- Hand back with the completion report from [docs/development-workflow.md](docs/development-workflow.md) and update `docs/STATUS.md`.

## Checks

Run the checks for the paths you changed:

- Rust: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo run -p holocron-protocol --bin export-types -- --check`
- Web (`apps/web`): `bun run typecheck`, `bun run lint`, `bun run test`, `bun run e2e`

`scripts/e2e.sh` runs the browser journeys: it packages and seeds a throwaway server, waits for the first summary round, runs `bun run e2e` and cleans up. Run it after server or web changes. Running `bun run e2e` by hand drives an already running packaged server at `HOLOCRON_E2E_URL` (default `http://127.0.0.1:8093`) and rebuilds nothing. When failures appear, run the same specs against a build of `main` before calling them regressions.

## Gotchas

- The first build compiles DuckDB from source: several GB of disk per `target/` and a lot of memory. Parallel worktrees share one `CARGO_TARGET_DIR`.
- Storage refuses writes below a 4 GiB disk reserve (`HOLOCRON_DISK_RESERVE_MIB`). Tests that fail with 503 or write errors on a full disk are a disk problem first.
- The repo has no Prettier config. Format web code only through `bun run lint` fixes or by hand; a formatter run with defaults rewrites unrelated lines.
- Locale catalogs `apps/web/src/i18n/{en,de}.json` must keep the same keys (`parity.test.ts`). Delete keys one by one after a search, not with a pattern.
- Sign-in is rate limited to 10 attempts per address per minute. The e2e setup signs in once and reuses the session.

## Demo

Used by the `demo` skill. Mike's demo means his real Umami data unless he asks for the seeded one.

- Real data: the recipe is in `.demo/NOTES.md`, which git ignores.
- Seeded data: `scripts/demo-seed.sh [data-dir] [port]` builds with the `dev-clock` feature, seeds demo, traffic, commerce and acceptance data, and serves on port 8110 with a frozen clock. The login is in `<data-dir>/demo.json`. Wait about a minute for the first summary round.

## Writing

Plain words, sentence case headings, straight quotes, no em dashes. Gitmoji commits with a lowercase plain-language subject, for example `✨ show a funnel as a narrowing ribbon`.
