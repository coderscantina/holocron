# Milestone 14 evidence: web setup and onboarding

Status on 2026-10-27: in progress. Everything is built and every criterion below except the import ones has evidence from this pass. The import criteria have tests that need the databases of `tools/import-rehearsal/test.sh` (Docker), which was not running. Built on macOS (Darwin 27, arm64) with Rust 1.99.1, Bun 1.4.4 and Playwright Chromium 1.64.1. The rules are in [0021](../decisions/0021-web-setup-and-onboarding.md).

## Acceptance criteria

| Criterion | Result | Evidence |
| --- | --- | --- |
| An empty volume reaches a signed-in admin with a first site that has received a pageview, without a shell | Passed | `setup.spec.ts` "a fresh start reaches a site with its first pageview, without a shell"; `the_wizard_creates_the_admin_and_hands_over` (`apps/server/tests/setup.rs`) |
| Both import fixtures import through the wizard with the CLI's manifest digests; a server killed during the events step resumes to the same digests | Not run in this pass | `both_snapshots_import_through_the_wizard_with_the_digests_of_the_cli` and `a_server_killed_during_the_events_step_resumes_from_the_wizard_to_the_same_digests` (`apps/server/tests/imports.rs`) are ignored without the rehearsal databases. Run `tools/import-rehearsal/test.sh` with Docker. |
| A backup restores through the wizard, including one from an older release that needs an upgrade | Passed | `a_restore_ends_setup_and_the_restored_admin_signs_in`, `an_interrupted_restore_starts_setup_mode_and_can_be_repeated`, `setup_modes_lock_file_does_not_block_a_restore`, `a_backup_from_a_newer_release_is_refused` (`apps/server/tests/setup.rs`). Browser: the seeded instance's backup restores and its admin sees the same sites. A backup written by the 26.10.23-2 binary (`v26.10.23.2-d1947bb`, the production release) restores, shows the Upgrading phase and signs in (`HOLOCRON_E2E_OLD_BACKUP`). |
| Setup operations are refused without the token and once an admin exists; collection answers 503 throughout setup | Passed | `restore_operations_need_the_token_and_end_with_setup`, `the_import_path_stays_in_setup_mode_and_returns_to_it_after_a_restart`, the token spec in `setup.spec.ts` |
| A plain source connection to a public address is refused; a TLS one is accepted | Changed, then passed in part | Mike settled on 2026-10-23 that an unencrypted source is a warning, not a refusal. The warning: unit test `a_public_source_without_tls_gets_a_warning_and_a_private_one_does_not` (`apps/server/src/imports/source.rs`). Reading over TLS: `a_source_url_that_asks_for_tls_is_read_over_tls` needs the rehearsal databases and did not run in this pass. |
| Setup specs pass in e2e against a server on an empty data directory; screens pass visual acceptance at 1440, 1024, 768 and 390 px, light and dark | Passed | `scripts/e2e.sh`: 77 passed, none skipped. 104 setup captures (token, check, path, fresh admin and site, import admin and source, restore pick, confirm, progress, failed, refused, sign-in) reviewed. |
| Rust and web checks from `AGENTS.md` pass | Passed | See Checks run |

## Found and fixed during visual acceptance

- Sign-in and setup didn't follow a change of the system color scheme after load: only the first-paint script applied it. `GuestLayout` now runs `useColorMode`, like `ShareLayout` (`c374d32`).
- At 1024 px the decorative bars ran under a wide setup column. They now show from `xl` on; the rule is in the [design](../ux-v2/design.md#setup-wizard) (`c0caa2c`).
- The captures caught the color fade after the scheme switch; the spec now waits for it (`749f986`).

## Checks run

| Check | Command | Outcome |
| --- | --- | --- |
| Rust format, lint | `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings` | Clean |
| Rust tests | `cargo test --workspace --no-fail-fast` | No failures; the rehearsal-database tests are ignored |
| Drift check | `cargo run -p holocron-protocol --bin export-types -- --check` | Up to date |
| Web | `bun run typecheck`, `bun run lint`, `bun run test` | Clean; 205 tests passed |
| Browser journeys | `HOLOCRON_E2E_OLD_BACKUP=<26.10.23-2 backup> scripts/e2e.sh` | 77 passed |

## Remaining

- Run `tools/import-rehearsal/test.sh` with Docker and record the three ignored import tests here, then mark the milestone complete.
- The failed restore step keeps the "You can close this page and come back" subtitle of the progress step.
