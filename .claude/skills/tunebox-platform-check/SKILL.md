---
name: tunebox-platform-check
description: Per-OS verification checklist for Tunebox (Linux, macOS, Windows) covering tray, media keys, notifications, close-to-tray and audio, with what has and hasn't actually been run. Use when touching tray.rs, media.rs, notify.rs, window/viewport code, or before reporting a cross-platform change as working.
---

# Platform check

Check the host first: `uname -s`. Only claim what you ran on *this* host; everything else is "compiles" or "untested".
Last recorded state is in `docs/STATUS.md` and D26–D29 in `docs/DECISIONS.md` — read them and update them if you learn more.

| Area | Code | Linux (Ubuntu 24.04) | macOS arm64 | Windows 10/11 |
|---|---|---|---|---|
| Tray | `tray.rs` | `ksni` (SNI over D-Bus), patched `vendor/ksni` (D27) | `tray-icon`; glyphs in code (D29); menu-bar icon not specifically checked | `tray-icon`; compile-checked only |
| Media keys | `media.rs` | MPRIS via souvlaki | MediaRemote via souvlaki | needs window handle; untested |
| Notifications | `notify.rs` | `notify-rust` on a short thread; only when playing and window unfocused | check separately | check separately |
| Title bar | `views/titlebar.rs` | own title bar (D20); `TUNEBOX_NATIVE_TITLEBAR=1` for system | native | native |
| Close-to-tray | `main.rs` | drops `WAYLAND_DISPLAY` → XWayland (D28) | n/a | n/a |
| Audio | `ytm-player/output.rs` (cpal) | ALSA/PipeWire/Pulse | CoreAudio | WASAPI; untested |

## Any host
```sh
cargo fmt --all --check && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo test --workspace
```

## Linux
* MPRIS: `busctl --user introspect org.mpris.MediaPlayer2.tunebox /org/mpris/MediaPlayer2` (PlaybackStatus, Metadata).
* Tray: `busctl --user introspect org.kde.StatusNotifierItem-<pid>-1 /StatusNotifierItem` must list `XAyatanaLabel`;
  dbusmenu `Event isvu <id> clicked s "" 0` on `/MenuBar` drives Show(1)/Quit(7). Needs a StatusNotifierHost
  (stock GNOME has none → close-to-tray is ignored by design).
* Close-to-tray: `xwininfo -root -tree | grep Tunebox` shows the XWayland window.
* Notification: `dbus-monitor "interface='org.freedesktop.Notifications'"` sees one `Notify` per track.
* Kill test instances by PID, never `pkill -f` with text from your own command line.

## macOS (this machine may be one)
* `cargo build --release -p ytm-app && ./target/release/tunebox`; bundle: `cd crates/ytm-app && cargo packager --release --formats app,dmg` → `dist/`.
* `scripts/shot.sh`, `measure*.sh` rely on Linux `/proc` and XWayland — don't use them; screenshot with `--screenshot out.png --shot-delay N` directly.
* Live test recipe (verified on Apple Silicon):
  1. `cargo` may not be on PATH in agent shells: `export PATH=$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH`.
  2. Hermetic gate: `cargo fmt --all --check && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo test --workspace`.
  3. `cargo build --release -p ytm-app`, then launch with a screenshot:
     `./target/release/tunebox --screenshot $OUT/home.png --shot-delay 6 > $OUT/run.log 2>&1`
     (exits by itself, even with `close_to_tray = true`; run in background if the harness blocks). Read the PNG (2560x1632 on Retina).
  4. Check `run.log`: expect "media controls attached" and "first frame" (~1 s); no errors.
  5. Your real config/session is used: `~/Library/Application Support/dev.tunebox.Tunebox/` (`config.toml`, `session.json`),
     so the screenshot shows the restored track and settings. If a run hangs, `ps aux | grep release/tunebox` and `kill <pid>`.
  6. Not covered by this recipe (needs a human): audible sound, media keys / Now Playing, menu-bar icon, notifications, Finder launch of the `.app`.
* Verify by eye: menu-bar icon (template image, light + dark), tray menu glyphs, media keys / Now Playing widget,
  Retina scaling, `.app` launches from Finder. Unsigned/un-notarised: Gatekeeper will warn.

## Windows
* Cross-compile check only: `scripts/package-windows.sh` or `cargo zigbuild --target x86_64-pc-windows-gnu -p ytm-app`
  (`file target/x86_64-pc-windows-gnu/release/tunebox.exe` → PE32+ GUI). Nothing has run on Windows; say so.

## Reporting
List per OS: *ran and observed* / *compiled only* / *not touched*. Audible sound has never been verified by a human
unless the user says so.
