#!/bin/bash
# Cross-compiles tunebox.exe for 64-bit Windows from Linux and zips it into dist/.
# Needs: rustup target x86_64-pc-windows-gnu, cargo-zigbuild, and zig on PATH (also provides `zig rc` for the icon).
#   rustup target add x86_64-pc-windows-gnu && cargo install cargo-zigbuild --locked
set -euo pipefail
cd "$(dirname "$0")/.."
cargo zigbuild --release -p ytm-app --target x86_64-pc-windows-gnu
STAGE=$(mktemp -d)/tunebox-windows-x64
mkdir -p "$STAGE" dist
cp target/x86_64-pc-windows-gnu/release/tunebox.exe "$STAGE/"
cp LICENSE "$STAGE/LICENSE.txt"
cp scripts/windows-README.txt "$STAGE/README.txt"
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
OUT="$PWD/dist/tunebox_${VERSION}_windows-x64.zip"
rm -f "$OUT"
(cd "$(dirname "$STAGE")" && zip -qr "$OUT" "$(basename "$STAGE")")
echo "$OUT ($(du -h "$OUT" | cut -f1))"
unzip -l "$OUT"
