# 0007: Access and account security

Status: Adopted (milestone 01, team decision). Items marked Confirmed restate [the briefing](../technical-briefing.md). This record settles open points of [the access fixture](../../fixtures/access/README.md) and the user erasure gap in [the gap fixtures](../../fixtures/gaps/README.md). Failed commands under an idempotency key are settled in [0002](0002-sessions-and-events.md) Request idempotency.

## Problem

Milestone 10 needs fixed rules for a user who must use 2FA but has not enrolled, for how a restricted share's limits treat comparison and recordings, for which commands need a fresh login, and for what deleting a user removes.

## Constraints

Confirmed: teams, site roles, 2FA, scoped API keys, restricted shared explorations; reauthorize exports, recordings, shared explorations and live subscriptions; handle 2FA secrets explicitly; scope keys to operations and sites; deletion is an ordered operation across both engines.

## Evidence

- Upstream 3.4.0 has 2FA requirement flags at installation, team and user level (`app_setting`, `two_factor_required`) and no defined state for an unenrolled user under a requirement ([capability matrix](../contracts/capability-matrix.md) CAP-ACCESS-04).
- Upstream shares are 14 section toggles with no lifetime or limits and never reach exports or replays (`fixtures/access/role-mapping.json`).
- Upstream `deleteUser` (`src/queries/prisma/user.ts:121`) deletes the user's personal websites with all their events and sessions, every team the user owns, and the links, pixels and boards of both. Cloud mode soft-deletes instead.
- The access fixture proposes enrollment-only sessions (AX-S020), counting the comparison window toward `range_max_days` (AX-T027), and playback without listing for a recordings grant (AX-T040).

## Choice

### Required 2FA without enrollment

A correct password for a user who must use 2FA but has not enrolled yields an enrollment-only session.

- Allowed: `twoFactor.begin`, `twoFactor.confirm`, `account.get`, `auth.logout`.
- Everything else: 403 `second_factor_enrollment_required`.
- After `twoFactor.confirm` the same session becomes a full session.
- When a requirement is switched on, existing sessions of unenrolled users drop to enrollment-only at their next request. No grace period: a grace period is a window in which the requirement is not met.
- API keys are separate scoped credentials and keep working; the requirement governs interactive login.

### Share limits and comparison

A share's limits bound every instant the viewer can read. The comparison window reads data, so the current range and the comparison range together must fit inside `range_max_days`. A 30-day share therefore allows 15 days with previous-period comparison, or 30 days without. A share owner who wants 30 days with comparison sets 60.

### Share recordings grant

`grants.recordings` allows listing and playing recordings that match the share's site, range limit and pinned filter. Without listing, a viewer has no way to reach a recording, since shares do not grant session lists. Recordings outside the share's state stay `not_found`.

### Install admin analytics access

`install.admin` manages users, teams, security policy and site metadata, and does not read analytics, recordings or exports of a site unless it is also a member of the site's workspace. An install admin who needs to look adds themselves to the workspace, which shows in the member list. Upstream admins read every site; this is the lossy mapping point 1 in `role-mapping.json`, kept on purpose because running the server is not the same as being allowed to read every customer's visitors.

### Member and site deletion

`member` keeps `site.configure` (settings, segments, annotations, shares) but loses `site.delete` and `site.reset`, which move to `admin` and `owner`. Irreversible data loss should not sit with the broadest editing role. Upstream members can delete sites; imported members lose that, and the import inventory says so.

### Own explorations

Visibility `own` means author only. Workspace admins and owners get `not_found` like anyone else; they can see that the author exists, not their private work. A private draft that admins can read would not be private. When the author is deleted, own explorations are deleted with them (User erasure below).

### Reauthentication for sensitive commands

These commands need a password check (and the second factor when enrolled) within the last 10 minutes of the current login session:

`account.changePassword`, `twoFactor.begin` and `twoFactor.confirm` when a second factor is already enrolled (replacing it, added 2026-10-06), `twoFactor.disable`, `twoFactor.regenerateRecoveryCodes`, `apiKeys.create`, `members.updateRole` when the new role is `admin` or `owner`, `sites.delete`, `sites.reset`, `sites.transfer`, `teams.delete`, `users.delete`, `users.resetCredentials` ([0012](0012-access-implementation.md)), `security.updatePolicy`.

- `auth.reauthenticate` performs the check and marks the session for 10 minutes.
- Without it: 403 `reauthentication_required`. The check comes after the capability check and before the revision and idempotency checks (access fixture decision order, between steps 5 and 6), so it never reveals whether a hidden resource exists.
- These operations can never be granted to an API key. Share viewers never reach them.
- The list covers account takeover, privilege raising, and irreversible data loss. Ordinary edits and reads do not need it.

### User erasure scope

`users.delete` is a durable job.

| Data | Effect |
| --- | --- |
| Account, email, name, password hash, preferences | deleted |
| Login sessions and subscriptions | ended at once; subscriptions get `access_revoked` |
| 2FA secrets and recovery codes | deleted |
| API keys | revoked at once, then deleted |
| The user's `own` explorations, pending invitations | deleted |
| Workspace explorations, boards, annotations, segments, goals, shares the user created | kept; author shown as "Deleted user" |
| Workspaces where the user is the only owner, personal sites | refused with 409 `user_owns_resources`, listing them, until they are transferred or deleted |
| The last install admin | refused with 409 `last_install_admin` |
| Consented visitor profiles | not affected; visitor data is not account data, and profile erasure is its own command ([0001](0001-identity-and-consent.md)) |

A tombstone keeps only the user ID and deletion time, so audit references stay resolvable. Upstream deletes a user's websites and owned teams with the user. Holocron refuses instead, because deleting analytics as a side effect of removing a login is the surprising outcome.

## Rejected alternatives

- Full access for unenrolled users until they choose to enroll: the requirement would mean nothing.
- Comparison outside the share's range limit: a share for 30 days would show 60.
- Reauthentication on every command: dashboards would prompt constantly, and people learn to type passwords on reflex.
- Deleting shared content with its author: boards and saved views of the whole team would disappear with one account.

## Consequences

- Access fixture: AX-S020 matches this record. AX-T027 stays a denial (30 days plus comparison under a 30-day limit). AX-T040 changes to allow listing within the share's state. Reauthentication and `users.delete` rows need adding.
- Access fixture rows for `p_install_admin`, member `site.delete`/`site.reset`, and admin access to `own` explorations lose their `open` notes: install admins are denied site analytics, members are denied deletion and reset (403 `forbidden`), admins get `not_found` for others' own explorations.
- CAP-OPS-07 has a rule source; milestone 12 tests erasure with the ordered deletion protocol.
- The error code list gains `reauthentication_required`, `user_owns_resources` and `last_install_admin`.

## Affected contracts

[Access fixture](../../fixtures/access/README.md) "Open points"; [capability matrix](../contracts/capability-matrix.md) CAP-ACCESS-03, CAP-ACCESS-04, CAP-SHARE-04 to 06, CAP-OPS-07; milestones 10 and 12.
