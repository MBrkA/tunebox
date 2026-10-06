//! The on-device library: liked songs, playlists and saved albums / artists /
//! playlists. Needs no account; stored in the data directory as a small `library.json` index
//! (liked songs, saved items, and each playlist's title/cover/size) plus one `playlists/<id>.json`
//! per playlist. Startup reads only the index; the songs of the playlists are read in the
//! background, and a change rewrites only the playlists it touched.
//!
//! This module is pure data + file IO. The UI mutates a `LocalLibrary` held in
//! `AppState` and hands a snapshot to the backend, which writes it atomically
//! off the UI thread.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use ytm_api::{AlbumSummary, ArtistSummary, PlaylistSummary, SearchItem, Track};

/// Version of backup / export files.
const FORMAT_VERSION: u32 = 1;
/// Version of the on-disk `library.json` index (1 = everything in one file).
const INDEX_VERSION: u32 = 2;
pub const ID_PREFIX: &str = "local:";

pub fn is_local_id(id: &str) -> bool {
    id.starts_with(ID_PREFIX)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalPlaylist {
    pub id: String,
    pub title: String,
    pub tracks: Vec<Track>,
    /// File name (inside the covers folder) of a user-chosen cover; replaces the song mosaic.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cover: Option<String>,
    /// `Some` while the songs are still being read from disk (`tracks` is empty until then).
    #[serde(skip)]
    pub stub: Option<Stub>,
}

/// What the index knows about a playlist whose songs are not loaded yet.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Stub {
    pub count: usize,
    pub covers: Vec<String>,
}

/// Longest side of a stored cover, in pixels.
pub const COVER_SIZE: u32 = 512;

/// Folder for custom playlist covers: `covers/` next to the library file.
pub fn covers_dir(library_file: &Path) -> PathBuf {
    library_file
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("covers")
}

/// Crops the image to a centred square, shrinks it to at most [`COVER_SIZE`] and encodes it as JPEG,
/// so huge pictures never end up in the library folder.
pub fn process_cover(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let img = image::load_from_memory(bytes).map_err(|e| format!("not a usable image ({e})"))?;
    let side = img.width().min(img.height());
    if side == 0 {
        return Err("the image is empty".into());
    }
    let square = img.crop_imm(
        (img.width() - side) / 2,
        (img.height() - side) / 2,
        side,
        side,
    );
    let size = side.min(COVER_SIZE);
    let rgb = square
        .resize_exact(size, size, image::imageops::FilterType::Lanczos3)
        .to_rgb8();
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 88)
        .encode_image(&rgb)
        .map_err(|e| e.to_string())?;
    Ok(out)
}

impl LocalPlaylist {
    /// Number of songs (known from the index before they are loaded).
    pub fn song_count(&self) -> usize {
        self.stub.as_ref().map_or(self.tracks.len(), |s| s.count)
    }

    /// True until the songs have been read from disk.
    pub fn pending(&self) -> bool {
        self.stub.is_some()
    }

    /// `file://` URI of the custom cover, if one is set.
    pub fn cover_uri(&self, dir: &Path) -> Option<String> {
        self.cover
            .as_ref()
            .map(|f| format!("file://{}", dir.join(f).display()))
    }

