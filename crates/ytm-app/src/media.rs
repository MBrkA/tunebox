//! OS media integration: media keys and "now playing" metadata
//! (MPRIS on Linux, MediaRemote on macOS) through `souvlaki`.
//!
//! The `souvlaki` handle lives on its own thread. OS events are mapped to
//! [`Action`]s and sent through the same channel the UI uses; player state
//! changes are pushed to the OS from a small tokio task.

use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use souvlaki::{
    MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, MediaPosition, PlatformConfig,
    SeekDirection,
};
use tokio::sync::mpsc::UnboundedSender;
use ytm_player::{Command, Player, PlayerEvent, PlayerState, Status};

use crate::backend::Action;

const PROGRESS_REFRESH: Duration = Duration::from_secs(2);

/// What the OS should currently display.
#[derive(Debug, Clone, PartialEq)]
pub enum Update {
    Track {
        title: String,
        artist: String,
        album: Option<String>,
        cover_url: Option<String>,
        duration: Option<Duration>,
    },
    Playback {
        status: Status,
        position: Duration,
    },
    NoTrack,
}

/// Maps an OS media event to the action it should trigger.
/// `position` is the current playback position, needed for relative seeks.
pub fn event_to_action(event: &MediaControlEvent, position: Duration) -> Option<Action> {
    let cmd = match event {
        MediaControlEvent::Play => Command::Resume,
        MediaControlEvent::Pause => Command::Pause,
        MediaControlEvent::Toggle => Command::Toggle,
        MediaControlEvent::Next => Command::Next,
        MediaControlEvent::Previous => Command::Prev,
        MediaControlEvent::Stop => Command::Stop,
        MediaControlEvent::SetPosition(MediaPosition(p)) => Command::Seek(*p),
        MediaControlEvent::SeekBy(dir, by) => seek_relative(position, *dir, *by),
        // Undetermined amount: 10 s like most players.
        MediaControlEvent::Seek(dir) => seek_relative(position, *dir, Duration::from_secs(10)),
        MediaControlEvent::SetVolume(v) => Command::SetVolume(*v as f32),
        MediaControlEvent::OpenUri(_) | MediaControlEvent::Raise | MediaControlEvent::Quit => {
            return None
        }
    };
    Some(Action::Playback(cmd))
}

fn seek_relative(position: Duration, dir: SeekDirection, by: Duration) -> Command {
    Command::Seek(match dir {
        SeekDirection::Forward => position + by,
        SeekDirection::Backward => position.saturating_sub(by),
    })
}

/// Derives the OS-facing updates implied by a player state change.
pub fn diff(prev: Option<&PlayerState>, now: &PlayerState, position: Duration) -> Vec<Update> {
    let mut out = Vec::new();
    let track_changed = prev.and_then(|p| p.current_track().map(|t| &t.video_id))
        != now.current_track().map(|t| &t.video_id);
    if track_changed || prev.is_none() {
        match now.current_track() {
            Some(t) => out.push(Update::Track {
                title: t.title.clone(),
                artist: t.artist_line(),
                album: t.album.as_ref().map(|a| a.name.clone()),
                cover_url: crate::widgets::best_thumbnail(&t.thumbnails)
                    .map(|u| crate::thumbs::sized_url(u, 544)),
                duration: now.duration,
            }),
            None => out.push(Update::NoTrack),
        }
    }
    let status_changed = prev.map(|p| p.status) != Some(now.status);
    if track_changed || status_changed || prev.is_none() {
        out.push(Update::Playback {
            status: now.status,
            position,
        });
    }
    out
}

fn playback(status: Status, position: Duration) -> MediaPlayback {
    let progress = Some(MediaPosition(position));
    match status {
        Status::Playing => MediaPlayback::Playing { progress },
        Status::Paused => MediaPlayback::Paused { progress },
        // While loading, show the track as paused rather than lying about progress.
        Status::Loading => MediaPlayback::Paused { progress },
        Status::Idle => MediaPlayback::Stopped,
    }
}

