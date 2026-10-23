# Import schema fixtures

Schema snapshots of the two Umami layouts the Holocron importer accepts, plus broken variants it must refuse. Captured on 2026-10-03 for milestone 01, work item 6. Milestone 11 builds the read-only inspection against these files.

## Supported sources

| Source | Engine | Migrations | Tables | Upstream |
| --- | --- | --- | --- | --- |
| `postgresql-3.4.0/` | PostgreSQL 15 | 26 | 26 | `.sources/umami-3.4.0` (commit ec0ff50388c264ed8ce46f00967e92f7e71476ae), `prisma/schema.prisma`, `prisma/migrations/` |
| `mysql-2.20.2/` | MySQL 8.0 | 13 | 11 | `.sources/umami-2.20.2` (commit 3eb2e890efb5b18bdfbeab46b9946f48e38da424), `db/mysql/schema.prisma`, `db/mysql/migrations/` |

Umami 3.4.0 supports PostgreSQL only: `prisma/schema.prisma` declares `provider = "postgresql"`, `prisma/migrations/migration_lock.toml` says the same, `scripts/check-db.js` uses `@prisma/adapter-pg`, and the README lists only a PostgreSQL requirement. There is no MySQL 3.x schema to capture.

Both schemas use `relationMode = "prisma"` (`prisma/schema.prisma` line 9, `db/mysql/schema.prisma` line 8), so MySQL has no foreign keys. PostgreSQL 3.4.0 has one, `api_key_user_id_fkey`, from `prisma/migrations/26_add_api_key/migration.sql`.

## Files per snapshot

- `schema.sql`: schema-only DDL. `pg_dump --schema-only --no-owner` (with the random `\restrict` lines removed) or `mysqldump --no-data --skip-comments --no-tablespaces` (with `AUTO_INCREMENT=n` removed). Includes the `_prisma_migrations` table.
- `migrations.json`: the `_prisma_migrations` rows, sorted by name. Keys: `migration_name`, `checksum`, `applied_steps_count`, `logs`, `finished_at`, `rolled_back_at`. The random `id` and `started_at` are dropped. `finished_at` and `rolled_back_at` are normalized to `"<set>"` or `null`.
- `columns.json`: column inventory, `{engine, tables: [{name, columns, indexes, foreignKeys}]}`, tables sorted by name, columns by `position`. Column keys: `name`, `position`, `type`, `nullable`, `default` (MySQL adds `extra`). Index keys: `name`, `unique`, `primary`, `columns`. Foreign key keys: `name`, `columns`, `refTable`, `refColumns`, `onDelete`, `onUpdate`. MySQL is read from `information_schema`. PostgreSQL columns, indexes and keys come from `pg_catalog` (`format_type`, `pg_index`, `pg_constraint`) because `information_schema` has no usable index data and lacks exact types. PostgreSQL FK actions are single letters (`c` cascade). `_prisma_migrations` is left out.
- `sample-data.sql`: a handful of synthetic rows (2 users, 1 team, 2 websites, 2 sessions, events, event data, session data, revenue, report, segment) that load into `schema.sql`. PostgreSQL also covers board, annotation, link, pixel, share, API key, 2FA, session link, replay and heatmap rows. MySQL 2.20.2 has no such tables. All values are fake.
- `expected.json`: the inspection outcome (`accept` here).

## How the snapshots were produced

`tools/import-schemas/capture.sh` does everything below. It starts `tools/import-schemas/compose.yaml` (project `holocron-m01-schemas`, no host ports), applies the migrations, dumps, and tears the project down with `docker compose down -v`.

Migrations were applied with the official Umami images, running the command the images run on start (`scripts/check-db.js` in 3.4.0 runs `prisma migrate deploy`): `node_modules/.bin/prisma migrate deploy` with `DATABASE_URL` set. Prisma writes the real names and checksums. For partial cases a subset of the upstream migration directories is mounted over `/app/prisma/migrations`. Checksums equal the SHA-256 of each upstream `migration.sql` (checked for 01, 13, 26 on PostgreSQL and 13 on MySQL).

Images, all pinned by digest:

| Role | Image |
| --- | --- |
| PostgreSQL 15 | `postgres:15-alpine@sha256:f7d23353e1b15400d22ebe31189f4d314b87a4c129cc400c8c2d8d4ca127bf81` (as in `.sources/umami-3.4.0/docker-compose.yml`) |
| MySQL 8.0 | `mysql:8.0@sha256:7dcddc01f13bab2f15cde676d44d01f61fc9f99fe7785e86196dfc07d358ae2b` (`.sources/umami-2.20.2/README.md` line 39 names 8.0 as the minimum; the upstream compose files only use PostgreSQL) |
| Umami 3.4.0 | `ghcr.io/umami-software/umami:3.4.0@sha256:85909afc45bdcda1917394594a087421fdbb05610fded0fa9f6fb861abb2f367` |
| Umami 2.20.2 MySQL | `ghcr.io/umami-software/umami:mysql-v2.20.2@sha256:56cc5167a987060b982e4ad13ca86342bc8fb0012fe88a2b59f8b68c071545aa` |
| Umami 2.20.2 PostgreSQL | `ghcr.io/umami-software/umami:postgresql-v2.20.2@sha256:950133e4158949061b84fa29ce1a87f734655dca5a49a08e727eb23c4e3582cd` |

