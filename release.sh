#!/usr/bin/env bash
# Cuts a release from main: sets today's CalVer version (YY.M.D) in Cargo.toml and
# apps/web/package.json, regenerates the protocol contract (the OpenAPI document carries the
# version), writes a CHANGELOG.md section from the gitmoji commits since the last tag, commits
# "🔖 release YY.M.D", tags vYY.M.D-<short commit> and pushes main and the tag together. The
# tag starts .github/workflows/release.yml, which waits for CI on the release commit and
# publishes ghcr.io/coderscantina/holocron.
#
#   ./release.sh                     release, after a confirmation
#   ./release.sh highlights.md       put a hand-written summary above the commit list
#   ./release.sh --dry-run           print the version and section, change nothing
#   ./release.sh --yes               skip the confirmation
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

CHANGELOG=CHANGELOG.md
RELEASE_FILES=(Cargo.toml Cargo.lock apps/web/package.json "$CHANGELOG" crates/protocol/openapi.json apps/web/src/generated)

fail() {
  echo "release: $*" >&2
  exit 1
}

usage() {
  sed -n '2,/^set -euo/p' "$0" | sed '$d; s/^# \{0,1\}//'
}

dry_run=false
assume_yes=false
highlights=""
for arg in "$@"; do
  case "$arg" in
    --dry-run) dry_run=true ;;
    --yes | -y) assume_yes=true ;;
    -h | --help) usage; exit 0 ;;
    -*) fail "unknown option $arg. See ./release.sh --help." ;;
    *)
      [ -z "$highlights" ] || fail "only one highlights file, got $highlights and $arg"
      [ -f "$arg" ] || fail "no such highlights file: $arg"
      highlights="$arg"
      ;;
  esac
done

# ─── Version and changelog section ───────────────────────────────────────────

version=$(date +%y.%-m.%-d)
last_tag=$(git describe --tags --abbrev=0 --match 'v[0-9]*.[0-9]*.[0-9]*-*' 2>/dev/null || true)
# Oldest first, without earlier release commits.
subjects=$(git log --no-merges --reverse --format='%s' ${last_tag:+"$last_tag"..HEAD} | grep -v '^🔖' || true)
[ -n "$subjects" ] || fail "nothing to release since ${last_tag:-the first commit}"

features='✨|🎉'
fixes='🐛|🚑|🩹'
performance='⚡'
security='🔒|🔐|🛂'
interface='💄|♿|🌐|💬|🚸'

# Prints a changelog group: the subjects whose gitmoji matches the pattern.
group() {
  local lines
  lines=$(printf '%s\n' "$subjects" | grep -E "^($2)" || true)
  if [ -n "$lines" ]; then
    printf '### %s\n\n%s\n\n' "$1" "$(printf '%s\n' "$lines" | sed 's/^/- /')"
  fi
}

section=$(
  printf '## %s\n\n' "$version"
  if [ -n "$highlights" ]; then printf '%s\n\n' "$(cat "$highlights")"; fi
  group Features "$features"
  group Fixes "$fixes"
  group Performance "$performance"
  group Security "$security"
  group Interface "$interface"
  others=$(printf '%s\n' "$subjects" | grep -vE "^($features|$fixes|$performance|$security|$interface)" || true)
  if [ -n "$others" ]; then
    printf '### Other changes\n\n%s\n' "$(printf '%s\n' "$others" | sed 's/^/- /')"
  fi
)
count=$(printf '%s\n' "$subjects" | wc -l | tr -d ' ')
commits="$count commits"
[ "$count" != 1 ] || commits="1 commit"

if $dry_run; then
  echo "Would release $version with $commits since ${last_tag:-the first commit}:"
  echo
  printf '%s\n' "$section"
  exit 0
fi

# ─── Checks ──────────────────────────────────────────────────────────────────

[ "$(git branch --show-current)" = main ] || fail "releases are cut from main"
[ -z "$(git status --porcelain --untracked-files=no)" ] || fail "the working tree has uncommitted changes"
git remote get-url origin >/dev/null 2>&1 || fail "no origin remote. Add https://github.com/coderscantina/holocron as origin."
for tool in cargo perl; do
  command -v "$tool" >/dev/null || fail "$tool is not installed"
