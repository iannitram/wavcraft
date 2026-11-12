#!/usr/bin/env bash
# Wrap the release build of the wavcraft app as WavCraft.app (macOS).
#
#   bash tools/macos/bundle.sh
#
# Builds `cargo build --release -p wavcraft` (into $CARGO_TARGET_DIR, default `target`), then writes
# <target>/release/WavCraft.app: the binary as Contents/MacOS/WavCraft, Info.plist from
# tools/macos/Info.plist with the workspace version filled in, and an empty Contents/Resources.
# An existing bundle is replaced. The last line printed is the .app path.
set -euo pipefail

if [ "$(uname -s)" != "Darwin" ]; then
    echo "bundle.sh: this script makes a macOS app bundle; run it on a Mac" >&2
    exit 1
fi

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$script_dir/../.." && pwd)"
cd "$repo"

target="${CARGO_TARGET_DIR:-target}"

# The binary is the package name: apps/wavcraft/Cargo.toml has no [[bin]] table.
binary_name="wavcraft"
app_name="WavCraft"

# The version comes from [workspace.package] in the root Cargo.toml (apps/wavcraft inherits it).
version="$(awk '
    /^\[/ { in_pkg = ($0 == "[workspace.package]") }
    in_pkg && /^version[[:space:]]*=/ {
        line = $0
        sub(/^[^"]*"/, "", line)
        sub(/".*$/, "", line)
        print line
        exit
    }
' Cargo.toml)"
if [[ ! "$version" =~ ^[0-9]+(\.[0-9]+)*$ ]]; then
    echo "bundle.sh: could not read a plain version from [workspace.package] in Cargo.toml" >&2
    exit 1
fi

cargo build --release -p wavcraft

binary="$target/release/$binary_name"
if [ ! -x "$binary" ]; then
    echo "bundle.sh: no release binary at $binary after the build" >&2
    exit 1
fi

app="$target/release/$app_name.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$binary" "$app/Contents/MacOS/$app_name"
chmod 755 "$app/Contents/MacOS/$app_name"
sed "s/@VERSION@/$version/g" tools/macos/Info.plist > "$app/Contents/Info.plist"

# Ad-hoc signature, so the bundle has one identity for macOS (microphone permission, Gatekeeper).
if command -v codesign >/dev/null 2>&1; then
    codesign --force --sign - "$app" >/dev/null
fi

echo "$app"
