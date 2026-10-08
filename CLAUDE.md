# Tunebox — guide for Claude

Native YouTube Music client in Rust (egui UI, own InnerTube client, own audio pipeline). Targets Ubuntu 24.04
and macOS arm64. Read `docs/STATUS.md` (status/limits), `docs/DEVELOPMENT.md`, `docs/ARCHITECTURE.md` (threads, data flow) and
`docs/DECISIONS.md` (every non-obvious choice **with the measurements behind it**) before changing design.

## Workspace
| Crate | Role |
|---|---|
| `ytm-core` | `Config` (TOML), `tracing` setup |
| `ytm-api` | InnerTube `WEB_REMIX` client, parsers (`parse.rs`, `pages.rs`), `MusicApi` + `StreamResolver` traits, stream resolvers (`stream.rs`) |
| `ytm-player` | `Queue`, HTTP range source (`remote.rs`), fMP4 demuxer (`fmp4.rs`), AAC decode, resample, `AudioBuf`, `Player` handle (`engine.rs`) |
| `ytm-app` | binary `tunebox`: `AppState` (`state.rs`), `local.rs` (on-device library, JSON), `backend.rs` (Action→Event), `views/`, `widgets.rs`, `theme.rs`, `thumbs.rs`, `media.rs` (MPRIS/MediaRemote), `tray.rs` (tray icon: `ksni` on Linux, `tray-icon` on Windows/macOS), `notify.rs` |

## Commands (all must stay green)
```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace                         # hermetic: no network or sound card
cargo test -p ytm-api --features live-tests    # hits real YouTube; run when touching api/stream code
cargo run -p ytm-player --example play -- "query" [--null]   # terminal player
```
Release build + `.deb` + Windows zip (`scripts/package-windows.sh`, Zig cross-compile): see `.claude/skills/tunebox-release`. System packages are listed in `docs/DEVELOPMENT.md`.

## Live-test every change
After any add/edit that affects behavior or UI, don't stop at fmt/clippy/tests: build release, run the app and look at
the result (screenshot of the affected view via `--screenshot out.png --shot-delay N`, check the log for errors; for
playback/tray/media-key changes drive them for real). On macOS follow the recipe in `tunebox-platform-check`; on Linux
`tunebox-ui-capture`. Report what you actually ran and saw, and what a human still has to check.

## Rules that are easy to break
* **UI thread never blocks and never mutates state while drawing.** Views are `fn(&mut Ui, &mut AppState)`; they
  push `UiAction`s into a local `Vec` and apply them after the closures. Network/audio work goes through
  `Action` → `Backend` (tokio) → `Event` → `AppState::apply`. Every backend result calls `request_repaint`.
* **No continuous repaint.** Wake-ups come from input, events, `request_repaint_after` (see
  `next_progress_repaint`). Don't call `request_repaint()` per frame. Avoid per-frame clones of big lists
  (search results are `Arc`s).
* **Parsers are defensive.** Walk JSON with `nav::{path,find_all,find_first}`; skip what you don't understand,
  never `unwrap` on response data. Every parser change needs a fixture test (`crates/ytm-api/tests/fixtures`).
* **YouTube-specific code lives only in `ytm-api`** behind traits. UI/player are tested with mocks.
* **No accounts.** The app deliberately has no sign-in (removed on request; see D19). Personal data = the local
  library only. Never log URLs with signatures.
* **Stream facts (measured, see D5/D6):** native `VISIONOS` client + visitor id works without PO token; URLs must
  be fetched in ≤1 MiB bounded ranges; decode AAC itag 140 only (no Opus decoder). `ytm-player` reads fMP4 with
  its own demuxer because symphonia's probe downloads the whole file.
* Local library: mutate `AppState.local` only through `UiAction`s and call `local_changed()` (menu refresh + persist).
  Local playlist ids start with `local:` and must never be fetched from YouTube (`ensure_loaded` guards this).
* Icons are drawn from dedicated families (`theme::icons()` / `fill_icons()`), never `FontId::proportional`
  (Inter shadows some private-use codepoints).
* Egui is 0.36 (`App::ui`, `Panel::..show`, `ctx.set_global_style`); APIs differ from older tutorials.
* Tray: state (tooltip line, label, playing badge) goes through `TrayState` + `TuneboxApp::sync_tray`, diffed once per
  frame. `vendor/ksni` is a patched copy (adds `XAyatanaLabel`, see its `VENDORED.md`, D27) wired by
  `[patch.crates-io]` — don't "clean it up" or bump `ksni` without re-applying the patch. Windows/macOS glyphs are
  drawn in code (`tray::glyphs`, D29).
* Close-to-tray on Wayland: `main` drops `WAYLAND_DISPLAY` (XWayland) because a minimized Wayland window never
  processes tray Show/Quit (D28). Notifications count "hidden to tray"/minimized as background.
* Edit `Cargo.toml` deps with care: workspace `rust-version = "1.95"` makes cargo pick MSRV-compatible versions.

## Environment gotchas
* Unattended screenshots: Wayland hides unfocused windows (no frame callbacks) → run with
  `env -u WAYLAND_DISPLAY`. See `tunebox-ui-capture` skill.
* Linux builds draw their own title bar (`views/titlebar.rs`, D20); the scripts' XWayland runs show it too.
* Never `pkill -f <text that is in your own command line>` (kill by PID).
* `.claude/worktrees/` can hold GBs of other jobs' build output; never zip/scan the tree without excluding it.
* macOS (Apple Silicon) had a full pass on 2026-10-08 (views, CoreAudio, MediaRemote Now Playing + commands, notifications,
  menu-bar icon, local library, `.pkg` payload; see `docs/STATUS.md`). Installing the `.pkg` with Installer on another Mac
  has not been run. Re-check what you touch; don't extrapolate from that pass.
* Don't commit unless the user asks.

## Skills
`ytm-api-fixtures` (record/refresh fixtures, fix a broken parser) · `ytm-stream-debug` (playback won't start) ·
`tunebox-ui-capture` (run, screenshot, measure CPU/RSS) · `tunebox-add-feature` (new page/action/endpoint) ·
`tunebox-release` (verify, build, package).
