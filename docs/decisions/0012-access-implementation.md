# 0012: Access implementation

Status: Adopted (milestone 10, integration owner). Mike adopted every item that was Proposed on 2026-10-06 and added two rules: replacing an enrolled second factor needs reauthentication, and owners may demote other owners.

## Problem

[0007](0007-access-and-account-security.md) settles who may do what. Milestone 10 still needs to fix how API keys and share viewers present their credential, where the 2FA encryption key comes from, the TOTP and recovery code formats, how invitations work without an email service, how long sessions live, and how one-time secrets stay out of stored command answers.

## Constraints

Confirmed: scoped keys by operation and site, restricted shares reauthorized on every read, export and live frame, protected 2FA secret storage, no external email or auth dependency, no known default password, command retries never re-disclose one-time secrets (milestone 10 acceptance). The decision order in the [access fixture](../../fixtures/access/README.md) is binding, as are the envelope and idempotency rules of [0009](0009-protocol-envelope.md) and [0002](0002-sessions-and-events.md).

## Choice

### Credentials on the wire

| Principal | Credential | Dead credential |
| --- | --- | --- |
| Person | `holocron_session` cookie (HttpOnly, SameSite=Lax) | anonymous caller |
| Login challenge | `holocron_challenge` cookie, 5 minutes, 5 wrong codes | anonymous caller |
| API key | `Authorization: Bearer <secret>` | 401 `unauthenticated` for every operation |
| Share viewer | `Authorization: Share <token>` | 404 `not_found` for every operation |

- An `Authorization` header wins over the cookie. A signed-in person opening a share link reads as the share, never as themselves, and Vue Query caches share reads under the token.
- Commands sent with an API key skip the request marker and Origin check. A browser cannot attach `Authorization` cross-site without a CORS preflight, and the server answers none.
- `GET /api/subscribe` takes the same credentials. `EventSource` cannot set headers, so share pages read the stream with `fetch`. Tokens never appear in an API URL.
- The challenge after a correct password is a cookie, not a body field, so a stored `auth.login` answer holds no credential, as 0009 already does for sessions. The fixture's "partial_token present" is this cookie.
- Tokens, key secrets, invitation codes and recovery codes carry at least 80 random bits and are stored as SHA-256 digests, like session IDs. A lookup is one index probe.

Rejected: share routes bound to the path (`/api/share/<token>/queries/...`) put the token into access logs and add a second route shape. A share cookie collides with the person's cookie and cannot hold two shares in two tabs. A query parameter lands in logs.

### Registry metadata

Each operation declares in `domain!` whether an API key may hold it (`key`), what a share must grant to reach it (`share_read`, `share_export`, `share_recordings`), whether an enrollment-only session may call it (`enrollment`), whether it needs reauthentication (`reauth`, or `reauth_by_handler` for `members.updateRole`), and whether its result discloses a one-time secret (`secret`). The defaults are closed. The dispatcher applies key grants, share reach and enrollment before any resource lookup. Handlers call the reauthentication check after their capability checks; a `reauth` command that commits without it fails with `internal`, so a forgotten check cannot pass silently. The generated client gets `reauth` per command and the `keyGrantableCommands` and `keyGrantableQueries` lists.

### One-time secrets and idempotency

Results of `secret` operations carry `secret` and `secret_withheld`. The dispatcher stores the answer with `secret: null` and `secret_withheld: true`, so a replay returns the same key, share or invitation ID without the secret (R-IDEM-SECRET). A stored answer of a reauthenticated command is replayed only while the session's check is still fresh (AX-S025). Keys own their idempotency keys as `key:<id>`, apart from the owner's browser.

### Enrollment-only sessions

Not stored. Every request derives it from the installation, team and user requirements and the confirmed factor, so a requirement switched on applies at the next request and confirming enrollment upgrades the same session (0007).

Adopted (Mike, 2026-10-06): anonymous operations (`auth.session`, `auth.login`) also answer an enrollment-only session. 0007 lists only four operations; without `auth.session` the web app cannot learn that the session is enrollment-only.

### API keys

A key acts as its owner with the owner's current role, limited to its operations and to `all` or listed sites (R-KEY-CEILING, evaluated per request). Workspace-level resources (boards, members) are reachable only by `all`-site keys; a key limited to listed sites would otherwise read beyond them. A site transfer ends the site's key grants and shares.

