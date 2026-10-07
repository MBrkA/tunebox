use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui::{self, Align2, CornerRadius, Frame, Key, Margin, Modifiers, Panel, Stroke};
use tokio::sync::mpsc::UnboundedReceiver;
use ytm_player::{Command, Status};

use crate::backend::Event;
use crate::state::{AppState, Route};
use crate::theme::{self, c_bg, c_surface, c_text};
use crate::thumbs::ThumbLoader;
use crate::views;

/// Developer switches (hidden CLI flags) used to drive the app for screenshots.
#[derive(Default, Clone)]
pub struct DevOptions {
    /// Save a screenshot to this path and exit.
    pub screenshot: Option<PathBuf>,
    /// Run this search on start.
    pub query: Option<String>,
    /// Start playing the first song result.
    pub play_first: bool,
    /// Open the now-playing view.
    pub open_now_playing: bool,
    pub open_queue: bool,
    /// Open the keyboard-shortcut cheat-sheet.
    pub open_help: bool,
    /// Initial window size in points (`--size 800x600`).
    pub size: Option<[f32; 2]>,
    /// Search filter name (songs, albums, artists, playlists, videos).
    pub filter: Option<String>,
    /// Start on this route: home, explore, album:ID, artist:ID, playlist:ID.
    pub route: Option<String>,
    /// Like the first search results and put them in a new local playlist, then open it.
    pub local_demo: bool,
    /// Put the opened local playlist into edit mode.
    pub edit_playlist: bool,
    /// Open the "new playlist" dialog.
    pub new_playlist_dialog: bool,
    /// Seconds to wait before taking the screenshot.
    pub shot_delay: f32,
}

pub struct TuneboxApp {
    state: AppState,
    events: UnboundedReceiver<Event>,
    thumbs: Arc<ThumbLoader>,
    dev: DevOptions,
    started: Instant,
    dev_query_sent: bool,
    dev_played: bool,
    shot_requested: bool,
    first_frame_logged: bool,
    dev_local_done: bool,
    published_menu_version: Option<u64>,
    session_version: u64,
    session_saved: Instant,
    session_armed: bool,
    perf: FrameStats,
    tray: crate::tray::Tray,
    tray_state: crate::tray::TrayState,
    announcer: crate::notify::Announcer,
}

/// Rolling frame-time statistics, logged at debug level.
#[derive(Default)]
struct FrameStats {
    frames: u32,
    total: Duration,
    max: Duration,
    since: Option<Instant>,
}

impl FrameStats {
    fn record(&mut self, took: Duration) {
        let since = *self.since.get_or_insert_with(Instant::now);
        self.frames += 1;
        self.total += took;
        self.max = self.max.max(took);
        if since.elapsed() >= Duration::from_secs(5) {
            tracing::debug!(
                frames = self.frames,
                fps = self.frames as f32 / since.elapsed().as_secs_f32(),
                avg_ms = self.total.as_secs_f32() * 1000.0 / self.frames as f32,
                max_ms = self.max.as_secs_f32() * 1000.0,
                "ui frame stats"
            );
            *self = Self::default();
        }
    }
}

impl TuneboxApp {
    pub fn new(
        state: AppState,
        events: UnboundedReceiver<Event>,
        thumbs: Arc<ThumbLoader>,
        dev: DevOptions,
        tray: crate::tray::Tray,
    ) -> Self {
        Self {
            state,
            events,
            thumbs,
            dev,
            started: Instant::now(),
            dev_query_sent: false,
            dev_played: false,
            shot_requested: false,
            first_frame_logged: false,
            dev_local_done: false,
            published_menu_version: None,
            session_version: 0,
            session_saved: Instant::now(),
            session_armed: false,
            perf: FrameStats::default(),
            tray,
            tray_state: crate::tray::TrayState::default(),
            announcer: crate::notify::Announcer::default(),
        }
    }