fn run_thread(
    actions: UnboundedSender<Action>,
    player: Player,
    updates: Receiver<Update>,
    hwnd: Option<usize>,
) {
    let config = PlatformConfig {
        dbus_name: "tunebox",
        display_name: "Tunebox",
        // Only Windows uses the window handle (MPRIS/MediaRemote ignore it).
        hwnd: hwnd.map(|h| h as *mut std::ffi::c_void),
    };
    let mut controls = match MediaControls::new(config) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(error = ?e, "media controls unavailable");
            return;
        }
    };
    let attach_player = player.clone();
    if let Err(e) = controls.attach(move |event: MediaControlEvent| {
        if let Some(action) = event_to_action(&event, attach_player.position()) {
            let _ = actions.send(action);
        }
    }) {
        tracing::warn!(error = ?e, "could not attach media key handler");
        return;
    }
    tracing::info!("media controls attached");
    // Blocks until the sender is dropped (app exit).
    while let Ok(update) = updates.recv() {
        let result = match &update {
            Update::Track {
                title,
                artist,
                album,
                cover_url,
                duration,
            } => controls.set_metadata(MediaMetadata {
                title: Some(title),
                artist: Some(artist),
                album: album.as_deref(),
                cover_url: cover_url.as_deref(),
                duration: *duration,
            }),
            Update::NoTrack => controls.set_metadata(MediaMetadata::default()),
            Update::Playback { status, position } => {
                controls.set_playback(playback(*status, *position))
            }
        };
        if let Err(e) = result {
            tracing::debug!(error = ?e, "media update failed");
        }
    }
}

