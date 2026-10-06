use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("could not determine a home directory")]
    NoHome,
    #[error("io error on {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("invalid config: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("could not serialise config: {0}")]
    Serialize(#[from] toml::ser::Error),
}

/// User-tunable settings, stored as TOML. Every field has a default so
/// older/partial files keep loading.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Playback volume, 0.0..=1.0.
    pub volume: f32,
    /// Content language (`hl`) sent to InnerTube.
    pub language: String,
    /// App (chrome) language: en, tr, de, es or fr. Independent of the content language above.
    pub ui_language: String,
    /// `dark` or `light`.
    pub theme: String,
    /// Refetch Home / Explore / … when opened after this many minutes; 0 = only once per session.
    pub refresh_minutes: u32,
    /// Legacy (read only): where older versions kept `library.json`. [`Config::load`] turns it into a
    /// data-directory location; it is never written any more.
    #[serde(skip_serializing)]
    pub library_path: Option<PathBuf>,
    /// Maximum on-disk thumbnail cache size in megabytes.
    pub thumbnail_cache_mb: u64,
    /// Preferred audio itag order. Must be formats the decoder supports (AAC: 140, 139);
    /// Opus (251) needs libopus, which symphonia does not provide.
    pub preferred_itags: Vec<u32>,
    /// Path to a `yt-dlp` binary used as a stream-resolution fallback.
    pub ytdlp_path: Option<PathBuf>,
    /// Show a system-tray icon (applies at the next start).
    pub tray_icon: bool,
    /// Closing the window hides it to the tray instead of quitting (needs a working tray icon).
    pub close_to_tray: bool,
    /// Show the current song as text next to the tray icon (hosts that support it, e.g. GNOME's
    /// AppIndicator extension).
    pub tray_label: bool,
    /// Desktop notification when the song changes while the window is in the background.
    pub notifications: bool,
    /// Continue where the last session left off (queue, track, position), paused.
    pub restore_session: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            volume: 0.8,
            language: "en".into(),
            ui_language: "en".into(),
            theme: "dark".into(),
            refresh_minutes: 30,
            library_path: None,
            thumbnail_cache_mb: 256,
            preferred_itags: vec![140, 139],
            ytdlp_path: None,
            tray_icon: true,
            close_to_tray: false,
            tray_label: false,
            notifications: true,
            restore_session: true,
        }
    }
}

impl Config {
    pub fn dirs() -> Result<ProjectDirs, ConfigError> {
        ProjectDirs::from("dev", "tunebox", "Tunebox").ok_or(ConfigError::NoHome)
    }

    /// The data directory unless the user moved it: `<data dir>/tunebox`.
    pub fn default_data_dir() -> Result<PathBuf, ConfigError> {
        Ok(Self::dirs()?.data_dir().to_path_buf())
    }

    /// The folder that holds all of the app's own data: `config.toml`, `library.json`, `playlists/`,
    /// `covers/`. Normally [`Self::default_data_dir`]; after "Change folder…" a small `location` file
    /// in the default folder points to the chosen one.
    pub fn data_dir() -> Result<PathBuf, ConfigError> {
        Ok(resolve_data_dir(&Self::default_data_dir()?))
    }

    /// Remembers `dir` as the data folder (or forgets the choice when it is the default one).
    pub fn set_data_dir(dir: &Path) -> Result<(), ConfigError> {
        set_data_dir_pointer(&Self::default_data_dir()?, dir)
    }

    pub fn default_path() -> Result<PathBuf, ConfigError> {
        Ok(Self::data_dir()?.join(CONFIG_FILE))
    }

