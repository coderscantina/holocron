#!/usr/bin/env bash
# Loads each schema.sql and sample-data.sql into a fresh database in fresh containers and prints row counts.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/fixtures/import-schemas"
DC=(docker compose -f "$ROOT/tools/import-schemas/compose.yaml")
trap '"${DC[@]}" down -v --remove-orphans >/dev/null 2>&1 || true' EXIT
"${DC[@]}" up -d --wait pg my

"${DC[@]}" exec -T pg psql -U umami -qc 'create database verify' -d postgres
for f in schema.sql sample-data.sql; do
  "${DC[@]}" exec -T pg psql -U umami -d verify -q -v ON_ERROR_STOP=1 < "$OUT/postgresql-3.4.0/$f"
done
"${DC[@]}" exec -T pg psql -U umami -d verify -At -c "select count(*) from \"user\" u, website w, session s, website_event e, event_data d"

"${DC[@]}" exec -T my mysql -uroot -pumami -e 'create database verify' 2>/dev/null
for f in schema.sql sample-data.sql; do
  "${DC[@]}" exec -T my mysql -uroot -pumami --default-character-set=utf8mb4 verify < "$OUT/mysql-2.20.2/$f" 2>/dev/null
done
"${DC[@]}" exec -T my mysql -uroot -pumami -N verify -e 'select count(*) from `user` u, website w, session s, website_event e, event_data d' 2>/dev/null
echo "samples load"
