#!/bin/bash
# Builds the macOS release and packages it as dist/Tunebox.app and dist/Tunebox_<ver>_aarch64.pkg (installer: copies
# Tunebox.app to /Applications and clears the quarantine flag). The .dmg is a separate step
# (`cargo packager --release --formats dmg`); it re-bundles the .app, dropping the signature added here.
# Needs: `cargo install cargo-packager --locked`, Xcode command line tools (codesign, pkgbuild).
#
# The app is not Developer ID signed or notarised. A quarantined, bundle-unsigned .app copied from the .dmg is
# reported as "damaged" by Gatekeeper on other Macs; the .pkg avoids that (see docs/DECISIONS.md D37). The .pkg itself
# is unsigned, so the first open still needs System Settings → Privacy & Security → Open Anyway.
set -euo pipefail
cd "$(dirname "$0")/.."

export PATH="$HOME/.cargo/bin:$PATH"
if ! cargo packager --version >/dev/null 2>&1; then
  echo "cargo-packager missing: cargo install cargo-packager --locked" >&2
  exit 1
fi

VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
ARCH=$(uname -m)
[ "$ARCH" = arm64 ] && ARCH=aarch64
APP=dist/Tunebox.app
PKG=dist/Tunebox_${VERSION}_${ARCH}.pkg

cargo build --release -p ytm-app
mkdir -p dist
rm -rf "$APP"
(cd crates/ytm-app && cargo packager --release --formats app)

# Seal the whole bundle with an ad-hoc signature (the linker only signs the inner binary).
codesign --force --deep --sign - "$APP"
codesign --verify --deep --strict "$APP"

# Installer payload: <root>/Tunebox.app installed to /Applications.
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
mkdir -p "$WORK/root" "$WORK/scripts"
ditto "$APP" "$WORK/root/Tunebox.app"

# Not relocatable: otherwise Installer "upgrades" any other Tunebox.app it finds on disk (e.g. a dev build in dist/)
# instead of writing to /Applications.
pkgbuild --analyze --root "$WORK/root" "$WORK/components.plist" >/dev/null
plutil -replace 0.BundleIsRelocatable -bool NO "$WORK/components.plist"

cat >"$WORK/scripts/postinstall" <<'EOF'
#!/bin/sh
# $3 = target volume. Installer does not quarantine payload files; clear it anyway in case an older copy carried it.
/usr/bin/xattr -dr com.apple.quarantine "${3%/}/Applications/Tunebox.app" 2>/dev/null || true
exit 0
EOF
chmod +x "$WORK/scripts/postinstall"

rm -f "$PKG"
pkgbuild --root "$WORK/root" \
  --component-plist "$WORK/components.plist" \
  --scripts "$WORK/scripts" \
  --identifier dev.tunebox.Tunebox.pkg \
  --version "$VERSION" \
  --install-location /Applications \
  "$PKG"

echo "== $PKG ($(du -h "$PKG" | cut -f1))"
pkgutil --payload-files "$PKG" | head -5
