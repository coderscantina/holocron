# 0013: Historical import from Umami

Status: Adopted (milestone 11, team decision) unless an item says otherwise. [0016](0016-configurable-retention.md) replaces the fixed 12-month import cutoff with `--retention` (default `forever`) and tolerates MySQL column types of the same family. [0021](0021-web-setup-and-onboarding.md) runs the import inside `serve` in setup mode, resumes it there after a crash, and warns about a source off private networks read without TLS ([amendment](#amended-by-0021-import-inside-serve-and-source-tls)). Items Mike adopted on 2026-10-06 say so. Items marked Confirmed restate the briefing or a decision Mike made. Evidence: [m11 historical import and cutover](../evidence/m11-historical-import-and-cutover.md).

## Problem

Milestone 11 has to bring an existing Umami installation into a new Holocron data directory: accounts, configuration, history and recordings, from two schema versions on two engines, without changing the source. An import of a large installation runs for a long time, so it has to survive interruption without duplicating anything, and the operator needs proof of what arrived and what did not.

## Constraints

Confirmed: only PostgreSQL Umami 3.4.0 and MySQL Umami 2.20.2; read-only source connections and consistent snapshots; a new data directory; stable website UUIDs; source identifiers kept as restricted provenance; credentials preserved through tested conversion paths, logins and API keys reissued, 2FA handled explicitly; supported definitions mapped, everything else inventoried with its representation and a reason; deterministic, resumable, reconciled jobs; recomputed sessions with visible limitations; no invented identity continuity; public shares become restricted shares that keep answering at their old URLs (Mike, 2026-10-04); the 12-month retention cutoff applies to imports and the preview shows the skipped count ([0006](0006-retention-clock-and-operations.md)).

## Evidence

- `fixtures/import-schemas/` holds the two accepted layouts and ten refusal layouts captured with the pinned Umami images, with expected codes and messages.
- Umami hashes passwords with bcryptjs, cost 10 (`src/lib/password.ts`). It encrypts TOTP secrets with AES-256-GCM under `TWO_FACTOR_ENCRYPTION_KEY` as `ciphertext:iv:tag` hex (`src/lib/two-factor/crypto.ts`). otplib's defaults (SHA-1, 6 digits, 30 seconds) are the parameters Holocron's TOTP uses ([0012](0012-access-implementation.md)). Recovery codes are bcrypt hashes of a different format than Holocron's.
- Umami serves 3.x shares at `/share/<slug>/[[...path]]` and 2.x website shares at `/share/<share_id>/<website name>`. Slugs default to 16 random characters (`src/app/api/share/route.ts`).
- Umami stores replay chunks as gzip-compressed JSON arrays of rrweb events (`src/queries/sql/replays/saveRecording.ts`).
- A Umami session ID is a hash of website, IP, user agent and a monthly salt. Visit IDs on MySQL were backfilled by migration `05_add_visit_id`, so they do not mark inactivity sessions.

## Choice

### Inspection

- The `_prisma_migrations` rows and the column catalog must match the embedded snapshot of the fixture exactly, checked in the fixture README's order: MySQL claiming a 3.x migration (`unsupported_engine`), a failed or rolled-back migration (`migration_failed`), PostgreSQL with exactly the 13 shared 2.x migrations (`unsupported_pairing`), missing, unknown and modified migrations, then missing tables, missing columns and changed types or nullability. A database without the migration table is `not_umami`. Every message names what failed, says how to fix it, and ends with "Holocron did not change the source."
- Credentials that can insert, update, delete, truncate or alter any source table get the warning `credentials_writable` with the SQL to create a read-only user, and the import proceeds (Mike, 2026-10-23; until then they were refused with `credentials_not_read_only`). The session is read only either way (`default_transaction_read_only` on PostgreSQL, `SET SESSION TRANSACTION READ ONLY` on MySQL).
- `holocron import inspect` also previews the import: row counts per table, events in and outside the retention cutoff, the newest and oldest event, and the source size.

### Snapshot, destination and run state

- A run reads in one repeatable-read, read-only transaction. A fingerprint of every table's row count and the newest event is stored at the first start; a resume against a source with another fingerprint is refused with `source_changed`. Cutover stops source collection first, so the snapshot is still.
- The destination is a directory made by `holocron init` without sites or analytics (`destination_not_new` otherwise). The init admin keeps fresh credentials and is not from the source.
- The run's clock is pinned at its first start. Every retention cutoff, share expiry and resume uses it.
- State lives in the control database (`0008_imports.sql`): `import_runs` (one row, `running` or `complete`, source, fingerprint, options, clock, manifest), `import_steps` (checkpoint per step) and `import_inventory`. Operator jobs refuse to run while a run is `running` (`import_in_progress`, exit 3). `serve` starts in setup mode instead (amended by [0021](0021-web-setup-and-onboarding.md), see below).

### Writing and resuming

- Steps run in order: `control`, `events`, `identify`, `heatmaps`, `session_links`, `replays`, then correction and reconciliation. A step that is done is skipped on resume.
- `control` writes accounts and configuration in one control-database transaction with its inventory and checkpoint.
- Timestamped facts go in chunks of `--chunk-rows` source rows (default 5000) in primary key order. A chunk enters the inbox through `Storage::admit_import` under the key `<step>:<n>`, with a row in the inbox's `import_chunks` ledger in the same SQLite transaction (`crates/storage/migrations/0003_import_chunks.sql`). A key seen before is not admitted again; the same key with another record count is refused. The checkpoint then moves the cursor. A crash between the two repeats the chunk under the same key. Transfer applies imported records exactly like collected ones, with sequences in import order, so `(event_time, sequence)` is deterministic ([0002](0002-sessions-and-events.md)).
- Imports yield to the inbox budget: a chunk that would pass it waits for transfer to drain. The disk reserve applies as at admission. Memory is bounded by the chunk size.
- Replay parts go to `recordings.sqlite` through the same store collection uses, keyed by site, Umami visit ID and chunk index ([0011](0011-replay-store-and-isolation.md)). The store answers a repeated part as a duplicate.
- Correction runs once, after the last chunk, instead of after every batch.

### History and identity

- Event IDs, website IDs, link IDs and pixel IDs stay the source UUIDs. Event time is Umami's `created_at`, which is also used as receive time.
- Pageviews, custom events, performance, link and pixel hits map to their kinds. Properties go through the collection property rules, built from `event_data` with exact decimal text, so typing, limits and revenue rounding equal live collection. Revenue is the collection rule on `data.revenue` and `data.currency`, not the `revenue` table. Reconciliation compares both, which shows a MySQL source that skipped Umami's manual revenue data migration.
- `session_data` rows become identify records. Heatmap clicks and scroll depths become interactions by visit. `session_link` rows stay in the inventory.
- Imported events get a daily pseudonym derived from the Umami session ID and the local day, keyed by the installation root secret: "imported visitors" are Umami session IDs per day. They never get a persistent profile, so retention, cohorts and identified people exclude imported history ([0003](0003-metric-formulas.md) Retention). Sessions are recomputed from event times by the normal inactivity rule; visit IDs are not used. Adopted (Mike, 2026-10-06): this is the population the briefing calls imported visitors. Umami derives a session from IP, user agent and a monthly salt, so like a live daily pseudonym it stands for one browser on one day.
- Restricted provenance (source session, visit and distinct ID per event) goes to DuckDB `import_provenance`, which no report reads. Each chunk replaces its own rows, so a repeated chunk leaves no duplicate.
- Events, identify data and heatmaps before the retention cutoff are not imported and are counted per site. Unsaved replays older than 14 days are skipped; saved replays are imported and saved. Heatmaps older than 90 UTC days are skipped.
- `imported_history` in DuckDB records the span of imported events per site. Results whose range reaches it carry `freshness.imported`, and the dashboard shows "Includes Umami history through <date>" with the limitations.

### Accounts and access

- Users keep their UUIDs. The login is the username when it is an email address, otherwise `<username>@<login domain>` (Adopted (Mike, 2026-10-06): `--login-domain`, default `umami.invalid`). The manifest lists every login.
- Adopted (Mike, 2026-10-06): bcrypt hashes are kept, verified at login and replaced with argon2 at the first successful login. Umami passwords keep working without a reset. A hash in another format is replaced with an unusable one (`password_not_convertible`).
- Login sessions are never imported. API keys are inventoried (`api_key_reissue_required`) and reissued with scopes after cutover.
- With `--two-factor-key-file` (Umami's `TWO_FACTOR_ENCRYPTION_KEY`), confirmed TOTP secrets are decrypted and sealed under `secrets.key` ([0012](0012-access-implementation.md)), so authenticator apps keep working. Without the key, or when a secret does not decrypt, the account gets `two_factor_required` and must enroll at its first login. Recovery codes are inventoried. The installation, team and user 2FA requirements carry over.
- Roles map as `fixtures/access/role-mapping.json` says: `admin` becomes install admin, team roles become owner, admin, member and viewer. Personal sites, links, pixels and boards go into a personal workspace with an ID derived from the user ID; a view-only user only reads theirs. Lossy points are inventoried per object (`install_admin_reads_no_analytics`, `member_loses_site_delete`).

### Definitions

- Segments map to segment filters (dimension fields, string and number property filters, `match: any` to an OR group). Cohort segments, regular-expression and date operators are inventoried.
- Funnel, journey, retention, goal and attribution reports become saved explorations of the report's owner, checked by the planner, over the last 30 days. Fixed date ranges, journey start and end steps, the attribution currency and the description are listed as lost. Other report types are inventoried. Saved explorations are personal, while Umami reports were visible to everyone who could see the site (`visible_to_owner_only`).
- Boards keep name and workspace; their layout is inventoried (`partially_mapped: cards`). Annotations keep their time and text. Links and pixels keep slug and URL.

### Shares

- Confirmed (Mike, 2026-10-04): a public website share becomes a restricted exploration share, and a board share a board share, whose token is the old slug or `share_id`. The token is stored as its SHA-256 digest like every share token, so `/share/<slug>` opens it. The web app sends `/share/<token>/<anything>` to the share view and the server serves the app for share paths with dots, so 2.x links ending in a website name and 3.x links with a section path land there.
- Limits follow the shown sections: overview measures and fields always, events, UTM, revenue and performance fields when shown, up to 366 days, viewer filters allowed as upstream. Other sections are listed as lost. Link and pixel shares are inventoried.
- Adopted (Mike, 2026-10-06): imported shares expire 366 days after the import, the longest lifetime Holocron allows ([0012](0012-access-implementation.md) Shares). Old links stop working then unless an owner extends them.
- A slug shorter than Umami's 16 generated characters was chosen by hand and is inventoried as `share_slug_guessable`.

### Reconciliation

The manifest compares, per site, link and pixel: events by kind with their time ranges, properties, identify values, heatmap interactions and replay parts (stored plus inventoried), revenue per currency against the `revenue` table, and sessions and visitor-days in the destination. Every row of the configuration tables must be imported under its ID or inventoried. Fifty events are compared field by field with their source rows. Digests of events, properties, identify data, sessions, heatmaps and provenance identify the destination; a resumed run must reproduce them. `status` is `reconciled` only when nothing differs.

### Amended by 0021: import inside `serve` and source TLS

[0021](0021-web-setup-and-onboarding.md) adds the setup wizard as the second way to run an import. The mechanics above stay the same: the inbox ledger, checkpoints, the source fingerprint, the pinned clock and the digests.

- The wizard runs the import as a job inside `serve` in setup mode. Setup mode holds the data directory lock, collection answers 503 and no runtime or background job runs until the admin acknowledges the report. Then the same process serves normally.
- The admin step of the import path writes `holocron.setup-import` before the directory counts as initialized, so a restart before the import started comes back to the wizard. The marker goes when the report is acknowledged.
- `serve` on a directory with a `running` import, from the wizard or from a killed `holocron import run`, starts in setup mode and offers to resume. The admin signs in again with the password; the setup token is spent once an admin exists.
- The source URL and the Umami 2FA key are held in the job's memory only. Resuming asks for them again, and the fingerprint refuses a different source.
- Each committed checkpoint reports to a setup-only progress stream: rows read per step against the source table's row count at the first start, and an estimate from the rate of the last 30 seconds. The CLI runs without it and prints only the manifest.
- "Discard import and start over" rebuilds the directory around the install admin, its sessions and second factor, and `secrets.key`. Everything imported goes.
- Source TLS, for the CLI and the wizard alike: a source URL should ask for TLS (`sslmode=require` or stricter on PostgreSQL, `ssl-mode=REQUIRED` or stricter on MySQL) unless the host is a socket, loopback, a private or link-local address, or a single-label name such as a Docker service. Names are judged as written, not resolved, so `host.docker.internal` or a VPC-internal DNS name counts as public too. Otherwise the inspect report carries the warning `source_unencrypted`, the log repeats it, and the import proceeds (Mike, 2026-10-23; until then the source was refused with `source_tls_required`). The connector is rustls with the webpki roots; a private CA, such as Amazon RDS's, is passed as `sslrootcert` or `ssl-ca`.

## Rejected alternatives

- Writing DuckDB directly from the importer with its own ledger: it would duplicate the transfer protocol, sequences and dirty-day marking. The inbox path reuses all of it and adds one ledger table.
- A checkpoint that commits with the records: the inbox and the control database are separate files, and the control database cannot be part of the inbox transaction. The key ledger inside the inbox makes the crash window harmless instead.
- Visit IDs as sessions: MySQL backfilled them, and upstream sessions follow receive time ([0002](0002-sessions-and-events.md)).
- Distinct IDs as persistent profiles: they carry no consent signal, and 0001 forbids inferring continuity.
- Requiring a password reset for every account: it breaks every login at cutover although bcrypt verifies safely.
- Fresh share tokens: old links would stop at cutover, against Mike's decision.

## Consequences

- Milestone 12 must back up `import_*` tables, `import_chunks`, `import_provenance` and `imported_history` with the rest, and fence erasure and restore while `import_runs.state = 'running'`. Erasure of a visitor must reach `import_provenance` and the `session_link` rows in `import_inventory`.
- The dashboard shows imported history through `freshness.imported`. Other surfaces that display freshness can use it.
- Release notes must list the logins, 2FA and API key changes operators tell their users about.

## Affected contracts

[Analytics contract](../analytics-contract.md) imported identity; [access role mapping](../../fixtures/access/role-mapping.json) lossy points; [import fixtures](../../fixtures/import-schemas/README.md); protocol `Freshness.imported`; [0002](0002-sessions-and-events.md), [0006](0006-retention-clock-and-operations.md), [0011](0011-replay-store-and-isolation.md), [0012](0012-access-implementation.md); milestones 11 and 12.
