# Decisions

Each entry: context, decision, trade-off.

## D1. Workspace layout
Five crates: `ytm-core` (config, logging; added beyond the brief so that api/player/auth/app share one
config type without depending on each other), `ytm-api`, `ytm-player`, `ytm-auth`, `ytm-app`.

## D2. Config
TOML at the platform config dir (`directories`), all fields `#[serde(default)]` so old files keep loading.

## D3. Testing limits
Development happens mainly on Ubuntu. macOS is built by the CI matrix (`macos-14`) and has been built, packaged and run
by the author on an Apple Silicon Mac; the macOS-specific parts (MediaRemote via souvlaki, Keychain, bundling) were not
individually checked.
The Dribbble shot could not be viewed; the fallback visual spec from the brief is used.

## D4. InnerTube parsing: `serde_json::Value` + path helpers, not rigid structs
Responses are walked with `nav::{path, find_all, find_first}` and mapped into our own typed models
(`models.rs`). Rows are discovered by key (`musicResponsiveListItemRenderer`) and classified by their
navigation endpoint (`watchEndpoint` / `browseEndpoint.pageType`), so layout changes above the row level
(tabs, shelves, section nesting) do not break parsing. Anything unrecognised is skipped, never an error.
Fixtures in `crates/ytm-api/tests/fixtures` are recorded from the live API.

## D5. Stream resolution: native `VISIONOS` client, `yt-dlp` fallback, no JS runtime
Measured on 2026-10-05 against the live service (all with `videoId` from search):

| Client | Result |
|---|---|
| `WEB_REMIX` player | `UNPLAYABLE` |
| `ANDROID_VR`, `ANDROID_MUSIC` | `LOGIN_REQUIRED` ("confirm you're not a bot") |
| `TVHTML5_SIMPLY_EMBEDDED_PLAYER` | "no longer supported" |
| `IOS` | URLs resolve but **HTTP 403 after the first ~1 MiB** (PO-token gate) — unusable for playback |
| `VISIONOS` + `X-Goog-Visitor-Id` | `OK`, plain `url` (no cipher), whole file streams in 1 MiB ranges, no PO token |

The `VISIONOS` client is what current yt-dlp uses by default. We call it directly; the visitor id comes from
the `responseContext.visitorData` of any earlier InnerTube response (no watch-page scrape, no cookies).
Because the URLs carry no `signatureCipher`, neither signature deciphering nor `n`-parameter solving was
needed, so we embed **no JS runtime** (`boa`/`deno_core` would add ~tens of MB and a large attack surface).
Options considered:
- `rusty_ytdl`: depends on a JS-eval path for ciphers and tracks YouTube changes slowly; rejected.
- Embedded JS runtime: only needed if we must decipher; deferred until/unless native resolution stops working.
- `yt-dlp` subprocess: kept as an automatic fallback (`ChainResolver`), configurable via `ytdlp_path`.
  Trade-off: external binary, ~1 s start-up, but updated by its community within days of breakage.

Risk: client names/versions (`VISIONOS 1.02`) are a moving target; the whole thing is isolated in
`ytm-api/src/stream.rs` behind `StreamResolver`. A `live-tests` test fails loudly if the 1 MiB gate returns.

## D6. Audio format: itag 140 (AAC-LC 128k) preferred over 251 (Opus)
The brief asks for 251/140. symphonia 0.5 has no Opus decoder; supporting 251 would need libopus (C
dependency, extra packaging on macOS). 140 is pure Rust via symphonia (`isomp4`+`aac`), fragmented MP4 decode
and seeking verified. Default order is therefore `[140, 139]`; 251 can be added when an Opus decoder is
integrated. Stream URLs must be fetched in bounded `Range` requests (≤1 MiB each worked; open-ended
ranges are rejected).

## D7. TLS: rustls (via reqwest 0.13), not OpenSSL
Avoids linking system OpenSSL on macOS and keeps one TLS stack. `libssl-dev` is therefore not strictly
required, but is still listed for the Linux packages (the `keyring`/dbus stack and some tools may use it).

