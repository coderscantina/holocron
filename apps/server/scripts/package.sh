#!/usr/bin/env sh
# Builds the deployable package: the release `holocron` binary with the built web app beside it
# in `web/`, which `holocron serve` finds without configuration. Node is needed only here, at
# build time.
#
#   apps/server/scripts/package.sh [output-dir]    default: <target>/package/holocron
#
# HOLOCRON_PACKAGE_FEATURES adds cargo features to the server build, e.g. `dev-clock` for a frozen clock
# (`holocron serve --frozen-clock`). Default: none, as for a release.
set -eu

root=$(cd "$(dirname "$0")/../../.." && pwd)
target=${CARGO_TARGET_DIR:-$root/target}
out=${1:-$target/package/holocron}
features=${HOLOCRON_PACKAGE_FEATURES:-}

(cd "$root/apps/web" && bun run build)
cargo build --release --manifest-path "$root/Cargo.toml" -p holocron-server --bin holocron \
  ${features:+--features "$features"}

rm -rf "$out"
mkdir -p "$out"
cp "$target/release/holocron" "$out/holocron"
cp -R "$root/apps/web/dist" "$out/web"
echo "packaged $out"
