#!/usr/bin/env bash
# Builds NoteLiner's release binary, packages it (deb and AppImage), and
# installs the deb of the version in apps/noteliner/native/Cargo.toml.
#
#   ./install-noteliner.sh
set -euo pipefail

cd "$(dirname "$0")"

cargo build --release -p noteliner-native
FORMATS="deb appimage" scripts/package-native.sh noteliner

# The `version` key of the [package] table.
VERSION="$(perl -0ne 'print $1 if /\[package\][^\[]*?\nversion\s*=\s*"([^"]*)"/' apps/noteliner/native/Cargo.toml)"
DEB="$(ls target/packages/noteliner/noteliner_"$VERSION"-*.deb 2>/dev/null | sort -V | tail -n 1)"
if [[ -z "$DEB" ]]; then
  echo "no deb for NoteLiner $VERSION in target/packages/noteliner/" >&2
  exit 1
fi

echo "Installing $DEB"
sudo apt install "./$DEB"
