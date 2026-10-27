# Status

Updated 2026-10-27. Read by agents at the start of a thread; rewrite it before handing back (`project-status` skill). Deployment details, accounts and local processes live in `.demo/NOTES.md`, which git ignores; keep them out of this file.

## Branches

The branch of record is `main`, published to https://github.com/coderscantina/holocron as one initial commit. Release `v26.10.26.2-bf9a44f` is out (2026-10-26), with the image `ghcr.io/coderscantina/holocron:26.10.26.2` for amd64 and arm64: a dependency update on top of 26.10.23.2 (vueuse 15, vitest 5, the held-back Rust majors, current action pins). DuckDB stays on 1.5 by a tilde pin. The tag `v26.10.26-0cc3e13` has no image: its CI failed on DuckDB 2.0. Milestone 14's fresh and import paths are in it.

`main` holds, locally and not pushed, the Revenue Summary and Events Board work (rebased and fast-forwarded on 2026-10-27); `origin/main` is behind it. Its web checks and the touched e2e specs pass.

`feat/agent-query-api` (unmerged, rebased on the local `main`, worktree `../holocron-agent-query`): milestone 15 complete, report queries for agents ([0022](decisions/0022-agent-report-queries.md), Adopted; [milestone](milestones/15-agent-query-api.md)). Glob `matches`, funnel v2 `group_by` with step durations, `POST /api/queries/{name}`, `analytics.report`, `holocron query` (refusals exit 5), `/mcp` with tools, resources and four recipe prompts, the "Read analytics" key preset, the glob filter, and the funnel split table, picked-group re-read and medians between steps ([design](ux-v2/design.md#split-and-step-durations)). Every Rust and web check passes; the full e2e suite passes except 7 specs that fail the same way on `main` (see Next); visual acceptance done at 1440, 1024, 768 and 390, light and dark.

## Waiting on Mike

- Merge `feat/agent-query-api` into `main`, and push `main` when ready.
- Decide whether the Revenue source switch keeps "Page" (it is the page the purchase fired on, usually checkout).
- Set the `holocron` package to public on GitHub, so it pulls without credentials, and upload `.github/social-preview.png` as the social preview.
- Decision 0008 is still Proposed, pending the storage gate.

Settled on 2026-10-23: the fresh and import paths ship now, restore through the wizard, 0006 and CAP-ACCESS-01 follow in a later release; an unencrypted or writable import source is a warning, not a refusal; the install-wide first-hit marker, host names judged as written, `sslmode=require` as TLS and setup sign-in for a Holocron admin without 2FA stay as built; "Continue without import" is offered on every import step.

## Next

1. Stale e2e specs, failing on `main` as on milestone 15: `retention-revenue.spec.ts:28` looks for a site `roadhouse.at` that `seed_demo` names `roadhouse.test`; `setup.spec.ts` (2) needs a server in setup mode, not the seeded one; `shell.spec.ts` (4) expect site pace and findings that need a summary round or the frozen clock. Fix the specs or document the run recipe.
2. Mike's production move from Umami (`.demo/NOTES.md`), starting with ops01 in setup mode.
3. The rest of milestone 14: restore and upgrade through the wizard, 0006 and CAP-ACCESS-01, the setup screens' visual acceptance at four widths.
4. Milestone 13, the integrated release gate ([milestones/README.md](milestones/README.md)), and the 0008 storage gate, about a month after the production move on a rented 2 vCPU / 4 GiB host.
5. DuckDB 2.0 (duckdb-rs 1.20000) is held back by Mike's call on 2026-10-26: it brings nothing Holocron uses, and the summaries suite ran in 24.8 s against 12.3 s on 1.5 (debug builds). Its test failures have two causes, for a later attempt: a new `async_threads` pool (40 threads per database here) exhausts the process thread limit, fixed by `async_threads = 0` in `analytics::open`; and libduckdb-sys now defines `DEBUG` in debug builds, which slows tests until 60 s test sessions expire, fixed by `debug = false` for libduckdb-sys in the dev profile. 2.0 also writes the latest storage format, so its files do not open in an older release. Check that 1.5 data files open before moving.
