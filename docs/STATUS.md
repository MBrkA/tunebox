# Status, limitations and next steps

## Status

**Verified on Ubuntu 24.04 (this repo's author environment):** search (+suggestions), browse pages (home, explore,
artist, album, playlist), up-next/radio, lyrics, stream resolution and playback (seek, volume, skip, shuffle, repeat,
gapless hand-over, prefetch), queue, now-playing view with colour backdrop, HiDPI and resizing, MPRIS media keys and
metadata, the local library (liked songs, playlists, saved items; survives restarts), `.deb` packaging. Measured on a release build: cold start to first frame ≈ 100 ms;
UI thread ≈ 1–2 % CPU while playing (≈ 0.65 ms per frame at ~3 repaints/s; the rest of the process is audio/decode
≈ 1.5 %); idle ≈ 1.3 %; RSS ≈ 140 MB (GPU) / 210 MB (software rasteriser) with a 20-track queue.

**macOS (Apple Silicon):** full pass on 2026-10-08 on an M2 MacBook Air, macOS 15.3.1 (release build; CI also builds,
tests and packages it on `macos-14`). **Ran and observed:**
* fmt/clippy/hermetic tests and the `live-tests` suite pass.
* Every view screenshotted with live data at Retina scale (1280×820 window → 2560×1640, sharp): Home, Explore, Moods,
  New releases, Charts, search (All + Albums), album, artist, Library, Playlists, History, Settings, shortcut overlay,
  Now playing, queue at the 640×480 minimum; dark theme follows the system, light theme via `theme = "light"`.
  First frame 166–234 ms.
* Playback through CoreAudio: the device runs and the clock keeps real time (0:33 after 35 s); several tracks in a row,
  streams resolve in 0.1–0.6 s. Sound itself was not heard (the speakers were muted).
* System Now Playing (MediaRemote, read with a small Swift probe): Tunebox becomes the Now Playing app with title, artist,
  duration and artwork. MediaRemote commands, the path media keys and Control Center use, all work: play, pause, toggle,
  next, previous (restarts after 3 s), seek to 60 s.
* Notifications: with another app in front the decision is `background=true` for every track change and `show()` returns
  no error; Notification Center lists `dev.tunebox.Tunebox`.
* Local library: likes and a playlist are written to `library.json`/`playlists/` and show up after a restart.
* `.app` launched through LaunchServices (`open`, same as Finder). The CI-built `.pkg` payload: bundle sealed
  (`codesign --verify --deep --strict` ok, ad-hoc), arm64 only, no non-system dylibs, LSMinimumSystemVersion 12.0,
  postinstall clears quarantine; a quarantined copy of the bare `.app` is rejected by `spctl` (why we ship the `.pkg`).
* Cost: idle ≈ 2.5 % CPU, playing ≈ 3–5 %, paused ≈ 2 %; physical footprint (Activity Monitor) ≈ 227 MB idle on Home
  with artwork, ≈ 202 MB playing.
* Fixed after this pass: lyrics were hard-coded white (unreadable in the light theme; the backdrop is now also washed
  out there), songs in the artist "top result" search card had no artist, the shortcut overlay said `Alt` (now `Option`).
* One run saw HTTP 403 on chunks 4–5 and a track failing after re-resolve; not reproduced in two more app runs and three
  CLI runs. Watch for it.

**Checked by the author on the same Mac:** sound from the speakers, the menu-bar icon and its menu, notification banners
on track change, physical media keys. **Not verified:** installing the `.pkg` with Installer and the first-open
"Open Anyway" flow on another Mac.
**Signing and notarisation** are wired into CI (`package` job) but have never run: they switch on when the repository
has the `APPLE_*` secrets (see the `tunebox-release` skill); without them CI ships the unsigned `.pkg` (D37).

**Added later, run on macOS (Apple Silicon) only:** time-synced lyrics (LRCLIB; the highlighted line followed real playback),
listening history (a play was written after 30 s of real playback), the History page and Home shelves from the history
(screenshots), "More like …" shelves fetched live, queue drag-to-reorder (unit tests with real egui pointer frames; the
drag itself was not done by hand). Not run on Linux or Windows.

**Not verified:**
* Audible output on Linux — the audio device opens and drains in real time with zero underruns, but sound itself was
  not heard there (on macOS it was, see above).

## Known limitations

* Unofficial API: YouTube can break the client or the stream resolver at any time. Native stream resolution (the
  `VISIONOS` client) is the most fragile part; a `yt-dlp` fallback is built in.
* Only AAC (itag 140/139) is decoded; Opus (itag 251) would need libopus (see `docs/DECISIONS.md` D6).
* The local library is per-device: no sync, import or export yet (the file is plain JSON, easy to back up).
* Synced lyrics need LRCLIB to have the song (it covers most popular songs); otherwise YouTube's plain text is shown. Home is short (YouTube serves anonymous users only a couple of shelves);
  the app pads it with Explore sections.
* No downloads/offline mode. The tray icon and notifications are verified only on Ubuntu 24.04 (GNOME/Wayland and XWayland). On macOS and Windows the `tray-icon` path compiles (Windows cross-compiled); the macOS app has been run but its menu-bar icon and notifications were not specifically checked, and nothing has run on Windows.
* Narrow windows adapt: below 1000 px the sidebar becomes an icon rail, below 900 / 700 px the player bar drops the volume slider, then the track text and shuffle/repeat (the shortcuts still work), and the lyrics column of the now-playing view is hidden below 820 px. The smallest window is 640×480.
* Explore has all three of YouTube's sections: New releases, Charts (with a country picker) and Moods & genres (shortcuts, a full page, and each category's playlists). Chart rows that are not songs, albums, artists or playlists (e.g. podcast shows) are not shown.

## Next steps

1. Finish checking macOS by hand (media keys, menu-bar icon, notification shown, Finder launch) and add the Apple signing secrets so CI signs and notarises.
2. Home recommendations are a first cut (radio of your top tracks); "made for you" mixes and per-artist shelves are open.
3. Solve the `n` parameter / Opus so stream resolution does not depend on one InnerTube client.
4. Podcasts.