    fn session_snapshot(&self) -> crate::session::Session {
        let ps = &self.state.ps;
        let (tracks, current) = match ps.current {
            // a shuffled queue is reshuffled on restore, so only the unshuffled order is worth keeping
            Some(cur) if !ps.shuffle => {
                let (order, at) = crate::session::save_order(ps.tracks.len(), cur, &ps.upcoming);
                (order.iter().map(|&i| ps.tracks[i].clone()).collect(), at)
            }
            cur => (ps.tracks.as_ref().clone(), cur.unwrap_or(0)),
        };
        crate::session::Session {
            tracks,
            current,
            position_ms: self.state.player.position().as_millis() as u64,
            shuffle: ps.shuffle,
            repeat: crate::session::repeat_code(ps.repeat),
        }
    }

    /// Persists the play session when the queue/track changed, and every few seconds while
    /// playing (for the position). Writes happen on a worker thread.
    fn autosave_session(&mut self, ctx: &egui::Context) {
        const EVERY: Duration = Duration::from_secs(10);
        let ps = &self.state.ps;
        // Do not overwrite the saved session with the empty start-up state before it is restored.
        self.session_armed |= !ps.tracks.is_empty();
        if !self.session_armed || !self.state.config.restore_session {
            return;
        }
        let playing = ps.status == ytm_player::Status::Playing;
        let changed = ps.version != self.session_version;
        if !changed && !(playing && self.session_saved.elapsed() >= EVERY) {
            if playing {
                ctx.request_repaint_after(EVERY.saturating_sub(self.session_saved.elapsed()));
            }
            return;
        }
        self.session_version = ps.version;
        self.session_saved = Instant::now();
        let snap = self.session_snapshot();
        std::thread::spawn(move || snap.save());
    }

    fn drive_dev(&mut self, ctx: &egui::Context) {
        if !self.dev_query_sent {
            if let Some(q) = self.dev.query.clone() {
                self.state.search.filter = match self.dev.filter.as_deref() {
                    Some("songs") => ytm_api::SearchFilter::Songs,
                    Some("videos") => ytm_api::SearchFilter::Videos,
                    Some("albums") => ytm_api::SearchFilter::Albums,
                    Some("artists") => ytm_api::SearchFilter::Artists,
                    Some("playlists") => ytm_api::SearchFilter::Playlists,
                    _ => ytm_api::SearchFilter::All,
                };
                self.state.submit_search(&q);
            }
            if let Some(r) = self.dev.route.clone() {
                let route = match r.split_once(':') {
                    Some(("album", id)) => Route::Album(id.into()),
                    Some(("artist", id)) => Route::Artist(id.into()),
                    Some(("playlist", id)) => Route::Playlist(id.into()),
                    Some(("mood", params)) => Route::Mood(params.into()),
                    _ if r == "moods" => Route::Moods,
                    _ if r == "new-releases" => Route::NewReleases,
                    _ if r == "charts" => Route::Charts(String::new()),
                    Some(("charts", code)) => Route::Charts(code.into()),
                    _ if r == "explore" => Route::Explore,
                    _ if r == "library" => Route::Library,
                    _ if r == "playlists" => Route::Playlists,
                    _ if r == "history" => Route::History,
                    _ if r == "settings" => Route::Settings,
                    _ => Route::Home,
                };
                self.state.navigate(route);
            }
            self.state.queue_open = self.dev.open_queue;
            self.state.help_open = self.dev.open_help;
            if self.dev.new_playlist_dialog {
                self.state
                    .run(crate::state::UiAction::NewPlaylistFor(Vec::new()));
            }
            self.dev_query_sent = true;
        }
        if self.dev.play_first && !self.dev_played && !self.state.search.items.is_empty() {
            let tracks = self.state.search.tracks();
            if !tracks.is_empty() {
                self.state.play_tracks((*tracks).clone(), 0);
                self.dev_played = true;
            }
        }
        if self.dev.local_demo && !self.dev_local_done && !self.state.search.tracks().is_empty() {
            self.dev_local_done = true;
            let tracks: Vec<_> = self.state.search.tracks().iter().take(4).cloned().collect();
            for t in tracks.iter().take(3) {
                self.state.toggle_like(t);
            }
            self.state
                .run(crate::state::UiAction::NewPlaylistFor(tracks));
            if let Some(np) = self.state.new_playlist.as_mut() {
                np.title = "Road trip".into();
            }
            self.state.create_playlist();
            if let Some(p) = self.state.local.playlists.first() {
                let id = p.id.clone();
                self.state.navigate(Route::Playlist(id));
                self.state.run(crate::state::UiAction::SetPlaylistEdit(
                    self.dev.edit_playlist,
                ));
            }
        }
        if self.dev.open_now_playing && self.state.ps.current.is_some() {
            self.state.now_playing_open = true;
        }
        if let Some(path) = &self.dev.screenshot {
            let ready = self.started.elapsed().as_secs_f32() >= self.dev.shot_delay;
            if ready && !self.shot_requested {
                self.shot_requested = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
            }
            let shot = ctx.input(|i| {
                i.events.iter().find_map(|e| match e {
                    egui::Event::Screenshot { image, .. } => Some(image.clone()),
                    _ => None,
                })
            });
            if let Some(img) = shot {
                let (w, h) = (img.width() as u32, img.height() as u32);
                let rgba: Vec<u8> = img.pixels.iter().flat_map(|p| p.to_array()).collect();
                if let Err(e) = image::save_buffer(path, &rgba, w, h, image::ColorType::Rgba8) {
                    tracing::error!(error = %e, "could not save screenshot");
                } else {
                    tracing::info!(path = %path.display(), w, h, "screenshot saved");
                }
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            ctx.request_repaint_after(Duration::from_millis(200));
        }
    }

    /// Closing the window hides it while the tray icon is showing and the user asked for that.
    fn close_to_tray(&mut self, ctx: &egui::Context) {
        let close = ctx.input(|i| i.viewport().close_requested());
        // `--screenshot` closes the window to exit; never hide it to the tray then.
        let capturing = self.dev.screenshot.is_some();
        if close
            && !capturing
            && self.state.config.close_to_tray
            && self.tray.active()
            && !self.tray.quitting()
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.tray.hide_window(ctx);
        }
    }

