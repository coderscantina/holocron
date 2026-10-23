#!/usr/bin/env bash
# Cutover and rollback rehearsal of milestone 11 against isolated Umami deployments (compose
# project holocron-m11), once for Umami 3.4.0 on PostgreSQL and once for 2.20.2 on MySQL. Follows
# docs/operations/import-and-cutover.md step by step with the real `holocron` binary:
#   live source collects -> collection stops -> read-only user -> inspect -> import -> source
#   unchanged -> Holocron serves, logs in, answers the old share URL and collects -> rollback:
#   Holocron stops, Umami starts again on the untouched source and collects.
# Needs Docker, curl and jq. Removes its containers, volumes and data directories at the end.
# HOLOCRON may point at a binary; the default is target/debug/holocron, built when missing.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
HERE="$ROOT/tools/import-rehearsal"
FIXTURES="$ROOT/fixtures/import-schemas"
DC=(docker compose -f "$HERE/compose.yaml" --profile rehearsal)
WORK="$(mktemp -d)"
SERVE_PID=""
cleanup() {
  [ -n "$SERVE_PID" ] && kill "$SERVE_PID" 2>/dev/null || true
  "${DC[@]}" down -v --remove-orphans >/dev/null 2>&1 || true
  rm -rf "$WORK"
}
trap cleanup EXIT

HOLOCRON="${HOLOCRON:-$ROOT/target/debug/holocron}"
# The rehearsal data is tiny; keep 256 MiB free instead of the 4 GiB production reserve.
export HOLOCRON_DISK_RESERVE_MIB="${HOLOCRON_DISK_RESERVE_MIB:-256}"
[ -x "$HOLOCRON" ] || (cd "$ROOT" && cargo build -q -p holocron-server)

SITE=00000000-0000-4000-8000-000000000201
UA="Mozilla/5.0 (Macintosh; Intel Mac OS X 14_5) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.5 Safari/605.1.15"

step() { printf '\n== %s\n' "$*"; }

# wait_for <url>: polls until the URL answers.
wait_for() {
  for _ in $(seq 1 120); do curl -sf -o /dev/null "$1" && return 0; sleep 1; done
  echo "timed out waiting for $1" >&2; return 1
}

# track <base url> <path>: one pageview of site 201 as the unchanged tracker sends it.
track() {
  curl -sf -X POST "$1/api/send" -H 'Content-Type: application/json' -H "User-Agent: $UA" \
    -d "{\"type\":\"event\",\"payload\":{\"website\":\"$SITE\",\"hostname\":\"example.test\",\"url\":\"$2\",\"language\":\"en-US\",\"screen\":\"1920x1080\"}}" \
    -o /dev/null -w '%{http_code}'
}