    /// Art of the first four songs that have any; empty unless the playlist has more than one song,
    /// so single-song lists keep a plain cover.
    pub fn covers(&self) -> Vec<String> {
        if let Some(stub) = &self.stub {
            return stub.covers.clone();
        }
        if self.tracks.len() < 2 {
            return Vec::new();
        }
        let urls: Vec<String> = self
            .tracks
            .iter()
            .filter_map(|t| ytm_api::pick_thumbnail(&t.thumbnails, 0))
            .map(|t| t.url.clone())
            .take(4)
            .collect();
        if urls.len() < 2 {
            Vec::new()
        } else {
            urls
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LocalLibrary {
    pub version: u32,
    next_id: u64,
    /// Liked songs, most recently liked first.
    pub liked: Vec<Track>,
    pub playlists: Vec<LocalPlaylist>,
    pub albums: Vec<AlbumSummary>,
    pub artists: Vec<ArtistSummary>,
    /// YouTube playlists the user saved (not playlists created locally).
    pub saved_playlists: Vec<PlaylistSummary>,
    /// Playlists whose songs changed since the last snapshot.
    #[serde(skip)]
    dirty: BTreeSet<String>,
}

impl Default for LocalLibrary {
    fn default() -> Self {
        Self {
            version: FORMAT_VERSION,
            next_id: 1,
            liked: Vec::new(),
            playlists: Vec::new(),
            albums: Vec::new(),
            artists: Vec::new(),
            saved_playlists: Vec::new(),
            dirty: BTreeSet::new(),
        }
    }
}

/// Adds `item` at the front unless an entry with the same key exists, in which
/// case it is removed. Returns whether the item is now present.
fn toggle<T, K: PartialEq>(list: &mut Vec<T>, item: T, key: impl Fn(&T) -> K) -> bool {
    let k = key(&item);
    if let Some(pos) = list.iter().position(|x| key(x) == k) {
        list.remove(pos);
        false
    } else {
        list.insert(0, item);
        true
    }
}

impl LocalLibrary {
    // ---- liked songs ------------------------------------------------------

    pub fn is_liked(&self, video_id: &str) -> bool {
        self.liked.iter().any(|t| t.video_id == video_id)
    }

    /// Returns the new state.
    pub fn toggle_like(&mut self, track: &Track) -> bool {
        toggle(&mut self.liked, track.clone(), |t| t.video_id.clone())
    }

    // ---- playlists ----------------------------------------------------------

    pub fn playlist(&self, id: &str) -> Option<&LocalPlaylist> {
        self.playlists.iter().find(|p| p.id == id)
    }

    pub fn playlist_mut(&mut self, id: &str) -> Option<&mut LocalPlaylist> {
        self.playlists.iter_mut().find(|p| p.id == id)
    }

    /// A playlist whose songs are loaded (the only kind whose songs may be edited).
    fn loaded_mut(&mut self, id: &str) -> Option<&mut LocalPlaylist> {
        self.playlist_mut(id).filter(|p| !p.pending())
    }

    /// True once every playlist's songs are in memory.
    pub fn fully_loaded(&self) -> bool {
        self.playlists.iter().all(|p| !p.pending())
    }

    /// Ids of playlists whose songs still have to be read from disk.
    pub fn pending_ids(&self) -> Vec<String> {
        self.playlists
            .iter()
            .filter(|p| p.pending())
            .map(|p| p.id.clone())
            .collect()
    }

    /// Hands over songs read in the background. Playlists that were loaded, replaced or deleted in
    /// the meantime are left alone.
    pub fn attach_tracks(&mut self, id: &str, tracks: Vec<Track>) {
        if let Some(p) = self.playlist_mut(id).filter(|p| p.pending()) {
            p.tracks = tracks;
            p.stub = None;
        }
    }

    /// Makes the next snapshot rewrite every playlist (after a restore).
    pub fn mark_all_dirty(&mut self) {
        self.dirty = self.playlists.iter().map(|p| p.id.clone()).collect();
    }

    /// Creates a playlist (new ones go first) and returns its id.
    pub fn create_playlist(&mut self, title: &str, tracks: Vec<Track>) -> String {
        let id = format!("{ID_PREFIX}{}", self.next_id);
        self.next_id += 1;
        let mut unique: Vec<Track> = Vec::with_capacity(tracks.len());
        for t in tracks {
            if !unique.iter().any(|u| u.video_id == t.video_id) {
                unique.push(t);
            }
        }
        self.playlists.insert(
            0,
            LocalPlaylist {
                id: id.clone(),
                title: clean_title(title),
                tracks: unique,
                cover: None,
                stub: None,
            },
        );
        self.dirty.insert(id.clone());
        id
    }

    pub fn rename_playlist(&mut self, id: &str, title: &str) -> bool {
        match self.playlist_mut(id) {
            Some(p) => {
                p.title = clean_title(title);
                true
            }
            None => false,
        }
    }

    pub fn delete_playlist(&mut self, id: &str) -> bool {
        let before = self.playlists.len();
        self.playlists.retain(|p| p.id != id);
        self.playlists.len() != before
    }

    /// Appends tracks that are not already in the playlist; returns how many were added.
    pub fn add_to_playlist(&mut self, id: &str, tracks: &[Track]) -> usize {
        let Some(p) = self.loaded_mut(id) else {
            return 0;
        };
        let mut added = 0;
        for t in tracks {
            if !p.tracks.iter().any(|x| x.video_id == t.video_id) {
                p.tracks.push(t.clone());
                added += 1;
            }
        }
        if added > 0 {
            self.dirty.insert(id.to_owned());
        }
        added
    }

    pub fn remove_from_playlist(&mut self, id: &str, index: usize) -> bool {
        match self.loaded_mut(id) {
            Some(p) if index < p.tracks.len() => {
                p.tracks.remove(index);
                self.dirty.insert(id.to_owned());
                true
            }
            _ => false,
        }
    }

    /// Moves a track by `delta` positions (negative = up), clamped to the list.
    pub fn move_in_playlist(&mut self, id: &str, index: usize, delta: isize) -> bool {
        let Some(p) = self.loaded_mut(id) else {
            return false;
        };
        if index >= p.tracks.len() {
            return false;
        }
        let to = (index as isize + delta).clamp(0, p.tracks.len() as isize - 1) as usize;
        if to == index {
            return false;
        }
        let t = p.tracks.remove(index);
        p.tracks.insert(to, t);
        self.dirty.insert(id.to_owned());
        true
    }

    // ---- saved albums / artists / playlists --------------------------------------

    pub fn is_saved(&self, item: &SearchItem) -> bool {
        match item {
            SearchItem::Album(a) => self.albums.iter().any(|x| x.browse_id == a.browse_id),
            SearchItem::Artist(a) => self.artists.iter().any(|x| x.browse_id == a.browse_id),
            SearchItem::Playlist(p) => self
                .saved_playlists
                .iter()
                .any(|x| x.playlist_id == p.playlist_id),
            SearchItem::Track(t) => self.is_liked(&t.video_id),
        }
    }

    /// Saves or unsaves; returns the new state. Tracks are handled as likes.
    pub fn toggle_saved(&mut self, item: &SearchItem) -> bool {
        match item {
            SearchItem::Album(a) => toggle(&mut self.albums, a.clone(), |x| x.browse_id.clone()),
            SearchItem::Artist(a) => toggle(&mut self.artists, a.clone(), |x| x.browse_id.clone()),
            SearchItem::Playlist(p) => toggle(&mut self.saved_playlists, p.clone(), |x| {
                x.playlist_id.clone()
            }),
            SearchItem::Track(t) => self.toggle_like(t),
        }
    }

    /// Playlists for the Playlists page: own playlists as cards (`local:` ids), then saved ones.
    pub fn playlist_cards(&self, covers: &Path) -> Vec<SearchItem> {
        self.playlists
            .iter()
            .map(|p| {
                SearchItem::Playlist(PlaylistSummary {
                    playlist_id: p.id.clone(),
                    title: p.title.clone(),
                    subtitle: match p.song_count() {
                        1 => "1 song".into(),
                        n => format!("{n} songs"),
                    },
                    thumbnails: match p.cover_uri(covers) {
                        Some(url) => vec![ytm_api::Thumbnail {
                            url,
                            ..Default::default()
                        }],
                        None => p
                            .tracks
                            .iter()
                            .find(|t| !t.thumbnails.is_empty())
                            .map(|t| t.thumbnails.clone())
                            .unwrap_or_default(),
                    },
                    covers: if p.cover.is_some() {
                        Vec::new()
                    } else {
                        p.covers()
                    },
                })
            })
            .chain(
                self.saved_playlists
                    .iter()
                    .cloned()
                    .map(SearchItem::Playlist),
            )
            .collect()
    }

    // ---- persistence ----------------------------------------------------------------

    /// Keep ids unique even if the stored counter was lost or edited.
    fn normalize(&mut self) {
        let max = self
            .playlists
            .iter()
            .filter_map(|p| p.id.strip_prefix(ID_PREFIX)?.parse::<u64>().ok())
            .max()
            .unwrap_or(0);
        self.next_id = self.next_id.max(max + 1);
    }

    /// Parses a backup file. Stricter than `load`: every field defaults, so without this check
    /// any JSON object (even `{}`) would "restore" an empty library over the real one.
    pub fn parse_backup(text: &str) -> Result<Self, String> {
        let value: serde_json::Value = serde_json::from_str(text)
            .map_err(|e| format!("not a Tunebox library backup ({e})"))?;
        let looks_right = value.get("version").is_some_and(|v| v.is_u64())
            && ["liked", "playlists"]
                .iter()
                .all(|k| value.get(*k).is_some_and(|v| v.is_array()));
        if !looks_right {
            return Err("not a Tunebox library backup".into());
        }
        let mut lib: Self = serde_json::from_value(value)
            .map_err(|e| format!("the backup could not be read ({e})"))?;
        if lib.version > FORMAT_VERSION {
            return Err(format!(
                "this backup is from a newer Tunebox (format version {})",
                lib.version
            ));
        }
        lib.normalize();
        Ok(lib)
    }

    pub fn default_path() -> Option<PathBuf> {
        ytm_core::Config::data_dir()
            .ok()
            .map(|d| d.join("library.json"))
    }

    /// Loads the index. A missing file is an empty library; a corrupt file is moved aside (never
    /// overwritten) and an empty library is returned along with a message for the user. The songs
    /// of the playlists stay pending (see [`read_playlist_tracks`]); a library in the old
    /// single-file format is read completely and migrated to the split layout.
    pub fn load(path: &Path) -> (Self, Option<String>) {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (Self::default(), None),
            Err(e) => {
                return (
                    Self::default(),
                    Some(format!("Could not read your local library: {e}")),
                )
            }
        };
        match serde_json::from_str::<IndexFile>(&text) {
            Ok(index) => Self::from_index(index, path),
            Err(e) => {
                let stamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                let aside = path.with_extension(format!("json.corrupt-{stamp}"));
                let moved = std::fs::rename(path, &aside).is_ok();
                (
                    Self::default(),
                    Some(format!(
                        "Your local library file was unreadable ({e}); {}",
                        if moved {
                            format!("it was kept as {}", aside.display())
                        } else {
                            "it was left untouched".into()
                        }
                    )),
                )
            }
        }
    }

    fn from_index(index: IndexFile, path: &Path) -> (Self, Option<String>) {
        let legacy = index.playlists.iter().any(|p| p.tracks.is_some());
        let mut lib = Self {
            next_id: index.next_id.max(1),
            liked: index.liked,
            albums: index.albums,
            artists: index.artists,
            saved_playlists: index.saved_playlists,
            playlists: index
                .playlists
                .into_iter()
                .map(|p| LocalPlaylist {
                    id: p.id,
                    title: p.title,
                    cover: p.cover,
                    stub: (!legacy).then_some(Stub {
                        count: p.count,
                        covers: p.covers,
                    }),
                    tracks: p.tracks.unwrap_or_default(),
                })
                .collect(),
            ..Self::default()
        };
        lib.normalize();
        if !legacy {
            return (lib, None);
        }
        // Migration: keep the old file next to the new layout, then write it.
        let backup = path.with_extension("json.v1");
        if !backup.exists() {
            let _ = std::fs::copy(path, &backup);
        }
        lib.mark_all_dirty();
        let warning = lib
            .snapshot()
            .write(path)
            .err()
            .map(|e| format!("Could not convert your library to the new format: {e}"));
        (lib, warning)
    }

    /// The small index file for this library (songs of playlists are not included).
    fn index(&self) -> IndexFile {
        IndexFile {
            version: INDEX_VERSION,
            next_id: self.next_id,
            liked: self.liked.clone(),
            playlists: self
                .playlists
                .iter()
                .map(|p| IndexPlaylist {
                    id: p.id.clone(),
                    title: p.title.clone(),
                    cover: p.cover.clone(),
                    count: p.song_count(),
                    covers: p.covers(),
                    tracks: None,
                })
                .collect(),
            albums: self.albums.clone(),
            artists: self.artists.clone(),
            saved_playlists: self.saved_playlists.clone(),
        }
    }

    /// What has to be written after a change: the index plus the playlists whose songs changed.
    /// Clears the changed-set, so only call it when the result will be written.
    pub fn snapshot(&mut self) -> Snapshot {
        let dirty = std::mem::take(&mut self.dirty);
        Snapshot {
            index: self.index(),
            playlists: self
                .playlists
                .iter()
                .filter(|p| !p.pending() && dirty.contains(&p.id))
                .map(|p| (p.id.clone(), p.tracks.clone()))
                .collect(),
        }
    }

    /// A snapshot of everything that is loaded (pending playlists keep their files untouched).
    pub fn snapshot_all(&self) -> Snapshot {
        Snapshot {
            index: self.index(),
            playlists: self
                .playlists
                .iter()
                .filter(|p| !p.pending())
                .map(|p| (p.id.clone(), p.tracks.clone()))
                .collect(),
        }
    }

    /// Writes the whole library (index and every loaded playlist).
    #[cfg(test)]
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        self.snapshot_all().write(path)
    }
}

/// Contents of `library.json`. Version 1 files also carry each playlist's `tracks`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct IndexFile {
    version: u32,
    next_id: u64,
    pub liked: Vec<Track>,
    playlists: Vec<IndexPlaylist>,
    pub albums: Vec<AlbumSummary>,
    artists: Vec<ArtistSummary>,
    saved_playlists: Vec<PlaylistSummary>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct IndexPlaylist {
    id: String,
    title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    cover: Option<String>,
    count: usize,
    covers: Vec<String>,
    #[serde(skip_serializing)]
    tracks: Option<Vec<Track>>,
}

/// The file with one playlist's songs, in the `playlists/` folder next to `library.json`.
fn playlist_file(library_file: &Path, id: &str) -> PathBuf {
    library_file
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("playlists")
        .join(format!("{}.json", id.replace(':', "-")))
}

fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

/// A consistent set of files to write; built on the UI thread, written on a worker.
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub index: IndexFile,
    pub playlists: Vec<(String, Vec<Track>)>,
}