    /// Updates the tray tooltip and, with the window in the background, notifies about the song.
    fn track_changed(&mut self, ctx: &egui::Context) {
        // macOS decides in `notify::spawn_watcher` (this runs only while the window is redrawn).
        if cfg!(target_os = "macos") {
            return;
        }
        let playing = self.state.ps.status == Status::Playing;
        let track = self.state.ps.current_track();
        let id = track.map(|t| t.video_id.clone());
        if !self.announcer.is_new(id.as_deref(), playing) {
            return;
        }
        let Some(track) = track else { return };
        let (title, artist) = (track.title.clone(), track.artist_line());
        let (focused, minimized) = ctx.input(|i| {
            let v = i.viewport();
            (v.focused.unwrap_or(true), v.minimized.unwrap_or(false))
        });
        let background = !focused || minimized || self.tray.hidden();
        tracing::info!(
            focused,
            minimized,
            hidden = self.tray.hidden(),
            enabled = self.state.config.notifications,
            "new track: notification decision"
        );
        if self.state.config.notifications && background {
            crate::notify::song_changed(&title, &artist);
        }
    }

    /// Keeps the tray tooltip, label and play badge in step with the player.
    fn sync_tray(&mut self) {
        self.tray.sync_dock();
        crate::notify::set_enabled(self.state.config.notifications);
        let playing = self.state.ps.status == Status::Playing;
        let line = self.state.ps.current_track().map_or(String::new(), |t| {
            crate::notify::summary_line(&t.title, &t.artist_line())
        });
        let label = (self.state.config.tray_label && !line.is_empty())
            .then(|| crate::tray::shorten_label(&line));
        let next = crate::tray::TrayState {
            line,
            label,
            playing,
        };
        if next != self.tray_state {
            self.tray.set_state(&next);
            self.tray_state = next;
        }
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        handle_shortcuts(ctx, &mut self.state);
    }
}

