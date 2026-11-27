#!/usr/bin/env bash
# Builds BlockLiner's release binary, packages it (deb and AppImage), and
# installs the deb of the version in apps/blockliner/native/Cargo.toml.
#
#   ./install-blockliner.sh
set -euo pipefail

cd "$(dirname "$0")"

cargo build --release -p blockliner-native
FORMATS="deb appimage" scripts/package-native.sh blockliner

# The `version` key of the [package] table.
VERSION="$(perl -0ne 'print $1 if /\[package\][^\[]*?\nversion\s*=\s*"([^"]*)"/' apps/blockliner/native/Cargo.toml)"
DEB="$(ls target/packages/blockliner/blockliner_"$VERSION"-*.deb 2>/dev/null | sort -V | tail -n 1)"
if [[ -z "$DEB" ]]; then
  echo "no deb for BlockLiner $VERSION in target/packages/blockliner/" >&2
  exit 1
fi

echo "Installing $DEB"
sudo apt install "./$DEB"
