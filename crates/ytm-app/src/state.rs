//! The single model the UI renders from. The UI reads it and mutates it only
//! through `apply` (backend events) and the small command methods below, which
//! send [`Action`]s; nothing else holds UI-relevant state.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::mpsc::UnboundedSender;
use ytm_api::{
    AlbumPage, ArtistPage, CategoryPage, ChartsPage, HomePage, Lyrics, MoodsPage, PlaylistPage,
    SearchFilter, SearchItem, Track,
};
use ytm_player::{Command, Player, PlayerState};

use crate::backend::{Action, Event};
use crate::local::{is_local_id, LocalLibrary};

const SUGGEST_DEBOUNCE: Duration = Duration::from_millis(180);
const TOAST_TIME: Duration = Duration::from_secs(5);
/// Shown when an action needs every playlist loaded but startup is still reading them.
const STILL_LOADING: &str = "Your playlists are still loading, try again in a moment";

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Route {
    Home,
    Explore,
    NewReleases,
    /// Charts for a country code (`""` = YouTube's default for the user's location).
    Charts(String),
    /// The full "Moods & genres" page.
    Moods,
    /// One mood/genre category, by its `params`.
    Mood(String),
    Library,
    Playlists,
    Settings,
    Search,
    Album(String),
    Artist(String),
    Playlist(String),
}

/// Data that is fetched on demand.
#[derive(Debug, Clone, Default)]
pub enum Load<T> {
    #[default]
    Idle,
    Loading,
    Ready(Arc<T>),
    Failed(String),
}

impl<T> Load<T> {
    pub fn is_idle_or_failed(&self) -> bool {
        matches!(self, Self::Idle | Self::Failed(_))
    }

    fn from_result(r: Result<T, String>) -> Self {
        match r {
            Ok(v) => Self::Ready(Arc::new(v)),
            Err(e) => Self::Failed(e),
        }
    }
}