/// Seconds moved by the plain arrow keys.
const SEEK_STEP_SECS: i64 = 5;
/// Volume change of Ctrl/Cmd+Up/Down.
const VOLUME_STEP: f32 = 0.05;

/// Handles the app-wide keyboard shortcuts (see `views::help` for the list shown to the user).
pub(crate) fn handle_shortcuts(ctx: &egui::Context, st: &mut AppState) {
    if ctx.egui_wants_keyboard_input() {
        return;
    }
    // Symbol shortcuts go by the typed character so they work on every keyboard layout; the
    // event is removed so it cannot also be typed into the search box we are about to focus.
    let typed = |i: &mut egui::InputState, c: &str| {
        let before = i.events.len();
        i.events
            .retain(|e| !matches!(e, egui::Event::Text(t) if t == c));
        i.events.len() != before
    };
    ctx.input_mut(|i| {
        let cmd = Modifiers::COMMAND;
        let none = Modifiers::NONE;
        if i.consume_key(none, Key::Space) {
            st.playback(Command::Toggle);
        }
        if i.consume_key(cmd, Key::ArrowRight) {
            st.playback(Command::Next);
        }
        if i.consume_key(cmd, Key::ArrowLeft) {
            st.playback(Command::Prev);
        }
        // Combinations with modifiers first: a plain-key check would also consume them.
        if i.consume_key(Modifiers::ALT, Key::ArrowLeft) {
            st.back();
        }
        if i.consume_key(none, Key::ArrowRight) {
            st.seek_by(SEEK_STEP_SECS);
        }
        if i.consume_key(none, Key::ArrowLeft) {
            st.seek_by(-SEEK_STEP_SECS);
        }
        if i.consume_key(cmd, Key::ArrowUp) {
            st.adjust_volume(VOLUME_STEP);
        }
        if i.consume_key(cmd, Key::ArrowDown) {
            st.adjust_volume(-VOLUME_STEP);
        }
        if i.consume_key(none, Key::M) {
            st.toggle_mute();
        }
        if i.consume_key(none, Key::L) {
            st.toggle_like_current();
        }
        if i.consume_key(none, Key::S) {
            let on = !st.ps.shuffle;
            st.playback(Command::SetShuffle(on));
        }
        if i.consume_key(none, Key::R) {
            st.playback(Command::SetRepeat(st.ps.repeat.cycle()));
        }
        if i.consume_key(none, Key::Q) {
            st.queue_open = !st.queue_open;
        }
        if i.consume_key(none, Key::N) {
            st.now_playing_open = !st.now_playing_open;
        }
        for (key, route) in [
            (Key::Num1, Route::Home),
            (Key::Num2, Route::Explore),
            (Key::Num3, Route::Library),
            (Key::Num4, Route::Playlists),
            (Key::Comma, Route::Settings),
        ] {
            if i.consume_key(cmd, key) {
                st.now_playing_open = false;
                st.navigate_fresh(route);
            }
        }
        if typed(i, "/") {
            st.now_playing_open = false;
            st.search_focus_requested = true;
        }
        if typed(i, "?") {
            st.help_open = !st.help_open;
        }
        if i.consume_key(none, Key::Escape) {
            // Close the topmost thing first: help, the queue drawer, then the now-playing view.
            if st.help_open {
                st.help_open = false;
            } else if st.queue_open {
                st.queue_open = false;
            } else {
                st.now_playing_open = false;
            }
        }
    });
}

