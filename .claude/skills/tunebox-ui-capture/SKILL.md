---
name: tunebox-ui-capture
description: Run the Tunebox egui app unattended, take screenshots of any view, and measure CPU/RSS/frame time. Use to verify UI changes visually, check HiDPI/resizing, or profile idle/playing cost.
---

# Capturing and measuring the UI

Scripts live in `scripts/` (copy of what was used during development):
* `scripts/shot.sh <name> <delay-secs> <app args…>` → `$OUT/<name>.png` (default `OUT=/tmp/tunebox-shots`).
* `scripts/measure.sh` / `scripts/measure2.sh <label> <warmup> <window> <app args…>` → total CPU, RSS, per-thread CPU.

Build first (`cargo build [--release] -p ytm-app`; scripts use `target/release/tunebox` unless `BIN=` is set).

Dev flags: `--query Q --filter songs|videos|albums|artists|playlists --route home|explore|album:ID|artist:ID|playlist:ID
--play-first --now-playing --queue --local-demo --screenshot out.png --shot-delay N`.

**Gotcha:** on Wayland a hidden window never gets frame callbacks → nothing happens. The scripts run with
`env -u WAYLAND_DISPLAY` (XWayland, llvmpipe software rendering). Consequence: total CPU there is dominated by
llvmpipe threads; judge our cost by the `tunebox` main-thread number and by
`RUST_LOG=tunebox=debug` "ui frame stats" (avg_ms per frame). HiDPI: `WINIT_X11_SCALE_FACTOR=2`.

Reference numbers (release, 20-track queue): first frame ≈100 ms; UI thread ≈1–2 % while playing at ~3 repaints/s,
≈0.65 ms/frame; idle ≈1.3 %; RSS ≈140 MB (GPU) / 210 MB (software). Targets: idle <2 %, <300 MB, cold start <1.5 s.
Look at the PNG with the image viewer (Read tool); check icons (tofu/odd glyphs = wrong font family), truncation,
spacing and contrast. Sample IDs: album `MPREb_7ltM34kr0mH`, artist `UCRr1xG_2WIDs18a6cIiCxeA`,
playlist `PLSdoVPM5WnnfbGVqQTCXjRnZd8hYLY0Cd`.
