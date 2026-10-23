# Source manifest

`sources/manifest.json` pins every external source the contracts depend on. This page explains it. The JSON is the authority, this page is the reading guide.

## Fetch

```sh
scripts/fetch-sources.sh
```

The script needs bash, git, jq, and shasum or sha256sum. It clones each source into `.sources/<name>` at the pinned commit (shallow, over https), then checks:

- HEAD equals the pinned commit
- the license file, lockfiles, and referenced files match their recorded sha256, read from the pinned commit and not the working tree
- the bundled Umami scripts in `sources/umami-3.4.0/` match their recorded sha256

Re-running is safe. A checkout already at the pinned commit is not fetched again. A source that cannot be reached is reported and skipped, the rest continue, and the exit code is 1 if anything failed. Entries marked `"distributable": false` are not fetched. Set `HOLOCRON_SOURCES_DIR` to fetch somewhere other than `.sources/`.

## What is pinned

| Name | Role | Commit | Tag | License |
| --- | --- | --- | --- | --- |
| umami-3.4.0 | Runtime compatibility target | `ec0ff50388c264ed8ce46f00967e92f7e71476ae` | v3.4.0 | MIT (`LICENSE`) |
| umami-2.20.2 | Importer source, MySQL schema only | `3eb2e890efb5b18bdfbeab46b9946f48e38da424` | v2.20.2 | MIT (`LICENSE`) |
| kit | UX reference | `f447e797f1c75b967742d47b9cbb6538b32234e3` | none | MIT, confirmed by Mike on 2026-10-03. `composer.json` declares it. No `LICENSE` file yet |
| folio | Architecture reference | `219e9aaa73b925c084a923835b97d14c142edd09` | none | MIT, confirmed by Mike on 2026-10-03. No `LICENSE` file yet |
| werkstatt | Described pattern only, not distributable | none pinned | none | proprietary, all rights reserved |

Clone URLs are in the manifest: `github.com/umami-software/umami`, `github.com/coderscantina/kit`, `github.com/badmike/folio`. Werkstatt has no fetch URL.

Why each exists:

- Umami 3.4.0 is the behavior Holocron collection must match. Handlers and scripts are read together, not by route name.
- Umami 2.20.2 is read only to build the MySQL importer. Nothing at runtime targets it.
- Kit and Folio are the sources the docs link for UI and architecture patterns. The manifest records the repo-relative path and sha256 of every file the docs reference (25 in Kit, 7 in Folio). All of them exist at the pinned commits.
- Werkstatt is private and proprietary. Its commit `beac11cd` exists only locally. The manifest keeps an entry marked not distributable, with the reason. The patterns the docs rely on are written out in [architecture patterns](architecture-patterns.md), so nobody needs its files.

Lockfiles are recorded with digests: `pnpm-lock.yaml` for both Umami versions and Folio, `apps/server/Cargo.lock` for Folio, `composer.lock` and `bun.lock` for Kit.

## Umami 3.4.0 scripts

The repo does not contain the built scripts. `public/script.js` and `public/recorder.js` are produced by `rollup -c rollup.tracker.config.js` and `rollup -c rollup.recorder.config.js`, which the `build:docker` script runs. The official image `ghcr.io/umami-software/umami:3.4.0` (index digest `sha256:85909afc45bdcda1917394594a087421fdbb05610fded0fa9f6fb861abb2f367`) ships them in `/app/public`. The tag format is `3.4.0`, not `postgresql-v3.4.0`.

| File | Bytes | sha256 |
| --- | --- | --- |
| `sources/umami-3.4.0/script.js` | 4773 | `654f12c7548be726eba5b71d5ef8bffc2a41a39a0053e2a49a847869ccd7ac3f` |
| `sources/umami-3.4.0/recorder.js` | 190816 | `21e6312866db01a71841b772c9882333231be1bc3fdbeeac1ed5193794ad80c6` |

Both files were copied out of the arm64 image with `docker create` and `docker cp`. They are small MIT-licensed artifacts, stored in this repo so collection fixtures do not need Docker.

Building from the pinned commit (`pnpm install --frozen-lockfile`, then the two rollup configs, with no `COLLECT_API_*` variables set) produced byte-identical files. The image is therefore the default build: empty `COLLECT_API_HOST` and endpoint `/api/send`.

### Script aliases and endpoint settings

Read from `rollup.*.config.js`, `next.config.ts`, and `docker/proxy.ts`:

- `COLLECT_API_HOST` and `COLLECT_API_ENDPOINT` are build-time replacements inside the scripts. A custom endpoint needs a rebuilt or patched `script.js` (`scripts/update-tracker.js` rewrites `/api/send`). `COLLECT_API_ENDPOINT` is also read at runtime by `next.config.ts` and `docker/proxy.ts`.
- `TRACKER_SCRIPT_NAME` is a comma-separated list of extra paths that serve `/script.js`.
- `TRACKER_SCRIPT_URL` rewrites `/script.js` to another URL.
- `BASE_PATH` prefixes all of these paths.
- The recorder has no alias setting. It is served at `/recorder.js`.

## Limitations

- Kit and Folio are MIT licensed, confirmed by Mike on 2026-10-03. Neither repo has a `LICENSE` file yet. Mike will add them. Until then the manifest records no license file hash for either.
- Werkstatt is proprietary. Do not copy its code into Holocron. Use [architecture patterns](architecture-patterns.md).
- The shipped scripts were taken from the arm64 image only. The amd64 image was not compared, though the source build matching it makes a difference unlikely.
