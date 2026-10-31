#!/usr/bin/env bash
# Builds the native apps' release binaries and packages. See docs/packaging.md.
#
#   scripts/package-native.sh                    # all apps, the host's default formats
#   scripts/package-native.sh noteliner          # one app
#   FORMATS="deb appimage" scripts/package-native.sh
#
# Formats: deb, rpm (Linux; cargo-deb / cargo-generate-rpm), appimage (Linux),
# nsis (Windows), app and dmg (macOS) (cargo-packager). Outputs land in
# target/packages/<app>/. Needs: cargo install cargo-deb cargo-generate-rpm cargo-packager
set -euo pipefail

cd "$(dirname "$0")/.."
ALL_APPS=(threadliner pageliner noteliner blockliner)
APPS=("${@:-${ALL_APPS[@]}}")

case "$(uname -s)" in
  Linux) DEFAULT_FORMATS="deb rpm appimage" ;;
  Darwin) DEFAULT_FORMATS="app dmg" ;;
  MINGW*|MSYS*|CYGWIN*) DEFAULT_FORMATS="nsis" ;;
  *) echo "unsupported host: $(uname -s)" >&2; exit 1 ;;
esac
FORMATS="${FORMATS:-$DEFAULT_FORMATS}"

cargo run -q -p xtask -- icons

pkgs=()
for app in "${APPS[@]}"; do pkgs+=(-p "$app-native"); done
cargo build --release "${pkgs[@]}"

for app in "${APPS[@]}"; do
  out="target/packages/$app"
  mkdir -p "$out"
  packager_formats=()
  for format in $FORMATS; do
    case "$format" in
      deb) cargo deb -p "$app-native" --no-build -o "$out/" ;;
      # -p is the crate's folder (its help calls it a name, but it's resolved as a path).
      rpm) cargo generate-rpm -p "apps/$app/native" -o "$out/" ;;
      appimage|nsis|app|dmg) packager_formats+=("$format") ;;
      *) echo "unknown format: $format" >&2; exit 1 ;;
    esac
  done
  if ((${#packager_formats[@]})); then
    formats_csv=$(IFS=,; echo "${packager_formats[*]}")
    cargo packager -p "$app-native" --release --formats "$formats_csv"
  fi
done

ls -lh target/packages/*/ 2>/dev/null