/// Deferred UI intents, collected while drawing and applied afterwards.
#[derive(Debug, Clone)]
pub enum UiAction {
    PlayTracks(Vec<Track>, usize),
    PlayNext(Track),
    Enqueue(Vec<Track>),
    Go(Route),
    Radio(String),
    /// Like/unlike a song (saved on this device).
    ToggleLike(Track),
    /// Save/unsave an album, artist or playlist on this device.
    ToggleSaved(SearchItem),
    AddToLocalPlaylist {
        playlist_id: String,
        title: String,
        tracks: Vec<Track>,
    },
    LocalRemoveTrack {
        playlist_id: String,
        index: usize,
    },
    LocalMoveTrack {
        playlist_id: String,
        index: usize,
        delta: isize,
    },
    /// Open the rename dialog for a local playlist.
    RenamePlaylist(String),
    /// Enter/leave edit mode on the open local playlist (drag to reorder, remove buttons).
    SetPlaylistEdit(bool),
    /// Open the "new playlist" dialog pre-filled with these tracks.
    NewPlaylistFor(Vec<Track>),
    /// Ask for confirmation, then delete (see `ConfirmDeletePlaylist`).
    DeletePlaylist(String),
    ConfirmDeletePlaylist(String),
    BackupLibrary,
    RestoreLibrary,
    /// Replace the library with the restored backup the user confirmed.
    ApplyRestore,
    MoveDataDir,
    ExportPlaylist(String),
    ChangePlaylistCover(String),
    RemovePlaylistCover(String),
    ImportPlaylist,
    /// Shuffle-play these tracks from a random starting point.
    ShufflePlay(Vec<Track>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LibraryTab {
    #[default]
    Songs,
    Albums,
    Artists,
}

pub struct NewPlaylist {
    pub title: String,
    pub tracks: Vec<Track>,
    /// `Some(id)`: renaming that local playlist instead of creating one.
    pub rename: Option<String>,
}

#[derive(Default)]
pub struct LyricsState {
    pub video_id: String,
    pub load: Load<Option<Lyrics>>,
}

#[derive(Default)]
pub struct SearchState {
    /// Text in the search box (not necessarily submitted).
    pub input: String,
    /// The query the results belong to.
    pub query: String,
    pub filter: SearchFilter,
    pub seq: u64,
    pub loading: bool,
    pub loading_more: bool,
    /// Shared so views can hold the list without copying it every frame.
    pub items: Arc<Vec<SearchItem>>,
    songs: Arc<Vec<Track>>,
    pub continuation: Option<String>,
    pub error: Option<String>,
    pub suggestions: Vec<String>,
    pub suggest_seq: u64,
    suggest_due: Option<Instant>,
    pub show_suggestions: bool,
    /// Keyboard-highlighted suggestion row.
    pub suggest_sel: Option<usize>,
    /// Where the suggestion popup was drawn last frame (keeps it open while clicked).
    pub popup_rect: Option<eframe::egui::Rect>,
}

impl SearchState {
    /// All tracks in the result list, in display order (queue source for "play").
    pub fn tracks(&self) -> Arc<Vec<Track>> {
        self.songs.clone()
    }

    pub fn set_items(&mut self, items: Vec<SearchItem>) {
        self.items = Arc::new(items);
        self.refresh_songs();
    }

    pub fn append_items(&mut self, more: Vec<SearchItem>) {
        Arc::make_mut(&mut self.items).extend(more);
        self.refresh_songs();
    }

    fn refresh_songs(&mut self) {
        self.songs = Arc::new(
            self.items
                .iter()
                .filter_map(|i| match i {
                    SearchItem::Track(t) => Some(t.clone()),
                    _ => None,
                })
                .collect(),
        );
    }
}

pub struct Toast {
    pub text: String,
    pub since: Instant,
}

pub struct AppState {
    pub route: Route,
    history: Vec<Route>,
    pub search: SearchState,
    pub player: Player,
    /// Snapshot of the player taken at the start of each frame.
    pub ps: PlayerState,
    pub now_playing_open: bool,
    pub queue_open: bool,
    /// The keyboard-shortcut cheat-sheet (`?`).
    pub help_open: bool,
    /// Set by the `/` shortcut; the top bar focuses the search box and clears it.
    pub search_focus_requested: bool,
    pub home: Load<HomePage>,
    pub explore: Load<HomePage>,
    pub new_releases: Load<HomePage>,
    pub charts: HashMap<String, Load<ChartsPage>>,
    pub moods: Load<MoodsPage>,
    pub mood_pages: HashMap<String, Load<CategoryPage>>,
    pub artists: HashMap<String, Load<ArtistPage>>,
    pub albums: HashMap<String, Load<AlbumPage>>,
    pub playlists: HashMap<String, Load<PlaylistPage>>,
    pub playlist_more_loading: bool,
    pub lyrics: LyricsState,
    pub library_tab: LibraryTab,
    /// Bumped whenever data shown in context menus changes.
    pub menu_version: u64,
    pub new_playlist: Option<NewPlaylist>,
    /// Local playlist waiting for the user to confirm its deletion.
    pub confirm_delete: Option<String>,
    /// A parsed backup waiting for the user to confirm replacing the library.
    pub pending_restore: Option<Box<LocalLibrary>>,
    /// The options inside that backup, if it has any.
    pub pending_restore_settings: Option<crate::local::BackupSettings>,
    /// User settings (also on disk); changed from the Settings page.
    pub config: ytm_core::Config,
    /// `config.tray_icon` as the running app started with it (the tray is only set up at launch).
    tray_at_start: bool,
    close_to_tray_at_start: bool,
    /// The folder with all app data (settings, library, playlists, covers). Tests point it at a temp
    /// folder so they never touch the real settings.
    pub data_dir: std::path::PathBuf,
    /// When each listing page was last fetched, for the refresh interval.
    fetched: HashMap<Route, Instant>,
    /// The open local playlist is in edit mode.
    pub playlist_edit: bool,
    /// The on-device library: liked songs, playlists, saved albums/artists/playlists.
    pub local: LocalLibrary,
    /// Volume to restore when un-muting.
    pub muted_volume: Option<f32>,
    /// When the volume last changed and is not yet written to the config file.
    volume_dirty_since: Option<Instant>,
    pub toast: Option<Toast>,
    seq: u64,
    tx: UnboundedSender<Action>,
}

impl AppState {
    pub fn new(player: Player, tx: UnboundedSender<Action>) -> Self {
        let ps = player.state();
        let mut state = Self {
            route: Route::Home,
            history: Vec::new(),
            search: SearchState::default(),
            player,
            ps,
            now_playing_open: false,
            queue_open: false,
            help_open: false,
            search_focus_requested: false,
            home: Load::Idle,
            explore: Load::Idle,
            new_releases: Load::Idle,
            charts: HashMap::new(),
            moods: Load::Idle,
            mood_pages: HashMap::new(),
            artists: HashMap::new(),
            albums: HashMap::new(),
            playlists: HashMap::new(),
            playlist_more_loading: false,
            lyrics: LyricsState::default(),
            library_tab: LibraryTab::Songs,
            menu_version: 0,
            new_playlist: None,
            confirm_delete: None,
            pending_restore: None,
            pending_restore_settings: None,
            config: ytm_core::Config::default(),
            tray_at_start: ytm_core::Config::default().tray_icon,
            close_to_tray_at_start: ytm_core::Config::default().close_to_tray,
            data_dir: ytm_core::Config::data_dir().unwrap_or_default(),
            fetched: HashMap::new(),
            playlist_edit: false,
            local: LocalLibrary::default(),
            muted_volume: None,
            volume_dirty_since: None,
            toast: None,
            seq: 0,
            tx,
        };
        state.ensure_loaded();
        state
    }

    /// Installs the library loaded from disk.
    pub fn with_config(mut self, config: ytm_core::Config) -> Self {
        self.tray_at_start = config.tray_icon;
        self.close_to_tray_at_start = config.close_to_tray;
        self.config = config;
        self
    }

    /// A setting was changed that only takes effect after the app is started again.
    pub fn restart_required(&self) -> bool {
        // Close-to-tray only needs a restart where it changes the windowing backend (Linux, D28).
        self.config.tray_icon != self.tray_at_start
            || (cfg!(target_os = "linux")
                && self.config.close_to_tray != self.close_to_tray_at_start)
    }

    pub fn with_local(mut self, local: LocalLibrary) -> Self {
        self.local = local;
        self.menu_version += 1;
        self
    }

    /// Writes the settings file.
    pub fn save_config(&self) -> Result<(), ytm_core::config::ConfigError> {
        self.config.save_to(&self.data_dir.join("config.toml"))
    }

    /// Writes a changed volume to the config once it has been steady for a moment (sliders emit a
    /// stream of values). Returns how long until a pending write is due, so the caller can wake up.
    pub fn flush_volume(&mut self) -> Option<std::time::Duration> {
        const SETTLE: std::time::Duration = std::time::Duration::from_millis(500);
        let since = self.volume_dirty_since?;
        if let Some(left) = SETTLE.checked_sub(since.elapsed()) {
            return Some(left);
        }
        self.volume_dirty_since = None;
        if let Err(e) = self.save_config() {
            tracing::warn!("could not save volume: {e}");
        }
        None
    }

    /// Folder for custom playlist covers (next to the library file).
    pub fn covers_dir(&self) -> Option<std::path::PathBuf> {
        Some(crate::local::covers_dir(
            &self.data_dir.join("library.json"),
        ))
    }

    /// Removes a custom cover image that is no longer used by any playlist.
    fn delete_cover_file(&self, file: Option<String>) {
        if let (Some(file), Some(dir)) = (file, self.covers_dir()) {
            let _ = std::fs::remove_file(dir.join(file));
        }
    }

    /// Call after every mutation of `self.local`: refreshes menus and saves in the background.
    fn local_changed(&mut self) {
        self.menu_version += 1;
        let snapshot = self.local.snapshot();
        self.send(Action::PersistLocal(Box::new(snapshot)));
    }

    fn next_seq(&mut self) -> u64 {
        self.seq += 1;
        self.seq
    }

    pub fn send(&self, action: Action) {
        let _ = self.tx.send(action);
    }

    pub fn playback(&self, cmd: Command) {
        self.send(Action::Playback(cmd));
    }

    /// Called once at the start of every frame.
    pub fn refresh_player(&mut self) {
        self.ps = self.player.state();
        // Remember the volume across restarts (a muted player keeps the level it will restore).
        let level = self.muted_volume.unwrap_or(self.ps.volume);
        if (level - self.config.volume).abs() > 0.004 {
            self.config.volume = level;
            self.volume_dirty_since = Some(Instant::now());
        }
        if let Some(t) = &self.toast {
            if t.since.elapsed() > TOAST_TIME {
                self.toast = None;
            }
        }
        if let Some(msg) = &self.ps.error {
            // Playback errors are also toasted via events; keep the banner in the player bar only.
            let _ = msg;
        }
    }

    // ---- navigation -----------------------------------------------------

    pub fn navigate(&mut self, route: Route) {
        if self.route != route {
            let prev = std::mem::replace(&mut self.route, route);
            self.history.push(prev);
            if self.history.len() > 64 {
                self.history.remove(0);
            }
        }
        self.now_playing_open = false;
        // Edit mode never carries over to another page.
        self.playlist_edit = false;
        self.expire_stale();
        self.ensure_loaded();
    }

    /// With a refresh interval set, a listing page opened after that long is fetched again.
    fn expire_stale(&mut self) {
        let minutes = self.config.refresh_minutes;
        if minutes == 0 {
            return;
        }
        let max_age = Duration::from_secs(u64::from(minutes) * 60);
        let route = self.route.clone();
        let stale = self
            .fetched
            .get(&route)
            .is_some_and(|t| t.elapsed() >= max_age);
        if stale {
            self.fetched.remove(&route);
            match route {
                Route::Home => self.home = Load::Idle,
                Route::Explore => self.explore = Load::Idle,
                Route::NewReleases => self.new_releases = Load::Idle,
                Route::Moods => self.moods = Load::Idle,
                _ => {}
            }
        }
    }

    /// Starts loading whatever the current route needs (no-op if loaded/loading).
    pub fn ensure_loaded(&mut self) {
        match self.route.clone() {
            Route::Home if self.home.is_idle_or_failed() => {
                self.home = Load::Loading;
                self.send(Action::LoadHome);
            }
            Route::Explore if self.explore.is_idle_or_failed() => {
                self.explore = Load::Loading;
                self.send(Action::LoadExplore);
            }
            Route::NewReleases if self.new_releases.is_idle_or_failed() => {
                self.new_releases = Load::Loading;
                self.send(Action::LoadNewReleases);
            }
            Route::Charts(country)
                if self
                    .charts
                    .get(&country)
                    .is_none_or(Load::is_idle_or_failed) =>
            {
                self.charts.insert(country.clone(), Load::Loading);
                self.send(Action::LoadCharts(country));
            }
            Route::Moods if self.moods.is_idle_or_failed() => {
                self.moods = Load::Loading;
                self.send(Action::LoadMoods);
            }
            Route::Mood(params)
                if self
                    .mood_pages
                    .get(&params)
                    .is_none_or(Load::is_idle_or_failed) =>
            {
                self.mood_pages.insert(params.clone(), Load::Loading);
                self.send(Action::LoadMood(params));
            }
            Route::Artist(id) if self.artists.get(&id).is_none_or(Load::is_idle_or_failed) => {
                self.artists.insert(id.clone(), Load::Loading);
                self.send(Action::LoadArtist(id));
            }
            Route::Album(id) if self.albums.get(&id).is_none_or(Load::is_idle_or_failed) => {
                self.albums.insert(id.clone(), Load::Loading);
                self.send(Action::LoadAlbum(id));
            }
            Route::Playlist(id)
                if !is_local_id(&id)
                    && self.playlists.get(&id).is_none_or(Load::is_idle_or_failed) =>
            {
                self.playlists.insert(id.clone(), Load::Loading);
                self.send(Action::LoadPlaylist(id));
            }
            _ => {}
        }
    }

    /// The UI language changed: YouTube's own text (moods, titles, …) must be fetched again in it.
    pub fn language_changed(&mut self, hl: &str) {
        self.send(Action::SetLanguage(hl.to_owned()));
        self.refetch_everything();
    }

    fn refetch_everything(&mut self) {
        self.fetched.clear();
        self.home = Load::Idle;
        self.explore = Load::Idle;
        self.new_releases = Load::Idle;
        self.moods = Load::Idle;
        self.charts.clear();
        self.mood_pages.clear();
        self.artists.clear();
        self.albums.clear();
        self.playlists.retain(|id, _| is_local_id(id));
        if self.route == Route::Search && !self.search.query.is_empty() {
            self.run_search();
        }
        self.ensure_loaded();
    }

    /// Forces a reload of the current page (used by the error "Try again" button).
    pub fn reload(&mut self) {
        match self.route.clone() {
            Route::Home => self.home = Load::Idle,
            Route::Explore => self.explore = Load::Idle,
            Route::NewReleases => self.new_releases = Load::Idle,
            Route::Charts(country) => {
                self.charts.remove(&country);
            }
            Route::Moods => self.moods = Load::Idle,
            Route::Mood(params) => {
                self.mood_pages.remove(&params);
            }
            Route::Artist(id) => {
                self.artists.remove(&id);
            }
            Route::Album(id) => {
                self.albums.remove(&id);
            }
            Route::Playlist(id) => {
                self.playlists.remove(&id);
            }
            _ => {}
        }
        self.ensure_loaded();
    }

    pub fn load_more_playlist(&mut self, id: &str) {
        let Some(Load::Ready(page)) = self.playlists.get(id) else {
            return;
        };
        let Some(token) = page.continuation.clone() else {
            return;
        };
        if self.playlist_more_loading {
            return;
        }
        self.playlist_more_loading = true;
        self.send(Action::LoadPlaylistMore {
            id: id.to_owned(),
            token,
        });
    }

    /// Fetches lyrics for the current track if they are not loaded yet.
    pub fn ensure_lyrics(&mut self) {
        let Some(id) = self.ps.current_track().map(|t| t.video_id.clone()) else {
            return;
        };
        if self.lyrics.video_id != id {
            self.lyrics = LyricsState {
                video_id: id.clone(),
                load: Load::Loading,
            };
            self.send(Action::LoadLyrics(id));
        }
    }

    pub fn is_liked(&self, video_id: &str) -> bool {
        self.local.is_liked(video_id)
    }

    pub fn is_saved(&self, item: &SearchItem) -> bool {
        self.local.is_saved(item)
    }

    /// Likes or unlikes a song (saved on this device).
    pub fn toggle_like(&mut self, track: &Track) {
        let liked = self.local.toggle_like(track);
        self.local_changed();
        if liked {
            self.toast(format!("Added “{}” to liked songs", track.title));
        }
    }

    /// Confirms the playlist dialog: renames, or creates the playlist.
    pub fn create_playlist(&mut self) {
        let Some(np) = self.new_playlist.take() else {
            return;
        };
        let title = np.title.trim().to_owned();
        if title.is_empty() {
            return;
        }
        if let Some(id) = np.rename {
            if self.local.rename_playlist(&id, &title) {
                self.local_changed();
            }
            return;
        }
        let id = self.local.create_playlist(&title, np.tracks);
        self.local_changed();
        self.toast(format!("Created playlist “{title}”"));
        // Show it right away when created from the Playlists page.
        if self.route == Route::Playlists {
            self.navigate(Route::Playlist(id));
        }
    }

    /// `(id, title)` of local playlists, for "add to playlist" menus.
    pub fn local_playlist_options(&self) -> Vec<(String, String)> {
        self.local
            .playlists
            .iter()
            .map(|p| (p.id.clone(), p.title.clone()))
            .collect()
    }

    pub fn run(&mut self, action: UiAction) {
        match action {
            UiAction::ToggleLike(track) => self.toggle_like(&track),
            UiAction::ToggleSaved(item) => {
                let saved = self.local.toggle_saved(&item);
                self.local_changed();
                self.toast(
                    if saved {
                        "Saved to your library on this device"
                    } else {
                        "Removed from your library"
                    }
                    .into(),
                );
            }
            UiAction::AddToLocalPlaylist {
                playlist_id,
                title,
                tracks,
            } => {
                let added = self.local.add_to_playlist(&playlist_id, &tracks);
                self.local_changed();
                self.toast(match added {
                    0 => format!("Already in “{title}”"),
                    n => format!("Added {n} to “{title}”"),
                });
            }
            UiAction::LocalRemoveTrack { playlist_id, index } => {
                if self.local.remove_from_playlist(&playlist_id, index) {
                    self.local_changed();
                }
            }
            UiAction::LocalMoveTrack {
                playlist_id,
                index,
                delta,
            } => {
                if self.local.move_in_playlist(&playlist_id, index, delta) {
                    self.local_changed();
                }
            }
            UiAction::SetPlaylistEdit(on) => self.playlist_edit = on,
            UiAction::RenamePlaylist(id) => {
                if let Some(p) = self.local.playlist(&id) {
                    self.new_playlist = Some(NewPlaylist {
                        title: p.title.clone(),
                        tracks: Vec::new(),
                        rename: Some(id),
                    });
                }
            }
            UiAction::NewPlaylistFor(tracks) => {
                self.new_playlist = Some(NewPlaylist {
                    title: String::new(),
                    tracks,
                    rename: None,
                });
            }
            UiAction::ExportPlaylist(id) => match self.local.playlist(&id) {
                Some(p) if p.pending() => self.toast(STILL_LOADING.into()),
                Some(p) => self.send(Action::ExportPlaylist(Box::new(p.clone()))),
                None => {}
            },
            UiAction::ChangePlaylistCover(id) => {
                if let Some(dir) = self.covers_dir() {
                    self.send(Action::PickPlaylistCover { id, dir });
                }
            }
            UiAction::RemovePlaylistCover(id) => {
                if let Some(old) = self.local.playlist_mut(&id).and_then(|p| p.cover.take()) {
                    self.delete_cover_file(Some(old));
                    self.local_changed();
                }
            }
            UiAction::ImportPlaylist => self.send(Action::ImportPlaylist),
            UiAction::DeletePlaylist(id) => {
                if self.local.playlist(&id).is_some() {
                    self.confirm_delete = Some(id);
                }
            }
            UiAction::BackupLibrary if !self.local.fully_loaded() => {
                self.toast(STILL_LOADING.into());
            }
            UiAction::BackupLibrary => {
                self.send(Action::BackupLibrary(
                    Box::new(self.local.clone()),
                    Box::new(crate::local::BackupSettings::from_config(&self.config)),
                ));
            }
            UiAction::RestoreLibrary => self.send(Action::RestoreLibrary),
            UiAction::MoveDataDir => {
                self.send(Action::MoveDataDir(Box::new(self.local.snapshot_all())));
            }
            UiAction::ApplyRestore => {
                if let Some(lib) = self.pending_restore.take() {
                    self.local = *lib;
                    self.local.mark_all_dirty();
                    self.local_changed();
                    if matches!(self.route, Route::Playlist(ref id) if is_local_id(id)) {
                        self.navigate(Route::Playlists);
                    }
                    if let Some(s) = self.pending_restore_settings.take() {
                        s.apply_to(&mut self.config);
                        let lang = crate::i18n::Lang::from_code(&self.config.ui_language);
                        crate::i18n::set(lang);
                        self.config.language = lang.hl().into();
                        if let Err(e) = self.save_config() {
                            tracing::warn!("could not save settings: {e}");
                        }
                        self.language_changed(lang.hl());
                    }
                    self.toast("Backup restored".into());
                }
            }
            UiAction::ConfirmDeletePlaylist(id) => {
                self.confirm_delete = None;
                let cover = self.local.playlist(&id).and_then(|p| p.cover.clone());
                if self.local.delete_playlist(&id) {
                    self.delete_cover_file(cover);
                    self.local_changed();
                    if self.route == Route::Playlist(id) {
                        self.back();
                    }
                    self.toast("Playlist deleted".into());
                }
            }
            UiAction::PlayTracks(tracks, start) => self.play_tracks(tracks, start),
            UiAction::PlayNext(t) => self.playback(Command::PlayNext(t)),
            UiAction::Enqueue(ts) => self.playback(Command::Enqueue(ts)),
            UiAction::Go(route) => self.navigate(route),
            UiAction::Radio(id) => self.send(Action::StartRadio(id)),
            UiAction::ShufflePlay(tracks) => {
                if tracks.is_empty() {
                    return;
                }
                let nanos = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.subsec_nanos() as usize)
                    .unwrap_or(0);
                self.playback(Command::SetShuffle(true));
                let start = nanos % tracks.len();
                self.play_tracks(tracks, start);
            }
        }
    }

    // ---- playback helpers (keyboard shortcuts, tray) ----------------------

    /// Mutes, or restores the level from before muting.
    pub fn toggle_mute(&mut self) {
        if self.ps.volume > 0.001 {
            self.muted_volume = Some(self.ps.volume);
            self.playback(Command::SetVolume(0.0));
        } else {
            let restore = self.muted_volume.take().unwrap_or(0.6);
            self.playback(Command::SetVolume(restore));
        }
    }

    /// Moves the volume by `delta` (0.0..=1.0 scale); leaves mute.
    pub fn adjust_volume(&mut self, delta: f32) {
        self.muted_volume = None;
        let v = (self.ps.volume + delta).clamp(0.0, 1.0);
        self.playback(Command::SetVolume(v));
    }

    /// Seeks relative to the current position, clamped to the track.
    pub fn seek_by(&mut self, secs: i64) {
        if self.ps.current.is_none() {
            return;
        }
        let pos = self.player.position().as_secs_f64() + secs as f64;
        let mut target = pos.max(0.0);
        if let Some(d) = self.ps.duration {
            target = target.min((d.as_secs_f64() - 1.0).max(0.0));
        }
        self.playback(Command::Seek(Duration::from_secs_f64(target)));
    }

    /// Likes or unlikes the track that is playing.
    pub fn toggle_like_current(&mut self) {
        if let Some(track) = self.ps.current_track().cloned() {
            self.toggle_like(&track);
        }
    }

    pub fn can_go_back(&self) -> bool {
        !self.history.is_empty()
    }

    pub fn back(&mut self) {
        if let Some(prev) = self.history.pop() {
            self.route = prev;
        }
    }

    // ---- search -----------------------------------------------------------

    pub fn submit_search(&mut self, query: &str) {
        let query = query.trim();
        if query.is_empty() {
            return;
        }
        self.search.input = query.to_owned();
        self.search.query = query.to_owned();
        self.search.show_suggestions = false;
        self.search.suggestions.clear();
        self.search.suggest_due = None;
        self.run_search();
        self.navigate(Route::Search);
    }

    pub fn set_filter(&mut self, filter: SearchFilter) {
        if self.search.filter != filter {
            self.search.filter = filter;
            if !self.search.query.is_empty() {
                self.run_search();
            }
        }
    }

    fn run_search(&mut self) {
        let seq = self.next_seq();
        let s = &mut self.search;
        s.seq = seq;
        s.loading = true;
        s.loading_more = false;
        s.error = None;
        s.set_items(Vec::new());
        s.continuation = None;
        let (query, filter) = (s.query.clone(), s.filter);
        self.send(Action::Search { seq, query, filter });
    }

    pub fn load_more_results(&mut self) {
        let Some(token) = self.search.continuation.clone() else {
            return;
        };
        if self.search.loading || self.search.loading_more {
            return;
        }
        self.search.loading_more = true;
        let seq = self.search.seq;
        self.send(Action::SearchMore { seq, token });
    }

    /// Call when the search box text changed.
    pub fn search_text_changed(&mut self) {
        let s = &mut self.search;
        if s.input.trim().is_empty() {
            s.suggestions.clear();
            s.suggest_due = None;
            s.show_suggestions = false;
            s.suggest_sel = None;
        } else {
            s.suggest_sel = None;
            s.suggest_due = Some(Instant::now() + SUGGEST_DEBOUNCE);
            s.show_suggestions = true;
        }
    }

    /// Fires the debounced suggestion request when due; returns when the UI
    /// should wake next (so no polling repaint is needed).
    pub fn poll_suggestions(&mut self) -> Option<Duration> {
        let due = self.search.suggest_due?;
        let now = Instant::now();
        if now < due {
            return Some(due - now);
        }
        self.search.suggest_due = None;
        let seq = self.next_seq();
        self.search.suggest_seq = seq;
        let query = self.search.input.trim().to_owned();
        self.send(Action::Suggest { seq, query });
        None
    }

    // ---- playback shortcuts ---------------------------------------------------

    pub fn play_tracks(&self, tracks: Vec<Track>, start: usize) {
        self.playback(Command::Play { tracks, start });
    }

    // ---- backend events --------------------------------------------------------

    pub fn apply(&mut self, event: Event) {
        match event {
            Event::SearchDone { seq, result } if seq == self.search.seq => {
                self.search.loading = false;
                match result {
                    Ok(page) => {
                        self.search.set_items(page.items);
                        self.search.continuation = page.continuation;
                    }
                    Err(e) => self.search.error = Some(e),
                }
            }
            Event::SearchMoreDone { seq, result } if seq == self.search.seq => {
                self.search.loading_more = false;
                match result {
                    Ok(page) => {
                        self.search.append_items(page.items);
                        self.search.continuation = page.continuation;
                    }
                    Err(e) => self.toast(format!("Could not load more: {e}")),
                }
            }
            Event::Suggestions { seq, items } if seq == self.search.suggest_seq => {
                self.search.suggestions = items;
            }
            Event::Home(r) => {
                self.fetched.insert(Route::Home, Instant::now());
                self.home = Load::from_result(r)
            }
            Event::Explore(r) => {
                self.fetched.insert(Route::Explore, Instant::now());
                self.explore = Load::from_result(r)
            }
            Event::NewReleases(r) => {
                self.fetched.insert(Route::NewReleases, Instant::now());
                self.new_releases = Load::from_result(r)
            }
            Event::Charts { country, result } => {
                self.charts.insert(country, Load::from_result(result));
            }
            Event::Moods(r) => {
                self.fetched.insert(Route::Moods, Instant::now());
                self.moods = Load::from_result(r)
            }
            Event::Mood { params, result } => {
                self.mood_pages.insert(params, Load::from_result(result));
            }
            Event::Artist { id, result } => {
                self.artists.insert(id, Load::from_result(result));
            }
            Event::Album { id, result } => {
                self.albums.insert(id, Load::from_result(result));
            }
            Event::Playlist { id, result } => {
                self.playlists.insert(id, Load::from_result(result));
            }
            Event::PlaylistMore { id, result } => {
                self.playlist_more_loading = false;
                match (result, self.playlists.get(&id)) {
                    (Ok((tracks, token)), Some(Load::Ready(page))) => {
                        let mut next = (**page).clone();
                        next.tracks.extend(tracks);
                        next.continuation = token;
                        self.playlists.insert(id, Load::Ready(Arc::new(next)));
                    }
                    (Err(e), _) => self.toast(format!("Could not load more tracks: {e}")),
                    _ => {}
                }
            }
            Event::Lyrics { video_id, result } => {
                if self.lyrics.video_id == video_id {
                    self.lyrics.load = Load::from_result(result);
                }
            }
            Event::PlaylistTracks(lists) => {
                let mut problem = None;
                for (id, result) in lists {
                    match result {
                        Ok(tracks) => self.local.attach_tracks(&id, tracks),
                        Err(e) => {
                            problem = Some(e);
                            self.local.attach_tracks(&id, Vec::new());
                        }
                    }
                }
                if let Some(e) = problem {
                    self.toast(format!("Some playlists could not be read: {e}"));
                }
                self.menu_version += 1;
            }
            Event::PlaylistImported { title, tracks } => {
                let n = tracks.len();
                let id = self.local.create_playlist(&title, tracks);
                self.local_changed();
                self.navigate(Route::Playlist(id));
                self.toast(format!("Imported “{title}” ({n} songs)"));
            }
            Event::RestoreRequested(lib, settings) => {
                self.pending_restore = Some(lib);
                self.pending_restore_settings = settings;
            }
            Event::PlaylistCover { id, file } => {
                let dir = self.covers_dir();
                if let Some(p) = self.local.playlist_mut(&id) {
                    let old = p.cover.replace(file);
                    self.delete_cover_file(old);
                    self.local_changed();
                } else if let Some(dir) = dir {
                    // the playlist was deleted while the picker was open
                    let _ = std::fs::remove_file(dir.join(file));
                }
            }
            Event::DataDirMoved(path) => {
                self.toast(format!("Data moved to {}", path.display()));
                self.data_dir = path;
            }
            Event::Toast(text) => self.toast(text),
            // stale results
            Event::SearchDone { .. } | Event::SearchMoreDone { .. } | Event::Suggestions { .. } => {
            }
        }
    }

    pub fn toast(&mut self, text: String) {
        self.toast = Some(Toast {
            text,
            since: Instant::now(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver};
    use ytm_api::{ApiError, PlaylistPage, SearchPage, StreamInfo, StreamResolver};
    use ytm_player::{OutputKind, PlayerOptions};

    struct NoStreams;

    #[async_trait::async_trait]
    impl StreamResolver for NoStreams {
        async fn resolve(&self, _: &str) -> Result<StreamInfo, ApiError> {
            Err(ApiError::NoStream)
        }
    }

    fn track(id: &str) -> Track {
        Track {
            video_id: id.into(),
            title: id.into(),
            ..Track::default()
        }
    }

    fn state() -> (AppState, UnboundedReceiver<Action>) {
        let player = Player::spawn(
            &tokio::runtime::Handle::current(),
            std::sync::Arc::new(NoStreams),
            PlayerOptions {
                output: ytm_player::OutputKind::Null,
                ..PlayerOptions::default()
            },
        )
        .unwrap();
        let _ = OutputKind::Null;
        let (tx, rx) = unbounded_channel();
        let mut app = AppState::new(player, tx);
        // Never write the real settings from tests.
        app.data_dir =
            std::env::temp_dir().join(format!("tunebox-test-data-{}", std::process::id()));
        (app, rx)
    }

    fn drain(rx: &mut UnboundedReceiver<Action>) -> Vec<Action> {
        std::iter::from_fn(|| rx.try_recv().ok()).collect()
    }

    /// Runs one egui frame with these events and applies the app's keyboard shortcuts.
    fn frame(app: &mut AppState, events: Vec<eframe::egui::Event>) {
        let ctx = eframe::egui::Context::default();
        let input = eframe::egui::RawInput {
            events,
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| {
            crate::app::handle_shortcuts(ui.ctx(), app);
        });
        out.textures_delta.clear();
    }

    fn key(key: eframe::egui::Key, modifiers: eframe::egui::Modifiers) -> eframe::egui::Event {
        eframe::egui::Event::Key {
            key,
            physical_key: Some(key),
            pressed: true,
            repeat: false,
            modifiers,
        }
    }

    fn text(s: &str) -> eframe::egui::Event {
        eframe::egui::Event::Text(s.into())
    }

    #[tokio::test]
    async fn letter_shortcuts_toggle_panels_and_the_help_sheet() {
        use eframe::egui::{Key, Modifiers};
        let (mut app, _rx) = state();
        frame(&mut app, vec![key(Key::Q, Modifiers::NONE)]);
        assert!(app.queue_open);
        frame(&mut app, vec![key(Key::Q, Modifiers::NONE)]);
        assert!(!app.queue_open);
        frame(&mut app, vec![key(Key::N, Modifiers::NONE)]);
        assert!(app.now_playing_open);

        frame(&mut app, vec![text("?")]);
        assert!(app.help_open, "? opens the cheat-sheet");
        app.queue_open = true;
        frame(&mut app, vec![key(Key::Escape, Modifiers::NONE)]);
        assert!(!app.help_open, "Esc closes the sheet first");
        assert!(app.queue_open && app.now_playing_open);
        frame(&mut app, vec![key(Key::Escape, Modifiers::NONE)]);
        assert!(!app.queue_open && app.now_playing_open, "then the queue");
        frame(&mut app, vec![key(Key::Escape, Modifiers::NONE)]);
        assert!(!app.now_playing_open, "then now-playing");
    }

    #[tokio::test]
    async fn navigation_shortcuts_change_page_and_focus_search() {
        use eframe::egui::{Key, Modifiers};
        let (mut app, _rx) = state();
        frame(&mut app, vec![key(Key::Num3, Modifiers::COMMAND)]);
        assert_eq!(app.route, Route::Library);
        frame(&mut app, vec![key(Key::Comma, Modifiers::COMMAND)]);
        assert_eq!(app.route, Route::Settings);
        frame(&mut app, vec![key(Key::ArrowLeft, Modifiers::ALT)]);
        assert_eq!(app.route, Route::Library, "Alt+Left goes back");
        app.now_playing_open = true;
        frame(&mut app, vec![key(Key::Num2, Modifiers::COMMAND)]);
        assert_eq!(app.route, Route::Explore);
        assert!(
            !app.now_playing_open,
            "navigating leaves the now-playing view"
        );

        frame(&mut app, vec![text("/")]);
        assert!(
            app.search_focus_requested,
            "/ asks the top bar to focus search"
        );
    }

    #[tokio::test]
    async fn mute_shortcut_remembers_the_level_and_unmutes() {
        use eframe::egui::{Key, Modifiers};
        let (mut app, _rx) = state();
        app.ps.volume = 0.7;
        frame(&mut app, vec![key(Key::M, Modifiers::NONE)]);
        assert_eq!(app.muted_volume, Some(0.7));
        app.ps.volume = 0.0;
        frame(&mut app, vec![key(Key::M, Modifiers::NONE)]);
        assert_eq!(app.muted_volume, None, "the saved level was used to unmute");
    }

    #[tokio::test]
    async fn shortcuts_are_ignored_while_a_text_field_has_focus() {
        use eframe::egui::{Event, Key, Modifiers};
        let (mut app, _rx) = state();
        let ctx = eframe::egui::Context::default();
        let id = eframe::egui::Id::new("field");
        let mut field = String::new();
        let mut run = |events: Vec<Event>, app: &mut AppState, focus: bool| {
            let input = eframe::egui::RawInput {
                events,
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, |ui| {
                if focus {
                    ui.memory_mut(|m| m.request_focus(id));
                }
                ui.add(eframe::egui::TextEdit::singleline(&mut field).id(id));
                crate::app::handle_shortcuts(ui.ctx(), app);
            });
            out.textures_delta.clear();
        };
        run(vec![], &mut app, true);
        run(vec![], &mut app, false); // focus is now established
        run(
            vec![key(Key::Q, Modifiers::NONE), text("q")],
            &mut app,
            false,
        );
        assert!(
            !app.queue_open,
            "typing q in a field must not toggle the queue"
        );
    }

    #[tokio::test]
    async fn cover_files_are_deleted_on_change_remove_and_playlist_delete() {
        let (mut app, _rx) = state();
        let tmp = std::env::temp_dir().join(format!("tunebox-cover-test-{}", std::process::id()));
        let covers = tmp.join("covers");
        std::fs::create_dir_all(&covers).unwrap();
        app.data_dir = tmp.clone();
        let id = app.local.create_playlist("P", vec![track("a")]);
        let put = |name: &str| std::fs::write(covers.join(name), b"x").unwrap();
        let exists = |name: &str| covers.join(name).exists();

        put("one.jpg");
        app.apply(Event::PlaylistCover {
            id: id.clone(),
            file: "one.jpg".into(),
        });
        put("two.jpg");
        app.apply(Event::PlaylistCover {
            id: id.clone(),
            file: "two.jpg".into(),
        });
        assert!(
            !exists("one.jpg") && exists("two.jpg"),
            "change removes the old file"
        );

        app.run(UiAction::RemovePlaylistCover(id.clone()));
        assert!(!exists("two.jpg"));
        assert!(app.local.playlist(&id).unwrap().cover.is_none());

        put("three.jpg");
        app.apply(Event::PlaylistCover {
            id: id.clone(),
            file: "three.jpg".into(),
        });
        app.run(UiAction::ConfirmDeletePlaylist(id));
        assert!(
            !exists("three.jpg"),
            "deleting the playlist removes its cover"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[tokio::test]
    async fn starts_on_home_and_requests_it_once() {
        let (mut app, mut rx) = state();
        assert_eq!(app.route, Route::Home);
        assert!(matches!(drain(&mut rx).as_slice(), [Action::LoadHome]));
        app.ensure_loaded();
        assert!(
            drain(&mut rx).is_empty(),
            "already loading: no duplicate request"
        );
        app.apply(Event::Home(Err("boom".into())));
        assert!(matches!(app.home, Load::Failed(_)));
        app.reload();
        assert!(
            matches!(drain(&mut rx).as_slice(), [Action::LoadHome]),
            "retry after failure"
        );
    }

    #[tokio::test]
    async fn search_flow_ignores_stale_results() {
        let (mut app, mut rx) = state();
        drain(&mut rx);
        app.submit_search("  daft punk ");
        assert_eq!(app.route, Route::Search);
        assert_eq!(app.search.query, "daft punk");
        let Some(Action::Search { seq: first, .. }) = drain(&mut rx).pop() else {
            panic!("expected a search action");
        };
        app.set_filter(SearchFilter::Songs);
        let Some(Action::Search {
            seq: second,
            filter,
            ..
        }) = drain(&mut rx).pop()
        else {
            panic!("filter change must re-run the search");
        };
        assert_eq!(filter, SearchFilter::Songs);
        assert_ne!(first, second);

        let page = |id: &str| SearchPage {
            items: vec![SearchItem::Track(track(id))],
            continuation: Some("tok".into()),
        };
        app.apply(Event::SearchDone {
            seq: first,
            result: Ok(page("old")),
        });
        assert!(
            app.search.loading,
            "stale result must not clear the loading state"
        );
        assert!(app.search.items.is_empty());
        app.apply(Event::SearchDone {
            seq: second,
            result: Ok(page("new")),
        });
        assert!(!app.search.loading);
        assert_eq!(app.search.tracks()[0].video_id, "new");

        app.load_more_results();
        assert!(matches!(
            drain(&mut rx).as_slice(),
            [Action::SearchMore { .. }]
        ));
        app.load_more_results();
        assert!(
            drain(&mut rx).is_empty(),
            "no second request while one is in flight"
        );
        app.apply(Event::SearchMoreDone {
            seq: second,
            result: Ok(SearchPage {
                items: vec![SearchItem::Track(track("more"))],
                continuation: None,
            }),
        });
        assert_eq!(app.search.items.len(), 2);
        assert!(app.search.continuation.is_none());
    }

    #[tokio::test]
    async fn blank_queries_are_ignored() {
        let (mut app, mut rx) = state();
        drain(&mut rx);
        app.submit_search("   ");
        assert!(drain(&mut rx).is_empty());
        assert_eq!(app.route, Route::Home);
    }

    #[tokio::test]
    async fn navigation_history_and_lazy_loading() {
        let (mut app, mut rx) = state();
        drain(&mut rx);
        app.navigate(Route::Album("MPREb_x".into()));
        assert!(matches!(drain(&mut rx).as_slice(), [Action::LoadAlbum(id)] if id == "MPREb_x"));
        app.navigate(Route::Artist("UC1".into()));
        assert!(app.can_go_back());
        app.back();
        assert_eq!(app.route, Route::Album("MPREb_x".into()));
        // coming back to a loading/loaded page does not refetch
        app.navigate(Route::Artist("UC1".into()));
        app.back();
        assert!(drain(&mut rx)
            .iter()
            .all(|a| !matches!(a, Action::LoadAlbum(_))));
        app.back();
        assert_eq!(app.route, Route::Home);
        assert!(!app.can_go_back());
    }

    #[tokio::test]
    async fn playlist_pagination_appends() {
        let (mut app, mut rx) = state();
        drain(&mut rx);
        app.navigate(Route::Playlist("PL1".into()));
        drain(&mut rx);
        let page = PlaylistPage {
            playlist_id: "PL1".into(),
            tracks: vec![track("a")],
            continuation: Some("next".into()),
            ..PlaylistPage::default()
        };
        app.apply(Event::Playlist {
            id: "PL1".into(),
            result: Ok(page),
        });
        app.load_more_playlist("PL1");
        assert!(
            matches!(drain(&mut rx).as_slice(), [Action::LoadPlaylistMore { token, .. }] if token == "next")
        );
        app.load_more_playlist("PL1");
        assert!(drain(&mut rx).is_empty());
        app.apply(Event::PlaylistMore {
            id: "PL1".into(),
            result: Ok((vec![track("b")], None)),
        });
        let Some(Load::Ready(p)) = app.playlists.get("PL1") else {
            panic!()
        };
        assert_eq!(p.tracks.len(), 2);
        assert!(p.continuation.is_none());
        app.load_more_playlist("PL1");
        assert!(drain(&mut rx).is_empty(), "no more pages");
    }

    #[tokio::test]
    async fn suggestions_are_debounced_and_sequenced() {
        let (mut app, mut rx) = state();
        drain(&mut rx);
        app.search.input = "dau".into();
        app.search_text_changed();
        assert!(app.poll_suggestions().is_some(), "wait for the debounce");
        assert!(drain(&mut rx).is_empty());
        tokio::time::sleep(Duration::from_millis(250)).await;
        assert!(app.poll_suggestions().is_none());
        let Some(Action::Suggest { seq, query }) = drain(&mut rx).pop() else {
            panic!("suggest action expected");
        };
        assert_eq!(query, "dau");
        app.apply(Event::Suggestions {
            seq: seq + 99,
            items: vec!["stale".into()],
        });
        assert!(app.search.suggestions.is_empty());
        app.apply(Event::Suggestions {
            seq,
            items: vec!["daft punk".into()],
        });
        assert_eq!(app.search.suggestions, ["daft punk"]);
        app.search.input.clear();
        app.search_text_changed();
        assert!(app.search.suggestions.is_empty() && !app.search.show_suggestions);
    }

    #[tokio::test]
    async fn lyrics_requested_per_track_only_once() {
        let (mut app, mut rx) = state();
        drain(&mut rx);
        app.ensure_lyrics();
        assert!(
            drain(&mut rx).is_empty(),
            "nothing playing, nothing to fetch"
        );
        app.apply(Event::Lyrics {
            video_id: "other".into(),
            result: Ok(None),
        });
        assert!(
            matches!(app.lyrics.load, Load::Idle),
            "unrelated result is ignored"
        );
    }

    #[tokio::test]
    async fn liking_saves_on_this_device() {
        let (mut app, mut rx) = state();
        drain(&mut rx);
        app.toggle_like(&track("vid"));
        assert!(app.is_liked("vid"));
        let sent = drain(&mut rx);
        assert!(
            matches!(sent.as_slice(), [Action::PersistLocal(lib)] if lib.index.liked.iter().any(|t| t.video_id == "vid")),
            "{sent:?}"
        );
        app.toggle_like(&track("vid"));
        assert!(!app.is_liked("vid"));
        assert!(
            matches!(drain(&mut rx).as_slice(), [Action::PersistLocal(lib)] if !lib.index.liked.iter().any(|t| t.video_id == "vid"))
        );
    }

    #[tokio::test]
    async fn new_playlist_requires_a_title() {
        let (mut app, mut rx) = state();
        drain(&mut rx);
        app.run(UiAction::NewPlaylistFor(vec![track("a")]));
        assert!(app.new_playlist.is_some());
        app.create_playlist();
        assert!(drain(&mut rx).is_empty(), "blank title: nothing is created");
        app.run(UiAction::NewPlaylistFor(vec![track("a")]));
        app.new_playlist.as_mut().unwrap().title = " Road trip ".into();
        app.create_playlist();
        assert!(app.new_playlist.is_none());
        assert_eq!(app.local.playlists.len(), 1, "created on this device");
        assert_eq!(app.local.playlists[0].title, "Road trip");
        assert_eq!(app.local.playlists[0].tracks.len(), 1);
        assert!(
            matches!(drain(&mut rx).as_slice(), [Action::PersistLocal(l)] if l.playlists.len() == 1)
        );
    }

    #[tokio::test]
    async fn local_playlist_editing_flow() {
        let (mut app, mut rx) = state();
        drain(&mut rx);
        app.run(UiAction::NewPlaylistFor(vec![track("a"), track("b")]));
        app.new_playlist.as_mut().unwrap().title = "P".into();
        app.create_playlist();
        let id = app.local.playlists[0].id.clone();
        drain(&mut rx);

        app.run(UiAction::AddToLocalPlaylist {
            playlist_id: id.clone(),
            title: "P".into(),
            tracks: vec![track("b"), track("c")],
        });
        assert_eq!(
            app.local.playlist(&id).unwrap().tracks.len(),
            3,
            "duplicate b skipped"
        );
        assert!(app.toast.as_ref().unwrap().text.contains("Added 1"));
        app.run(UiAction::AddToLocalPlaylist {
            playlist_id: id.clone(),
            title: "P".into(),
            tracks: vec![track("c")],
        });
        assert!(app.toast.as_ref().unwrap().text.contains("Already in"));

        app.run(UiAction::LocalMoveTrack {
            playlist_id: id.clone(),
            index: 2,
            delta: -2,
        });
        assert_eq!(app.local.playlist(&id).unwrap().tracks[0].video_id, "c");
        app.run(UiAction::LocalRemoveTrack {
            playlist_id: id.clone(),
            index: 0,
        });
        assert_eq!(app.local.playlist(&id).unwrap().tracks.len(), 2);

        app.run(UiAction::RenamePlaylist(id.clone()));
        assert_eq!(
            app.new_playlist.as_ref().unwrap().title,
            "P",
            "dialog starts with the current title"
        );
        app.new_playlist.as_mut().unwrap().title = "Renamed".into();
        app.create_playlist();
        assert_eq!(app.local.playlist(&id).unwrap().title, "Renamed");
        assert_eq!(
            app.local.playlists.len(),
            1,
            "rename must not create a second playlist"
        );
        assert!(
            drain(&mut rx)
                .iter()
                .all(|a| matches!(a, Action::PersistLocal(_))),
            "every edit persists"
        );
    }

    #[tokio::test]
    async fn export_sends_the_playlist_and_import_creates_and_opens_one() {
        let (mut app, mut rx) = state();
        let id = app
            .local
            .create_playlist("Mix", vec![track("a"), track("b")]);
        drain(&mut rx);
        app.run(UiAction::ExportPlaylist(id));
        assert!(drain(&mut rx).iter().any(
            |a| matches!(a, Action::ExportPlaylist(p) if p.title == "Mix" && p.tracks.len() == 2)
        ));
        app.run(UiAction::ExportPlaylist("local:nope".into()));
        assert!(drain(&mut rx).is_empty());

        app.run(UiAction::ImportPlaylist);
        assert!(drain(&mut rx)
            .iter()
            .any(|a| matches!(a, Action::ImportPlaylist)));

        app.apply(Event::PlaylistImported {
            title: "Mix".into(),
            tracks: vec![track("c")],
        });
        assert_eq!(app.local.playlists.len(), 2);
        let Route::Playlist(new_id) = app.route.clone() else {
            panic!("import opens the new playlist");
        };
        assert_eq!(app.local.playlist(&new_id).unwrap().tracks.len(), 1);
        assert!(drain(&mut rx)
            .iter()
            .any(|a| matches!(a, Action::PersistLocal(_))));
    }

    #[tokio::test]
    async fn deleting_the_open_local_playlist_goes_back() {
        let (mut app, mut rx) = state();
        let id = app.local.create_playlist("P", vec![]);
        app.navigate(Route::Playlists);
        app.navigate(Route::Playlist(id.clone()));
        assert!(
            drain(&mut rx)
                .iter()
                .all(|a| !matches!(a, Action::LoadPlaylist(_))),
            "local playlists are never fetched from YouTube"
        );
        app.run(UiAction::DeletePlaylist(id.clone()));
        assert!(
            app.local.playlist(&id).is_some(),
            "asks for confirmation first"
        );
        assert_eq!(app.confirm_delete.as_deref(), Some(id.as_str()));
        app.run(UiAction::ConfirmDeletePlaylist(id.clone()));
        assert!(app.confirm_delete.is_none());
        assert!(app.local.playlist(&id).is_none());
        assert_eq!(app.route, Route::Playlists);
        // deleting something that does not exist is harmless and persists nothing
        drain(&mut rx);
        app.run(UiAction::DeletePlaylist("PLremote".into()));
        assert!(app.confirm_delete.is_none(), "nothing to confirm");
        app.run(UiAction::ConfirmDeletePlaylist("PLremote".into()));
        assert!(drain(&mut rx).is_empty());
    }

    #[tokio::test]
    async fn cancelling_the_delete_dialog_keeps_the_playlist() {
        let (mut app, _rx) = state();
        let id = app.local.create_playlist("Keep", vec![]);
        app.run(UiAction::DeletePlaylist(id.clone()));
        app.confirm_delete = None; // what Cancel / Esc do
        assert!(app.local.playlist(&id).is_some());
    }

    #[tokio::test]
    async fn restore_replaces_the_library_only_after_confirmation() {
        let (mut app, mut rx) = state();
        app.local.create_playlist("Mine", vec![]);
        let mut backup = LocalLibrary::default();
        backup.create_playlist("From backup", vec![track("z")]);
        drain(&mut rx);
        app.apply(Event::RestoreRequested(Box::new(backup), None));
        assert_eq!(app.local.playlists[0].title, "Mine", "not replaced yet");
        app.run(UiAction::ApplyRestore);
        assert_eq!(app.local.playlists.len(), 1);
        assert_eq!(app.local.playlists[0].title, "From backup");
        assert!(app.pending_restore.is_none());
        assert!(drain(&mut rx)
            .iter()
            .any(|a| matches!(a, Action::PersistLocal(_))));
        // nothing pending: applying again changes nothing
        app.run(UiAction::ApplyRestore);
        assert_eq!(app.local.playlists.len(), 1);
    }

    #[tokio::test]
    async fn restoring_a_backup_with_options_applies_them() {
        let (mut app, _rx) = state();
        let settings = crate::local::BackupSettings {
            theme: "light".into(),
            ui_language: "en".into(),
            refresh_minutes: 180,
        };
        app.apply(Event::RestoreRequested(Box::default(), Some(settings)));
        assert_eq!(
            app.config.refresh_minutes, 30,
            "not applied before confirming"
        );
        app.run(UiAction::ApplyRestore);
        assert_eq!(app.config.refresh_minutes, 180);
        assert_eq!(app.config.theme, "light");
        assert!(app.pending_restore_settings.is_none());
    }

    #[tokio::test]
    async fn pages_older_than_the_refresh_interval_are_fetched_again() {
        let (mut app, mut rx) = state();
        app.config.refresh_minutes = 30;
        app.navigate(Route::Home);
        app.apply(Event::Home(Ok(ytm_api::HomePage::default())));
        drain(&mut rx);
        // fresh: reopening does not fetch
        app.navigate(Route::Explore);
        app.navigate(Route::Home);
        assert!(!drain(&mut rx).iter().any(|a| matches!(a, Action::LoadHome)));
        // old: reopening fetches again
        let old = Instant::now()
            .checked_sub(Duration::from_secs(31 * 60))
            .unwrap();
        app.fetched.insert(Route::Home, old);
        app.navigate(Route::Library);
        app.navigate(Route::Home);
        assert!(drain(&mut rx).iter().any(|a| matches!(a, Action::LoadHome)));
        // interval 0 = never
        app.apply(Event::Home(Ok(ytm_api::HomePage::default())));
        app.config.refresh_minutes = 0;
        app.fetched.insert(Route::Home, old);
        drain(&mut rx);
        app.navigate(Route::Library);
        app.navigate(Route::Home);
        assert!(!drain(&mut rx).iter().any(|a| matches!(a, Action::LoadHome)));
    }

    #[tokio::test]
    async fn new_releases_and_charts_load_lazily_per_country() {
        let (mut app, mut rx) = state();
        drain(&mut rx);
        app.navigate(Route::NewReleases);
        assert!(matches!(
            drain(&mut rx).as_slice(),
            [Action::LoadNewReleases]
        ));
        app.navigate(Route::Charts(String::new()));
        assert!(matches!(drain(&mut rx).as_slice(), [Action::LoadCharts(c)] if c.is_empty()));
        // another country is a separate request; the first stays cached
        app.navigate(Route::Charts("US".into()));
        assert!(matches!(drain(&mut rx).as_slice(), [Action::LoadCharts(c)] if c == "US"));
        app.back();
        assert!(
            drain(&mut rx).is_empty(),
            "no refetch when returning to a loaded country"
        );

        app.apply(Event::Charts {
            country: "US".into(),
            result: Err("boom".into()),
        });
        app.navigate(Route::Charts("US".into()));
        assert!(
            matches!(drain(&mut rx).as_slice(), [Action::LoadCharts(c)] if c == "US"),
            "failed pages retry"
        );
        app.apply(Event::Charts {
            country: String::new(),
            result: Ok(ytm_api::ChartsPage::default()),
        });
        assert!(matches!(app.charts[""], Load::Ready(_)));
        app.apply(Event::NewReleases(Ok(ytm_api::HomePage::default())));
        assert!(matches!(app.new_releases, Load::Ready(_)));
    }

    #[tokio::test]
    async fn mood_pages_load_lazily_once_and_retry_after_failure() {
        let (mut app, mut rx) = state();
        drain(&mut rx);
        app.navigate(Route::Moods);
        assert!(matches!(drain(&mut rx).as_slice(), [Action::LoadMoods]));
        app.navigate(Route::Mood("p1".into()));
        assert!(matches!(drain(&mut rx).as_slice(), [Action::LoadMood(p)] if p == "p1"));
        // going back and forth does not refetch a loading/loaded page
        app.back();
        app.navigate(Route::Mood("p1".into()));
        assert!(drain(&mut rx)
            .iter()
            .all(|a| !matches!(a, Action::LoadMood(_))));

        app.apply(Event::Mood {
            params: "p1".into(),
            result: Err("boom".into()),
        });
        assert!(matches!(app.mood_pages["p1"], Load::Failed(_)));
        app.reload();
        assert!(
            matches!(drain(&mut rx).as_slice(), [Action::LoadMood(p)] if p == "p1"),
            "retry"
        );

        app.apply(Event::Mood {
            params: "p2".into(),
            result: Ok(ytm_api::CategoryPage::default()),
        });
        assert!(
            matches!(app.mood_pages["p2"], Load::Ready(_)),
            "pages are cached per category"
        );
        app.apply(Event::Moods(Ok(ytm_api::MoodsPage::default())));
        assert!(matches!(app.moods, Load::Ready(_)));
    }

    #[tokio::test]
    async fn edit_mode_toggles_and_never_leaks_to_other_pages() {
        let (mut app, mut rx) = state();
        let id = app
            .local
            .create_playlist("P", vec![track("a"), track("b"), track("c")]);
        app.navigate(Route::Playlist(id.clone()));
        assert!(!app.playlist_edit);
        app.run(UiAction::SetPlaylistEdit(true));
        assert!(app.playlist_edit);
        drain(&mut rx);

        // reordering while editing persists each change and keeps edit mode on
        app.run(UiAction::LocalMoveTrack {
            playlist_id: id.clone(),
            index: 0,
            delta: 2,
        });
        let order: Vec<_> = app
            .local
            .playlist(&id)
            .unwrap()
            .tracks
            .iter()
            .map(|t| t.video_id.as_str())
            .collect();
        assert_eq!(order, ["b", "c", "a"]);
        app.run(UiAction::LocalRemoveTrack {
            playlist_id: id.clone(),
            index: 1,
        });
        assert_eq!(app.local.playlist(&id).unwrap().tracks.len(), 2);
        assert!(app.playlist_edit);
        assert_eq!(drain(&mut rx).len(), 2, "one persist per edit");

        app.navigate(Route::Home);
        assert!(!app.playlist_edit, "leaving the page ends edit mode");
        app.navigate(Route::Playlist(id));
        assert!(!app.playlist_edit, "and it is off when coming back");
    }

    #[tokio::test]
    async fn saving_albums_artists_and_playlists_on_device() {
        let (mut app, mut rx) = state();
        drain(&mut rx);
        let album = SearchItem::Album(ytm_api::AlbumSummary {
            browse_id: "MPRE1".into(),
            title: "Discovery".into(),
            ..Default::default()
        });
        assert!(!app.is_saved(&album));
        app.run(UiAction::ToggleSaved(album.clone()));
        assert!(app.is_saved(&album));
        assert!(
            matches!(drain(&mut rx).as_slice(), [Action::PersistLocal(l)] if l.index.albums.len() == 1)
        );
        app.run(UiAction::ToggleSaved(album.clone()));
        assert!(!app.is_saved(&album));
    }

    #[tokio::test]
    async fn library_loaded_from_disk_is_used() {
        let mut lib = LocalLibrary::default();
        lib.toggle_like(&track("old"));
        let (app, _rx) = state();
        let app = app.with_local(lib);
        assert!(app.is_liked("old"));
        assert!(
            app.menu_version > 0,
            "menus refresh when the library is installed"
        );
    }

    #[test]
    fn tracks_filters_non_tracks() {
        let mut s = SearchState::default();
        s.set_items(vec![
            SearchItem::Track(track("a")),
            SearchItem::Artist(ytm_api::ArtistSummary::default()),
            SearchItem::Track(track("b")),
        ]);
        let ids: Vec<_> = s.tracks().iter().map(|t| t.video_id.clone()).collect();
        assert_eq!(ids, ["a", "b"]);
    }
}
