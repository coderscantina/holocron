# Status

Updated 2026-10-27. Read by agents at the start of a thread; rewrite it before handing back (`project-status` skill). Deployment details, accounts and local processes live in `.demo/NOTES.md`, which git ignores; keep them out of this file.

## Branches

The branch of record is `main`, published to https://github.com/coderscantina/holocron as one initial commit. Release `v26.10.26.2-bf9a44f` is out (2026-10-26), with the image `ghcr.io/coderscantina/holocron:26.10.26.2` for amd64 and arm64: a dependency update on top of 26.10.23.2 (vueuse 15, vitest 5, the held-back Rust majors, current action pins). DuckDB stays on 1.5 by a tilde pin. The tag `v26.10.26-0cc3e13` has no image: its CI failed on DuckDB 2.0. Milestone 14's fresh and import paths are in it.

`main` is ahead of `origin/main`, local and not pushed (Mike's call on 2026-10-27; the next release waits): the Revenue Summary and Events Board, milestone 15 (report queries for agents, `/mcp`, `holocron query`, funnel split and step durations; [evidence](evidence/m15-agent-query-api.md)), and the cleanup wave: the session field Entry page (`entry_path`, analytics schema 6 with a backfill, also Revenue's source switch instead of Page), `scripts/e2e.sh` (one command, full suite green), keys list only their owner's member sites, German duration units, and `tools/shadow-compare` for a shadow run before switching Umami off.

## Waiting on Mike

- Production: upgrade ops01 to `26.10.26.2` (checklist G, backup first), then the shadow comparison and the Umami switch-off (checklist H). Both are in `.demo/cutover-checklist.md`; the comparison needs a fresh RDS dump in the local `umami` database.
- Push `main` and cut the next release when ready.
- Set the `holocron` package to public on GitHub, so it pulls without credentials, and upload `.github/social-preview.png` as the social preview.
- Decision 0008 is still Proposed, pending the storage gate.

Settled on 2026-10-23: the fresh and import paths ship now, restore through the wizard, 0006 and CAP-ACCESS-01 follow in a later release; an unencrypted or writable import source is a warning, not a refusal; the install-wide first-hit marker, host names judged as written, `sslmode=require` as TLS and setup sign-in for a Holocron admin without 2FA stay as built; "Continue without import" is offered on every import step.

## Next

1. The rest of milestone 14, next by Mike's call on 2026-10-27: restore and upgrade through the wizard, 0006 and CAP-ACCESS-01, the setup screens' visual acceptance at four widths.
2. Milestone 13, the integrated release gate ([milestones/README.md](milestones/README.md)), and the 0008 storage gate, about a month after the production move on a rented 2 vCPU / 4 GiB host.
3. DuckDB 2.0 (duckdb-rs 1.20000) is held back by Mike's call on 2026-10-26: it brings nothing Holocron uses, and the summaries suite ran in 24.8 s against 12.3 s on 1.5 (debug builds). Its test failures have two causes, for a later attempt: a new `async_threads` pool (40 threads per database here) exhausts the process thread limit, fixed by `async_threads = 0` in `analytics::open`; and libduckdb-sys now defines `DEBUG` in debug builds, which slows tests until 60 s test sessions expire, fixed by `debug = false` for libduckdb-sys in the dev profile. 2.0 also writes the latest storage format, so its files do not open in an older release. Check that 1.5 data files open before moving.