## D8. Own fragmented-MP4 demuxer (`ytm-player/src/fmp4.rs`)
Measured: symphonia 0.5.5's `isomp4` reader walks **every** `moof` while probing a fragmented file
(`max_offset == file_len` on a 5 MB track), so a 7-minute track needed ~10 s of downloading before the first
sample. YouTube's DASH audio always has a `sidx` index, so we read only `ftyp/moov/sidx`, fetch one ~5 s
segment at a time, and hand raw AAC frames to symphonia's AAC decoder (built from the `esds`
AudioSpecificConfig). Seeking = `sidx` lookup + decode-and-discard up to the target frame.
Result: decoder open 9.9 s → 0.56 s on the same track; seeks into not-yet-downloaded regions only need
the one chunk. Non-fMP4 inputs (plain `.m4a` from the yt-dlp fallback) fall back to symphonia's generic probe.

## D9. Download strategy
`RemoteFile` fetches 256 KiB chunks with 3 parallel workers sharing one pooled `reqwest::blocking` client
(request latency is 0.15–0.35 s and a single connection yields ≈1 MB/s, versus 16 KB/s needed for playback).
Fetched chunks stay in memory so seeking backwards/replaying/repeat-one costs nothing. Whole tracks are
downloaded eagerly (≈4–7 MB each; only the current and next track are held).
A dead URL (403/404/410) is not retried; the engine re-resolves once and resumes at the same position.
Possible follow-up: stream URLs still contain an unsolved `n` parameter; throughput is fine for playback
today but a future throttle would show up as `underruns > 0`.

## D10. Engine architecture (`ytm-player/src/engine.rs`)
Controller task (tokio) + decode thread + cpal callback, joined by one shared `AudioBuf` (Mutex<VecDeque>
with a generation counter). A mutex in the audio callback is not textbook-real-time; it is only ever held
for microseconds by the producer and the callback uses `try_lock` (silence on contention), which measured
0 underruns over real-device playback. The buffer also does position and track-boundary accounting, so the
controller learns about gapless hand-over without extra synchronisation. Seek/skip/new-queue = `flush()`,
which bumps the generation so stale jobs abort themselves. The next track is resolved, downloaded and its
decode job queued while the current one plays; the decode thread starts it the moment the current finishes
(gapless to within the resampler delay, which is compensated). Volume uses a squared curve with a per-callback
ramp to avoid zipper noise. Resampling uses rubato's FFT resampler (fixed 44.1→48 kHz ratio), start-up delay trimmed.
Output formats: f32, i16, u16 via cpal. Headless/CI uses `OutputKind::Null` (real-time drain, no sound card).

## D11. Test strategy for playback
Queue logic: pure unit tests. `remote.rs`: local range server. Engine: `tests/engine.rs` runs the full
engine (null output) against a mock resolver and a local range server serving generated WAVs
(symphonia `wav`/`pcm` features are enabled for tests only), covering gapless advance, seek/pause/volume,
skip/jump/prev, repeat-one, shuffle, error-skip, queue edits. Real-device and live-network playback were
verified manually (`cargo run -p ytm-player --example play -- "query"`); audible output could not be
verified from this environment, only that the device opens and drains at real time without underruns.

## D12. UI stack: egui/eframe 0.36, wgpu renderer with glow fallback
`eframe` is built with both `wgpu` and `glow`; `main` starts with wgpu and, if creating the renderer fails
(no Vulkan/GL adapter), retries with glow. wgpu is preferred because it picks Vulkan/Metal natively and keeps
frame times low when scrolling image-heavy grids. Observed in this environment: on native Wayland the Intel GPU
is used; under XWayland without DRI3 wgpu fell back to the llvmpipe software rasteriser and still rendered
correctly. egui 0.36 requires Rust ≥ 1.95, hence `rust-version = "1.95"`.
Immediate-mode consequences: the model is a single `AppState`; views are plain functions
`fn(&mut Ui, &mut AppState)`; they never mutate state mid-draw but push `UiAction`s that are applied after the
frame's closures return (avoids borrow conflicts and keeps drawing pure).