impl eframe::App for TuneboxApp {
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if !self.state.config.restore_session {
            crate::session::clear();
        } else if self.session_armed {
            self.session_snapshot().save();
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let frame_start = Instant::now();
        let ctx = ui.ctx().clone();
        if !self.first_frame_logged {
            self.first_frame_logged = true;
            if let Some(start) = crate::START.get() {
                tracing::info!(ms = start.elapsed().as_millis() as u64, "first frame");
            }
        }
        while let Ok(ev) = self.events.try_recv() {
            self.state.apply(ev);
        }
        self.state.refresh_player();
        if let Some(wait) = self.state.flush_volume() {
            ctx.request_repaint_after(wait);
        }
        if self.published_menu_version != Some(self.state.menu_version) {
            self.published_menu_version = Some(self.state.menu_version);
            views::common::publish_menu_data(
                &ctx,
                views::common::MenuData {
                    local_playlists: self.state.local_playlist_options(),
                    liked: self
                        .state
                        .local
                        .liked
                        .iter()
                        .map(|t| t.video_id.clone())
                        .collect(),
                },
            );
        }
        theme::follow_system(&ctx, theme::Pref::from_code(&self.state.config.theme));
        self.shortcuts(&ctx);
        if crate::single::take_show_request() {
            self.tray.mark_shown();
        }
        self.close_to_tray(&ctx);
        self.track_changed(&ctx);
        self.sync_tray();
        self.autosave_session(&ctx);
        self.drive_dev(&ctx);

        let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
        let chrome = crate::layout::chrome(ctx.content_rect().width());
        if views::titlebar::enabled() {
            Panel::top("titlebar")
                .exact_size(views::titlebar::HEIGHT)
                .show_separator_line(false)
                .frame(Frame::new().inner_margin(Margin::ZERO))
                .show(ui, |ui| {
                    if let Some(action) = views::titlebar::show(ui, "Tunebox", maximized) {
                        ctx.send_viewport_cmd(action.command(maximized));
                    }
                });
        }

        Panel::bottom("player_bar")
            .exact_size(88.0)
            .show_separator_line(false)
            .frame(
                Frame::new()
                    .fill(c_surface())
                    .stroke(Stroke::new(1.0, theme::c_border()))
                    .inner_margin(Margin::ZERO),
            )
            .show(ui, |ui| views::player_bar::show(ui, &mut self.state));

        // Eased 0→1 on open; evaluated every frame so it restarts from 0 after closing.
        let np_t = ui.ctx().animate_bool_with_time_and_easing(
            egui::Id::new("np_open"),
            self.state.now_playing_open,
            0.35,
            egui::emath::easing::cubic_out,
        );
        if self.state.now_playing_open {
            egui::CentralPanel::default()
                .frame(Frame::new().fill(c_bg()))
                .show(ui, |ui| {
                    ui.set_opacity(np_t);
                    views::now_playing::show(ui, &mut self.state, &self.thumbs, np_t)
                });
        } else {
            Panel::left("sidebar")
                .exact_size(chrome.sidebar_width)
                .show_separator_line(false)
                .frame(
                    Frame::new()
                        .fill(c_bg())
                        .inner_margin(Margin::symmetric(if chrome.compact { 8 } else { 12 }, 0)),
                )
                .show(ui, |ui| {
                    views::sidebar::show(ui, &mut self.state, chrome.compact)
                });

            Panel::top("top_bar")
                .exact_size(76.0)
                .show_separator_line(false)
                .frame(
                    Frame::new()
                        .fill(c_bg())
                        .inner_margin(Margin::symmetric(chrome.page_margin, 0)),
                )
                .show(ui, |ui| views::topbar::show(ui, &mut self.state));

            egui::CentralPanel::default()
                .frame(
                    Frame::new()
                        .fill(c_bg())
                        .inner_margin(Margin::symmetric(chrome.page_margin, 0)),
                )
                .show(ui, |ui| {
                    // Every page gets its own scroll positions: without this, egui sees the same
                    // (unnamed) scroll area on each page and keeps the old offset.
                    let page = page_id(
                        &self.state.route,
                        self.state.search.seq,
                        self.state.page_epoch(&self.state.route),
                    );
                    ui.push_id(page, |ui| match self.state.route.clone() {
                        Route::Search => views::search::show(ui, &mut self.state),
                        Route::Home => views::browse::home(ui, &mut self.state),
                        Route::Explore => views::browse::explore(ui, &mut self.state),
                        Route::NewReleases => views::browse::new_releases(ui, &mut self.state),
                        Route::Charts(country) => {
                            views::charts::page(ui, &mut self.state, &country)
                        }
                        Route::Moods => views::moods::page(ui, &mut self.state),
                        Route::Mood(params) => views::moods::category(ui, &mut self.state, &params),
                        Route::Album(id) => views::album::show(ui, &mut self.state, &id),
                        Route::Artist(id) => views::artist::show(ui, &mut self.state, &id),
                        Route::Playlist(id) => views::playlist::show(ui, &mut self.state, &id),
                        Route::Library => views::library::library(ui, &mut self.state),
                        Route::Playlists => views::library::playlists(ui, &mut self.state),
                        Route::History => views::history::show(ui, &mut self.state),
                        Route::Settings => views::settings::show(ui, &mut self.state),
                    });
                });
        }

        // The queue floats above whatever page is showing (also over the now-playing view).
        let top_inset = if views::titlebar::enabled() {
            views::titlebar::HEIGHT
        } else {
            0.0
        };
        views::queue::overlay(&ctx, &mut self.state, top_inset, 88.0);
        views::dialogs::show(&ctx, &mut self.state);
        views::help::show(&ctx, &mut self.state);
        if views::titlebar::enabled() {
            views::titlebar::window_chrome(&ctx, maximized);
        }
        toast(&ctx, &mut self.state);

        // Repaint only when something is changing.
        if let Some(wake) = self.state.poll_suggestions() {
            ctx.request_repaint_after(wake);
        }
        match self.state.ps.status {
            Status::Playing => {
                ctx.request_repaint_after(self.next_progress_repaint());
            }
            // The loading spinner animates on its own; keep the clock honest meanwhile.
            Status::Loading => ctx.request_repaint_after(Duration::from_millis(100)),
            _ => {}
        }
        if self.state.toast.is_some() {
            ctx.request_repaint_after(Duration::from_millis(500));
        }
        self.perf.record(frame_start.elapsed());
    }
}

