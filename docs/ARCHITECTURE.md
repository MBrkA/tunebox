# Architecture

## Crates

```
                        ┌────────────────────────────────────────────┐
                        │                  ytm-app                   │
                        │  eframe/egui UI · AppState · backend task  │
                        │  local library (library.json) · media keys │
                        └───────┬───────────────┬────────────┬───────┘
                                │               │            │
                         ┌──────▼─────┐   ┌─────▼────┐  ┌────▼─────┐
                         │ ytm-player │   │ ytm-api  │  │ ytm-core │
                         │ queue,     │   │ InnerTube│  │ config,  │
                         │ decode,    │   │ client,  │  │ logging  │
                         │ output     │   │ parsers, │  └──────────┘
                         └──────┬─────┘   │ streams  │        ▲
                                │         └────┬─────┘        │
                                └──────────────┴──────────────┘
                              (all depend on ytm-core;
                               ytm-player depends on ytm-api)
```

| Crate | Responsibility | Talks to |
|---|---|---|
| `ytm-core` | `Config` (TOML), `tracing` setup | – |
| `ytm-api` | InnerTube `WEB_REMIX` client, JSON → typed models, `MusicApi` and `StreamResolver` traits, native + yt-dlp stream resolvers | YouTube |
| `ytm-player` | `Queue`, HTTP range source, fMP4 demuxer, AAC decode, resampling, output, `Player` handle | `StreamResolver`, audio device |
| `ytm-app` | window, theme, widgets, views, `AppState`, on-device library, backend task, OS integration | everything above |

All YouTube-specific knowledge is behind two traits (`MusicApi`, `StreamResolver`) in `ytm-api`; the UI and
player are tested against mocks.

## Threads and data flow (`ytm-app`)

```
 UI thread (egui, immediate mode)              tokio runtime (2 workers)
 ───────────────────────────────              ─────────────────────────────
  AppState  ◄── apply(Event) ◄── mpsc ◄──────  Backend::handle(Action)
     │                                              │  ├─ MusicApi calls (reqwest)
     │  Action (search, playback, …)                │  ├─ thumbnails (ThumbLoader)
     └──────────── mpsc ───────────────────────────►│  └─ Player commands
     reads Player::state()  (Arc snapshot)                     │
     reads Player::position() (atomics)                        ▼
                                              ┌─ controller task (queue, prefetch)
  every background result calls               │
  ctx.request_repaint()                       ├─ decode thread ──► AudioBuf ──► cpal callback
                                              └─ download workers (HTTP ranges)
```

* The UI thread **only reads** `AppState` and **sends** `Action`s; state changes happen in
  `AppState::apply(Event)` and a handful of command methods. OS media-key events enter through the same `Action` channel.
* Nothing repaints continuously. Wake-ups: input, background results (`request_repaint`), a 100 ms tick while a track
  is playing/loading (seek bar), and a debounce timer for search suggestions (`request_repaint_after`).
* Thumbnails: `ThumbLoader` is an egui `BytesLoader` (memory map + size-capped disk cache); `egui_extras` decodes
  and caches textures. Widgets only request images that are on screen.

## Playback pipeline (`ytm-player`)

```
 resolve (StreamResolver) ─► RemoteFile (3 workers, 256 KiB range chunks, cached in RAM)
        ─► Fmp4 demuxer (moov+sidx, 1 segment at a time) ─► symphonia AAC decoder
        ─► channel remap ─► rubato FFT resampler ─► AudioBuf (3 s) ─► cpal callback (volume, pause)
```

Details and rationale: `DECISIONS.md` (D5–D10). The next track is resolved, downloaded and its decode job queued
while the current one plays; the decode thread starts it as soon as the current one is fully decoded.
