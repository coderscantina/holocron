#!/usr/bin/env bash
# Rebuilds fixtures/import-schemas/{postgresql-3.4.0,mysql-2.20.2}/ and the DB-derived files of
# fixtures/import-schemas/refusal/*/ from the pinned official Umami images (see compose.yaml).
# Needs docker, jq, shasum and the upstream clones in .sources/. Run from anywhere.
# Hand-written files (expected.json, sample-data.sql) are not touched.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
HERE="$ROOT/tools/import-schemas"
OUT="$ROOT/fixtures/import-schemas"
SRC3="$ROOT/.sources/umami-3.4.0/prisma/migrations"
SRC2MY="$ROOT/.sources/umami-2.20.2/db/mysql/migrations"
SRC2PG="$ROOT/.sources/umami-2.20.2/db/postgresql/migrations"
TMP="$(mktemp -d)"
DC=(docker compose -f "$HERE/compose.yaml")
trap '"${DC[@]}" down -v --remove-orphans >/dev/null 2>&1 || true; rm -rf "$TMP"' EXIT

pg()  { "${DC[@]}" exec -T pg psql -U umami -v ON_ERROR_STOP=1 "$@"; }
my()  { "${DC[@]}" exec -T my mysql -uroot -pumami --default-character-set=utf8mb4 "$@" 2>/dev/null; }

# newdb <pg|my> <name>
newdb() {
  if [ "$1" = pg ]; then pg -d postgres -qc "create database $2" ; else my -e "create database $2"; fi
}

# migrate <service> <pg|my> <db> <migrations dir>  (dir is mounted over /app/prisma/migrations)
migrate() {
  local url
  if [ "$2" = pg ]; then url="postgresql://umami:umami@pg:5432/$3"; else url="mysql://root:umami@my:3306/$3"; fi
  "${DC[@]}" run --rm -T -e DATABASE_URL="$url" -v "$4:/app/prisma/migrations:ro" "$1" >/dev/null
}

# subset <src dir> <dest> <last migration prefix number>  (copies 01..N plus the lock file)
subset() {
  mkdir -p "$2"; cp "$1/migration_lock.toml" "$2/"
  for d in "$1"/*/; do n="$(basename "$d")"; [ "${n%%_*}" -le "$3" ] && cp -R "$d" "$2/$n"; done
  return 0
}

# dump <pg|my> <db> <outdir>
dump() {
  mkdir -p "$3"
  if [ "$1" = pg ]; then
    "${DC[@]}" exec -T pg pg_dump -U umami -d "$2" --schema-only --no-owner \
      | grep -v '^\\\(un\)\{0,1\}restrict' > "$3/schema.sql"
    pg -d "$2" -At -f - < "$HERE/inventory-pg.sql" | jq -S '.tables |= (sort_by(.name) | map(.columns |= sort_by(.position)))' > "$3/columns.json"
    pg -d "$2" -At -c "select coalesce(jsonb_agg(m),'[]') from _prisma_migrations m" > "$TMP/m.json"
  else
    "${DC[@]}" exec -T my mysqldump -uroot -pumami --no-data --skip-comments --no-tablespaces "$2" 2>/dev/null \
      | sed 's/ AUTO_INCREMENT=[0-9]*//' > "$3/schema.sql"
    my -N -B -r "$2" < "$HERE/inventory-mysql.sql" | jq -S '.tables |= (sort_by(.name) | map(.columns |= sort_by(.position)))' > "$3/columns.json"
    my -N -B -r "$2" -e "select coalesce(json_arrayagg(json_object('migration_name',migration_name,'checksum',checksum,'finished_at',finished_at,'rolled_back_at',rolled_back_at,'applied_steps_count',applied_steps_count,'logs',logs)),json_array()) from _prisma_migrations" > "$TMP/m.json"
  fi
  # id and started_at are random per run, finished_at is a timestamp: keep only whether they are set.
  jq -S 'map({migration_name, checksum, applied_steps_count, logs,
              finished_at: (if .finished_at then "<set>" else null end),
              rolled_back_at: (if .rolled_back_at then "<set>" else null end)}) | sort_by(.migration_name)' \
    "$TMP/m.json" > "$3/migrations.json"
}

