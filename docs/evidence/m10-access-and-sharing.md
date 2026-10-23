# Milestone 10 evidence: access and restricted sharing

Status on 2026-10-05: built and tested locally. Every acceptance criterion has a passing check. The milestone stays in progress until Mike decides the Proposed items of [0012](../decisions/0012-access-implementation.md) and the open points below. Each of them could change behaviour that is now tested.

Decisions are in [0007](../decisions/0007-access-and-account-security.md) (rules) and [0012](../decisions/0012-access-implementation.md) (credentials on the wire, registry flags, secret storage, TOTP, invitations, sessions).

## Run

- Source: commit `211e279` plus the uncommitted milestone 10 changes in this worktree.
- Host: Apple M1 Pro, 10 cores, 32 GiB, macOS (Darwin 27.0.0), Chromium from Playwright. Debug build.

Commands, from the repository root:

```sh
cargo test -p holocron-server -p holocron-protocol      # includes tests/access.rs, accounts.rs, sharing.rs, teams.rs
cargo clippy -p holocron-server -p holocron-protocol --all-targets
cargo fmt --check -p holocron-server -p holocron-protocol
cargo run -p holocron-protocol --bin export-types -- --check
(cd apps/web && bun run typecheck && bun run lint && bun run test && bun run build)

# browser: packaged server on fresh seed data
holocron init --data-dir D --admin-email mike@example.test --admin-password-file PW
cargo run -p holocron-server --example seed_demo -- D mike@example.test 'VIEWER PW'
cargo run -p holocron-server --example seed_traffic -- D
cargo run -p holocron-server --example seed_commerce -- D
holocron serve --data-dir D --bind 127.0.0.1:8099 --web-dir apps/web/dist
(cd apps/web && HOLOCRON_E2E_URL=http://127.0.0.1:8099 HOLOCRON_E2E_PASSWORD=... \
  HOLOCRON_E2E_VIEWER_PASSWORD=... HOLOCRON_E2E_SHOTS=../../docs/evidence/m10 npx playwright test)
```

## What was built

| Work item | Where | Behaviour |
| --- | --- | --- |
| 1 account, sessions | `identity/{auth,account,caller}.rs` | Login with a second-factor step through a 5-minute challenge cookie, logout, 30-day lifetime, 7-day idle timeout, `auth.revokeSessions`, password change ending other sessions. Recovery without email: recovery codes and `users.resetCredentials` by an install admin |
| 2 teams, roles | `teams/mod.rs`, `sites/manage.rs` | Teams, members, rank rules, offline invitations as single-use links (`/teams/join?code=`), site create, list, transfer and delete. Capabilities checked per operation in the fixture's decision order |
| 3 2FA | `identity/{two_factor,totp,secrets}.rs` | TOTP (RFC 6238 vectors), 10 single-use recovery codes stored as SHA-256, secrets encrypted with XChaCha20-Poly1305 under `<data-dir>/secrets.key`, enrollment-only sessions derived per request from the installation, team and user requirements |
| 4 API keys | `identity/api_keys.rs` | `Authorization: Bearer`, grants by operation (registry `key` flag) and site, ceiling at the owner's current role, one-time disclosure, immediate revocation |
| 5 shares | `sharing/{mod,limits,operations}.rs`, read handlers in `explorations/`, `recordings/operations.rs` | `Authorization: Share`, a stored copy of the state and its limits, resolved on every read, export and live frame. Outside the share is `not_found`, widening is `share_scope_exceeded`. Board shares and the recordings grant |
| 6 Kit screens | `apps/web/src/pages/{account,admin,auth,share,workspace}/`, `components/{account,admin,sharing,workspace}/` | Account pages reached from AppUserMenu only, login second factor and forced enrollment, workspace members and invitations, site sharing tab, admin users and security, share viewer at `/share/:token` without the app shell |
| 7 caches, reauth | `api/client.ts`, `lib/reauth.ts`, `composables/useSession.ts`, `useLive.ts` | Logout, lost session and `access_revoked` clear Vue Query caches and close live streams. The password prompt runs before every command the registry marks `reauth`; a new sensitive command fails typecheck until it has a prompt rule |

## Acceptance criteria