`tools/import-schemas/verify-samples.sh` loads each `schema.sql` and `sample-data.sql` into a fresh database in fresh containers. Both load without errors.

## What the importer must check

Proposed rules for milestone 11, in this order:

1. Engine and pairing. PostgreSQL is accepted only at the 3.4.0 signature, MySQL only at the 2.20.2 signature.
2. Migration state. Compare `_prisma_migrations` with `migrations.json` of the matching snapshot, by name and checksum. Every expected name must be present with a matching checksum, `finished_at` set, and `rolled_back_at` null. A failed row refuses before any missing or unknown check. Any name outside the list refuses.
3. Required tables and columns. Every table and column in `columns.json` must exist with the same type and nullability. Extra tables and columns are not refused. This catches changes made by hand after migration.

The migration table is the only version signal. Two limits follow:

- The first 13 PostgreSQL migrations of Umami 2.20.2 (`.sources/umami-2.20.2/db/postgresql/migrations`) have the same names and checksums as migrations 01 to 13 of 3.4.0. A PostgreSQL 2.20.2 database cannot be told apart from a 3.x upgrade that stopped after migration 13. Both get the `unsupported_pairing` message.
- Umami's data migrations (`.sources/umami-2.20.2/scripts/data-migrations/populate-revenue-table.sql`, `convert-utm-clid-columns.sql`) are run by hand and leave no `_prisma_migrations` row. A source that skipped them passes inspection, and the `revenue` table can be empty while `event_data` holds revenue values. Reconciliation in milestone 11 must account for this.

## Refusal cases

Each directory under `refusal/` has `schema.sql`, `migrations.json`, `columns.json` (same format as above) and `expected.json` with `outcome`, `code`, `details` and the operator `message`. Cases a to f are real databases made with the capture script and the edit listed. Case g is hand-built.

| Directory | Edit to the real snapshot | Code |
| --- | --- | --- |
| `a-postgresql-3-last-migration-missing` | Migrations 01 to 25 only. No `api_key` table. | `migration_missing` |
| `b1-postgresql-3-failed-migration` | Same schema as a, plus a `26_add_api_key` row with `finished_at` null and a synthetic log. Prisma leaves such a row when a migration fails. | `migration_failed` |
| `b2-postgresql-3-rolled-back-migration` | Same as b1, with `rolled_back_at` set (what `prisma migrate resolve --rolled-back` does). | `migration_failed` |
| `c1-mysql-2-before-visit-id` | Migrations 01 to 04 only. `website_event` has no `visit_id` (added in `db/mysql/migrations/05_add_visit_id`). | `migration_missing` |
| `c2-mysql-2-before-revenue` | Migrations 01 to 12 only. No `revenue` table. | `migration_missing` |
| `d-mysql-2-unknown-newer-migration` | All 13 plus a synthetic `14_future_example` that adds `website.future_flag`. Not from upstream. | `migration_unknown` |
| `e1-postgresql-3-column-renamed` | All 26 migrations, then `alter table website_event rename column visit_id to visit_uuid`. | `column_missing` |
| `e2-mysql-2-column-dropped` | All 13 migrations, then drop `website_event.visit_id` and its two indexes. | `column_missing` |
| `f-postgresql-2-unsupported-pairing` | Real Umami 2.20.2 PostgreSQL migrations (13), no edit. | `unsupported_pairing` |
| `g-mysql-3-unsupported-engine` | Only `migrations.json`: the MySQL 2.20.2 table plus the real 3.4.0 row for `14_add_link_and_pixel`. No DDL, because upstream 3.x has no MySQL schema. | `unsupported_engine` |

Every refusal is `outcome: "refuse"`. The importer must open no write transaction on the source and must name the failing migration or column, as in the `message` fields. The unsupported-engine rule for g applies when a MySQL database lists any migration name that exists in 3.4.0 but not in 2.20.2.

## Regenerating

Run `tools/import-schemas/capture.sh` from anywhere. It needs Docker, `jq` and the `.sources/` clones, and about two minutes. It overwrites the snapshot files and leaves `expected.json` and `sample-data.sql` alone.
