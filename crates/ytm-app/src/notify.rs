//! Desktop notification when the song changes.

/// Remembers which track was last announced, so pausing/resuming or a state refresh of the same
/// track never repeats it.
#[derive(Debug, Default)]
pub struct Announcer {
    last: Option<String>,
}

impl Announcer {
    /// Feed the playing track (`None` when nothing is loaded). Returns true exactly once per track,
    /// and only once it is actually playing: loading a track that then fails would be noise.
    pub fn is_new(&mut self, video_id: Option<&str>, playing: bool) -> bool {
        match video_id {
            None => {
                self.last = None;
                false
            }
            Some(id) if self.last.as_deref() == Some(id) => false,
            Some(id) if playing => {
                self.last = Some(id.to_owned());
                true
            }
            Some(_) => false,
        }
    }
}

/// Mirrors `Config::notifications` for the watcher thread, which never touches `AppState`.
static ENABLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

pub fn set_enabled(on: bool) {
    ENABLED.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// macOS stops redrawing a window that is covered or minimized, and the UI only decides about
/// notifications while drawing, so there the decision is made here instead: on a task that follows
/// the player directly and asks the system whether another app is in front.
#[cfg(target_os = "macos")]
pub fn spawn_watcher(rt: &tokio::runtime::Handle, player: ytm_player::Player) {
    use tokio::sync::broadcast::error::RecvError;
    use ytm_player::Status;

    fn other_app_in_front() -> bool {
        let front = objc2_app_kit::NSWorkspace::sharedWorkspace().frontmostApplication();
        front.is_none_or(|app| app.processIdentifier() as u32 != std::process::id())
    }

    let mut events = player.subscribe();
    rt.spawn(async move {
        let mut announcer = Announcer::default();
        loop {
            match events.recv().await {
                Ok(_) | Err(RecvError::Lagged(_)) => {}
                Err(RecvError::Closed) => break,
            }
            let st = player.state();
            let track = st.current_track();
            let id = track.map(|t| t.video_id.as_str());
            if !announcer.is_new(id, st.status == Status::Playing) {
                continue;
            }
            let Some(track) = track else { continue };
            let enabled = ENABLED.load(std::sync::atomic::Ordering::Relaxed);
            let background = other_app_in_front();
            tracing::info!(enabled, background, "new track: notification decision");
            if enabled && background {
                song_changed(&track.title, &track.artist_line());
            }
        }
    });
}

/// "Title — Artist", or just the title.
pub fn summary_line(title: &str, artist: &str) -> String {
    if artist.is_empty() {
        title.to_owned()
    } else {
        format!("{title} — {artist}")
    }
}

/// Shows the notification without blocking the UI (the D-Bus / OS call can take a moment).
pub fn song_changed(title: &str, artist: &str) {
    let (title, artist) = (title.to_owned(), artist.to_owned());
    let spawned = std::thread::Builder::new()
        .name("notify".into())
        .spawn(move || {
            // macOS attributes a notification to a bundle id; without ours it is dropped.
            #[cfg(target_os = "macos")]
            {
                static BUNDLE: std::sync::Once = std::sync::Once::new();
                BUNDLE.call_once(|| {
                    if let Err(e) = notify_rust::set_application("dev.tunebox.Tunebox") {
                        tracing::warn!(error = %e, "could not set the notification bundle id");
                    }
                });
            }
            let shown = notify_rust::Notification::new()
                .appname("Tunebox")
                .summary(&title)
                .body(&artist)
                .icon("tunebox")
                .timeout(notify_rust::Timeout::Milliseconds(5000))
                .show();
            if let Err(e) = shown {
                tracing::warn!(error = %e, "could not show notification");
            }
        });
    if let Err(e) = spawned {
        tracing::debug!(error = %e, "could not start notification thread");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn announces_each_track_once_and_only_when_playing() {
        let mut a = Announcer::default();
        assert!(!a.is_new(None, false));
        assert!(!a.is_new(Some("a"), false), "still loading");
        assert!(a.is_new(Some("a"), true));
        assert!(!a.is_new(Some("a"), true), "same track again");
        assert!(!a.is_new(Some("a"), false), "paused");
        assert!(a.is_new(Some("b"), true));
        assert!(!a.is_new(None, false));
        assert!(
            a.is_new(Some("b"), true),
            "playing again after the queue was cleared"
        );
    }

    #[test]
    fn summary_joins_title_and_artist() {
        assert_eq!(summary_line("Song", "Band"), "Song — Band");
        assert_eq!(summary_line("Song", ""), "Song");
    }
}
