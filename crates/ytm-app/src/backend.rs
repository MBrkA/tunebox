//! Everything that talks to the network or the player on behalf of the UI.
//!
//! The UI never blocks: it sends an [`Action`] and later receives an
//! [`Event`]. Every event wakes the UI with `request_repaint`.

use std::sync::Arc;

use eframe::egui;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use ytm_api::{
    AlbumPage, ArtistPage, CategoryPage, ChartsPage, HomePage, Lyrics, MoodsPage, MusicApi,
    PlaylistPage, SearchFilter, SearchPage, Track,
};
use ytm_player::{Command, Player, PlayerEvent};

/// Requests from the UI (and, later, OS media keys) to the backend.
#[derive(Debug, Clone)]
pub enum Action {
    Search {
        seq: u64,
        query: String,
        filter: SearchFilter,
    },
    SearchMore {
        seq: u64,
        token: String,
    },
    Suggest {
        seq: u64,
        query: String,
    },
    /// Content language (`hl`) for all following API requests.
    SetLanguage(String),
    LoadHome,
    LoadExplore,
    LoadArtist(String),
    LoadAlbum(String),
    LoadPlaylist(String),
    LoadPlaylistMore {
        id: String,
        token: String,
    },
    LoadLyrics(String),
    LoadNewReleases,
    /// Charts for a country code; empty = let YouTube choose.
    LoadCharts(String),
    LoadMoods,
    /// A mood/genre category, identified by its `params`.
    LoadMood(String),
    /// Ask where to save, then write this playlist as a Tunebox playlist file.
    ExportPlaylist(Box<crate::local::LocalPlaylist>),
    /// Ask for an image, crop/shrink it and store it in `dir` (answered with `Event::PlaylistCover`).
    PickPlaylistCover {
        id: String,
        dir: std::path::PathBuf,
    },
    /// Ask for a playlist file and load it (answered with `Event::PlaylistImported`).
    ImportPlaylist,
    /// Ask where to save, then write a backup of the whole library.
    BackupLibrary(
        Box<crate::local::LocalLibrary>,
        Box<crate::local::BackupSettings>,
    ),
    /// Ask for a backup file and answer with `Event::RestoreRequested`.
    RestoreLibrary,
    /// Ask for a folder and move all app data there (the snapshot covers changes not yet on disk).
    MoveDataDir(Box<crate::local::Snapshot>),
    /// Write the on-device library to disk.
    PersistLocal(Box<crate::local::Snapshot>),
    /// Read the songs of these device playlists (answered with `Event::PlaylistTracks`).
    LoadPlaylistTracks(Vec<String>),
    /// Replace the queue with the radio seeded from this track.
    StartRadio(String),
    Playback(Command),
}

/// Results flowing back to the UI.
#[derive(Debug)]
pub enum Event {
    SearchDone {
        seq: u64,
        result: Result<SearchPage, String>,
    },
    SearchMoreDone {
        seq: u64,
        result: Result<SearchPage, String>,
    },
    Suggestions {
        seq: u64,
        items: Vec<String>,
    },
    Home(Result<HomePage, String>),
    Explore(Result<HomePage, String>),
    NewReleases(Result<HomePage, String>),
    Charts {
        country: String,
        result: Result<ChartsPage, String>,
    },
    Moods(Result<MoodsPage, String>),
    Mood {
        params: String,
        result: Result<CategoryPage, String>,
    },
    Artist {
        id: String,
        result: Result<ArtistPage, String>,
    },
    Album {
        id: String,
        result: Result<AlbumPage, String>,
    },
    Playlist {
        id: String,
        result: Result<PlaylistPage, String>,
    },
    PlaylistMore {
        id: String,
        result: Result<(Vec<Track>, Option<String>), String>,
    },
    Lyrics {
        video_id: String,
        result: Result<Option<Lyrics>, String>,
    },
    /// A playlist file the user picked, already parsed.
    PlaylistImported {
        title: String,
        tracks: Vec<Track>,
    },
    /// A backup file the user picked; the UI asks for confirmation before replacing the library.
    RestoreRequested(
        Box<crate::local::LocalLibrary>,
        Option<crate::local::BackupSettings>,
    ),
    /// A new cover for a device playlist was written to the covers folder.
    PlaylistCover {
        id: String,
        file: String,
    },
    /// Songs of device playlists, read from disk after startup.
    PlaylistTracks(Vec<(String, Result<Vec<Track>, String>)>),
    /// The library file now lives at this path.
    DataDirMoved(std::path::PathBuf),
    Toast(String),
}