## D13. Thumbnails and colour
`ThumbLoader` implements egui's `BytesLoader` for `http(s)` URIs: in-memory map + on-disk cache (hash of URL,
size-capped by `thumbnail_cache_mb`, oldest-first pruning at start). `egui_extras` decodes and caches textures.
Source size is chosen from {60,120,226,302,544,800} px to match the on-screen size × DPI by rewriting the
`=w…-h…` suffix of googleusercontent URLs, and widgets never request images that are scrolled out of view.
The now-playing backdrop colour is computed once per artwork on a blocking thread (hue-binned, saturation-weighted,
brightness-capped so white text stays readable) and cross-faded with `animate_value_with_time`.

## D14. Visual design
The Dribbble shot could not be viewed (no access to the attachment/link from the tool environment), so the
brief's fallback spec is implemented: `#0F0F0F` background, `#181818` surfaces, accent `#FF0033`, 12–14 px
art radius, Inter (OFL-1.1, bundled) for text, Phosphor icons (MIT, bundled via `egui-phosphor`) in dedicated
icon-only font families (Inter defines glyphs at some of the same private-use codepoints and would otherwise
shadow them — found by screenshot). All styling lives in `theme.rs`; custom widgets (icon buttons, pills, seek
bar, cards, track rows, gradient) draw with `Painter` in `widgets.rs`. The app is dark-only by design.

## D15. Authentication (REMOVED — see D19)
* **Cookie import is the primary path.** The web client authenticates every request with the session cookies plus
  `Authorization: SAPISIDHASH <t>_<sha1(t SAPISID origin)>` (and the `SAPISID1PHASH`/`SAPISID3PHASH` variants when
  those cookies exist). It needs no Google Cloud project, and the algorithm is covered by a reference-vector test.
* **OAuth device-code flow is implemented but needs the user's own client id/secret** ("TVs and Limited Input
  devices" client). The shared YouTube-TV client that older tools used is not something a distributed app should
  embed. It is tested against a fake server (pending / slow_down / denied / expired / refresh); it has **not**
  been exercised against Google, and whether InnerTube accepts OAuth bearer tokens for the `WEB_REMIX` client is
  unverified.
* Credentials live in the OS keychain through `keyring` 4 (`v1` mode: Keychain on macOS, the Secret Service over
  D-Bus on Linux — it talks to gnome-keyring/KWallet directly rather than linking libsecret). Verified here with
  a real round-trip including a 4 KB value (`cargo test -p ytm-auth -- --ignored`). All credentials are wrapped
  in `Secret` (redacting `Debug`); nothing logs tokens or cookies.
* **Not verified:** every signed-in call (library, liked songs, like/unlike, playlist edit) — there was no account
  to test with. Their request shapes follow ytmusicapi, parsing reuses the anonymous-page parsers, and the
  plumbing is covered by tests against a local fake InnerTube server.

## D16. Media keys / now playing
`souvlaki` 0.8 (MPRIS via libdbus on Linux, MediaRemote on macOS) runs on its own thread and sends `Action`s
through the same channel as the UI. A tokio task diffs `PlayerState` and pushes metadata/playback changes, and
refreshes the progress every 2 s while playing because desktop shells poll `Position`. Verified on Linux against
the real session bus (`busctl`): name registration, status, metadata (title/artists/length/art URL),
`PlayPause`, `Next`, position. **macOS MediaRemote was not individually checked** (the app runs on macOS; CI compiles it).
Not implemented: system tray (needs a GTK main loop next to winit on Linux) and desktop notifications — both were
optional in the brief.

## D17. Packaging
`cargo-packager` (MIT/Apache-2.0) configured in `crates/ytm-app/Cargo.toml` (`.deb` + AppImage on Linux,
`.app` + `.dmg` on macOS). The `.deb` was built and inspected on Ubuntu; the macOS `.app`/`.dmg` (arm64, unsigned) were built on a Mac and the app was run, but the bundles were not otherwise inspected.

