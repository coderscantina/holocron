#!/usr/bin/env bash
# Runs the import tests of apps/server/tests/imports.rs against fresh PostgreSQL 15 and MySQL 8.0
# containers (compose project holocron-m11) and removes them afterwards. Needs Docker.
# Extra arguments go to the test binary, for example a test name filter.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
DC=(docker compose -f "$ROOT/tools/import-rehearsal/compose.yaml")
trap '"${DC[@]}" down -v --remove-orphans >/dev/null 2>&1 || true' EXIT
"${DC[@]}" up -d --wait pg my

export HOLOCRON_IMPORT_PG="postgres://umami:umami@127.0.0.1:55411/umami"
export HOLOCRON_IMPORT_MYSQL="mysql://root:umami@127.0.0.1:53311/umami"
cd "$ROOT"
cargo test -p holocron-server --test imports -- --ignored "$@"
