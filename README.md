# Tunebox

A fast, native YouTube Music player for the desktop. No browser, no Electron, no web view: Tunebox talks to
YouTube directly, decodes audio itself and draws its own UI with [egui](https://github.com/emilk/egui).
Opens its first frame in about 100 ms and idles at roughly 100 MB of RAM.

<!-- TODO: add a screenshot (see .claude/skills/tunebox-ui-capture) -->

## Features

* Search with suggestions; browse Home, Explore (new releases, charts, moods & genres), artists, albums and playlists
* Queue, shuffle, repeat, gapless playback, up-next radio and lyrics
* **No account, no sign-in.** Likes, playlists and saved albums/artists stay on your device as plain JSON
* Media keys and system media controls (MPRIS on Linux), tray icon with play/pause/next/previous, song-change notifications
* Dark and light themes, many interface languages, HiDPI, resizable down to 640×480
* Keyboard-driven: press **?** for the shortcut cheat-sheet

## Measurements

Absolute numbers, not CPU percentages (a percentage depends on the core count, clock speed and OS, so it does
not compare across machines). Release build, 20-track queue unless noted. Reproduce with the
[`tunebox-ui-capture`](.claude/skills/tunebox-ui-capture/SKILL.md) scripts.

| Metric | Value | Measured on |
|---|---|---|
| Cold start to first frame | ≈ 100 ms | Linux, release build |
| UI work per frame | ≈ 0.65 ms (about 3 repaints/s while playing, so ≈ 2 ms of UI CPU per second) | Linux, release build |
| Idle CPU time | 0.23 s of CPU over 30 s (≈ 8 ms per second), window open, nothing playing | Apple M2, macOS |
| Memory (RSS) | ≈ 140 MB (GPU renderer) / ≈ 210 MB (software rasteriser) | Linux, release build |
| Memory (RSS), idle window | ≈ 102 MB | Apple M2, macOS |
| Threads | 21 | Apple M2, macOS |
| Executable | 29 MB (macOS arm64), ≈ 27 MB (Windows) | release builds |
| Installer | 10.6 MB `.dmg`; `.app` bundle 23 MB on disk | macOS arm64 |

These are single runs by the author, not a benchmark suite, and they are not a head-to-head against a browser:
no web-client numbers were measured for this README. To compare on your own machine, run Tunebox and
music.youtube.com side by side and look at the process totals in Activity Monitor / `htop` (add up every renderer
and helper process for the browser, not just the tab).

## Install

| Platform | Status | How |
|---|---|---|
| Ubuntu 24.04 (x86_64) | Tested | `.deb` from [Releases](../../releases): `sudo apt install ./tunebox_*.deb` |
| macOS (Apple Silicon) | Builds, packages and runs; only lightly tested | `.pkg` from Releases (unsigned: first open via System Settings → Privacy & Security → Open Anyway) |
| Windows 10/11 (x64) | **Untested** | unzip and run `tunebox.exe` (unsigned: SmartScreen will warn) |

### Build from source

Needs Rust 1.95+.

```sh
# Ubuntu 24.04
sudo apt install build-essential pkg-config libasound2-dev libssl-dev libdbus-1-dev \
  libxkbcommon-dev libwayland-dev libgl1-mesa-dev libxcb-render0-dev libxcb-shape0-dev \
  libxcb-xfixes0-dev libgtk-3-dev
# macOS: xcode-select --install

cargo build --release
./target/release/tunebox
```

## Using it

* Search with `Ctrl`+`K` or `/`; play/pause with `Space`; heart a song to like it; right-click for *Add to playlist*.
* Your data lives in `~/.local/share/tunebox/` on Linux (Settings → Application data shows and can move it, and backs it up).
* On GNOME the tray icon needs the AppIndicator extension (Ubuntu ships it enabled).

More in the [user guide](docs/USER_GUIDE.md).

## Limitations

* Uses YouTube's unofficial, undocumented API, so it can break without notice (a `yt-dlp` fallback is built in if installed).
* Plays AAC streams only; no downloads or offline mode; lyrics are not time-synced; the library has no sync between devices.

Full list and what has been verified where: [docs/STATUS.md](docs/STATUS.md).

## Contributing

Start with the [development guide](docs/DEVELOPMENT.md), then [ARCHITECTURE](docs/ARCHITECTURE.md) and
[DECISIONS](docs/DECISIONS.md) (every non-obvious choice, with measurements).

## Disclaimer

Tunebox uses YouTube's *unofficial* InnerTube API and may conflict with the YouTube / YouTube Music Terms of Service.
It is for personal use only, does not bypass paid-tier restrictions or DRM, and does not download or export media.
Not affiliated with or endorsed by Google or YouTube.

## License

MIT, see [LICENSE](LICENSE).
