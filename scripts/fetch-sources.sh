#!/usr/bin/env bash
# Fetch every source pinned in sources/manifest.json into .sources/<name>.
# Idempotent: a checkout already at the pinned commit is not fetched again.
# Verifies the commit, lockfile hashes, referenced file hashes, and the
# bundled Umami scripts. Entries marked "distributable": false are described
# only and never fetched. Unreachable sources are reported and skipped; the
# exit code is non-zero if anything failed.
#
# Needs: bash, git, jq, shasum or sha256sum.
# Override the target directory with HOLOCRON_SOURCES_DIR.

set -uo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
manifest="$root/sources/manifest.json"
dest="${HOLOCRON_SOURCES_DIR:-$root/.sources}"
export GIT_TERMINAL_PROMPT=0

for tool in git jq; do
  command -v "$tool" >/dev/null || { echo "missing required tool: $tool" >&2; exit 2; }
done

if command -v sha256sum >/dev/null; then
  sha256() { sha256sum | cut -d' ' -f1; }
else
  sha256() { shasum -a 256 | cut -d' ' -f1; }
fi

failures=0
fail() { echo "  FAIL: $*" >&2; failures=$((failures + 1)); }

# check_blob <repo dir> <commit> <path> <expected sha256>
check_blob() {
  local actual
  actual="$(git -C "$1" show "$2:$3" 2>/dev/null | sha256)"
  if [ "$actual" != "$4" ]; then
    fail "$3 hash mismatch (expected $4, got $actual)"
  fi
}

mkdir -p "$dest"

while read -r name; do
  src() { jq -r --arg n "$name" ".sources[] | select(.name == \$n) | $1" "$manifest"; }
  url="$(src .url)"
  commit="$(src .commit)"
  dir="$dest/$name"
  echo "== $name @ ${commit:0:12}"

  if [ "$(git -C "$dir" rev-parse HEAD 2>/dev/null)" != "$commit" ]; then
    if [ ! -d "$dir/.git" ]; then
      mkdir -p "$dir"
      git -C "$dir" init -q
      git -C "$dir" remote add origin "$url"
    fi
    if ! git -C "$dir" fetch -q --depth 1 origin "$commit" 2>/dev/null; then
      if [ "$(src '.visibility // "public"')" = private ]; then
        fail "$name is private or unreachable at $url: $(src '.blocker // "no access"')"
      else
        fail "cannot fetch $commit from $url"
      fi
      continue
    fi
    git -C "$dir" checkout -q --detach "$commit" || { fail "checkout of $commit failed"; continue; }
  fi

  if [ "$(git -C "$dir" rev-parse HEAD)" != "$commit" ]; then
    fail "HEAD is not $commit"
    continue
  fi

  lic="$(src '.license.file // empty')"
  lic_sha="$(src '.license.sha256 // empty')"
  [ -n "$lic" ] && [ -n "$lic_sha" ] && check_blob "$dir" "$commit" "$lic" "$lic_sha"

  while IFS=$'\t' read -r path sha; do
    [ -n "$path" ] && check_blob "$dir" "$commit" "$path" "$sha"
  done < <(src '(.lockfiles[], (.referencedFiles[] | select(.sha256 != null))) | [.path, .sha256] | @tsv')
done < <(jq -r '.sources[] | select(.distributable != false) | .name' "$manifest")

echo "== bundled scripts"
while IFS=$'\t' read -r path sha; do
  [ -n "$path" ] || continue
  actual="$(sha256 < "$root/$path" 2>/dev/null)"
  [ "$actual" = "$sha" ] || fail "$path hash mismatch (expected $sha, got $actual)"
done < <(jq -r '.sources[].builtArtifacts.files[]? | [.path, .sha256] | @tsv' "$manifest")

if [ "$failures" -gt 0 ]; then
  echo "$failures check(s) failed" >&2
  exit 1
fi
echo "all sources verified in $dest"