| Criterion | Result | Evidence |
| --- | --- | --- |
| Positive and negative cases for every protected surface | Pass. 1229 fixture rows through the real dispatcher: 724 user, 283 key, 128 share, 94 signed-out. A coverage test fails when a protected registry operation lacks an allowed or a denied row for any principal class | `tests/access.rs` |
| Revoked members, keys, shares and sessions lose access, also after reconnect | Pass. Live streams close with `access_revoked`, and a reconnect with the old cursor answers like an unknown credential | AX-S009 to S015, `accounts.rs`, `sharing.rs`, `teams.rs`, `access.rs` |
| Retries never re-disclose one-time secrets | Pass. Stored answers of `secret` operations hold `secret: null, secret_withheld: true` | AX-S006, e2e "API keys…" |
| 2FA and recovery pass; secrets absent from logs and exports | Pass. Wrong-code limit holds under 8 parallel guesses. Handlers log only error values and the HTTP trace records method and path, never headers or bodies. Analytics exports read no account tables | AX-S017, S019, S020, S027, `accounts.rs`, code inspection |
| Account actions work against the Rust API with accessible Kit forms and typed errors | Pass. `e2e/access.spec.ts` 9 of 9 | Browser evidence |
| No known default bootstrap password or external auth dependency | Pass | AX-S023 |
| Sharing never broadens the exploration or caller scope | Pass. Comparison counts toward `range_max_days`, pinned filters stay at the root, board shares only show the board's own cards | Share rows AX-T012 to T047, `sharing/limits.rs` unit tests |

Milestone 09's last access criterion now passes too: `recordings.list` and `recordings.get` keep to a share's range and pinned filter, and the share viewer lists and plays replays only with the recordings grant.

## Fixture changes

- 830 rows added: grid rows for the 68 protected operations that had none, and 21 targeted rows (invitations, 2FA reenrollment, hidden workspaces, board-share reach, `users.resetCredentials`). New rule `R-INVITE`.
- Corrected contradictions: AX-G047 now denies `fields.values` on an ungranted property (as T032 and T033 do), and AX-G257 to G259 answer `not_found` for a key naming an invisible site (as T059 and R-KEY-CEILING do). Seven rows had `kind` wrong.
- AX-T143 runs with a new state `only_install_admin`, since the principals include two install admins.

## Browser evidence

All specs pass on fresh seed data: access 9 of 9, behavioral, exploration and recordings 19 of 19, retention-revenue and shell 15 of 15. 61 screenshots in [`m10/`](m10/) at 1440 and 390 px, light and dark, including share replays and board shares. The run found and fixed four client bugs: the password prompt behind an open dialog, the app shell's 401s sending share viewers to sign-in, the share page ignoring a theme switch, and the share replay page showing a session link.

## Not verified

- `sites.reset` removes no data yet. It passes every access check and answers `internal`, because deleting analytical rows needs the ordered deletion of milestone 12 (CAP-OPS-06). `sites.delete` and `teams.delete` hide the data at once and leave unreachable analytical rows until then.
- Live share updates while new data arrives. The stream connects; nothing was collected during the run.
- Release build, reference host and browsers other than Chromium.

## Open points for Mike

Decided by Mike on 2026-10-06: every Proposed item of 0012 is adopted, replacing an enrolled second factor needs reauthentication, and owners may demote other owners. The last-owner guard stays ([0012](../decisions/0012-access-implementation.md), [0007](../decisions/0007-access-and-account-security.md)).

- The six Proposed items of 0012.
- Replacing an enrolled second factor (`twoFactor.begin` and `confirm`) needs no fresh login, so a stolen session could swap the factor. Recommendation: add it to the reauthentication list of 0007.
- Owners cannot demote other owners (R-RANK), so a second owner can only leave on their own.
- Rows marked `proposed (milestone 10)` follow the implementation where no record applies, for example share viewers may not call `sharing.list`.

## Handoff

- 11: credential and membership conversion. Imported keys are reissued; bcrypt hashes need a tested conversion; 2FA secrets need encryption under `secrets.key`.
- 12: back up `secrets.key` apart from the database, implement site data removal for `sites.reset`, `sites.delete` and `teams.delete`, and add `jobs.status` if `users.delete` becomes asynchronous.