done

git fetch --quiet --tags origin main
if git rev-parse --verify --quiet origin/main >/dev/null; then
  head=$(git rev-parse HEAD)
  remote=$(git rev-parse origin/main)
  base=$(git merge-base HEAD origin/main)
  if [ "$head" != "$remote" ]; then
    [ "$head" != "$base" ] || fail "main is behind origin/main. Pull first."
    [ "$remote" = "$base" ] || fail "main has diverged from origin/main. Rebase or merge first."
    echo "main is $(git rev-list --count origin/main..HEAD) commits ahead of origin/main; the release pushes them."
  fi
fi

# CI on HEAD, when it has run. A red run stops the release here instead of in the workflow.
if command -v gh >/dev/null && gh auth status >/dev/null 2>&1; then
  ci=$(gh run list --workflow ci.yml --commit "$(git rev-parse HEAD)" --limit 1 \
    --json status,conclusion --jq '.[0] | "\(.status) \(.conclusion)"' 2>/dev/null || true)
  case "$ci" in
    "completed success" | "") ;;
    completed*) fail "CI failed on $(git rev-parse --short HEAD) ($ci). Fix main first." ;;
    *) echo "CI is still running on $(git rev-parse --short HEAD); the release workflow waits for the release commit's run." ;;
  esac
fi

# ─── Confirm ─────────────────────────────────────────────────────────────────

printf '%s\n\n' "$section"
echo "Release $version with $commits since ${last_tag:-the first commit}, and push main and the tag to origin."
if ! $assume_yes; then
  [ -t 0 ] || fail "no terminal to confirm. Pass --yes to release without asking."
  read -r -p "Release? (y/n) " reply
  [[ $reply =~ ^[Yy]$ ]] || exit 1
fi

# ─── Release ─────────────────────────────────────────────────────────────────

# Until the push succeeds, a failure leaves only local changes; say how to drop them.
start=$(git rev-parse HEAD)
pushed=false
on_exit() {
  if [ $? -ne 0 ] && ! $pushed; then
    echo "release: stopped before pushing. Undo the local changes with:" >&2
    echo "  git reset --hard $start${tag:+ && git tag -d $tag}" >&2
  fi
}
trap on_exit EXIT

perl -0pi -e "s/(\[workspace\.package\]\nversion = )\"[^\"]*\"/\${1}\"$version\"/" Cargo.toml
perl -pi -e "s/^  \"version\": \"[^\"]*\"/  \"version\": \"$version\"/" apps/web/package.json
grep -q "^version = \"$version\"" Cargo.toml || fail "could not set the version in Cargo.toml"
cargo update --workspace --quiet
cargo run --quiet -p holocron-protocol --bin export-types

# The new section goes above the newest one, below the file's introduction.
[ -f "$CHANGELOG" ] || printf '# Changelog\n\nAll notable changes to Holocron are documented here.\nCommits follow the [Gitmoji](https://gitmoji.dev/) convention.\n' > "$CHANGELOG"
section_file=$(mktemp)
printf '%s\n\n' "$section" > "$section_file"
awk 'FNR == NR { section = section $0 "\n"; next }
     !done && /^## / { printf "%s", section; done = 1 }
     { print }
     END { if (!done) printf "\n%s", section }' "$section_file" "$CHANGELOG" > "$CHANGELOG.next"
mv "$CHANGELOG.next" "$CHANGELOG"
rm "$section_file"

git add -- "${RELEASE_FILES[@]}"
unexpected=$(git status --porcelain --untracked-files=no | grep -v '^M  \|^A  ' || true)
[ -z "$unexpected" ] || fail "the release changed files it does not commit:
$unexpected"
git commit --quiet -m "🔖 release $version"
tag="v$version-$(git rev-parse --short HEAD)"
git rev-parse --verify --quiet "refs/tags/$tag" >/dev/null && fail "tag $tag already exists"
git tag -a "$tag" -m "$tag"

# One atomic push: the tag never reaches origin without its commit.
git push --atomic origin HEAD:main "refs/tags/$tag"
pushed=true

echo "Released $tag. Follow the image build with:"
echo "  gh run watch \$(gh run list --workflow release.yml --limit 1 --json databaseId --jq '.[0].databaseId')"
