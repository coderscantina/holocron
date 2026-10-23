#!/usr/bin/env sh
# Builds and seeds the demo instance the README visuals come from, then serves it on a frozen
# clock, so every run shows the same numbers. Then, in a second terminal, after the first
# summary round (about 60 s):
#
#   bun scripts/demo-shots.ts
#
#   scripts/demo-seed.sh [data-dir] [port]    defaults: /tmp/holocron-demo 8110
#
# The data is the v2 acceptance fixture (docs/evidence/v09-acceptance.md): the "Lagoon" team
# with six sites, 70 days of history, a newsletter spike today, goals, a funnel board and
# replays, next to the "Roadhouse" team with 45 days of traffic and commerce. The data
# directory must not exist yet. It ends up with `demo.json`, the URL and sign-in the shots use.
#
# The server trusts 127.0.0.1 as a proxy, so the live visitors `demo-shots.ts` sends during the
# capture can carry a location for the realtime globe. Set CARGO_TARGET_DIR to build elsewhere.
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
target=${CARGO_TARGET_DIR:-$root/target}
data=${1:-/tmp/holocron-demo}
port=${2:-8110}
now=2026-10-13T15:20:00+02:00
email=mike@lagoon.test
password=holocron-demo-password

if [ -e "$data" ]; then
  echo "$data exists; remove it or pass another data directory" >&2
  exit 1
fi

HOLOCRON_PACKAGE_FEATURES=dev-clock "$root/apps/server/scripts/package.sh"
cargo build --release --manifest-path "$root/Cargo.toml" -p holocron-server --features dev-clock \
  --example seed_demo --example seed_traffic --example seed_commerce --example seed_acceptance

holocron=$target/package/holocron/holocron
examples=$target/release/examples
mkdir -p "$data"
secret=$(mktemp)
printf '%s' "$password" > "$secret"
"$holocron" init --data-dir "$data" --admin-email "$email" --admin-password-file "$secret"
rm -f "$secret"
"$examples/seed_demo" "$data" "$email" "$password"
"$examples/seed_traffic" "$data" 45 --now "$now"
"$examples/seed_commerce" "$data" 45 --now "$now"
"$examples/seed_acceptance" "$data" "$email" --now "$now"

printf '{"url":"http://127.0.0.1:%s","email":"%s","password":"%s","now":"%s"}\n' \
  "$port" "$email" "$password" "$now" > "$data/demo.json"

HOLOCRON_COLLECT_TRUSTED_PROXIES=127.0.0.1 exec "$holocron" serve \
  --data-dir "$data" --bind "127.0.0.1:$port" --frozen-clock "$now"
