# Milestone 10: access and restricted sharing

## Outcome

Existing self-hosted operators can manage users, teams, site roles, 2FA, scoped keys, and restricted shared explorations through a complete Kit account experience.

## Dependencies and ownership

Requires the protected foundation in 03 and exploration in 06. Identity owns credentials, sessions, scopes, capability mapping, and revocation. Frontend owns Kit account/team/site/share screens. Integrate recording capabilities from 09 when available.

## Work

1. Complete account bootstrap, secure login/logout, session lifetime/revocation, credential change, and supported recovery paths.
2. Implement teams, membership and invitation workflow, site roles, and explicit capability checks for reports, exports, recordings, configuration, and deletion.
3. Implement 2FA enrollment, confirmation, challenge, recovery codes, disable/reenroll, and protected secret storage.
4. Implement scoped API keys with one-time disclosure, operation/site grants, revocation, and audit-safe metadata.
5. Implement shared explorations constrained to permitted sites, measures/fields, actions, and lifetime. Resolve effective share scope server-side on every read/export and live delivery.
6. Adapt Kit account resources, forms, validation, dialogs, tables, toasts, and AppUserMenu. Account links and identity stay out of the sidebar.
7. Clear browser query/live caches on logout, revocation, or scope changes. Protect sensitive operations with the settled reauthentication policy.

## Acceptance criteria

- The capability matrix has positive and negative cases for every protected surface, including guessed IDs, nested properties, exports, recordings, and subscriptions.
- Revoked members, keys, shares, or sessions lose ongoing access, including after reconnect.
- Command retries never re-disclose protected one-time secrets from durable response storage.
- 2FA and recovery cases pass; secrets are absent from logs and ordinary exports.
- Account actions work against the Rust API with accessible Kit forms and typed translated errors.
- No known default bootstrap password or unexpected external auth dependency exists.
- Sharing never broadens the underlying exploration or caller scope.

## Evidence and handoff

Publish permission cases, secret/revocation behavior, and integrated account browser workflows. State how offline operators issue/accept invitations; an external email service is not a production requirement. Hand credential and membership conversion rules to 11.