impl Snapshot {
    /// Atomically writes the changed playlists, then the index, then removes the files of deleted
    /// playlists.
    pub fn write(&self, path: &Path) -> std::io::Result<()> {
        for (id, tracks) in &self.playlists {
            write_atomic(&playlist_file(path, id), &serde_json::to_vec(tracks)?)?;
        }
        write_atomic(path, &serde_json::to_vec_pretty(&self.index)?)?;
        let keep: BTreeSet<std::ffi::OsString> = self
            .index
            .playlists
            .iter()
            .filter_map(|p| playlist_file(path, &p.id).file_name().map(|n| n.to_owned()))
            .collect();
        if let Some(dir) = playlist_file(path, "x").parent() {
            for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
                let name = entry.file_name();
                let ours = name.to_string_lossy().starts_with("local-");
                if ours && !keep.contains(&name) {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
        Ok(())
    }
}

/// Reads the songs of the given playlists (slow part of startup, run off the UI thread). A missing
/// or unreadable file yields an error message; an unreadable one is moved aside, never overwritten.
pub fn read_playlist_tracks(
    library_file: &Path,
    ids: &[String],
) -> Vec<(String, Result<Vec<Track>, String>)> {
    ids.iter()
        .map(|id| {
            let file = playlist_file(library_file, id);
            let result = match std::fs::read(&file) {
                Ok(bytes) => serde_json::from_slice::<Vec<Track>>(&bytes).map_err(|e| {
                    let mut aside = file.as_os_str().to_owned();
                    aside.push(".corrupt");
                    let _ = std::fs::rename(&file, PathBuf::from(aside));
                    format!("a playlist file was unreadable ({e})")
                }),
                Err(e) => Err(format!("a playlist file is missing ({e})")),
            };
            (id.clone(), result)
        })
        .collect()
}

/// The options stored in a backup next to the library. Only choices that make sense on another
/// computer: the `ytdlp_path` and the cache size stay local.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupSettings {
    pub theme: String,
    pub ui_language: String,
    pub refresh_minutes: u32,
}

impl BackupSettings {
    pub fn from_config(cfg: &ytm_core::Config) -> Self {
        Self {
            theme: cfg.theme.clone(),
            ui_language: cfg.ui_language.clone(),
            refresh_minutes: cfg.refresh_minutes,
        }
    }

