---
name: tunebox-add-feature
description: Step-by-step recipe for adding a page, endpoint, backend action or UI control to Tunebox following its architecture (trait → parser+fixture → Action/Event → AppState → view). Use when implementing any new user-facing feature.
---

# Adding a feature the Tunebox way

1. **API (ytm-api):** add a model in `models.rs`; a method on `MusicApi` (lib.rs) + impl in `client.rs`
   ; a defensive parser in `pages.rs`/`parse.rs`; record a
   fixture and add a test (see `ytm-api-fixtures`). For request-shape tests use a local fake server with `InnerTube::with_base_url`.
2. **Backend (ytm-app/backend.rs):** add `Action::X` and `Event::X`; handle in `Backend::handle` (spawn a task,
   `emit` the result — `emit` repaints).
3. **State (state.rs):** fields (`Load<T>` for fetched data, ids → `HashMap<String, Load<T>>`), request in
   `ensure_loaded`/a command method (dedupe in-flight work), apply the event in `apply`. Ignore stale results
   (sequence numbers / ids). Bump `menu_version` if context menus depend on it.
4. **View (views/):** `fn show(ui, app)`; read state, collect `Vec<UiAction>`, apply after drawing. Reuse
   `common::{centered_spinner, error_panel, track_rows, carousel, track_menu}` and `widgets::*`. Long lists:
   `ScrollArea::show_rows`. Only request images for visible rects (`paint_art` already does). New route: add to
   `Route`, `app.rs` match, `ensure_loaded`.
5. **Tests:** `AppState` tests in `state.rs` (they spawn a `Player` with `OutputKind::Null` and read the action
   channel); parser tests with fixtures; engine tests if playback changes.
6. **Verify visually** (`tunebox-ui-capture`), then run the full command list in `CLAUDE.md`.
7. Record non-obvious choices (with evidence) in `docs/DECISIONS.md`; update `docs/STATUS.md` status/limits.
