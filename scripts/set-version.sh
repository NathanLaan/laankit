#!/usr/bin/env bash
# Sets the version of every Marina app (native, Tauri and Electron builds) and
# refreshes the lock files.
#
#   scripts/set-version.sh 2.3.0
#   scripts/set-version.sh 2.3.0-rc.1
#   NO_LOCKS=1 scripts/set-version.sh 2.3.0     # skip Cargo.lock / package-lock.json
#
# Updates:
#   apps/<app>/native/Cargo.toml, apps/<app>/src-tauri/Cargo.toml   [package] version
#   apps/<app>/src-tauri/tauri.conf.json                            "version"
#   package.json, apps/<app>/package.json                           "version"
#   apps/pageliner/src/renderer/App.svelte                          fallback appVersion
#   packaging/linux/*.metainfo.xml                                  a new <release> entry
#   Cargo.lock, package-lock.json                                   workspace entries only
#
# Shared crates (crates/*) keep their own versions. Uses perl for the edits, so
# it behaves the same with GNU and BSD (macOS) tools.
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: $0 <version>   (e.g. 2.3.0 or 2.3.0-rc.1)" >&2
  exit 2
fi
VERSION="$1"
# Semver without build metadata: what Cargo, npm and AppStream all accept.
if [[ ! "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]]; then
  echo "not a version: $VERSION (expected MAJOR.MINOR.PATCH or MAJOR.MINOR.PATCH-PRERELEASE)" >&2
  exit 2
fi
export VERSION

cd "$(dirname "$0")/.."
APPS=(threadliner pageliner noteliner)
TODAY="$(date +%Y-%m-%d)"
export TODAY

changed=()

# Replaces the first match of a perl regex in a file; fails if nothing matched.
# The pattern's first group is kept; the version replaces the rest.
set_first() {
  local file="$1" pattern="$2"
  [[ -f "$file" ]] || { echo "missing: $file" >&2; exit 1; }
  PATTERN="$pattern" perl -0pi -e '
    BEGIN { $found = 0 }
    $found = s/$ENV{PATTERN}/$1$ENV{VERSION}$2/ ? 1 : 0;
    END { exit($found ? 0 : 3) }
  ' "$file" || { echo "no version found in $file" >&2; exit 1; }
  changed+=("$file")
}

for app in "${APPS[@]}"; do
  for toml in "apps/$app/native/Cargo.toml" "apps/$app/src-tauri/Cargo.toml"; do
    # The `version` key of the [package] table (not a dependency's).
    set_first "$toml" '(\[package\][^\[]*?\nversion\s*=\s*")[^"]*(")'
  done
  set_first "apps/$app/src-tauri/tauri.conf.json" '("version"\s*:\s*")[^"]*(")'
  set_first "apps/$app/package.json" '("version"\s*:\s*")[^"]*(")'
done
set_first package.json '("version"\s*:\s*")[^"]*(")'
# PageLiner's About shows this until the real version arrives over IPC.
set_first apps/pageliner/src/renderer/App.svelte "(let appVersion = \\\$state\\(')[^']*(')"

# AppStream: one <release> per version, newest first.
for xml in packaging/linux/*.metainfo.xml; do
  [[ -f "$xml" ]] || continue
  if grep -q "<release version=\"$VERSION\"" "$xml"; then
    continue
  fi
  perl -0pi -e 's|(<releases>\n)(\s*)|$1$2<release version="$ENV{VERSION}" date="$ENV{TODAY}"/>\n$2|' "$xml"
  grep -q "<release version=\"$VERSION\"" "$xml" || { echo "no <releases> in $xml" >&2; exit 1; }
  changed+=("$xml")
done

if [[ -z "${NO_LOCKS:-}" ]]; then
  # Only the workspace members' entries change; no dependency is upgraded.
  cargo update --workspace --quiet --offline 2>/dev/null || cargo update --workspace --quiet
  changed+=(Cargo.lock)
  npm install --package-lock-only --ignore-scripts --no-audit --no-fund --offline >/dev/null 2>&1 \
    || npm install --package-lock-only --ignore-scripts --no-audit --no-fund >/dev/null
  changed+=(package-lock.json)
fi

echo "Set version $VERSION in:"
printf '  %s\n' "${changed[@]}"
