# 0021: Web setup and onboarding

Status: Adopted by Mike on 2026-10-22, item by item on 2026-10-21. Variant A, the guest column, is the wizard's look. It amends [0013](0013-historical-import.md) (import inside `serve`, source TLS), [0006](0006-retention-clock-and-operations.md) (bootstrap credentials) and CAP-ACCESS-01 in the [capability matrix](../contracts/capability-matrix.md).

## Problem

A new install needs `holocron init` on the data volume before the first `serve`, and a move from Umami needs `holocron import run` with a database URL, both from a shell. On Coolify that means a one-off container on the right volume and network, and the admin password only appears once in its output. Self-hosters should finish setup in the browser instead, and the onboarding should carry them to a working first site or a reconciled import.

## Constraints

- Briefing: "Generate or require bootstrap credentials; do not ship a known default password."
- 0013: the import destination is a new directory without sites or analytics, the init admin has fresh credentials not taken from the source, and a run is deterministic and resumable.
- `serve`, `init`, `import run` and `restore` take the same exclusive data directory lock.
- Restore needs an empty target and brings its own accounts and `secrets.key`.

## Choice

### Setup mode

- `holocron serve` on an empty data directory starts in setup mode instead of exiting `not_initialized`. It serves the dashboard's `/setup` route, the setup operations and `/health`; every other dashboard route redirects to `/setup`.
- Collection answers 503 and stores nothing until setup finishes. Background jobs, summaries and the dashboard API stay off.
- `init_incomplete` and `restore_incomplete` directories also start in setup mode, which offers to repeat the interrupted step.

### Claiming the install

- At each boot in setup mode the server generates a one-time setup token and logs it. `HOLOCRON_SETUP_TOKEN` pins it for scripted installs.
- The wizard's first step asks for the token. Setup operations without it are refused. The token is spent once an admin exists or a restore finishes.
- Token checks follow the origin check of every command and are rate limited like sign-in.

### Wizard

1. Setup token.
2. Server check, warnings only: `HOLOCRON_PUBLIC_URL` against the browser's origin, the visitor address the server sees (a missing trusted proxy), free disk against the reserve, the version. Each warning names the variable that fixes it.
3. Path: Fresh start, Import from Umami or Restore a backup.
4. Fresh start and import create a fresh install admin and sign it in. Restore skips this step and ends at sign-in with the restored accounts.

### Fresh start

The admin adds the first site in the Default team (name, domain, timezone), sees the tracker snippet and a live "first pageview" check that turns green when the first hit arrives.

### Import from Umami

- The import runs inside `serve` as a setup phase, with 0013's mechanics unchanged: the inbox ledger, checkpoints, the source fingerprint and the pinned clock. When it finishes, the server leaves setup mode and serves normally without a restart. This replaces 0013's "`serve` refuses to start while a run is `running`": after a crash, `serve` starts in setup mode and offers to resume.
- The source URL is held in memory only. Resuming after a restart asks for it again; the fingerprint check refuses a different source.
- The importer gains TLS for PostgreSQL and MySQL. A URL that doesn't require TLS to a host other than loopback, a private range or a single-label name such as a Docker network host gets the warning `source_unencrypted` in the inspect report, the wizard and the log, and the import proceeds (Mike, 2026-10-23; it was refused before). The CLI follows the same rule.
- Before the run the wizard shows the inspect preview (tables, counts, oldest and newest event, what each retention choice leaves behind) or the refusal with its fix. Then the options: timezone (from the browser), retention, and the Umami 2FA key as a paste field held in memory. Login domain and chunk size sit under Advanced.
- Progress is per step, with rows done against the inspect total and an estimate from the recent rate, over a setup-only server-sent event stream. Reloading the page reattaches.
- After the run: reconciled or the differences, the logins with their password and 2FA outcome, what was not imported and why, and the manifest as a download. Then the cutover steps: tracker script name and endpoint variables, and a live "first hit" marker per site.
- "Discard import and start over", in setup mode only, removes the imported data and keeps the admin and `secrets.key`. It is confirmed by typing the instance name.
- "Continue without import" (Mike, 2026-10-23) sits below the source, options and report steps. It discards a started or finished import first, with the same confirmation, then ends setup like a fresh start and continues with the first site. Finishing setup is refused only while an import runs or stopped partway. A running import is not stopped from the wizard.

### Restore a backup

- The wizard lists the `holocron-backup-*` directories in the mounted backup directory (`/backups` in the compose files) with date and size, and takes any other path on the host. There is no browser upload.
- Setup mode's lock file does not count against restore's empty-target rule.
- A backup from an older release is upgraded right after the restore. Restore and upgrade report their phases on the same progress stream.

### CLI

`init`, `import` and `restore` stay and share their code with the web flow. Their behavior changes only by the source TLS warning.

## Rejected alternatives

- Bootstrap from environment variables only (`HOLOCRON_ADMIN_EMAIL` and a password): smallest change, but no onboarding, and a password in the environment.
- First visitor claims the install: anyone who finds a fresh URL first owns it.
- Run the import, then exit for the orchestrator to restart into `serve`: depends on the restart policy and splits one flow across two processes.
- Storing the source URL sealed under `secrets.key` for unattended resume: puts a credential to another system into the data directory and every backup.
- Accepting tracker hits during setup and applying them after the import: reopens 0013's new-destination rule and its ordering.
- Using the Umami admin as install admin: conflicts with 0013, and nobody would be signed in while the import runs.
- Browser upload of backups: several GB through the proxy needs resumable uploads and a staging area.

## Consequences

- Holocron's first public release waits for this work ([milestone 14](../milestones/14-web-setup-and-onboarding.md)).
- The install guide, the Umami migration guide and the runbook lead with the web path; the CLI stays the documented alternative.
- e2e gains setup specs against a separate server started on an empty data directory.

## Affected contracts

[0006](0006-retention-clock-and-operations.md), [0013](0013-historical-import.md), [capability matrix](../contracts/capability-matrix.md) CAP-ACCESS-01, protocol operations for setup, [design](../ux-v2/design.md) for the wizard, milestone 13's dependencies.