#[derive(Clone)]
pub struct Backend {
    local_path: Arc<std::sync::Mutex<Option<std::path::PathBuf>>>,
    local_write: Arc<std::sync::Mutex<()>>,
    api: Arc<dyn MusicApi>,
    player: Player,
    events: UnboundedSender<Event>,
    ctx: egui::Context,
}

impl Backend {
    pub fn spawn(
        rt: &tokio::runtime::Handle,
        api: Arc<dyn MusicApi>,
        local_path: Option<std::path::PathBuf>,
        player: Player,
        ctx: egui::Context,
        mut actions: UnboundedReceiver<Action>,
        events: UnboundedSender<Event>,
    ) {
        let backend = Self {
            local_path: Arc::new(std::sync::Mutex::new(local_path)),
            local_write: Arc::new(std::sync::Mutex::new(())),
            api,
            player,
            events,
            ctx,
        };
        // Surface playback errors as toasts.
        let b = backend.clone();
        rt.spawn(async move {
            let mut rx = b.player.subscribe();
            loop {
                match rx.recv().await {
                    Ok(PlayerEvent::Error(msg)) => b.emit(Event::Toast(format!("Playback: {msg}"))),
                    Ok(_) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(_) => break,
                }
            }
        });
        let rt2 = rt.clone();
        rt.spawn(async move {
            while let Some(action) = actions.recv().await {
                backend.handle(&rt2, action);
            }
        });
    }

    fn emit(&self, event: Event) {
        let _ = self.events.send(event);
        self.ctx.request_repaint();
    }