Adopted (Mike, 2026-10-06): a key that reaches a site may follow its `analytics:<site>` watermark topic without an operation grant. The topic says only that newer data is visible. The fixture lists `realtime.site` as a grant; that topic does not exist yet and will need one.

### Shares

A share stores a copy of the shared state and its limits, so it survives edits to or deletion of the saved exploration and its author. Every share expires, at most 366 days ahead. Limits are named measures, dimensions, properties, `range_max_days` (current plus comparison range), a pinned filter and whether viewers may narrow. Outside a share every refusal is `not_found`, including a missing capability; widening inside it is `share_scope_exceeded`.

### 2FA secrets

- Encrypted with XChaCha20-Poly1305 under a 32-byte key in `<data-dir>/secrets.key` (mode 0600), created by `holocron init` or the first start, never stored in `control.sqlite`. Random 24-byte nonce per encryption; the user ID is associated data, so a ciphertext cannot be moved to another account.
- Losing `secrets.key` disables time-based codes for everyone. Recovery codes keep working, and an install admin can reset a factor. Backups (milestone 12) must carry the file separately from the database copy, or a leaked backup leaks the secrets.
- Rejected: deriving the key from the installation root secret, which lives in the same database; an environment variable as the only source, which container tooling prints and operators lose.

### TOTP and recovery codes

- RFC 6238 with HMAC-SHA1, 6 digits, 30 seconds, 160-bit secrets, one step of clock skew either way. Each step is accepted once per account. SHA1 because common authenticator apps ignore other algorithms; HMAC-SHA1 is not affected by SHA1 collisions.
- 10 recovery codes of 16 Crockford base32 characters (80 bits), shown as `xxxx-xxxx-xxxx-xxxx`, compared without spaces, dashes or case. Each works once; regeneration replaces the set.

### Invitations without email

`teams.invite` creates a single-use code for one role with a label and a lifetime of 1 to 30 days (7 by default) and discloses it once. The admin passes on the link `/teams/join?code=<code>` by any channel. A signed-in person redeems it with `teams.join`. Unknown, used, revoked and expired codes answer the same `not_found`.

Adopted (Mike, 2026-10-06): accounts are created only by install admins (`users.create`, generated one-time password), as upstream does. Invitations do not create accounts, so there is no self sign-up surface.

### Account recovery

Without email, recovery is the second factor's recovery codes and the install admin. Adopted (Mike, 2026-10-06): `users.resetCredentials` replaces a password with a generated one and/or removes the second factor, ends the user's sessions, and needs reauthentication. It is on 0007's reauthentication list.

### Sessions

Absolute lifetime 30 days (unchanged), idle timeout 7 days, `last_seen_at` written at most once a minute. A password change and `auth.revokeSessions` end every other session of the person; the current one stays (AX-S021). Reauthentication marks the session for 10 minutes (0007).

### Teams

Adopted (Mike, 2026-10-06): owners may make other owners (by role change or invitation), otherwise a team could never gain a second owner, and owners may demote other owners, an exception to R-RANK. The last owner cannot leave or be demoted, so no team is left ownerless.

Open for Mike: an owner still cannot remove another owner from the team (`members.remove` follows R-RANK). Demotion followed by removal reaches the same result in two steps.

## Consequences

- Agents implement against the declared operations in `crates/protocol/src/operations/{auth,account,two_factor,api_keys,users,security,teams,sharing,site_management}.rs` and migration `0007_access.sql`.
- The access fixture harness sends keys and shares as `Authorization` headers (`Harness::api_key`, `Harness::share`, `query_with`, `command_with`).
- `fixtures/access` args are mapped by the harness: `{"code": ...}` becomes `{"kind":"totp","code":...}`, key `sites` lists become `{"kind":"only","sites":[...]}`, `sites.transfer` takes `{"to":{"direction":...}}`.
- Milestone 12 backs up `secrets.key` and adds `jobs.status` if `users.delete` becomes asynchronous.

## Affected contracts

[Access fixture](../../fixtures/access/README.md); [capability matrix](../contracts/capability-matrix.md) CAP-SHARE-04 to 06, CAP-ACCESS-01 to 12; [0007](0007-access-and-account-security.md); [0009](0009-protocol-envelope.md) command order steps 1, 3 and 6; milestones 10, 11 and 12.
