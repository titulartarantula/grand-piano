#!/usr/bin/env bash
# Build the Windows setup program, dist/GrandPiano-Setup-<version>.exe, on Linux.
#
# Needs: cargo-xwin (see README, "Building") and NSIS 3 (Debian/Ubuntu: apt install nsis).
# Usage: packaging/build-installer.sh [--no-build]
#   --no-build  package the exe already in target/ instead of building it first
#
# The version comes from Cargo.toml. On a small machine, set
# CARGO_BUILD_JOBS=1 CARGO_PROFILE_RELEASE_LTO=false to keep memory down.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

target=x86_64-pc-windows-msvc
version="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n1)"
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "Can't read a x.y.z version from Cargo.toml (got '$version')" >&2; exit 1; }

command -v makensis >/dev/null || { echo "makensis not found: install NSIS 3 (apt install nsis)" >&2; exit 1; }

if [[ "${1:-}" != "--no-build" ]]; then
    cargo xwin build --release --target "$target"
fi
exe="target/$target/release/GrandPiano.exe"
[[ -f "$exe" ]] || { echo "$exe not found; build it first" >&2; exit 1; }

stage="$root/target/installer-stage"
rm -rf "$stage"
mkdir -p "$stage" dist
cp "$exe" "$stage/GrandPiano.exe"
cp assets/icon.ico "$stage/GrandPiano.ico"
cp LICENSE packaging/LICENSE-SAMPLES.txt README.md "$stage/"
# The licence page shows both licences; Windows line endings for the text box.
{ cat LICENSE; printf '\n\n'; cat packaging/LICENSE-SAMPLES.txt; } | sed 's/$/\r/' > "$stage/LICENSES.txt"

out="dist/GrandPiano-Setup-$version.exe"
makensis -V2 -WX -DVERSION="$version" -DSTAGE="$stage" -DOUTFILE="$root/$out" packaging/installer.nsi
sha256sum "$out" | tee "$out.sha256"