    fn handle(&self, rt: &tokio::runtime::Handle, action: Action) {
        match action {
            Action::Playback(cmd) => self.player.send(cmd),
            Action::SetLanguage(hl) => self.api.set_language(&hl),
            Action::BackupLibrary(library, settings) => {
                let this = self.clone();
                rt.spawn_blocking(move || {
                    let picked = rfd::FileDialog::new()
                        .set_title("Back up data")
                        .set_file_name("tunebox-backup.json")
                        .add_filter("Tunebox backup", &["json"])
                        .save_file();
                    let Some(path) = picked else { return };
                    let result = crate::local::backup_bytes(&library, &settings)
                        .map_err(std::io::Error::other)
                        .and_then(|bytes| std::fs::write(&path, bytes));
                    match result {
                        Ok(()) => {
                            this.emit(Event::Toast(format!("Backup saved to {}", path.display())))
                        }
                        Err(e) => this.emit(Event::Toast(format!("Could not back up: {e}"))),
                    }
                });
            }
            Action::RestoreLibrary => {
                let this = self.clone();
                rt.spawn_blocking(move || {
                    let picked = rfd::FileDialog::new()
                        .set_title("Restore from backup")
                        .add_filter("Tunebox backup", &["json"])
                        .pick_file();
                    let Some(path) = picked else { return };
                    let parsed = std::fs::read_to_string(&path)
                        .map_err(|e| e.to_string())
                        .and_then(|t| crate::local::parse_backup_file(&t));
                    match parsed {
                        Ok((lib, settings)) => {
                            this.emit(Event::RestoreRequested(Box::new(lib), settings))
                        }
                        Err(e) => this.emit(Event::Toast(format!("Could not restore: {e}"))),
                    }
                });
            }
            Action::MoveDataDir(snapshot) => {
                let this = self.clone();
                rt.spawn_blocking(move || {
                    let Some(target) = rfd::FileDialog::new()
                        .set_title("Choose a folder for Tunebox data")
                        .pick_folder()
                    else {
                        return;
                    };
                    let _guard = this.local_write.lock().unwrap_or_else(|e| e.into_inner());
                    match move_data_dir(&this, &snapshot, &target) {
                        Ok(()) => this.emit(Event::DataDirMoved(target)),
                        Err(e) => this.emit(Event::Toast(e)),
                    }
                });
            }
            Action::Search { seq, query, filter } => {
                let this = self.clone();
                rt.spawn(async move {
                    let result = this
                        .api
                        .search(&query, filter)
                        .await
                        .map_err(|e| e.to_string());
                    this.emit(Event::SearchDone { seq, result });
                });
            }
            Action::SearchMore { seq, token } => {
                let this = self.clone();
                rt.spawn(async move {
                    let result = this
                        .api
                        .search_continue(&token)
                        .await
                        .map_err(|e| e.to_string());
                    this.emit(Event::SearchMoreDone { seq, result });
                });
            }
            Action::LoadHome => {
                let this = self.clone();
                rt.spawn(async move {
                    let result = async {
                        let mut home = this.api.home().await.map_err(|e| e.to_string())?;
                        // YouTube's anonymous Home has only a couple of shelves and no continuation:
                        // pad it with Explore's so the page is not nearly empty.
                        if home.sections.len() < 4 {
                            if let Ok(explore) = this.api.explore().await {
                                home.sections.extend(explore.sections);
                            }
                        }
                        Ok(home)
                    }
                    .await;
                    this.emit(Event::Home(result));
                });
            }
            Action::LoadExplore => {
                let this = self.clone();
                rt.spawn(async move {
                    let result = this.api.explore().await.map_err(|e| e.to_string());
                    this.emit(Event::Explore(result));
                });
            }
            Action::LoadNewReleases => {
                let this = self.clone();
                rt.spawn(async move {
                    let result = this.api.new_releases().await.map_err(|e| e.to_string());
                    this.emit(Event::NewReleases(result));
                });
            }
            Action::LoadCharts(country) => {
                let this = self.clone();
                rt.spawn(async move {
                    let result = this
                        .api
                        .charts(Some(country.as_str()).filter(|c| !c.is_empty()))
                        .await
                        .map_err(|e| e.to_string());
                    this.emit(Event::Charts { country, result });
                });
            }
            Action::LoadMoods => {
                let this = self.clone();
                rt.spawn(async move {
                    let result = this.api.moods_and_genres().await.map_err(|e| e.to_string());
                    this.emit(Event::Moods(result));
                });
            }
            Action::LoadMood(params) => {
                let this = self.clone();
                rt.spawn(async move {
                    let result = this
                        .api
                        .mood_category(&params)
                        .await
                        .map_err(|e| e.to_string());
                    this.emit(Event::Mood { params, result });
                });
            }
            Action::LoadArtist(id) => {
                let this = self.clone();
                rt.spawn(async move {
                    let result = this.api.artist(&id).await.map_err(|e| e.to_string());
                    this.emit(Event::Artist { id, result });
                });
            }
            Action::LoadAlbum(id) => {
                let this = self.clone();
                rt.spawn(async move {
                    let result = this.api.album(&id).await.map_err(|e| e.to_string());
                    this.emit(Event::Album { id, result });
                });
            }
            Action::LoadPlaylist(id) => {
                let this = self.clone();
                rt.spawn(async move {
                    let result = this.api.playlist(&id).await.map_err(|e| e.to_string());
                    this.emit(Event::Playlist { id, result });
                });
            }
            Action::LoadPlaylistMore { id, token } => {
                let this = self.clone();
                rt.spawn(async move {
                    let result = this
                        .api
                        .playlist_continue(&token)
                        .await
                        .map_err(|e| e.to_string());
                    this.emit(Event::PlaylistMore { id, result });
                });
            }
            Action::LoadLyrics(video_id) => {
                let this = self.clone();
                rt.spawn(async move {
                    let result = this.api.lyrics(&video_id).await.map_err(|e| e.to_string());
                    this.emit(Event::Lyrics { video_id, result });
                });
            }
            Action::StartRadio(video_id) => {
                let this = self.clone();
                rt.spawn(async move {
                    match this.api.up_next(&video_id).await {
                        Ok(next) if !next.tracks.is_empty() => {
                            this.player.send(Command::Play {
                                tracks: next.tracks,
                                start: 0,
                            });
                        }
                        Ok(_) => {
                            this.emit(Event::Toast("No radio available for this track".into()))
                        }
                        Err(e) => this.emit(Event::Toast(format!("Could not start radio: {e}"))),
                    }
                });
            }
            Action::ExportPlaylist(playlist) => {
                let this = self.clone();
                rt.spawn_blocking(move || {
                    let picked = rfd::FileDialog::new()
                        .set_title("Export playlist")
                        .set_file_name(crate::local::export_file_name(&playlist.title))
                        .add_filter("Tunebox playlist", &["json"])
                        .save_file();
                    let Some(path) = picked else { return };
                    let text = crate::local::export_playlist(&playlist);
                    match std::fs::write(&path, text) {
                        Ok(()) => this.emit(Event::Toast(format!(
                            "Exported {} songs to {}",
                            playlist.tracks.len(),
                            path.display()
                        ))),
                        Err(e) => this.emit(Event::Toast(format!("Could not export: {e}"))),
                    }
                });
            }
            Action::PickPlaylistCover { id, dir } => {
                let this = self.clone();
                rt.spawn_blocking(move || {
                    let picked = rfd::FileDialog::new()
                        .set_title("Choose a playlist cover")
                        .add_filter("Image", &["png", "jpg", "jpeg", "webp"])
                        .pick_file();
                    let Some(path) = picked else { return };
                    let stamp = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_millis())
                        .unwrap_or(0);
                    let file = format!("{}-{stamp}.jpg", id.replace(':', "-"));
                    let result = std::fs::read(&path)
                        .map_err(|e| e.to_string())
                        .and_then(|b| crate::local::process_cover(&b))
                        .and_then(|jpeg| {
                            std::fs::create_dir_all(&dir)
                                .and_then(|()| std::fs::write(dir.join(&file), jpeg))
                                .map_err(|e| e.to_string())
                        });
                    match result {
                        Ok(()) => this.emit(Event::PlaylistCover { id, file }),
                        Err(e) => this.emit(Event::Toast(format!("Could not set the cover: {e}"))),
                    }
                });
            }
            Action::ImportPlaylist => {
                let this = self.clone();
                rt.spawn_blocking(move || {
                    let picked = rfd::FileDialog::new()
                        .set_title("Import playlist")
                        .add_filter("Tunebox playlist", &["json"])
                        .pick_file();
                    let Some(path) = picked else { return };
                    let parsed = std::fs::read_to_string(&path)
                        .map_err(|e| e.to_string())
                        .and_then(|t| crate::local::import_playlist(&t));
                    match parsed {
                        Ok((title, tracks)) => this.emit(Event::PlaylistImported { title, tracks }),
                        Err(e) => this.emit(Event::Toast(format!("Could not import: {e}"))),
                    }
                });
            }
            Action::LoadPlaylistTracks(ids) => {
                let Some(path) = self
                    .local_path
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .clone()
                else {
                    return;
                };
                let this = self.clone();
                rt.spawn_blocking(move || {
                    let loaded = crate::local::read_playlist_tracks(&path, &ids);
                    this.emit(Event::PlaylistTracks(loaded));
                });
            }
            Action::PersistLocal(library) => {
                let Some(path) = self
                    .local_path
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .clone()
                else {
                    return;
                };
                let (this, lock) = (self.clone(), self.local_write.clone());
                rt.spawn_blocking(move || {
                    // Serialised so an older snapshot can never overwrite a newer one.
                    let _guard = lock.lock().unwrap_or_else(|e| e.into_inner());
                    if let Err(e) = library.write(&path) {
                        tracing::error!(error = %e, "could not save local library");
                        this.emit(Event::Toast(format!(
                            "Could not save your local library: {e}"
                        )));
                    }
                });
            }
            Action::Suggest { seq, query } => {
                let this = self.clone();
                rt.spawn(async move {
                    // Suggestions are best-effort; failures are silent.
                    if let Ok(items) = this.api.search_suggestions(&query).await {
                        this.emit(Event::Suggestions { seq, items });
                    }
                });
            }
        }
    }
}