impl TuneboxApp {
    /// When the next *visible* change of playback progress happens: the clock
    /// ticks over to the next second, or the seek bar moves by one pixel.
    fn next_progress_repaint(&self) -> Duration {
        const BAR_PX: f64 = 640.0;
        let pos = self.state.player.position().as_secs_f64();
        let to_next_second = 1.0 - pos.fract();
        let to_next_pixel = self
            .state
            .ps
            .duration
            .map_or(1.0, |d| d.as_secs_f64() / BAR_PX);
        Duration::from_secs_f64(to_next_second.min(to_next_pixel).clamp(0.1, 1.0))
    }
}

/// Identifies the page being shown, so scroll offsets (and carousel positions) do not carry over
/// from one page to the next. A new search is a new page; loading *more* results is not.
fn page_id(route: &Route, search_seq: u64, epoch: u64) -> egui::Id {
    match route {
        Route::Search => egui::Id::new(("page", "search", search_seq)),
        // `epoch` changes when the page is opened fresh from the sidebar: back at the top
        other => egui::Id::new(("page", other, epoch)),
    }
}

fn toast(ctx: &egui::Context, state: &mut AppState) {
    let Some(t) = &state.toast else { return };
    egui::Area::new(egui::Id::new("toast"))
        .order(egui::Order::Tooltip)
        .anchor(Align2::RIGHT_BOTTOM, egui::vec2(-20.0, -108.0))
        .show(ctx, |ui| {
            Frame::new()
                .fill(theme::c_surface_active())
                .corner_radius(CornerRadius::same(12))
                .inner_margin(Margin::symmetric(16, 12))
                .stroke(Stroke::new(1.0, theme::c_border()))
                .show(ui, |ui| {
                    ui.set_max_width(380.0);
                    ui.label(egui::RichText::new(&t.text).color(c_text()));
                });
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{pos2, vec2, RawInput, Rect, ScrollArea};

    /// Draws one frame of a tall scroll area (optionally forcing its offset) and returns the offset.
    fn frame(ctx: &egui::Context, page: Option<egui::Id>, force: Option<f32>) -> f32 {
        let mut offset = f32::NAN;
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(400.0, 300.0))),
            ..RawInput::default()
        };
        let mut out = ctx.run_ui(input, |ui| {
            let mut show = |ui: &mut egui::Ui| {
                let mut area = ScrollArea::vertical().max_height(100.0);
                if let Some(y) = force {
                    area = area.vertical_scroll_offset(y);
                }
                offset = area
                    .show(ui, |ui| ui.allocate_space(vec2(10.0, 2000.0)))
                    .state
                    .offset
                    .y;
            };
            match page {
                Some(id) => {
                    ui.push_id(id, show);
                }
                None => show(ui),
            }
        });
        out.textures_delta.clear();
        offset
    }

    #[test]
    fn without_per_page_ids_the_scroll_offset_leaks_to_the_next_page() {
        // documents the bug this fixes
        let ctx = egui::Context::default();
        assert_eq!(frame(&ctx, None, Some(150.0)), 150.0);
        assert_eq!(
            frame(&ctx, None, None),
            150.0,
            "same id, so the offset is kept"
        );
    }

    #[test]
    fn a_fresh_visit_starts_at_the_top_but_back_keeps_the_position() {
        let ctx = egui::Context::default();
        let home = page_id(&Route::Home, 0, 0);
        assert_eq!(
            frame(&ctx, Some(home), Some(400.0)),
            400.0,
            "scrolled down on Home"
        );
        // go to Explore and come back with Back: same page id, same position
        let explore = page_id(&Route::Explore, 0, 0);
        assert_eq!(frame(&ctx, Some(explore), None), 0.0);
        assert_eq!(frame(&ctx, Some(home), None), 400.0, "Back restores it");
        // clicking Home in the sidebar bumps its epoch: back at the top, even from Home itself
        let fresh = page_id(&Route::Home, 0, 1);
        assert_ne!(home, fresh);
        assert_eq!(
            frame(&ctx, Some(fresh), None),
            0.0,
            "sidebar starts at the top"
        );
        assert_eq!(frame(&ctx, Some(fresh), Some(90.0)), 90.0);
        assert_eq!(
            frame(&ctx, Some(fresh), None),
            90.0,
            "and it then scrolls normally"
        );
    }

    #[test]
    fn every_page_starts_at_the_top() {
        let ctx = egui::Context::default();
        let genre = page_id(&Route::Mood("chill".into()), 0, 0);
        let other_genre = page_id(&Route::Mood("jazz".into()), 0, 0);
        assert_eq!(
            frame(&ctx, Some(genre), Some(150.0)),
            150.0,
            "scrolled down on one page"
        );
        assert_eq!(
            frame(&ctx, Some(genre), None),
            150.0,
            "staying on the page keeps the position"
        );
        assert_eq!(
            frame(&ctx, Some(other_genre), None),
            0.0,
            "another genre starts at the top"
        );
        let explore = page_id(&Route::Explore, 0, 0);
        assert_eq!(frame(&ctx, Some(explore), Some(300.0)), 300.0);
        let moods = page_id(&Route::Moods, 0, 0);
        assert_eq!(
            frame(&ctx, Some(moods), None),
            0.0,
            "Explore -> Moods & genres starts at the top"
        );
    }

    #[test]
    fn page_ids_distinguish_pages_but_not_loading_more_results() {
        assert_ne!(
            page_id(&Route::Album("a".into()), 0, 0),
            page_id(&Route::Album("b".into()), 0, 0)
        );
        assert_ne!(
            page_id(&Route::Charts("US".into()), 0, 0),
            page_id(&Route::Charts("TR".into()), 0, 0)
        );
        assert_ne!(page_id(&Route::Home, 0, 0), page_id(&Route::Explore, 0, 0));
        // a new search (new sequence number) is a new page; paging through the same results is not
        assert_ne!(page_id(&Route::Search, 1, 0), page_id(&Route::Search, 2, 0));
        assert_eq!(page_id(&Route::Search, 5, 0), page_id(&Route::Search, 5, 0));
    }
}
