# Access fixture

Milestone 01, closes the access and sharing gaps in the [capability matrix](../../docs/contracts/capability-matrix.md) (CAP-SHARE-04 to 06, CAP-ACCESS-01 to 12). Each row's `status` field says where its behavior comes from: `adopted (<decision record>, <section>)` for rows settled by [0007](../../docs/decisions/0007-access-and-account-security.md) or [0002](../../docs/decisions/0002-sessions-and-events.md), a quoted sentence of the [technical briefing](../../docs/technical-briefing.md), or `proposed (milestone 01)` for the rest. Operation names come from the matrix. Error codes and HTTP statuses are proposals; milestone 03 fixes the registry and may rename them, in which case this fixture is updated in the same change.

## Files

| File | Content |
| --- | --- |
| `principals.json` | 20 entries: users per role, an install admin who is also a member, three API keys, five share viewers, signed-out, one user removed mid-session, two 2FA users, one `users.delete` target. `p_removed` and `p_install_admin` have named states |
| `resources.json` | 2 workspaces, 3 sites, 1 saved exploration, 2 boards, 1 goal, 1 segment, 1 annotation, 1 recalculation, 3 invitations, 2 recordings, 5 shares, 3 keys, 1 collection key, 1 link, 1 pixel, and ids that never existed |
| `role-mapping.json` | Umami 3.4.0 role to Holocron role mapping with source lines, capability sets, and every lossy point |
| `cases.json` | 1298 rows: 1121 grid (94 operations x 12 principals, without the user roles for `twoFactor.regenerateRecoveryCodes`) and 177 targeted, plus the rule list and error codes |
| `scenarios.json` | 28 ordered scenarios (106 steps, 86 of them operations) for idempotency, revisions, revocation, subscriptions, 2FA, sessions, reauthentication, bootstrap, user erasure, owner demotion |

## Row format

A grid or targeted row in `cases.json`:

```json
{"id":"AX-G172","principal":"p_viewer","op":"sites.update","kind":"command","resource":"site_a1",
 "expect":{"result":"deny","status":403,"code":"forbidden"},"rule":"R-CAP","status":"proposed (milestone 01)"}
```

- `principal`, `resource`: aliases defined in the other files. `state` selects a principal state (`p_removed` after removal, `p_install_admin` as the only install admin).
- `kind`: `query` is `GET /api/queries/{op}`, `command` is `POST /api/commands/{op}`.
- `args`: only what matters for the decision. Strings like `"pinned, relative 90 days"` describe a state a harness builds from the pinned state of `exp_a1`.
- `expect.result`: `allow`, or `deny` with `status` and `code`.
- `rule`: id from the `rules` list, which gives the behaviour in one sentence.
- `same_response_group`: all rows with the same group must return identical status, code and body, request id excluded.
- `session`: login-session state for the reauthentication rule (`not reauthenticated`, `reauthenticated 9 minutes ago`). Rows without it run in a freshly reauthenticated session.
- `note`: reasoning or where upstream differs.

Scenario steps run in order against one fresh seed. `actor` is a principal (or `partial_token` for the token after a password step), `idempotency_key` names a key, `event` changes state outside the call, `expect_stream` describes what a subscription sees.

## Principals

| Principal | Meaning |
| --- | --- |
| `p_owner`, `p_admin`, `p_member`, `p_viewer` | Owner, team admin, team member, view-only in `ws_a` |
| `p_other_owner` | Owner of `ws_b`, outsider to `ws_a` |
| `p_install_admin` | Installation admin with no workspace membership |
| `p_install_admin_member` | Installation admin who also joined `ws_a` as member |
| `p_removed` | Member of `ws_a` until the scenario removes them. States `before_removal` and `after_removal` |
| `p_key_site` | Key of a member, five query operations plus `realtime.site`, `site_a1` only |
| `p_key_readonly` | Key of an admin, every query, no commands, all sites the owner reads |
| `p_key_revoked` | Revoked key |
| `p_share_viewer` | Viewer of `share_a1`: `exp_a1` on `site_a1`, two measures, two dimensions, 30 days, pinned filter, no recordings, no export |
| `p_share_viewer_rec` | Same share with recordings granted |
| `p_share_revoked`, `p_share_expired` | Dead tokens |
| `p_share_board` | Viewer of a board share limited to `site_a1` |
| `p_signed_out` | No credential |
| `p_2fa_enrolled`, `p_2fa_unenrolled` | Login, reauthentication and enrollment-only scenarios |
| `u_leaver` | Target of `users.delete` rows, owns nothing |

## Decision order

Every denial follows one order, so two different reasons cannot be told apart by an outsider.

