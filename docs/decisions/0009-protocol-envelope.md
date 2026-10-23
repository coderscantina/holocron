# 0009: Protocol envelope

Status: Adopted (milestone 03, team decision). It fixes the dashboard API shape that [architecture patterns](../contracts/architecture-patterns.md) describes and names the error codes milestone 01 left open.

## Problem

Every later milestone adds commands and queries. They need one envelope, one error body, one idempotency rule, fixed limits, and a way to keep Rust dispatch, TypeScript types and API documentation from drifting apart.

## Constraints

Confirmed: named commands and queries, a Rust registry as the single source, generated TypeScript and OpenAPI, typed errors, idempotency bound to principal, operation and argument fingerprint, expected revisions checked after scope resolution, origin protection on dashboard mutations only, no hand-maintained second schema.

## Evidence

- `fixtures/access/cases.json` proposes 15 error codes with HTTP statuses and a decision order. The 9 rows for implemented operations pass through the real dispatcher (`apps/server/tests/access.rs`).
- Scenarios AX-S001 to AX-S004, AX-S007, AX-S008, AX-S018 and AX-S024 run as HTTP tests (`apps/server/tests/commands.rs`, `access.rs`).
- Decision 0004 names the exploration refusals; 0006 names `retention_start` and `compare_retention_start` for clipped bounds.

## Choice

### Registry

Each domain declares its operations in `crates/protocol/src/operations/<domain>.rs` with the `domain!` macro: name, access (`Anonymous` or `Person`), argument type, result type, and a doc comment. The macro generates a handler trait with one method per operation and a dispatcher. `operations/mod.rs` merges the domains with one line each. The server implements the traits, so a missing handler or a changed type is a compile error, not a runtime 404.

Operation names are dotted, as in the access fixture: `sites.update`, `auth.login`, `workspaces.list`.

### Envelope

| Kind | Request | Answer |
| --- | --- | --- |
| Command | `POST /api/commands/<name>`, JSON body `CommandKey & Args` | `CommandMeta & Result`: `{ revision, ...result }` |
| Query | `GET /api/queries/<name>?<args>` | `SnapshotMeta & Result`: `{ revision, cursor, ...result }` |
| Error | any | `ApiErrorBody`: `{ error_code, message, errors?, current? }` |

- Wire conventions: snake_case fields, Unix millisecond timestamps, string IDs, absent optional fields left out. `i64` exports as `number`; revisions and timestamps stay below 2^53.
- Query arguments are query-string values. Arrays repeat the key; absent values are left out.
- `revision` of a command is the sequence of the last topic event it recorded, else the head of `installation`. `revision` of a query is the head of the topic the handler names, read in the same read transaction as the data. `cursor` is `<topic>@<revision>` and opaque to the browser.
- Results are always JSON objects so the envelope fields merge into them.

### Command order

1. Request marker header `x-holocron-request: 1` and an `Origin` equal to `HOLOCRON_PUBLIC_URL`'s origin, or to `http(s)://<Host>` when it is not set. Otherwise 403 `origin_rejected`. Browser collection routes (milestone 04) do not pass through this check.
2. Look up the name. Unknown answers 404 `not_found`, byte-identical to a hidden resource.
3. Resolve the caller from the session cookie. A `Person` operation without a session answers 401 `unauthenticated`.
4. Read at most 64 KiB of body (413 `payload_too_large`). Split `idempotency_key` (1 to 128 characters) from the arguments; a missing key or unparsable body is 400 `malformed_request`.
5. `BEGIN IMMEDIATE` on the single writer connection.
6. Look up the key for this owner (`user:<id>`, or `addr:<ip>` for anonymous callers) within 24 hours. A different operation or argument fingerprint is 409 `idempotency_conflict`. A match is authorized again: every site scope the original resolved is resolved again with the current membership and capability, and a refusal answers as it would for a fresh call. Then the stored answer is returned unchanged.
7. Run the handler. Scope resolution comes first (404 `not_found` for hidden, 403 `forbidden` for a missing capability), then `expected_revision` (409 `stale_revision` with `current`). Field validation answers 422 `invalid_arguments` with `errors` keyed by argument.
8. Compute the revision, store the answer with the resolved scopes, commit. A failed handler rolls back and stores nothing.
9. After commit, apply session cookie changes.

The argument fingerprint is SHA-256 over the arguments with object keys sorted.

