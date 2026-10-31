#!/usr/bin/env bash
# Generates packaging/flatpak/cargo-sources.json (the vendored crates the
# Flatpak manifests build offline from) from the workspace Cargo.lock, with
# flatpak-cargo-generator from flatpak-builder-tools. See docs/packaging.md.
#
# Needs python3 with the `aiohttp` and `toml` (or `tomlkit`) modules.
set -euo pipefail

cd "$(dirname "$0")/.."
# A fixed revision of flatpak-builder-tools, so the generator can't change under us.
REV="${FLATPAK_BUILDER_TOOLS_REV:-master}"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
curl -fsSL -o "$tmp/flatpak-cargo-generator.py" \
  "https://raw.githubusercontent.com/flatpak/flatpak-builder-tools/$REV/cargo/flatpak-cargo-generator.py"
python3 -I "$tmp/flatpak-cargo-generator.py" Cargo.lock -o packaging/flatpak/cargo-sources.json
echo "wrote packaging/flatpak/cargo-sources.json"
