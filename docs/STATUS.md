# Status, limitations and next steps

## Status

**Verified on Ubuntu 24.04 (this repo's author environment):** search (+suggestions), browse pages (home, explore,
artist, album, playlist), up-next/radio, lyrics, stream resolution and playback (seek, volume, skip, shuffle, repeat,
gapless hand-over, prefetch), queue, now-playing view with colour backdrop, HiDPI and resizing, MPRIS media keys and
metadata, the local library (liked songs, playlists, saved items; survives restarts), `.deb` packaging. Measured on a release build: cold start to first frame ≈ 100 ms;
UI thread ≈ 1–2 % CPU while playing (≈ 0.65 ms per frame at ~3 repaints/s; the rest of the process is audio/decode
≈ 1.5 %); idle ≈ 1.3 %; RSS ≈ 140 MB (GPU) / 210 MB (software rasteriser) with a 20-track queue.

**macOS (Apple Silicon):** built on a Mac (`cargo build --release`, `cargo packager --formats app,dmg` → `.app` and
`.dmg`, arm64) and run there by the author. CI also compiles and tests it (`macos-14`). Which features were checked on the
Mac was not recorded, so treat these as **not individually verified**: CoreAudio output, MediaRemote media keys
(`souvlaki`), the menu-bar tray, notifications, Retina scaling (relies on winit/egui defaults). The bundles are not
signed or notarised.

**Not verified:**
* Audible output — the audio device opens and drains in real time with zero underruns, but sound itself was not heard.

## Known limitations

* Unofficial API: YouTube can break the client or the stream resolver at any time. Native stream resolution (the
  `VISIONOS` client) is the most fragile part; a `yt-dlp` fallback is built in.
* Only AAC (itag 140/139) is decoded; Opus (itag 251) would need libopus (see `docs/DECISIONS.md` D6).
* The local library is per-device: no sync, import or export yet (the file is plain JSON, easy to back up).
* Lyrics are plain text (not time-synced). Home is short (YouTube serves anonymous users only a couple of shelves);
  the app pads it with Explore sections.
* No downloads/offline mode. The tray icon and notifications are verified only on Ubuntu 24.04 (GNOME/Wayland and XWayland). On macOS and Windows the `tray-icon` path compiles (Windows cross-compiled); the macOS app has been run but its menu-bar icon and notifications were not specifically checked, and nothing has run on Windows.
* Narrow windows adapt: below 1000 px the sidebar becomes an icon rail, below 900 / 700 px the player bar drops the volume slider, then the track text and shuffle/repeat (the shortcuts still work), and the lyrics column of the now-playing view is hidden below 820 px. The smallest window is 640×480.
* Explore has all three of YouTube's sections: New releases, Charts (with a country picker) and Moods & genres (shortcuts, a full page, and each category's playlists). Chart rows that are not songs, albums, artists or playlists (e.g. podcast shows) are not shown.

## Next steps

1. Check the macOS features one by one (audio, MediaRemote, menu-bar tray, notifications, Retina) and add signing/notarisation.
2. Import/export and optional sync for the local library.
3. Solve the `n` parameter / Opus so stream resolution does not depend on one InnerTube client.
4. Time-synced lyrics, podcasts.