"${DC[@]}" up -d --wait pg my

# Real snapshots: every upstream migration, applied by prisma migrate deploy.
newdb pg pg3_full;  migrate umami-pg-3 pg pg3_full "$SRC3";   dump pg pg3_full "$OUT/postgresql-3.4.0"
newdb my my2_full;  migrate umami-my-2 my my2_full "$SRC2MY"; dump my my2_full "$OUT/mysql-2.20.2"

# Refusal snapshots.
R="$OUT/refusal"
# a: PostgreSQL 3.x, 01..25 applied, 26_add_api_key missing
subset "$SRC3" "$TMP/pg3_25" 25
newdb pg a; migrate umami-pg-3 pg a "$TMP/pg3_25"; dump pg a "$R/a-postgresql-3-last-migration-missing"
# b: same schema, plus a 26_add_api_key row that started and never finished (b1), or was marked rolled back (b2)
CS26="$(shasum -a 256 "$SRC3/26_add_api_key/migration.sql" | cut -d' ' -f1)"
for v in b1 b2; do
  newdb pg $v; migrate umami-pg-3 pg $v "$TMP/pg3_25"
  rb=null; [ $v = b2 ] && rb='now()'
  pg -d $v -qc "insert into _prisma_migrations (id, checksum, migration_name, logs, rolled_back_at, started_at, applied_steps_count) values (gen_random_uuid()::text, '$CS26', '26_add_api_key', 'Migration failed to apply (synthetic fixture row)', $rb, now(), 0)"
done
dump pg b1 "$R/b1-postgresql-3-failed-migration"
dump pg b2 "$R/b2-postgresql-3-rolled-back-migration"
# c: MySQL 2.x stopped before 05_add_visit_id (c1) or before 13_add_revenue (c2)
subset "$SRC2MY" "$TMP/my_04" 4;  newdb my c1; migrate umami-my-2 my c1 "$TMP/my_04"; dump my c1 "$R/c1-mysql-2-before-visit-id"
subset "$SRC2MY" "$TMP/my_12" 12; newdb my c2; migrate umami-my-2 my c2 "$TMP/my_12"; dump my c2 "$R/c2-mysql-2-before-revenue"
# d: MySQL 2.20.2 plus a synthetic 14_future_example migration
cp -R "$SRC2MY" "$TMP/my_14"; mkdir "$TMP/my_14/14_future_example"
printf -- '-- Synthetic migration for the refusal fixture, not from upstream.\nALTER TABLE `website` ADD COLUMN `future_flag` BOOLEAN NOT NULL DEFAULT false;\n' > "$TMP/my_14/14_future_example/migration.sql"
newdb my d; migrate umami-my-2 my d "$TMP/my_14"; dump my d "$R/d-mysql-2-unknown-newer-migration"
# e: full migration table, but a required column changed afterwards
newdb pg e1; migrate umami-pg-3 pg e1 "$SRC3"
pg -d e1 -qc 'alter table website_event rename column visit_id to visit_uuid'
dump pg e1 "$R/e1-postgresql-3-column-renamed"
newdb my e2; migrate umami-my-2 my e2 "$SRC2MY"
my e2 -e 'alter table website_event drop index website_event_visit_id_idx, drop index website_event_website_id_visit_id_created_at_idx, drop column visit_id'
dump my e2 "$R/e2-mysql-2-column-dropped"
# f: PostgreSQL with the Umami 2.20.2 migrations
newdb pg f; migrate umami-pg-2 pg f "$SRC2PG"; dump pg f "$R/f-postgresql-2-unsupported-pairing"
# g: MySQL claiming a 3.x migration. Upstream 3.x has no MySQL schema, so this is hand-built, not captured:
# the mysql-2.20.2 migration table plus the real 3.4.0 row for 14_add_link_and_pixel.
mkdir -p "$R/g-mysql-3-unsupported-engine"
jq -S --argjson row "$(jq '.[] | select(.migration_name=="14_add_link_and_pixel")' "$OUT/postgresql-3.4.0/migrations.json")" \
  '. + [$row] | sort_by(.migration_name)' "$OUT/mysql-2.20.2/migrations.json" > "$R/g-mysql-3-unsupported-engine/migrations.json"