1. No valid credential: 401 `unauthenticated`. A revoked key looks like an unknown key.
2. Key: the operation must be in the key's grants, checked before any resource lookup (403 `operation_not_granted`). Then the resource must be inside the key's sites.
3. Share: anything outside the share's target, operations and actions is `not_found`. Inside it, a request that widens the share's range, filter, measures or fields is 403 `share_scope_exceeded`.
4. User: no membership in the resource's workspace, a missing resource, or a hidden resource is 404 `not_found`, identical for all three.
5. Visible but missing capability: 403 `forbidden`.
6. Sensitive commands need a login check within 10 minutes: 403 `reauthentication_required` ([0007](../../docs/decisions/0007-access-and-account-security.md)). It comes after the capability check, so it never reveals a hidden resource.
7. Then the expected revision (409 `stale_revision`) and the idempotency check against stored results, which is itself reauthorized first. Only committed results are stored under a key; a failed command stores nothing ([0002](../../docs/decisions/0002-sessions-and-events.md)).

## Using it

**Milestone 03.** Load `resources.json` and `principals.json` through a seed function, then run `cases.json` through the real HTTP dispatcher and compare status, code and body. Include the `same_response_group` byte comparison. Additionally call the SiteScope constructor directly for each site-scoped row: constructing a scope for a hidden site must fail with the same error kind as a missing site. The guessed-ID and cross-workspace rows (`hide-site`, `hide-board`) are the M03 acceptance cases. M03 can ship with the grid rows for the operations that exist and mark the rest pending by operation name; it must not edit expectations.

**Milestone 10.** Run all of it. The acceptance rule "positive and negative cases for every protected surface" is checked by a test that lists the generated operation registry and fails when an operation has no row in `cases.json` for each principal class (user roles, key, share, signed-out), or no allowed and no denied row. `apps/server/tests/access.rs` runs every row and both checks. Rows for operations the registry does not declare yet (`sessions.list`) are reported as pending. Allowed `sites.reset` rows pass every access check and then meet the handler's "not implemented" until milestone 12 adds the ordered deletion protocol. `scenarios.json` covers retries, revocation after reconnect, 2FA and bootstrap. Browser tests reuse the same principals for cache clearing on logout and revocation (no fixture rows; they check the client).

**Later milestones.** A new protected operation adds one grid row per principal. A new recording or export surface adds rows for `p_share_viewer`, `p_share_viewer_rec` and `p_key_site`.

## Role mapping

See `role-mapping.json` for source lines. Short version:

| Umami 3.4.0 | Holocron |
| --- | --- |
| team-owner | owner |
| team-manager | admin |
| team-member | member |
| team-view-only | viewer |
| admin (installation) | install_admin |
| user, view-only (installation) | user |

Where the mapping is lossy:

1. Upstream `admin` reads every site. Holocron install admins read no analytics unless they are workspace members ([0007](../../docs/decisions/0007-access-and-account-security.md), Install admin analytics access).
2. Upstream team members can delete sites. Holocron members lose `site.delete` and `site.reset`; they keep `site.configure` ([0007](../../docs/decisions/0007-access-and-account-security.md), Member and site deletion).
3. One upstream check (`canUpdateWebsite`) covers settings, segments, annotations and shares. Holocron splits it into four capabilities that member holds together.
4. Upstream view-only users can still edit personal sites they own. Holocron makes them read-only.
5. Upstream API keys are the whole user. Holocron keys are scoped and non-mintable, imported keys are reissued.
6. Upstream shares are 14 section toggles with no lifetime or limits and never reach exports or replays. They do not map to Holocron shares.
7. Upstream reports are visible to every site viewer. Holocron `own` explorations are author-only, admins included.
8. Upstream role change checks the target's rank but not the new role. Holocron checks both.
9. `site.transfer` is one capability; the to-user direction is owner-only.

## Settled by decision records

The earlier open readings are settled by [0007](../../docs/decisions/0007-access-and-account-security.md): install admins read no analytics, members lose `site.delete` and `site.reset`, own explorations are author-only, a share's comparison window counts toward `range_max_days`, a recordings grant includes listing, required-but-unenrolled users get an enrollment-only session, eleven commands need reauthentication within 10 minutes, and `users.delete` refuses while the user owns resources. Failed commands are not stored under an idempotency key ([0002](../../docs/decisions/0002-sessions-and-events.md)).

Milestone 10 added grid rows for every protected operation of the registry that had none, targeted rows for invitation codes, invitation rank, 2FA reenrollment, hidden workspaces and nested card sites, and the rule `R-INVITE`. Rows whose behavior no record settles say `proposed (milestone 10)`.

Mike approved two access rules on 2026-10-06. Replacing a confirmed second factor (`twoFactor.begin` and `twoFactor.confirm` while enrolled) needs reauthentication; a first enrollment does not (AX-T169 to AX-T175). Owners may demote other owners; nobody changes their own role and the last owner cannot leave, so a team keeps an owner (AX-T176, AX-T177, AX-S028).

Still unwritten rows: link and pixel shares (no operation exists yet), and the `operations.status` and `jobs.status` steps of AX-S016 and AX-S026 (no topic or operation exists yet).
