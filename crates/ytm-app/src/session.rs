//! The play session (queue, current track, position, shuffle/repeat), saved so the next start
//! continues where this one stopped. Restored paused; nothing plays until the user presses play.

use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use ytm_api::Track;
use ytm_player::{Command, Repeat};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Session {
    pub tracks: Vec<Track>,
    pub current: usize,
    pub position_ms: u64,
    pub shuffle: bool,
    /// 0 = off, 1 = all, 2 = one.
    pub repeat: u8,
}

fn path() -> Option<PathBuf> {
    ytm_core::Config::data_dir()
        .ok()
        .map(|d| d.join("session.json"))
}

pub fn repeat_code(r: Repeat) -> u8 {
    match r {
        Repeat::Off => 0,
        Repeat::All => 1,
        Repeat::One => 2,
    }
}

fn repeat_from(code: u8) -> Repeat {
    match code {
        1 => Repeat::All,
        2 => Repeat::One,
        _ => Repeat::Off,
    }
}

impl Session {
    /// `None` when there is nothing to restore.
    pub fn into_command(self) -> Option<Command> {
        if self.tracks.is_empty() {
            return None;
        }
        Some(Command::Restore {
            start: self.current.min(self.tracks.len() - 1),
            tracks: self.tracks,
            position: Duration::from_millis(self.position_ms),
            shuffle: self.shuffle,
            repeat: repeat_from(self.repeat),
        })
    }

    /// Atomically writes the session file (errors are only logged).
    pub fn save(&self) {
        let Some(path) = path() else { return };
        let result = serde_json::to_vec(self)
            .map_err(std::io::Error::other)
            .and_then(|bytes| {
                if let Some(dir) = path.parent() {
                    std::fs::create_dir_all(dir)?;
                }
                let tmp = path.with_extension("json.tmp");
                std::fs::write(&tmp, bytes)?;
                std::fs::rename(&tmp, &path)
            });
        if let Err(e) = result {
            tracing::warn!("could not save session: {e}");
        }
    }
}

/// Track indices in the order they should be saved so a restored (unshuffled) queue plays the
/// same way: the already-played ones, the current one, then the upcoming ones as the user ordered
/// them (drag-to-reorder). Returns the order and where the current track lands in it.
pub fn save_order(len: usize, current: usize, upcoming: &[usize]) -> (Vec<usize>, usize) {
    let mut before: Vec<usize> = (0..len)
        .filter(|i| *i != current && !upcoming.contains(i))
        .collect();
    let at = before.len();
    before.push(current);
    before.extend_from_slice(upcoming);
    (before, at)
}

/// Forgets the saved session (when the user turns the option off).
pub fn clear() {
    if let Some(p) = path() {
        let _ = std::fs::remove_file(p);
    }
}

/// A missing or unreadable file simply means a fresh start.
pub fn load() -> Option<Session> {
    let text = std::fs::read_to_string(path()?).ok()?;
    serde_json::from_str(&text).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_order_keeps_the_users_upcoming_order() {
        // 0 played, 1 current, upcoming dragged into 4, 2, 3
        assert_eq!(save_order(5, 1, &[4, 2, 3]), (vec![0, 1, 4, 2, 3], 1));
        assert_eq!(save_order(3, 0, &[1, 2]), (vec![0, 1, 2], 0));
        assert_eq!(save_order(3, 2, &[]), (vec![0, 1, 2], 2));
    }

    #[test]
    fn roundtrip_and_empty_session() {
        let s = Session {
            tracks: vec![Track {
                video_id: "a".into(),
                ..Track::default()
            }],
            current: 9,
            position_ms: 1500,
            shuffle: true,
            repeat: 2,
        };
        let back: Session = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        match back.into_command() {
            Some(Command::Restore {
                start,
                position,
                repeat,
                ..
            }) => {
                assert_eq!(start, 0, "clamped into range");
                assert_eq!(position, Duration::from_millis(1500));
                assert_eq!(repeat, Repeat::One);
            }
            other => panic!("unexpected {other:?}"),
        }
        assert!(Session::default().into_command().is_none());
        assert!(serde_json::from_str::<Session>("{}").is_ok());
    }
}