    /// Copies the backed-up choices into `cfg` (the content language follows the UI language).
    pub fn apply_to(&self, cfg: &mut ytm_core::Config) {
        cfg.theme = self.theme.clone();
        cfg.ui_language = self.ui_language.clone();
        cfg.refresh_minutes = self.refresh_minutes;
    }
}

/// Serialises the library plus the options as one backup file.
pub fn backup_bytes(lib: &LocalLibrary, settings: &BackupSettings) -> serde_json::Result<Vec<u8>> {
    let mut value = serde_json::to_value(lib)?;
    if let Some(map) = value.as_object_mut() {
        map.insert("settings".into(), serde_json::to_value(settings)?);
    }
    serde_json::to_vec_pretty(&value)
}

/// Parses a backup file: the library, plus the options when the file has them (older backups don't,
/// and an unreadable `settings` block is ignored rather than failing the whole restore).
pub fn parse_backup_file(text: &str) -> Result<(LocalLibrary, Option<BackupSettings>), String> {
    let lib = LocalLibrary::parse_backup(text)?;
    let settings = serde_json::from_str::<serde_json::Value>(text)
        .ok()
        .and_then(|v| serde_json::from_value::<BackupSettings>(v.get("settings")?.clone()).ok())
        .filter(|s| matches!(s.theme.as_str(), "dark" | "light"));
    Ok((lib, settings))
}

/// Marker written into exported playlist files.
pub const EXPORT_FORMAT: &str = "tunebox-playlist";
const EXPORT_VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct PlaylistFile {
    format: String,
    version: u32,
    title: String,
    #[serde(default)]
    tracks: Vec<Track>,
}

/// A portable, human-readable JSON document for one playlist (title + tracks).
pub fn export_playlist(p: &LocalPlaylist) -> String {
    let file = PlaylistFile {
        format: EXPORT_FORMAT.into(),
        version: EXPORT_VERSION,
        title: p.title.clone(),
        tracks: p.tracks.clone(),
    };
    serde_json::to_string_pretty(&file).expect("playlist serialises")
}

/// Parses an exported playlist; returns its title and tracks (tracks without a video id are dropped).
pub fn import_playlist(text: &str) -> Result<(String, Vec<Track>), String> {
    let file: PlaylistFile =
        serde_json::from_str(text).map_err(|e| format!("not a Tunebox playlist file ({e})"))?;
    if file.format != EXPORT_FORMAT {
        return Err("not a Tunebox playlist file".into());
    }
    if file.version > EXPORT_VERSION {
        return Err(format!(
            "this file is from a newer Tunebox (format version {})",
            file.version
        ));
    }
    let tracks: Vec<Track> = file
        .tracks
        .into_iter()
        .filter(|t| !t.video_id.trim().is_empty())
        .collect();
    Ok((file.title, tracks))
}

/// A file-name-safe version of a playlist title.
pub fn export_file_name(title: &str) -> String {
    let cleaned: String = title
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || " -_()".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect();
    let cleaned = cleaned.trim();
    format!(
        "{}.tunebox.json",
        if cleaned.is_empty() {
            "playlist"
        } else {
            cleaned
        }
    )
}

fn clean_title(title: &str) -> String {
    let t = title.trim();
    if t.is_empty() {
        "Untitled playlist".into()
    } else {
        t.chars().take(120).collect()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn backup_carries_options_and_old_backups_still_restore() {
        let mut lib = LocalLibrary::default();
        lib.create_playlist("P", vec![]);
        let cfg = ytm_core::Config {
            theme: "light".into(),
            ui_language: "tr".into(),
            refresh_minutes: 60,
            ..Default::default()
        };
        let bytes = backup_bytes(&lib, &BackupSettings::from_config(&cfg)).unwrap();
        let (back, settings) = parse_backup_file(std::str::from_utf8(&bytes).unwrap()).unwrap();
        assert_eq!(back.playlists.len(), 1);
        let mut target = ytm_core::Config {
            ytdlp_path: Some("/keep/me".into()),
            ..Default::default()
        };
        settings.unwrap().apply_to(&mut target);
        assert_eq!(
            (
                target.theme.as_str(),
                target.ui_language.as_str(),
                target.refresh_minutes
            ),
            ("light", "tr", 60)
        );
        assert_eq!(
            target.ytdlp_path,
            Some("/keep/me".into()),
            "paths stay local"
        );
        // a library-only backup has no options; a broken options block is ignored
        let plain = serde_json::to_string(&lib).unwrap();
        assert!(parse_backup_file(&plain).unwrap().1.is_none());
        let broken = plain.replacen('{', "{\"settings\":{\"theme\":\"x\"},", 1);
        assert!(parse_backup_file(&broken).unwrap().1.is_none());
    }

    #[test]
    fn backup_parsing_rejects_things_that_are_not_libraries() {
        let mut lib = LocalLibrary::default();
        lib.create_playlist("P", vec![]);
        let text = serde_json::to_string(&lib).unwrap();
        let back = LocalLibrary::parse_backup(&text).unwrap();
        assert_eq!(back.playlists.len(), 1);
        assert!(
            LocalLibrary::parse_backup("{}").is_err(),
            "{{}} must not wipe a library"
        );
        assert!(LocalLibrary::parse_backup("[1]").is_err());
        assert!(LocalLibrary::parse_backup("nope").is_err());
        let newer = text.replace("\"version\":1", "\"version\":99");
        assert!(LocalLibrary::parse_backup(&newer)
            .unwrap_err()
            .contains("newer"));
        // ids stay unique after a restore
        let mut back = back;
        let id = back.create_playlist("Q", vec![]);
        assert!(back.playlists.iter().filter(|p| p.id == id).count() == 1);
    }

    #[test]
    fn export_import_roundtrip_and_rejects_foreign_files() {
        let mut lib = LocalLibrary::default();
        let id = lib.create_playlist(
            "Road trip",
            vec![
                Track {
                    video_id: "aaaaaaaaaaa".into(),
                    title: "A".into(),
                    ..Track::default()
                },
                Track {
                    video_id: "bbbbbbbbbbb".into(),
                    title: "B".into(),
                    ..Track::default()
                },
            ],
        );
        let text = export_playlist(lib.playlist(&id).unwrap());
        let (title, tracks) = import_playlist(&text).unwrap();
        assert_eq!(title, "Road trip");
        assert_eq!(tracks, lib.playlist(&id).unwrap().tracks);

        assert!(import_playlist("{\"liked\": []}").is_err());
        assert!(import_playlist("garbage").is_err());
        let newer = text.replace("\"version\": 1", "\"version\": 99");
        assert!(import_playlist(&newer).unwrap_err().contains("newer"));
        let blank = r#"{"format":"tunebox-playlist","version":1,"title":"x","tracks":[{"video_id":"","title":"t","artists":[],"album":null,"duration_secs":null,"thumbnails":[]}]}"#;
        assert!(import_playlist(blank).unwrap().1.is_empty());
    }

    #[test]
    fn export_file_names_are_safe() {
        assert_eq!(
            export_file_name("Road/trip: 2024?"),
            "Road_trip_ 2024_.tunebox.json"
        );
        assert_eq!(export_file_name("  "), "playlist.tunebox.json");
    }

    use super::*;

    fn t(id: &str) -> Track {
        Track {
            video_id: id.into(),
            title: format!("song {id}"),
            ..Track::default()
        }
    }

    fn dir() -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "tunebox-local-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn likes_toggle_and_newest_first() {
        let mut lib = LocalLibrary::default();
        assert!(lib.toggle_like(&t("a")));
        assert!(lib.toggle_like(&t("b")));
        assert_eq!(lib.liked[0].video_id, "b");
        assert!(lib.is_liked("a"));
        assert!(!lib.toggle_like(&t("a")), "second toggle unlikes");
        assert!(!lib.is_liked("a") && lib.liked.len() == 1);
    }

    #[test]
    fn playlist_lifecycle() {
        let mut lib = LocalLibrary::default();
        let id = lib.create_playlist("  Road trip ", vec![t("a"), t("b"), t("a")]);
        assert!(is_local_id(&id));
        assert_eq!(lib.playlist(&id).unwrap().title, "Road trip");
        assert_eq!(
            lib.playlist(&id).unwrap().tracks.len(),
            2,
            "duplicates dropped"
        );
        assert_eq!(lib.add_to_playlist(&id, &[t("b"), t("c"), t("d")]), 2);
        assert_eq!(lib.add_to_playlist("local:999", &[t("x")]), 0);
        assert!(lib.remove_from_playlist(&id, 0));
        assert!(!lib.remove_from_playlist(&id, 99));
        let ids: Vec<_> = lib
            .playlist(&id)
            .unwrap()
            .tracks
            .iter()
            .map(|x| x.video_id.as_str())
            .collect();
        assert_eq!(ids, ["b", "c", "d"]);
        assert!(lib.rename_playlist(&id, ""));
        assert_eq!(lib.playlist(&id).unwrap().title, "Untitled playlist");
        assert!(lib.delete_playlist(&id));
        assert!(!lib.delete_playlist(&id));
    }

    #[test]
    fn ids_are_never_reused() {
        let mut lib = LocalLibrary::default();
        let a = lib.create_playlist("a", vec![]);
        lib.delete_playlist(&a);
        let b = lib.create_playlist("b", vec![]);
        assert_ne!(a, b);
    }

    #[test]
    fn reordering_clamps() {
        let mut lib = LocalLibrary::default();
        let id = lib.create_playlist("p", vec![t("a"), t("b"), t("c")]);
        assert!(lib.move_in_playlist(&id, 2, -1));
        assert!(lib.move_in_playlist(&id, 0, 10));
        assert!(!lib.move_in_playlist(&id, 2, 1), "already last");
        assert!(!lib.move_in_playlist(&id, 9, 1));
        let ids: Vec<_> = lib
            .playlist(&id)
            .unwrap()
            .tracks
            .iter()
            .map(|x| x.video_id.as_str())
            .collect();
        assert_eq!(ids, ["c", "b", "a"]);
    }

    #[test]
    fn saved_items_toggle_by_id() {
        let mut lib = LocalLibrary::default();
        let album = SearchItem::Album(AlbumSummary {
            browse_id: "MPRE1".into(),
            title: "Discovery".into(),
            ..AlbumSummary::default()
        });
        assert!(!lib.is_saved(&album));
        assert!(lib.toggle_saved(&album));
        assert!(lib.is_saved(&album));
        let artist = SearchItem::Artist(ArtistSummary {
            browse_id: "UC1".into(),
            name: "Daft Punk".into(),
            ..ArtistSummary::default()
        });
        let pl = SearchItem::Playlist(PlaylistSummary {
            playlist_id: "PL1".into(),
            title: "Mix".into(),
            ..PlaylistSummary::default()
        });
        assert!(lib.toggle_saved(&artist) && lib.toggle_saved(&pl));
        assert!(
            lib.toggle_saved(&SearchItem::Track(t("z"))),
            "tracks count as likes"
        );
        assert!(lib.is_liked("z"));
        assert!(!lib.toggle_saved(&album), "second toggle removes");
        assert!(lib.albums.is_empty() && lib.artists.len() == 1);
    }

    #[test]
    fn process_cover_crops_square_from_centre_and_caps_size() {
        let mut img = image::RgbImage::from_pixel(1600, 800, image::Rgb([0, 0, 255]));
        for x in 0..400 {
            for y in 0..800 {
                img.put_pixel(x, y, image::Rgb([255, 0, 0]));
            }
        }
        let mut png = std::io::Cursor::new(Vec::new());
        img.write_to(&mut png, image::ImageFormat::Png).unwrap();
        let out = process_cover(png.get_ref()).unwrap();
        let back = image::load_from_memory(&out).unwrap().to_rgb8();
        assert_eq!(back.dimensions(), (COVER_SIZE, COVER_SIZE));
        // the red left edge lies outside the centred 800×800 square
        let p = back.get_pixel(5, 5);
        assert!(p[2] > 200 && p[0] < 60, "{p:?}");
        assert!(process_cover(b"nope").is_err());
    }

    fn tmp_lib(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tunebox-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("library.json")
    }

    #[test]
    fn split_storage_roundtrip_loads_lazily_and_writes_only_changes() {
        let path = tmp_lib("split");
        let mut lib = LocalLibrary::default();
        let a = lib.create_playlist("A", vec![t("a1"), t("a2"), t("a3")]);
        let b = lib.create_playlist("B", vec![t("b1")]);
        lib.toggle_like(&t("liked"));
        let snap = lib.snapshot();
        assert_eq!(snap.playlists.len(), 2);
        snap.write(&path).unwrap();
        assert!(lib.snapshot().playlists.is_empty(), "nothing changed since");

        let index = std::fs::read_to_string(&path).unwrap();
        assert!(!index.contains("a1"), "songs are not in the index");

        let (mut back, warn) = LocalLibrary::load(&path);
        assert!(warn.is_none());
        let pa = back.playlist(&a).unwrap();
        assert!(pa.pending() && pa.tracks.is_empty() && pa.song_count() == 3);
        assert_eq!(back.pending_ids().len(), 2);
        assert!(back.is_liked("liked"));
        // pending playlists refuse song edits instead of losing data
        assert_eq!(back.add_to_playlist(&a, &[t("x")]), 0);
        for (id, r) in read_playlist_tracks(&path, &back.pending_ids()) {
            back.attach_tracks(&id, r.unwrap());
        }
        assert!(back.fully_loaded());
        assert_eq!(back.playlist(&a).unwrap().tracks.len(), 3);

        // only the touched playlist is rewritten; the other file stays as it was
        let before = std::fs::metadata(playlist_file(&path, &b))
            .unwrap()
            .modified()
            .unwrap();
        assert_eq!(back.add_to_playlist(&a, &[t("a4")]), 1);
        let snap = back.snapshot();
        assert_eq!(snap.playlists.len(), 1);
        snap.write(&path).unwrap();
        assert_eq!(
            std::fs::metadata(playlist_file(&path, &b))
                .unwrap()
                .modified()
                .unwrap(),
            before
        );

        // deleting removes the file
        assert!(back.delete_playlist(&b));
        back.snapshot().write(&path).unwrap();
        assert!(!playlist_file(&path, &b).exists());
        assert!(playlist_file(&path, &a).exists());
    }

    #[test]
    fn old_single_file_library_is_migrated() {
        let path = tmp_lib("migrate");
        let mut old = LocalLibrary::default();
        let id = old.create_playlist("Old", vec![t("o1"), t("o2")]);
        old.toggle_like(&t("l1"));
        std::fs::write(&path, serde_json::to_vec_pretty(&old).unwrap()).unwrap();

        let (lib, warn) = LocalLibrary::load(&path);
        assert!(warn.is_none(), "{warn:?}");
        assert!(lib.fully_loaded());
        assert_eq!(lib.playlist(&id).unwrap().tracks.len(), 2);
        assert!(path.with_extension("json.v1").exists(), "old file kept");
        assert!(playlist_file(&path, &id).exists());
        assert!(!std::fs::read_to_string(&path).unwrap().contains("o1"));
        let (again, _) = LocalLibrary::load(&path);
        assert_eq!(again.playlist(&id).unwrap().song_count(), 2);
        assert!(again.playlist(&id).unwrap().pending());
    }

    #[test]
    fn playlist_covers_are_first_four_arts_and_only_for_multi_song_lists() {
        let art = |id: &str| {
            let mut x = t(id);
            x.thumbnails = vec![ytm_api::Thumbnail {
                url: format!("u{id}"),
                width: 1,
                height: 1,
            }];
            x
        };
        let mut lib = LocalLibrary::default();
        let one = lib.create_playlist("One", vec![art("a")]);
        let many = lib.create_playlist("Many", ["a", "b", "c", "d", "e"].map(art).to_vec());
        assert!(lib.playlist(&one).unwrap().covers().is_empty());
        assert_eq!(
            lib.playlist(&many).unwrap().covers(),
            ["ua", "ub", "uc", "ud"]
        );
    }

    #[test]
    fn playlist_cards_use_first_track_art_and_counts() {
        let mut lib = LocalLibrary::default();
        let mut with_art = t("a");
        with_art.thumbnails = vec![ytm_api::Thumbnail {
            url: "u".into(),
            width: 1,
            height: 1,
        }];
        lib.create_playlist("Mine", vec![t("x"), with_art]);
        lib.toggle_saved(&SearchItem::Playlist(PlaylistSummary {
            playlist_id: "PLs".into(),
            title: "Saved".into(),
            ..Default::default()
        }));
        let cards = lib.playlist_cards(Path::new("/c"));
        let SearchItem::Playlist(own) = &cards[0] else {
            panic!()
        };
        assert_eq!(
            (own.subtitle.as_str(), own.thumbnails.len()),
            ("2 songs", 1)
        );
        assert!(matches!(&cards[1], SearchItem::Playlist(p) if p.playlist_id == "PLs"));
    }

    #[test]
    fn save_and_load_roundtrip_atomically() {
        let d = dir();
        let path = d.join("nested/library.json");
        let mut lib = LocalLibrary::default();
        lib.toggle_like(&t("a"));
        lib.create_playlist("P", vec![t("b")]);
        lib.save(&path).unwrap();
        assert!(
            !path.with_extension("json.tmp").exists(),
            "temp file renamed away"
        );
        let (mut back, warn) = LocalLibrary::load(&path);
        assert!(warn.is_none());
        for (id, r) in read_playlist_tracks(&path, &back.pending_ids()) {
            back.attach_tracks(&id, r.unwrap());
        }
        lib.dirty.clear();
        assert_eq!(back, lib);
        std::fs::remove_dir_all(d).ok();
    }

    #[test]
    fn missing_file_is_empty_and_corrupt_file_is_preserved() {
        let d = dir();
        let path = d.join("library.json");
        let (lib, warn) = LocalLibrary::load(&path);
        assert!(lib == LocalLibrary::default() && warn.is_none());

        std::fs::write(&path, "{ not json").unwrap();
        let (lib, warn) = LocalLibrary::load(&path);
        assert!(lib.liked.is_empty());
        assert!(warn.unwrap().contains("unreadable"));
        assert!(!path.exists(), "corrupt file moved aside, not deleted");
        let kept = std::fs::read_dir(&d).unwrap().flatten().count();
        assert_eq!(kept, 1);
        std::fs::remove_dir_all(d).ok();
    }

    #[test]
    fn older_or_partial_files_still_load_and_counter_recovers() {
        let d = dir();
        let path = d.join("library.json");
        std::fs::write(
            &path,
            r#"{"playlists":[{"id":"local:7","title":"Old","tracks":[]}]}"#,
        )
        .unwrap();
        let (mut lib, warn) = LocalLibrary::load(&path);
        assert!(warn.is_none());
        assert_eq!(lib.playlists.len(), 1);
        assert_eq!(
            lib.create_playlist("New", vec![]),
            "local:8",
            "counter continues after the highest id"
        );
        std::fs::remove_dir_all(d).ok();
    }
}