/// Starts the media thread and the task that feeds it.
pub fn spawn(
    rt: &tokio::runtime::Handle,
    actions: UnboundedSender<Action>,
    player: Player,
    hwnd: Option<usize>,
) {
    let (tx, rx): (Sender<Update>, Receiver<Update>) = mpsc::channel();
    {
        let player = player.clone();
        let spawned = std::thread::Builder::new()
            .name("ytm-media".into())
            .spawn(move || run_thread(actions, player, rx, hwnd));
        if let Err(e) = spawned {
            tracing::warn!(error = %e, "could not start media thread");
            return;
        }
    }
    rt.spawn(async move {
        let mut events = player.subscribe();
        let mut prev: Option<PlayerState> = None;
        loop {
            let state = player.state();
            for update in diff(prev.as_ref(), &state, player.position()) {
                if tx.send(update).is_err() {
                    return; // media thread gone
                }
            }
            prev = Some(state);
            // Wake on state changes, or every couple of seconds while playing so the
            // OS's progress bar (which polls) stays accurate.
            let tick = tokio::time::sleep(PROGRESS_REFRESH);
            tokio::select! {
                ev = events.recv() => match ev {
                    Ok(PlayerEvent::StateChanged | PlayerEvent::Error(_)) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
                },
                () = tick => {
                    if prev.as_ref().is_some_and(|s| s.status == Status::Playing) {
                        let update = Update::Playback { status: Status::Playing, position: player.position() };
                        if tx.send(update).is_err() {
                            return;
                        }
                    }
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use ytm_api::{ArtistRef, Thumbnail, Track};
    use ytm_player::Repeat;

    fn state(status: Status, id: Option<&str>) -> PlayerState {
        let tracks: Vec<Track> = id
            .map(|i| Track {
                video_id: i.into(),
                title: format!("title-{i}"),
                artists: vec![ArtistRef {
                    name: "Artist".into(),
                    id: None,
                }],
                thumbnails: vec![Thumbnail {
                    url: "https://x/y=w60-h60-l90-rj".into(),
                    width: 60,
                    height: 60,
                }],
                ..Track::default()
            })
            .into_iter()
            .collect();
        PlayerState {
            status,
            current: id.map(|_| 0),
            tracks: Arc::new(tracks),
            upcoming: Arc::new(vec![]),
            duration: Some(Duration::from_secs(200)),
            volume: 0.5,
            shuffle: false,
            repeat: Repeat::Off,
            error: None,
            version: 1,
        }
    }

    fn cmd(a: Option<Action>) -> Command {
        match a {
            Some(Action::Playback(c)) => c,
            other => panic!("expected playback action, got {other:?}"),
        }
    }

    #[test]
    fn keys_map_to_player_commands() {
        let p = Duration::from_secs(30);
        assert!(matches!(
            cmd(event_to_action(&MediaControlEvent::Play, p)),
            Command::Resume
        ));
        assert!(matches!(
            cmd(event_to_action(&MediaControlEvent::Pause, p)),
            Command::Pause
        ));
        assert!(matches!(
            cmd(event_to_action(&MediaControlEvent::Toggle, p)),
            Command::Toggle
        ));
        assert!(matches!(
            cmd(event_to_action(&MediaControlEvent::Next, p)),
            Command::Next
        ));
        assert!(matches!(
            cmd(event_to_action(&MediaControlEvent::Previous, p)),
            Command::Prev
        ));
        assert!(matches!(
            cmd(event_to_action(&MediaControlEvent::Stop, p)),
            Command::Stop
        ));
        assert!(event_to_action(&MediaControlEvent::Raise, p).is_none());
    }

    #[test]
    fn seeking_is_relative_to_the_current_position() {
        let p = Duration::from_secs(30);
        let seek = |e| match cmd(event_to_action(&e, p)) {
            Command::Seek(d) => d,
            c => panic!("{c:?}"),
        };
        assert_eq!(
            seek(MediaControlEvent::SeekBy(
                SeekDirection::Forward,
                Duration::from_secs(5)
            )),
            Duration::from_secs(35)
        );
        assert_eq!(
            seek(MediaControlEvent::SeekBy(
                SeekDirection::Backward,
                Duration::from_secs(50)
            )),
            Duration::ZERO,
            "clamped at 0"
        );
        assert_eq!(
            seek(MediaControlEvent::Seek(SeekDirection::Forward)),
            Duration::from_secs(40)
        );
        assert_eq!(
            seek(MediaControlEvent::SetPosition(MediaPosition(
                Duration::from_secs(99)
            ))),
            Duration::from_secs(99)
        );
    }

    #[test]
    fn first_state_publishes_track_and_playback() {
        let ups = diff(
            None,
            &state(Status::Playing, Some("a")),
            Duration::from_secs(1),
        );
        assert!(
            matches!(&ups[0], Update::Track { title, artist, album: None, duration: Some(d), cover_url: Some(u) }
            if title == "title-a" && artist == "Artist" && *d == Duration::from_secs(200) && u.contains("w544-h544"))
        );
        assert_eq!(
            ups[1],
            Update::Playback {
                status: Status::Playing,
                position: Duration::from_secs(1)
            }
        );
    }

    #[test]
    fn only_real_changes_are_forwarded() {
        let a = state(Status::Playing, Some("a"));
        // identical state: nothing to tell the OS
        assert!(diff(Some(&a), &a, Duration::ZERO).is_empty());
        // pause: playback only
        let paused = state(Status::Paused, Some("a"));
        assert_eq!(
            diff(Some(&a), &paused, Duration::from_secs(9)),
            vec![Update::Playback {
                status: Status::Paused,
                position: Duration::from_secs(9)
            }]
        );
        // next track: metadata and playback
        let b = state(Status::Playing, Some("b"));
        let ups = diff(Some(&a), &b, Duration::ZERO);
        assert!(
            matches!(ups[0], Update::Track { .. }) && matches!(ups[1], Update::Playback { .. })
        );
        // queue cleared
        let none = state(Status::Idle, None);
        let ups = diff(Some(&a), &none, Duration::ZERO);
        assert_eq!(ups[0], Update::NoTrack);
    }
}