    /// Loads the config at `path`, returning defaults if it does not exist.
    pub fn load_from(path: &Path) -> Result<Self, ConfigError> {
        match std::fs::read_to_string(path) {
            Ok(text) => Ok(toml::from_str(&text)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(source) => Err(ConfigError::Io {
                path: path.into(),
                source,
            }),
        }
    }

    /// Loads the settings from the data folder. Older versions kept them in the config directory and
    /// the library in a folder of the user's choice: both are carried over once.
    pub fn load() -> Result<Self, ConfigError> {
        let default_dir = Self::default_data_dir()?;
        let legacy = Self::dirs()?.config_dir().join(CONFIG_FILE);
        let mut dir = resolve_data_dir(&default_dir);
        let path = dir.join(CONFIG_FILE);
        let migrating = !path.exists() && legacy.exists();
        let mut cfg = Self::load_from(if migrating { &legacy } else { &path })?;
        let mut changed = migrating;
        if let Some(old_library) = cfg.library_path.take() {
            changed = true;
            if dir == default_dir {
                if let Some(parent) = old_library.parent().filter(|p| p.is_dir()) {
                    if set_data_dir_pointer(&default_dir, parent).is_ok() {
                        dir = parent.to_path_buf();
                    }
                }
            }
        }
        if changed && cfg.save_to(&dir.join(CONFIG_FILE)).is_ok() && migrating {
            let _ = std::fs::remove_file(&legacy);
        }
        Ok(cfg)
    }

    pub fn save_to(&self, path: &Path) -> Result<(), ConfigError> {
        let io = |source| ConfigError::Io {
            path: path.into(),
            source,
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(io)?;
        }
        std::fs::write(path, toml::to_string_pretty(self)?).map_err(io)
    }

    pub fn save(&self) -> Result<(), ConfigError> {
        self.save_to(&Self::default_path()?)
    }
}

const CONFIG_FILE: &str = "config.toml";
/// File in the default data folder that holds the path of the folder the user moved the data to.
const LOCATION_FILE: &str = "location";

/// `default_dir`, or the folder its `location` file points to.
pub fn resolve_data_dir(default_dir: &Path) -> PathBuf {
    match std::fs::read_to_string(default_dir.join(LOCATION_FILE)) {
        Ok(text) if !text.trim().is_empty() => PathBuf::from(text.trim()),
        _ => default_dir.to_path_buf(),
    }
}

/// Writes (or, for `dir == default_dir`, removes) the `location` file.
pub fn set_data_dir_pointer(default_dir: &Path, dir: &Path) -> Result<(), ConfigError> {
    let io = |source| ConfigError::Io {
        path: default_dir.join(LOCATION_FILE),
        source,
    };
    let file = default_dir.join(LOCATION_FILE);
    if dir == default_dir {
        return match std::fs::remove_file(&file) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(io(e)),
            _ => Ok(()),
        };
    }
    std::fs::create_dir_all(default_dir).map_err(io)?;
    std::fs::write(&file, dir.to_string_lossy().as_bytes()).map_err(io)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_dir_follows_the_location_file() {
        let tmp = tempfile::tempdir().unwrap();
        let default = tmp.path().join("default");
        let moved = tmp.path().join("moved");
        assert_eq!(resolve_data_dir(&default), default);
        set_data_dir_pointer(&default, &moved).unwrap();
        assert_eq!(resolve_data_dir(&default), moved);
        set_data_dir_pointer(&default, &default).unwrap();
        assert_eq!(resolve_data_dir(&default), default);
        set_data_dir_pointer(&default, &default).unwrap();
    }

    #[test]
    fn legacy_library_path_is_read_but_never_written() {
        let cfg: Config = toml::from_str("library_path = \"/x/library.json\"\n").unwrap();
        assert_eq!(cfg.library_path, Some("/x/library.json".into()));
        assert!(!toml::to_string_pretty(&cfg)
            .unwrap()
            .contains("library_path"));
    }

    #[test]
    fn missing_file_gives_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = Config::load_from(&dir.path().join("nope.toml")).unwrap();
        assert_eq!(cfg, Config::default());
    }

    #[test]
    fn roundtrip_and_partial_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub/config.toml");
        let cfg = Config {
            volume: 0.3,
            ..Config::default()
        };
        cfg.save_to(&path).unwrap();
        assert_eq!(Config::load_from(&path).unwrap(), cfg);

        std::fs::write(&path, "volume = 0.1\n").unwrap();
        let partial = Config::load_from(&path).unwrap();
        assert_eq!(partial.volume, 0.1);
        assert_eq!(partial.refresh_minutes, 30);
    }

    #[test]
    fn invalid_file_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("c.toml");
        std::fs::write(&path, "volume = [").unwrap();
        assert!(Config::load_from(&path).is_err());
    }
}