/// Copies everything in the data folder to `target`, makes it the data folder and points the
/// library there. Never overwrites: a folder that already holds Tunebox data is refused.
fn move_data_dir(
    this: &Backend,
    snapshot: &crate::local::Snapshot,
    target: &std::path::Path,
) -> Result<(), String> {
    let old = ytm_core::Config::data_dir().map_err(|e| e.to_string())?;
    if target == old {
        return Err("That is already the data folder".into());
    }
    if target.starts_with(&old) {
        return Err("Pick a folder outside the current data folder".into());
    }
    if target.join("library.json").exists() || target.join("config.toml").exists() {
        return Err(format!(
            "{} already holds Tunebox data — pick another folder",
            target.display()
        ));
    }
    copy_dir(&old, target).map_err(|e| format!("Could not copy your data there: {e}"))?;
    // Anything changed since the files were last written.
    snapshot
        .write(&target.join("library.json"))
        .map_err(|e| format!("Could not write the library there: {e}"))?;
    ytm_core::Config::set_data_dir(target)
        .map_err(|e| format!("Could not remember the new folder: {e}"))?;
    *this.local_path.lock().unwrap_or_else(|e| e.into_inner()) = Some(target.join("library.json"));
    Ok(())
}

/// Recursive copy of the data folder; the `location` pointer and half-written `.tmp` files stay.
fn copy_dir(from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)?.flatten() {
        let name = entry.file_name();
        let skip = name == "location" || name.to_string_lossy().ends_with(".tmp");
        let dest = to.join(&name);
        if skip {
            continue;
        } else if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &dest)?;
        } else {
            std::fs::copy(entry.path(), dest)?;
        }
    }
    Ok(())
}