## Dependency licenses (major dependencies)
| Dependency | License |
|---|---|
| egui / eframe / egui_extras (0.36), wgpu (30), image, reqwest, serde, keyring, directories, toml, thiserror, anyhow | MIT OR Apache-2.0 |
| tokio, tracing, souvlaki, rubato, audioadapter-buffers | MIT |
| cpal | Apache-2.0 |
| rustls | Apache-2.0 OR ISC OR MIT |
| symphonia (core, isomp4, AAC) | **MPL-2.0** (file-level copyleft; we use it unmodified as a library) |
| egui-phosphor / Phosphor icons | MIT OR Apache-2.0 / MIT |
| Inter font | SIL OFL-1.1 (`crates/ytm-app/assets/fonts/Inter-LICENSE.txt`) |

## D18. Local (no-account) library
Liked songs, playlists and saved albums/artists/playlists live **on the device** in one JSON file
(`<data dir>/tunebox/library.json`, `ytm-app/src/local.rs`), so the app is fully useful signed out.
* **Format/robustness:** versioned, `#[serde(default)]` everywhere (older/partial files load); atomic save (temp file
  + rename); a corrupt file is moved aside (`library.json.corrupt-<ts>`) and the user is told, never overwritten;
  playlist ids are `local:<n>` from a persistent counter, so ids are never reused.
* **Threading:** `AppState` owns the `LocalLibrary` and mutates it on the UI thread (cheap in-memory ops); each change
  sends `Action::PersistLocal(snapshot)` and the backend writes it in `spawn_blocking` behind a mutex so an older
  snapshot can never overwrite a newer one.
