# Status, limitations and next steps

## Status

**Verified on Ubuntu 24.04 (this repo's author environment):** search (+suggestions), browse pages (home, explore,
artist, album, playlist), up-next/radio, lyrics, stream resolution and playback (seek, volume, skip, shuffle, repeat,
gapless hand-over, prefetch), queue, now-playing view with colour backdrop, HiDPI and resizing, MPRIS media keys and
metadata, the local library (liked songs, playlists, saved items; survives restarts), `.deb` packaging. Measured on a release build: cold start to first frame ≈ 100 ms;
UI thread ≈ 1–2 % CPU while playing (≈ 0.65 ms per frame at ~3 repaints/s; the rest of the process is audio/decode
≈ 1.5 %); idle ≈ 1.3 %; RSS ≈ 140 MB (GPU) / 210 MB (software rasteriser) with a 20-track queue.

**macOS (Apple Silicon):** built on a Mac (`cargo build --release`, `cargo packager --formats app,dmg` → `.app` and
`.dmg`, arm64) and run there by the author. CI also compiles and tests it (`macos-14`).
Checked in a later session by running the release binary on an Apple Silicon Mac: **Retina scaling** (a 1280×860 window
screenshots at 2560×1632 and renders sharp), **CoreAudio output** (the device opens and the playback clock runs in real
time for 40+ s; sound itself was not heard), the media-controls object is created ("media controls attached"), and the
notification decision logic runs (it decided to notify for a track change while the window was in the background).
Still **not verified** (needs a person at the Mac; the screen was locked, so no further GUI runs were possible):
MediaRemote media keys / Now Playing widget, the menu-bar icon, that a notification is actually shown, launching the
`.app` from Finder. **Signing and notarisation** are wired into CI (`package` job) but have never run: they switch on
when the repository has the `APPLE_*` secrets (see the `tunebox-release` skill); without them the bundles stay unsigned
and Gatekeeper warns.
Until then, distribute the `.pkg` from `scripts/package-macos.sh` (D37), not the `.dmg` (reported as "damaged"
on other Macs).

**Added later, run on macOS (Apple Silicon) only:** time-synced lyrics (LRCLIB; the highlighted line followed real playback),
listening history (a play was written after 30 s of real playback), the History page and Home shelves from the history
(screenshots), "More like …" shelves fetched live, queue drag-to-reorder (unit tests with real egui pointer frames; the
drag itself was not done by hand). Not run on Linux or Windows.

**Not verified:**
* Audible output — the audio device opens and drains in real time with zero underruns, but sound itself was not heard.

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
