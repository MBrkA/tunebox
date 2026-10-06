---
name: ytm-stream-debug
description: Diagnose why a track will not play (LOGIN_REQUIRED, 403 after ~1 MiB, long "Loading", decode errors). Use when playback fails or stalls, or when YouTube changes client requirements.
---

# Playback / stream resolution debugging

Pipeline: `StreamResolver` (ytm-api/stream.rs) → `RemoteFile` (256 KiB chunks, 3 workers) → `Fmp4` demuxer →
symphonia AAC → resample → `AudioBuf` → cpal. Events/log: `RUST_LOG=ytm_player=debug,ytm_api=debug`.

* **Try the CLI first:** `cargo run -p ytm-api --example search -- "song"` prints the resolved stream; then
  `cargo run -p ytm-player --example play -- "song" --null`. Compare with `yt-dlp -f 140 -g <url>` (fallback path).
* **Which client works today?** POST `https://www.youtube.com/youtubei/v1/player` with candidate client JSON and
  headers `X-YouTube-Client-Name/Version`, `X-Goog-Visitor-Id` (visitorData from any InnerTube response's
  `responseContext`). Last measurements (2026-10-05): VISIONOS ok; IOS gives URLs but 403 after ~1 MiB (PO token);
  ANDROID_VR/ANDROID_MUSIC LOGIN_REQUIRED; WEB_REMIX UNPLAYABLE. yt-dlp's `_base.py` client table
  (`curl -sL https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp -o yt-dlp; unzip -p yt-dlp yt_dlp/extractor/youtube/_base.py`)
  shows what current yt-dlp uses — copy a working one into `InnerTubeResolver::fetch`.
* **Verify a URL really streams:** request ranges at 0, 2 MiB and 4 MiB (`Range: bytes=a-b`, ≤1 MiB). Open-ended
  or large ranges and PO-gated URLs return 403. The `live-tests` suite does the 2 MiB check.
* **Slow start:** the decoder must open after reading only `moov`/`sidx` + one segment. If it waits for the whole
  file, the fMP4 reader fell back to symphonia's generic probe (non-fragmented input).
* **Symptoms → causes:** `Unplayable("Sign in to confirm…")` = client/visitor rejected (resolver refreshes visitor
  once, then chain falls back to yt-dlp); `download failed: HTTP 403` mid-track = expired URL (engine re-resolves
  once); `underruns > 0` = starved audio callback or throttled download.
* Engine tests are hermetic (`crates/ytm-player/tests/engine.rs`: fake range server + WAV); add one there for any
  engine behaviour change.