* **Semantics when signed in:** the heart and "new playlist" default to the **device**/the account respectively —
  heart = account like when signed in, device like when signed out; new playlists default to the device with an
  explicit "On YouTube" choice. Library/Playlists pages show a "This device | YouTube account" toggle. Saved
  albums/artists/playlists are always device-side (we do not call YouTube's library-save endpoints for them).
  Song context menu: device playlists always, account playlists when signed in.
* Features: like, create/rename/delete/reorder/remove/add playlists, "save queue as playlist", save/unsave
  albums/artists/YouTube playlists, play/shuffle/queue from any of them.
* No sync between devices and no import/export yet (the JSON is human-readable and easy to back up).

## D19. Accounts and sign-in removed
The app is for local personal use, so all account functionality was deleted rather than hidden: the `ytm-auth` crate
(cookie/`SAPISIDHASH` import, OAuth device flow, OS-keychain storage), the `keyring`/D-Bus-secret-service dependency,
`ytm-api`'s `AuthProvider` trait and all signed-in endpoints (account, liked songs, library, like, playlist edit),
their parsers/tests, and every UI surface (sign-in dialog, sidebar account block, "This device | YouTube account"
toggles). What remains is the on-device library (D18), which now has no account counterpart: likes, playlists and saved
items are always device-side. Consequences: no secrets are stored or handled anywhere in the app; fewer dependencies
and system packages (`libsecret` no longer needed); the old implementation was never committed, so D15 above plus these notes are all that remains if accounts are ever
wanted again: SAPISIDHASH = `sha1("<unix> <SAPISID> <origin>")` sent with the session cookies; ytmusicapi request shapes
for `like/like`, `browse/edit_playlist` and `FEmusic_liked_playlists`.

## D20. Own title bar on Linux (native Wayland, GPU)
GNOME's compositor (Mutter) does not offer server-side decorations, so a native Wayland window gets winit's imitation
title bar. Options evaluated on the author's machine (GNOME 46 / Wayland):
* **Force X11 (XWayland)** — real system title bar, but this XWayland has no DRI3, so wgpu fell back to llvmpipe
  (software): ≈54 % of a core while playing vs ≈3–4 % on the GPU. Rejected.
* **Native Wayland + own title bar** — chosen. `with_decorations(false)` on Linux; `views/titlebar.rs` draws the bar
  (drag = `StartDrag`, double-click = maximise, minimise/maximise/close buttons) and `window_chrome` adds 6 px edge /
  12 px corner resize handles (`BeginResize`) and a 1 px outline. macOS keeps its native title bar.
* Escape hatch: `TUNEBOX_NATIVE_TITLEBAR=1` uses the system/imitation bar. Behaviour is covered by synthetic-input
  tests (buttons, drag, double-click, all eight resize zones); real compositor drag/resize/maximise could not be
  exercised from the test environment and should be tried by hand.
* No rounded window corners or shadow (needs a transparent window; not attempted).

## D21. Windows build
* **Target `x86_64-pc-windows-gnu` cross-compiled with Zig** (`cargo-zigbuild`) because no Windows toolchain, Wine or sudo
  was available. Zig supplies the C toolchain/linker (and `zig rc` for resources) from a user-space tarball. The MSVC
  build on a Windows CI runner is the reference path; both use the same sources.
* Windows-specific code: `windows_subsystem = "windows"` in release (no console window); `CREATE_NO_WINDOW` for the
  `yt-dlp` fallback; the Win32 window handle is passed to souvlaki (its Windows backend requires it); native title bar
  (our own bar is Linux-only, D20); `build.rs` embeds `assets/windows/tunebox.ico` + version info using whichever of
  `rc` / `llvm-rc` / `zig rc` exists, and only warns if none does.
* Verified: builds cleanly, valid PE32+ GUI x86-64, `.rsrc` with icon and version info, static imports limited to core
  Windows DLLs (no MinGW runtime DLLs). **Not verified: any runtime behaviour** (see docs/DEVELOPMENT.md "Windows").

## D22. Explore: New releases, Charts, Moods & genres
All three are plain `browse` calls (`FEmusic_new_releases`, `FEmusic_charts`, `FEmusic_moods_and_genres[_category]`) whose
responses are carousels, so they reuse the generic shelf parser and the carousel view. Specifics:
* **Charts country:** sent as `formData.selectedValues: ["<CC>"]` (`ZZ` = Global); without it YouTube picks the user's
  country. The country list comes from the page's selector menu; each option's code is only present inside the base64
  `formItemEntityKey` (`…explore_charts_country_menu_<id><CC>`), decoded by a tiny built-in decoder. Verified live
  (TR default → ZZ → US return different charts and report the matching country). Chart artists are ranked via
  `customIndexColumn` and shown as "#n · subscribers".
* **Mood buttons** (`musicNavigationButtonRenderer`, `FEmusic_moods_and_genres_category` + `params`) carry a stripe
  colour (0xAARRGGBB) used to tint their tile. They also appear inside the Explore response, so Explore needs no extra request.
* Rows without a title (New releases' featured mix card) are shown without a header.
* Not shown: items that are not songs/albums/artists/playlists (e.g. podcast shows in some countries' charts).
* Recorded fixtures: `new_releases`, `charts` (TR), `charts_ZZ`, `moods`, `mood_category` (trimmed from 2.8 MB).

## D23. No content-country setting (only language)
The Settings "Content country" (`context.client.gl`) was removed. Measured 2026-10-06 from a Turkish IP, same
`FEmusic_home` request with `gl` = TR / US / JP: Home was identical in kind (e.g. "Trending community playlists" full of
Turkish playlists under `gl=US`); adding `X-Forwarded-For`/`X-Real-IP` or a US `timeZone`/`utcOffsetMinutes` changed
nothing. YouTube geolocates by IP; `gl` is not honoured for Home/Explore. `hl` does work (translates titles) and stays.
Charts have their own explicit country picker (D22), which is real. Old `region = ...` keys in `config.toml` are ignored.

## D24. Library split into an index plus one file per playlist
Measured with a synthetic library of 200 playlists × 100 songs (20 000 songs, release build): `library.json` was
16.8 MB, loading took 46 ms on the startup path, every change rewrote the whole file (32 ms, on a worker) and cloned
the whole library on the UI thread (4 ms). That grows linearly with use, so:
* `library.json` is now a small index (liked songs, saved items, and per playlist: title, cover, song count, the
  first covers); each playlist's songs live in `playlists/local-<n>.json` next to it.
* Startup reads only the index; the songs are read on a worker (`Action::LoadPlaylistTracks` →
  `Event::PlaylistTracks`). Until then a playlist shows its count/mosaic from the index and a spinner on its page;
  editing its songs, backup, export and "move library" wait (toast) instead of risking data loss.
* A change writes only the touched playlists plus the index (`LocalLibrary::snapshot`), and the UI thread clones only
  those. Files of deleted playlists are swept on write (`local-*.json` only).
* Backups and exports keep the old single-document format. A version-1 `library.json` is migrated on first start
  (kept as `library.json.v1`).

## D25. One application-data folder (settings included)
`config.toml` used to live in the config directory (`~/.config/tunebox`) while the library had its own optional
path, so "my data" was in two places and a moved library needed a config entry to be found again. Now everything
(`config.toml`, `library.json`, `playlists/`, `covers/`) is in the data folder. Moving it from Settings copies the
whole folder (never overwrites a folder that already holds Tunebox data) and records the new place in a one-line
`location` file in the default data folder — the only thing that has to stay at a fixed path. Old installs migrate on
first start: `~/.config/tunebox/config.toml` is moved in, and a legacy `library_path` becomes the data folder.
Tests point `AppState::data_dir` at a temp folder; an earlier test run overwrote the real settings (see git history).

## D26. Tray icon, notifications, shortcuts and responsive breakpoints
* **Tray, Linux: `ksni` (StatusNotifierItem over D-Bus), not `tray-icon`.** `tray-icon` needs GTK plus libayatana-appindicator
  (not installed on the dev machine, and a new apt dependency for the `.deb`) and its own GTK main loop next to winit's.
  `ksni` is pure Rust on the zbus/tokio stack that was already in the tree, so no new system package. Cost: it needs a
  StatusNotifierHost (Ubuntu's AppIndicator extension is on by default; stock GNOME has none). Without a host
  `Tray::active()` stays false and "close to tray" is ignored, so the window can never be hidden with no way back.
  macOS/Windows use `tray-icon` (created in eframe's creator, i.e. on the main thread). Windows is cross-compiled
  (`cargo zigbuild`); macOS compiles and runs (built on a Mac).
* **Hide vs minimise.** Wayland compositors ignore "hide window", so with `WAYLAND_DISPLAY` set the tray's close-to-tray
  minimises instead. Tray callbacks run on other threads and talk to the UI only through `Action` and
  `egui::Context::send_viewport_cmd` (thread-safe).
* **Notifications: `notify-rust`, on a short-lived thread** (the D-Bus call can block). Sent once per track
  (`notify::Announcer`), only when the track is actually *playing* and the window is *not focused*; verified on the session
  bus (`dbus-monitor` saw one `Notify` with title and artist).
* **Shortcuts go by typed character for `?` and `/`** (`Event::Text`, removed from the input so the search box we focus
  does not receive it), so they work on any keyboard layout. Combinations with modifiers are checked *before* plain
  keys: `consume_key(NONE, ArrowLeft)` also matches Alt+Left (a unit test caught this). Dispatch is the free function
  `app::handle_shortcuts`, tested with synthetic egui events.
* **Breakpoints live in `layout.rs`** as pure functions of the window width (sidebar rail < 1000 px, page margin 14 px
  < 760 px, player-bar tiers at 900 / 700 px) with a test that the transport always fits between the bar's sides at every
  width from 600 to 2200 px. The minimum window shrank from 920×600 to 640×480; checked by screenshots at 1280×820,
  900×600 and 640×480.

## D27. Tray label via a patched `ksni`
* A Vitals-style text next to the tray icon is not part of the StatusNotifierItem spec; libayatana-appindicator adds the
  `XAyatanaLabel` property and `XAyatanaNewLabel` signal and the GNOME AppIndicator extension renders them. `ksni` 0.3.6
  has no hook for it, so `vendor/ksni` is a copy with a ~30-line patch (`Tray::ayatana_label`), wired through
  `[patch.crates-io]`; see `vendor/ksni/VENDORED.md`. Verified with `busctl introspect` on the running app
  (`XAyatanaLabel` present). Visual result in the GNOME panel was not captured.
* The state (tooltip line, label, playing badge) is diffed once per frame in `TuneboxApp::sync_tray` and only pushed to the
  tray when it changes, so there is no extra repaint or D-Bus traffic while a song plays.

## D28. Close-to-tray runs through XWayland
* Measured on Ubuntu 24.04 GNOME/Wayland: with the window minimized (the old Wayland "hide"), tray Show and Quit did
  nothing, because winit on Wayland cannot unminimize/hide from the client and gets no frame callbacks, so
  `send_viewport_cmd` is never processed. With `close_to_tray` and a tray icon, `main` drops `WAYLAND_DISPLAY` (when
  `DISPLAY` exists) so winit uses X11, where `Visible(false/true)` works and `request_repaint` wakes the loop.
  Cost: XWayland rendering (possible blur with fractional scaling). Without close-to-tray the app stays native Wayland.

## D29. Windows/macOS tray: glyphs drawn in code
* `tray::glyphs` rasterises play/pause/next/prev (4x4 supersampling) for the menu icons (`IconMenuItem`; macOS marks them
  as template images, Windows uses mid-grey so they read on light and dark menus) and the macOS menu-bar template.
  No asset files; unit-tested on Linux. macOS shows the label via `TrayIcon::set_title`.
* Not verified: the Windows build compiles (`cargo zigbuild`), nothing ran on Windows; the macOS-only lines
  (`set_icon_templated`, `set_title`) were checked against the `tray-icon` 0.26 / `muda` sources, and the app has since been
  run on macOS without checking the menu-bar icon specifically.

## D30. Listening history: append-only JSONL, a play counts after 30 s
* `history.jsonl` sits next to `library.json` (so "Change folder…" copies it) and holds one line per counted play
  (`{t, secs, track}`), appended by the backend, never rewritten while running; trimmed to the newest 20 000 plays on load.
  It is separate from `library.json` on purpose: the library snapshot is rewritten on every change and is the backup
  format; a growing log would make every like or rename rewrite it.
* A play counts once the track has been heard for 30 s, or half of it when shorter (the scrobbling convention), measured
  by `PlayTimer` from the frames the UI draws: a gap longer than 1 s between frames (sleep) adds only 1 s, pausing adds
  nothing, and repeat-one counts again after a full lap. Tested with synthetic `Instant`s. "Listening time" is the sum of
  the counted tracks' lengths, not wall-clock listening.
* `Config::record_history` turns recording and the Home shelves off; "Clear history" deletes the file. Nothing leaves the device.
* Checked live: 30 s of real playback appended one line (real video and artist id) to the file.

## D31. Home recommendations come from the radio of your own top tracks
* Home order: "Recently played" → first "More like <song>" → YouTube's first shelf → second "More like <song>" → the rest of
  YouTube's shelves, so Home stays mixed. Resuming what you just played is the main reason to open the app, so it is first.
  A "Your top artists" shelf was tried and removed: it is a stats view (the History page ranks artists), pushed the useful
  shelves down, and its cards could only borrow a track's cover art.
* Seeds are picked at random among the five most-played tracks of the last 30 days, one per artist (the latest likes when
  there is no history), so Home is not identical every day. Two seeds at most: each is another startup request and more
  shelves would bury YouTube's. Each seed asks the existing `up_next` (radio) endpoint, once per session (again after
  clearing the history).
* No song appears twice on Home: shelves skip what "Recently played" or an earlier shelf has, and a shelf left with fewer
  than 4 songs is dropped. Titles are translated when the shelves are built, so a language change rebuilds them.
* The shelves are built into `Arc<Vec<Section>>`s only when their inputs change, so drawing a frame copies nothing.

## D32. Synced lyrics from LRCLIB (new third-party service)
* YouTube's lyrics are plain text. Timestamps come from LRCLIB (`lrclib.net/api`): free, no key, asks clients to send a
  User-Agent. Lookup: `/get` with title, artist, album and duration; if that misses, `/search` with the cleaned title
  (trailing "(Official Video)" etc. removed), taking the entry whose length is within 3 s. A 404 is "no lyrics", other
  failures are errors; both are swallowed in `Backend` so YouTube's text still shows.
* It sends the song title, artist, album and length to a third party, so it is a setting (`synced_lyrics`, on by default,
  with the data it sends spelled out in Settings). The code is in `ytm-api::lrclib` behind a defaulted `MusicApi::synced_lyrics`
  (mocks need nothing). Parser tested on a real recorded response (`fixtures/lrclib_get.json`) and the request flow
  against a local fake server.
* UI: the current line is bright and larger, the list scrolls only when the line changes (manual scrolling is not
  fought), clicking a line seeks. Checked live: at 0:20 the "♪" gap line, at 0:43 "You don't even have to do too much"
  (41.2–44.1 s in the data) was the highlighted one.

## D33. Queue drag-to-reorder moves the play order
* `Queue::move_upcoming(from, to)` reorders positions in the "next up" list (`upcoming()`), i.e. the play order, so it also
  works while shuffled. The whole row is draggable (a click still plays it); the insertion line and drop maths are the
  playlist editor's (`widgets::drop_gap/drop_target`), and the move is sent only on release. Escape cancels.
* An unshuffled session is saved in play order (`session::save_order`), so the reordered queue survives a restart.

## D34. First-run language and theme follow the operating system
* `Config` defaults are `ui_language = "system"` and `theme = "system"`. A new install uses the OS language when the app has it
  (first match among the OS's preferred languages for en/tr/de/es/fr/zh, via `sys-locale`; English otherwise) and the OS
  light/dark setting. Existing config files keep what they saved ("en", "dark", …); only a missing field gets the new default.
* Theme: with "System" egui's `ThemePreference::System` is used (a forced Dark/Light would make winit report that theme back,
  so the OS could never be read again), and `theme::follow_system` runs every frame to restyle when the OS changes. Both egui
  themes carry the same visuals built from the active palette, so `apply_pref` rewrites them when the palette flips.
* Settings: theme has System / Light / Dark; the language list starts with "System default (<language>)".
* Backups carry the setting ("system" is accepted when restoring).

## D35. Sidebar and shortcut navigation start a page at the top
* Scroll offsets are kept per page id. Opening a page from the sidebar (or Cmd+1…4, Cmd+,) bumps that page's *epoch*, which is
  part of its id, so it gets new scroll areas and carousels at the top, also when it is the page already open. Back and
  in-page links keep the epoch, so they restore the old position like a browser. Other pages are not touched.

## D36. The tray icon and close-to-tray are off by default
* Both are opt-in (`tray_icon = false`, `close_to_tray = false`): a fresh install shows no tray or menu-bar icon and closing
  the window quits. They need the right desktop support (a StatusNotifierHost on GNOME, D28 on Wayland) and the macOS
  menu-bar icon has not been checked by hand, so the safe default is not to depend on them. Existing config files keep
  the value they saved; Settings > Desktop switches them on (applies at the next start).
* Settings > Desktop: "Show the song next to the tray icon" and "Closing the window keeps Tunebox running in the tray" only
  mean something with a tray icon, so while "Show tray icon" is off they are greyed out, ignore clicks and say why
  (`toggle_row_if` uses `ui.add_enabled_ui`). They are also switched *off*, not just greyed out: `Config::normalize` turns
  `close_to_tray` and `tray_label` off whenever `tray_icon` is off, when the config is loaded and when the tray switch is
  turned off, so a saved "on" can never apply to a tray that is not there (a greyed-out ON switch was misleading).
  Switching the tray back on leaves them off; the user turns them on again. Tested with real egui pointer frames (the same
  click toggles an enabled switch and does nothing to a disabled one) and a config test for the normalisation.
