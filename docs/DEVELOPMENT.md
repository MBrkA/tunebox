# Development guide

Build, run, test and package Tunebox. For design see [ARCHITECTURE.md](ARCHITECTURE.md) and [DECISIONS.md](DECISIONS.md); for what is verified where see [STATUS.md](STATUS.md).

## Build from source

Requires a recent stable Rust (**1.95+**, see `rust-toolchain.toml`).

### Ubuntu 24.04

```sh
sudo apt install build-essential pkg-config libasound2-dev libssl-dev libdbus-1-dev \
  libxkbcommon-dev libwayland-dev libgl1-mesa-dev libxcb-render0-dev libxcb-shape0-dev \
  libxcb-xfixes0-dev libgtk-3-dev
cargo build --release
./target/release/tunebox
```

### macOS (Apple Silicon)

```sh
xcode-select --install      # command line tools
cargo build --release
./target/release/tunebox
```

No other manual steps. Audio uses CoreAudio (macOS) / ALSA via PipeWire or PulseAudio (Linux). TLS is rustls.

### Optional: `yt-dlp` fallback

If the native stream resolver ever breaks, Tunebox falls back to a `yt-dlp` binary on `PATH`
(or `ytdlp_path` in the config file). It is not required.

## Run / develop

```sh
cargo run -p ytm-app                                   # the app (binary name: tunebox)
cargo run -p ytm-api    --example search -- "query"    # print results + resolved stream URL
cargo run -p ytm-player --example play   -- "query"    # terminal player (add --null for no sound)
cargo test --workspace                                 # unit + hermetic integration tests
cargo test -p ytm-api --features live-tests            # also hit the live service
cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all --check
```

Config lives in the platform config directory (`config.toml`); thumbnails are cached in the platform cache
directory (size-capped by `thumbnail_cache_mb`). Set `RUST_LOG=ytm_player=debug` for verbose logs.

Hidden developer flags of `tunebox` (used for automated screenshots): `--query <q>`, `--filter <songs|albums|…>`,
`--route <home|explore|album:ID|artist:ID|playlist:ID>`, `--play-first`, `--now-playing`, `--queue`, `--local-demo`, `--edit-playlist`,
`--help-overlay`, `--size <W>x<H>`, `--screenshot <png>`, `--shot-delay <secs>`. Tip: on Wayland a window the compositor considers hidden gets no frame
callbacks and never takes its screenshot; run with `env -u WAYLAND_DISPLAY` (XWayland) for unattended captures.

## Window title bar (Linux)

GNOME's Wayland compositor does not draw title bars for apps, and a native Wayland window otherwise gets an imitation
one from the windowing library. So on Linux Tunebox draws its own themed title bar (minimize, maximize/restore, close;
drag the empty area to move, double-click to maximize) and resize handles on the window edges, and runs natively on
Wayland with GPU rendering. Set `TUNEBOX_NATIVE_TITLEBAR=1` to use the system title bar instead (on GNOME/Wayland that
is the library's imitation). macOS always uses its native title bar.

## Windows

A 64-bit Windows 10/11 build is produced two ways:

* **From Linux** (what produced the shipped zip): `rustup target add x86_64-pc-windows-gnu`, `cargo install cargo-zigbuild --locked`,
  put [Zig](https://ziglang.org/download/) on `PATH`, then `scripts/package-windows.sh` → `dist/tunebox_<ver>_windows-x64.zip`
  (a single `tunebox.exe`, ~27 MB, no installer or extra DLLs; Zig also compiles the icon/version resources).
* **On Windows / CI**: `cargo build --release -p ytm-app` (MSVC); the `windows` CI job uploads the zip as an artifact.

**Not yet run on Windows** (no Windows machine or Wine was available): the exe is verified to be a valid 64-bit GUI
executable with an embedded icon, importing only standard Windows DLLs. Behaviour that is untested there: rendering
(wgpu → DirectX 12 / Vulkan), audio (WASAPI via cpal), media keys / the system media overlay (souvlaki, needs the window
handle), the data path (`%APPDATA%\tunebox\Tunebox\data\library.json`), and the `yt-dlp` fallback. The exe is not
code-signed, so SmartScreen will warn on first run.

## Packaging

```sh
cargo install cargo-packager --locked
cargo build --release -p ytm-app
cd crates/ytm-app
cargo packager --release --formats deb          # Linux: .deb (add appimage if you have appimagetool access)
cargo packager --release --formats app,dmg      # macOS: .app and .dmg
```

Output goes to `dist/`. The CI workflow builds all of them on demand (`workflow_dispatch` with `package: true`).

