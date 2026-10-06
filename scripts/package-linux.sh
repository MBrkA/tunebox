#!/bin/bash
# Builds the Linux release binary and packages it as dist/tunebox_<ver>_amd64.deb (via cargo-packager).
# Needs: `cargo install cargo-packager --locked`, system packages from docs/DEVELOPMENT.md.
set -euo pipefail
cd "$(dirname "$0")/.."

export PATH="$HOME/.cargo/bin:$PATH"
if ! cargo packager --version >/dev/null 2>&1; then
  echo "cargo-packager missing: cargo install cargo-packager --locked" >&2
  exit 1
fi

cargo build --release -p ytm-app
mkdir -p dist
(cd crates/ytm-app && cargo packager --release --formats deb)
DEB=$(ls -t dist/*.deb | head -1)
echo "== $DEB ($(du -h "$DEB" | cut -f1))"
dpkg-deb -I "$DEB" | sed -n '1,/Description/p'
