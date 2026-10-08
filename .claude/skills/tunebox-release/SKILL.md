---
name: tunebox-release
description: Pre-release verification and packaging checklist for Tunebox (.deb/AppImage on Linux, .app/.dmg on macOS, Windows .exe zip), including the project zip and the CI tag-to-draft-release flow. Use before tagging or handing off a build.
---

# Verify, build, package

Check the host first (`uname -s`): Linux builds the `.deb` and the Windows zip; macOS builds `.app`/`.dmg`. Each artifact
can only be built (and smoke-tested) on its own OS, except the Windows zip, which is cross-compiled from Linux. Per-OS
runtime checks (tray, media keys, notifications): see `tunebox-platform-check`.

## 1. Verify (any host)
1. `cargo fmt --all --check && cargo clippy --workspace --all-targets --all-features -- -D warnings`
2. `cargo test --workspace`; with network: `cargo test -p ytm-api --features live-tests`
3. `cargo build --release -p ytm-app` (binary `target/release/tunebox`, ≈35 MB stripped).
4. Smoke test with `tunebox-ui-capture` shots of home, a search, now-playing (Linux scripts; on macOS use
   `--screenshot out.png --shot-delay N` directly).

## 2. Package (version = workspace `Cargo.toml`, currently 0.1.0)
* **Linux `.deb`:** `scripts/package-linux.sh` (needs `cargo install cargo-packager --locked` once and the apt packages in
  `docs/DEVELOPMENT.md`) → `dist/tunebox_<ver>_amd64.deb`, prints `dpkg-deb -I`. Inspect contents with `dpkg-deb -c`.
* **macOS `.pkg` (ship this):** `scripts/package-macos.sh` → ad-hoc signed `dist/Tunebox.app` and
  `dist/tunebox_<ver>_macos-arm64.pkg` (installs to `/Applications`, postinstall clears quarantine; D37). A bare `.dmg`
  (`cd crates/ytm-app && cargo packager --release --formats dmg`) shows "damaged" on other Macs — don't hand it out.
  Not notarised: first open of the `.pkg` needs Privacy & Security → Open Anyway.
* **macOS `.app`/`.dmg`:** `cargo build --release -p ytm-app && cd crates/ytm-app && cargo packager --release --formats app,dmg`
  → `dist/Tunebox.app`, `dist/Tunebox_<ver>_aarch64.dmg`. Only hand out the `.dmg` once signing is set up (below); unsigned it shows "damaged" (D37). Built on a Mac;
  launch the `.app` from Finder and check by hand before calling it good.
* **Windows zip** (cross-compiled from Linux): `scripts/package-windows.sh` → `dist/tunebox_<ver>_windows-x64.zip` (single
  `tunebox.exe`, ~27 MB, no installer/DLLs). One-time: `rustup target add x86_64-pc-windows-gnu`,
  `cargo install cargo-zigbuild --locked`, Zig on `PATH` (also compiles the icon/version resource via `build.rs`,
  `assets/windows/`). Zig location is machine-specific (the Linux dev box used `~/.local/zig-x86_64-linux-0.17.0`); check
  `which zig`. Verify with `file target/x86_64-pc-windows-gnu/release/tunebox.exe` (PE32+ GUI). Nothing has run on
  Windows — say so. On a Windows box: `cargo build --release -p ytm-app`.
* **Freshness:** `ls -la dist/` and compare names/versions/timestamps with the version being handed off. A stale or missing
  artifact (e.g. a `.dmg` but no `.deb`, or a zip older than the last code change) must be rebuilt or called out. Rebuild
  the Windows zip whenever the `.deb` is rebuilt if the user asked for both.
* **Icons:** source is `crates/ytm-app/assets/icons/make_icons.py` (pure Python, ~90 s); rerun after design changes and
  rebuild to re-embed.

## 3. Project zip
`scripts/make-zip.sh` (source only; excludes `target/`, `dist/`, `.git`, `.claude/worktrees/` — that one holds GBs of other
jobs' builds and made `zip` hang for 20+ min). Output `../tunebox-<date>.zip`; it must contain `vendor/ksni` (patched
tray crate, D27).

## 4. CI and tagging (`.github/workflows/ci.yml`)
* Flow: `check` → `package` → `release`, each a matrix over ubuntu-24.04, macos-14 and windows-2022 (release: one job).
* `check`: every push/PR. fmt, clippy, hermetic tests on Linux/macOS; on Windows the tests must compile but their
  result is informational (no fmt/clippy gate there yet). No release build (it cost 2-6 min per push).
* `package`: manual dispatch with the `package` box ticked, or a `v*` tag. Linux → `deb,appimage`; macOS →
  `scripts/package-macos.sh` (`.pkg`, D37) or, when the Apple secrets are set, signed `app,dmg`; Windows → MSVC release
  exe zipped as `tunebox_<ver>_windows-x64.zip`. Only installer files (`.deb`, `.AppImage`, `.pkg`, `.dmg`, `.zip`) are
  uploaded, not the `.app` directory.
* Concurrency groups include the event, so a push to `main` doesn't cancel a manual packaging run.
* `release`: only on a `v*` tag; needs `package`, attaches all artifacts to a **draft** GitHub release with
  generated notes. Review and publish it by hand; never publish on the user's behalf unasked.
* Before tagging: bump the workspace version, make sure `docs/STATUS.md` is current, and don't tag unless the user asks.

## 5. Report
State what was and wasn't verified, per OS: what ran, what only compiled (Windows exe, Windows/macOS tray paths via
`cargo zigbuild --target x86_64-pc-windows-gnu`), and what no human has checked (audible sound, individual macOS features
such as the menu-bar icon, media keys, notifications). Kill any test instances by PID.

## macOS signing and notarisation (wired in CI, never run yet)
Needs a paid Apple Developer account. In the repository secrets set: `APPLE_CERTIFICATE` (base64 of the "Developer ID
Application" `.p12`: `base64 -i cert.p12 | pbcopy`), `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY` (e.g.
`Developer ID Application: Name (TEAMID)`), `APPLE_ID`, `APPLE_PASSWORD` (an app-specific password) and `APPLE_TEAM_ID`.
The `package` job then signs with the hardened runtime and notarises through `notarytool` (cargo-packager 0.11.8 does
both when those variables exist); with no secrets it packages unsigned as before. Locally the same works by exporting the
variables and running `cargo packager --release --formats app,dmg --config '{"macos":{"signingIdentity":"..."}}'`.
After the first signed build check: `codesign --verify --deep --strict Tunebox.app`, `spctl -a -vv Tunebox.app` and
`xcrun stapler validate Tunebox_*.dmg`. If notarisation rejects the app, `entitlements` (macOS packager option) is the
next thing to look at.
