#!/bin/sh
# Build a release zip: nouwall-<version>-windows-x64.zip in dist/.
# Usage: scripts/dist.sh [--no-build] [extra sample files...]
# Samples passed as arguments go into samples/, which nouwall imports on
# first start.
set -eu
cd "$(dirname "$0")/.."
TARGET=x86_64-pc-windows-msvc
if [ "${1:-}" = "--no-build" ]; then shift; else
    if command -v cargo-xwin >/dev/null 2>&1; then
        cargo xwin build --release --target "$TARGET"
    else
        cargo build --release --target "$TARGET"
    fi
fi
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
NAME="nouwall-$VERSION-windows-x64"
rm -rf "dist/$NAME" "dist/$NAME.zip"
mkdir -p "dist/$NAME"
cp "target/$TARGET/release/nouwall.exe" "target/$TARGET/release/nouwallctl.exe" \
   README.md README.id.md CHANGELOG.md LICENSE-MIT LICENSE-APACHE "dist/$NAME/"
if [ $# -gt 0 ]; then
    mkdir -p "dist/$NAME/samples"
    cp "$@" "dist/$NAME/samples/"
fi
(cd dist && python3 -c "
import shutil, sys; shutil.make_archive(sys.argv[1], 'zip', '.', sys.argv[1])" "$NAME")
ls -l "dist/$NAME.zip"
