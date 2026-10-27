#!/usr/bin/env sh
# Runs the browser journeys (`bun run e2e`) end to end on a throwaway instance: packages the
# server into its own directory, seeds a fresh data directory, serves it, waits for the first
# summary round, then runs Playwright. It stops the servers it started and removes its
# temporary directories on exit.
#
#   scripts/e2e.sh [--keep] [playwright args...]    e.g. scripts/e2e.sh e2e/shell.spec.ts
#
# --keep leaves the temporary directory (packaged build, data, server log) in place.
# HOLOCRON_E2E_PORT sets the port of the seeded server (default 8093).
#
# The data is seed_demo, seed_traffic and seed_commerce on the real clock, so dates are relative
# to today and no frozen-clock build is needed. The shell specs that show site pace and findings
# need a summary round, which runs one minute after the start; the script waits until every site
# has a row in `site_summaries`. The setup specs start their own servers on empty data
# directories from the same binary (HOLOCRON_E2E_BINARY). The restore specs among them restore a
# `holocron backup` this script takes of the seeded instance (HOLOCRON_E2E_BACKUP_DIR). Set
# HOLOCRON_E2E_OLD_BACKUP to a backup made by an older release (of the same seeded admin) to
# run the upgrade spec too. HOLOCRON_E2E_SHOTS=<dir> saves screenshots of the setup steps.
# Set CARGO_TARGET_DIR to build elsewhere.
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
target=${CARGO_TARGET_DIR:-$root/target}
port=${HOLOCRON_E2E_PORT:-8093}
keep=0
if [ "${1:-}" = "--keep" ]; then
  keep=1
  shift
fi

email=mike@example.test
password=e2e-admin-password
viewer_password=e2e-viewer-password
instance=Holocron-E2E

work=${HOLOCRON_E2E_DIR:-/tmp/holocron-e2e}
pid=
cleanup() {
  [ -z "$pid" ] || kill "$pid" 2>/dev/null || true
  [ -z "$pid" ] || wait "$pid" 2>/dev/null || true
  if [ "$keep" = 0 ]; then rm -rf "$work"; else echo "kept $work"; fi
}
trap cleanup EXIT INT TERM

if [ -e "$work/data" ]; then
  echo "$work/data exists from a kept run; remove $work" >&2
  trap - EXIT
  exit 1
fi
mkdir -p "$work"

package=$work/package
"$root/apps/server/scripts/package.sh" "$package"
cargo build --release --manifest-path "$root/Cargo.toml" -p holocron-server \
  --example seed_demo --example seed_traffic --example seed_commerce

holocron=$package/holocron
examples=$target/release/examples
data=$work/data
secret=$work/admin-password
mkdir -p "$data"
printf '%s' "$password" > "$secret"
"$holocron" init --data-dir "$data" --admin-email "$email" --admin-password-file "$secret"
rm -f "$secret"
"$examples/seed_demo" "$data" "$email" "$viewer_password"
"$examples/seed_traffic" "$data" 45
"$examples/seed_commerce" "$data" 45

"$holocron" serve --data-dir "$data" --bind "127.0.0.1:$port" \
  --public-url "http://127.0.0.1:$port" --instance-name "$instance" --disk-reserve-mib 64 > "$work/server.log" 2>&1 &
pid=$!
echo "server pid $pid on port $port"

waited=0
until curl -fsS "http://127.0.0.1:$port/health" > /dev/null 2>&1; do
  kill -0 "$pid" 2>/dev/null || { cat "$work/server.log" >&2; exit 1; }
  [ "$waited" -lt 60 ] || { echo "server did not become healthy" >&2; exit 1; }
  sleep 1
  waited=$((waited + 1))
done

echo "waiting for the first summary round"
waited=0
while :; do
  sites=$(sqlite3 "file:$data/control.sqlite?mode=ro" "SELECT count(*) FROM sites" 2>/dev/null || echo 0)
  done_=$(sqlite3 "file:$data/control.sqlite?mode=ro" "SELECT count(*) FROM site_summaries" 2>/dev/null || echo 0)
  [ "$sites" -gt 0 ] && [ "$done_" -ge "$sites" ] && break
  [ "$waited" -lt 300 ] || { echo "no summaries after 300 s ($done_/$sites)" >&2; exit 1; }
  sleep 2
  waited=$((waited + 2))
done

# The running server queues the backup as a job and waits for it, as in production.
backups=$work/backups
"$holocron" backup --data-dir "$data" --to "$backups" > "$work/backup.json"

cd "$root/apps/web"
set +e
HOLOCRON_E2E_URL="http://127.0.0.1:$port" \
HOLOCRON_E2E_EMAIL="$email" \
HOLOCRON_E2E_PASSWORD="$password" \
HOLOCRON_E2E_VIEWER_PASSWORD="$viewer_password" \
HOLOCRON_E2E_BINARY="$holocron" \
HOLOCRON_E2E_BACKUP_DIR="$backups" \
HOLOCRON_E2E_DB="$data/control.sqlite" \
HOLOCRON_E2E_INSTANCE_NAME="$instance" \
  bun run e2e "$@"
status=$?
exit "$status"