rehearse() {
  local engine=$1
  local db_service app_service app_port snapshot sql checksum count read_only
  if [ "$engine" = pg ]; then
    db_service=pg app_service=umami-pg app_port=53011 snapshot=postgresql-3.4.0
    sql() { "${DC[@]}" exec -T pg psql -U umami -d umami -v ON_ERROR_STOP=1 -qAt "$@"; }
    checksum() {
      sql -c "select string_agg(t || '=' || n, ' ' order by t) from (select tablename t,
        (xpath('/row/c/text()', query_to_xml(format('select md5(coalesce(string_agg(x::text, ''|'' order by x::text), '''')) c from public.%I x', tablename), false, false, '')))[1]::text n
        from pg_tables where schemaname = 'public') s"
    }
    count() { sql -c "select count(*) from website_event"; }
    read_only="postgres://holocron_import:import-password@127.0.0.1:55411/umami"
  else
    db_service=my app_service=umami-my app_port=53012 snapshot=mysql-2.20.2
    sql() { "${DC[@]}" exec -T my mysql -uroot -pumami --default-character-set=utf8mb4 -N umami "$@" 2>/dev/null; }
    checksum() {
      local tables
      tables=$(sql -e "select group_concat(concat('\`', table_name, '\`')) from information_schema.tables where table_schema = 'umami'")
      sql -e "checksum table $tables" | tr '\t' '=' | tr '\n' ' '
    }
    count() { sql -e "select count(*) from website_event"; }
    read_only="mysql://holocron_import:import-password@127.0.0.1:53311/umami"
  fi
  local umami="http://127.0.0.1:$app_port" holocron="http://127.0.0.1:53021"
  local data="$WORK/$engine"

  step "$snapshot: start Umami; its start applies the upstream migrations"
  "${DC[@]}" up -d --wait "$db_service"
  "${DC[@]}" up -d "$app_service"
  wait_for "$umami/api/heartbeat"
  # Umami seeds `admin` with password `umami`. The sample rows bring their own `admin`, so the
  # seeded account becomes `umami-admin`; its real bcrypt hash tests the converted login below.
  if [ "$engine" = pg ]; then
    sql -c "update \"user\" set username = 'umami-admin' where username = 'admin'"
  else
    sql -e "update \`user\` set username = 'umami-admin' where username = 'admin'"
  fi
  sql < "$FIXTURES/$snapshot/sample-data.sql"
  echo "source events after the sample data: $(count)"

  step "$snapshot: the live source collects"
  echo "Umami /api/send: $(track "$umami" /before-cutover)"
  echo "source events: $(count)"

  step "$snapshot: cutover 1, stop collection for the final snapshot"
  "${DC[@]}" stop "$app_service" >/dev/null
  curl -sf -o /dev/null "$umami/api/heartbeat" && { echo "Umami still answers" >&2; return 1; }
  echo "Umami stopped; source events: $(count)"

  step "$snapshot: cutover 2, a read-only database user for the importer"
  if [ "$engine" = pg ]; then
    sql -c "create role holocron_import login password 'import-password';
            grant connect on database umami to holocron_import; grant usage on schema public to holocron_import;
            grant select on all tables in schema public to holocron_import;"
  else
    sql -e "create user 'holocron_import'@'%' identified by 'import-password';
            grant select on umami.* to 'holocron_import'@'%';"
  fi
  local before; before="$(checksum)"

  step "$snapshot: cutover 3, inspect"
  "$HOLOCRON" import inspect --source "$read_only" --timezone Europe/Vienna > "$WORK/$engine-inspect.json"
  jq -c '{outcome, source, credentials, events: .preview.events}' "$WORK/$engine-inspect.json"

  step "$snapshot: cutover 4, import into a new data directory and reconcile"
  printf 'rehearsal admin password\n' > "$WORK/password"
  "$HOLOCRON" init --data-dir "$data" --admin-email ops@example.test --admin-password-file "$WORK/password"
  "$HOLOCRON" import run --data-dir "$data" --source "$read_only" --timezone Europe/Vienna > "$WORK/$engine-manifest.json"
  jq -c '{status, differences}' "$WORK/$engine-manifest.json"
  jq -c '.sites[] | select(.kind == "site") | {id, events, outside_retention, sessions, revenue}' "$WORK/$engine-manifest.json"
  jq -c '.logins' "$WORK/$engine-manifest.json"
  jq -c '[.inventory[] | "\(.table)/\(.reason)=\(.count)"]' "$WORK/$engine-manifest.json"
  [ "$(jq -r .status "$WORK/$engine-manifest.json")" = reconciled ]

  step "$snapshot: the source is unchanged"
  [ "$(checksum)" = "$before" ] && echo "table checksums equal before and after"
  echo "source events: $(count)"

  step "$snapshot: cutover 5, verify the destination"
  "$HOLOCRON" serve --data-dir "$data" --bind 127.0.0.1:53021 >"$WORK/serve.log" 2>&1 &
  SERVE_PID=$!
  wait_for "$holocron/health"
  local login
  login=$(curl -s -X POST "$holocron/api/commands/auth.login" -H 'Host: 127.0.0.1:53021' \
    -H 'Origin: http://127.0.0.1:53021' -H 'x-holocron-request: 1' -H 'Content-Type: application/json' \
    -d '{"email":"ops@example.test","password":"rehearsal admin password","idempotency_key":"rehearsal-login"}')
  echo "fresh admin login: $(jq -r .status <<<"$login")"
  login=$(curl -s -X POST "$holocron/api/commands/auth.login" -H 'Host: 127.0.0.1:53021' \
    -H 'Origin: http://127.0.0.1:53021' -H 'x-holocron-request: 1' -H 'Content-Type: application/json' \
    -d '{"email":"umami-admin@umami.invalid","password":"umami","idempotency_key":"rehearsal-umami-login"}')
  echo "converted Umami login umami-admin@umami.invalid with its Umami password: $(jq -r .status <<<"$login")"
  echo "old share URL /share/sample-share: $(curl -s -o /dev/null -w '%{http_code}' -H 'Authorization: Share sample-share' "$holocron/api/queries/sharing.resolve")"
  echo "share points at site: $(curl -s -H 'Authorization: Share sample-share' "$holocron/api/queries/sharing.resolve" | jq -r '.target.state.site')"

  step "$snapshot: cutover 6, redirect the tracker and test collection"
  echo "Holocron /api/send for the same website ID: $(track "$holocron" /after-cutover)"

  step "$snapshot: rollback, stop Holocron and start Umami on the untouched source"
  kill "$SERVE_PID"; wait "$SERVE_PID" 2>/dev/null || true; SERVE_PID=""
  "${DC[@]}" start "$app_service" >/dev/null
  wait_for "$umami/api/heartbeat"
  echo "Umami /api/send after rollback: $(track "$umami" /after-rollback)"
  echo "source events: $(count)"

  "${DC[@]}" down -v --remove-orphans >/dev/null 2>&1
}

rehearse pg
rehearse my
step "both rehearsals passed"
