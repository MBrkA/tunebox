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
cat > "$STAGE/README.txt" <<'TXT'
Tunebox - YouTube Music desktop client (Windows 10/11, 64-bit)

Run tunebox.exe. No installer, no account. Your liked songs and playlists are stored in
  %APPDATA%\tunebox\Tunebox\data\library.json   (copy it to back up your library)

Notes
* Windows SmartScreen may say "Windows protected your PC" because the file is not code-signed:
  choose "More info" -> "Run anyway".
* Needs a DirectX 12 or Vulkan capable graphics driver (any PC from the last ~8 years). The app
  falls back to OpenGL if neither works.
* Optional: put yt-dlp.exe on your PATH; Tunebox uses it automatically if YouTube changes
  something that breaks its built-in stream resolver.
* Unofficial: Tunebox uses YouTube's undocumented API and may stop working at any time. Personal use only.
TXT
OUT="$PWD/dist/tunebox-windows-x64.zip"
rm -f "$OUT"
(cd "$(dirname "$STAGE")" && zip -qr "$OUT" "$(basename "$STAGE")")
echo "$OUT ($(du -h "$OUT" | cut -f1))"
unzip -l "$OUT"