### Session commands

`auth.login` and `auth.logout` follow the same order. The session token travels only in `Set-Cookie` (HttpOnly, SameSite=Lax, Secure when the public URL is https), so a stored login answer contains no secret. A replayed login returns the stored account without issuing a second session; the client then sees `account: null` from `auth.session` and signs in again. Wrong password and unknown email answer the same 401 `invalid_credentials`, with an argon2 check against a decoy hash for unknown emails. Logins are limited to 10 per client address per minute (429 `rate_limited`).

### Cancellation

The browser passes an `AbortSignal` to `fetch`. On the server a dropped request drops the handler future, which rolls back an open transaction. Queries are cancelled after 30 seconds with 503 `query_timeout`. A command that committed before the client gave up is safe to retry with the same key.

### Limits

Generated into `apps/web/src/generated/limits.ts` from `crates/protocol/src/limits.rs`: idempotency key 128 characters and 24 hours, command body 64 KiB, query string 8 KiB (the 0004 URL limit), request budget 30 s, page size 25 by default and 100 at most, site name 100 characters, domain 253 characters, 10 logins per address per minute.

### Error codes

`unauthenticated` 401, `invalid_credentials` 401, `second_factor_required` 401, `second_factor_invalid` 401, `second_factor_enrollment_required` 403, `reauthentication_required` 403, `not_found` 404, `forbidden` 403, `operation_not_granted` 403, `grant_exceeds_principal` 403, `share_scope_exceeded` 403, `origin_rejected` 403, `stale_revision` 409, `idempotency_conflict` 409, `user_owns_resources` 409, `last_install_admin` 409, `malformed_request` 400, `invalid_arguments` 422, `duplicate_predicate_id` 422, `filter_too_deep` 422, `too_many_conditions` 422, `page_out_of_range` 422, `report_missing` 422, `report_invalid` 422, `report_field_unsupported` 422, `report_version_unsupported` 422, `report_too_large` 422, `payload_too_large` 413, `rate_limited` 429, `query_timeout` 503, `internal` 500.

The access fixture's 15 codes and statuses are kept unchanged. Field error codes are `required`, `too_short`, `too_long`, `too_many`, `invalid_format` and `invalid_value`, with an optional `limit`. Exploration limits that 0004 leaves unnamed (list items, dimensions, measures, relative range, series points) answer `invalid_arguments` with the field path and `too_many` or `invalid_value`.

Milestone 01 open items closed here:

- Duplicate predicate ID: `duplicate_predicate_id`, 422.
- Retention-clipped bounds: `retention_start` and `compare_retention_start` in the resolved bounds object, as 0006 and the workload contract already use them. They are fields, not errors.

### Generation and drift

`cargo run -p holocron-protocol --bin export-types` writes one TypeScript file per wire type (ts-rs 12), `commands.ts`, `queries.ts`, `errors.ts`, `limits.ts` and `index.ts` into `apps/web/src/generated`, and the OpenAPI 3.1 document to `crates/protocol/openapi.json` (schemas from schemars 1.2). Output is sorted, so an unchanged registry changes no byte. `-- --check` regenerates into a scratch directory and exits 1 on any difference; `cargo test -p holocron-protocol` runs the same comparison.

## Rejected alternatives

- A hand-written `match` in the server plus a test comparing names with the registry: the compiler already does it through the traits.
- Separate REST routes per resource: two shapes to document and protect.
- Storing the session token in the idempotency answer: a replay would hand out a credential.
- `utoipa` for OpenAPI: a second set of annotations next to the ts-rs ones. schemars reads the same serde attributes.
- A 405 for wrong methods on API paths: an unknown method answers `not_found` like an unknown route, so the API has one miss shape.

## Consequences

- New operations add entries to a domain file, implement one trait method, and regenerate. The drift check fails CI otherwise.
- Milestone 10 adds API keys, shares, 2FA and reauthentication into steps 3 and 7 without changing the envelope.
- The browser collection surface (milestone 04) keeps upstream's wire format and is not routed through this dispatcher.

## Affected contracts

[Architecture patterns](../contracts/architecture-patterns.md); [access fixture](../../fixtures/access/README.md) error codes; [0004](0004-exploration-state-and-limits.md) limits; [0006](0006-retention-clock-and-operations.md) retention bounds; milestones 04 to 10.
