//! Listening history: one line of JSON per counted play in `history.jsonl` (next to `library.json`,
//! so "Change folder…" carries it along). Append-only, so recording a play never rewrites the file;
//! it is trimmed to the newest [`KEEP`] plays when it is loaded. Stats are computed from memory.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use ytm_api::{ArtistRef, Track};

/// Plays kept (in memory and, after trimming, on disk).
pub const KEEP: usize = 20_000;
/// A play counts once the track has been heard for this long, or for half its length if shorter.
const COUNT_AFTER: Duration = Duration::from_secs(30);
const DAY: u64 = 86_400;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Play {
    /// Unix seconds when the play was counted.
    pub t: u64,
    /// Length of the track in seconds (0 = unknown): what the play adds to "listening time".
    pub secs: u32,
    pub track: Track,
}

pub fn file_for(library_file: &Path) -> PathBuf {
    library_file.with_file_name("history.jsonl")
}

pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

#[derive(Debug, Default)]
pub struct History {
    /// Oldest first.
    pub plays: Vec<Play>,
}

/// Which plays the stats cover.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Period {
    Week,
    #[default]
    Month,
    All,
}

impl Period {
    fn since(self, now: u64) -> u64 {
        match self {
            Self::Week => now.saturating_sub(7 * DAY),
            Self::Month => now.saturating_sub(30 * DAY),
            Self::All => 0,
        }
    }
}

#[derive(Debug, Default, PartialEq)]
pub struct Stats {
    pub plays: usize,
    pub listened_secs: u64,
    pub unique_tracks: usize,
    pub unique_artists: usize,
    pub top_tracks: Vec<(Track, u32)>,
    pub top_artists: Vec<(ArtistRef, u32)>,
    /// Plays per day for the last 14 days, oldest first (today last).
    pub per_day: [u32; 14],
}

impl History {
    /// Reads `path`; a missing file is an empty history, unreadable lines are skipped. A file with
    /// more than [`KEEP`] plays is rewritten with the newest ones.
    pub fn load(path: &Path) -> Self {
        let Ok(text) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        let mut plays: Vec<Play> = text
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect();
        if plays.len() > KEEP {
            plays.drain(..plays.len() - KEEP);
            let body: String = plays.iter().filter_map(line).collect();
            let tmp = path.with_extension("jsonl.tmp");
            if std::fs::write(&tmp, body).is_ok() {
                let _ = std::fs::rename(&tmp, path);
            }
        }
        Self { plays }
    }

    pub fn is_empty(&self) -> bool {
        self.plays.is_empty()
    }

    /// Adds a play and returns the line to append to the file.
    pub fn record(&mut self, play: Play) -> Option<String> {
        let text = line(&play);
        self.plays.push(play);
        if self.plays.len() > KEEP + KEEP / 4 {
            self.plays.drain(..KEEP / 4);
        }
        text
    }

    pub fn clear(&mut self) {
        self.plays.clear();
    }

    /// Distinct tracks, most recently played first.
    pub fn recent_tracks(&self, limit: usize) -> Vec<Track> {
        let mut seen = std::collections::HashSet::new();
        self.plays
            .iter()
            .rev()
            .filter(|p| seen.insert(p.track.video_id.as_str()))
            .take(limit)
            .map(|p| p.track.clone())
            .collect()
    }

    /// Play log, newest first, for the "Recently played" list.
    pub fn newest_first(&self) -> impl Iterator<Item = &Play> {
        self.plays.iter().rev()
    }

    pub fn stats(&self, period: Period, now: u64, top: usize) -> Stats {
        let since = period.since(now);
        let mut tracks: HashMap<&str, (&Track, u32)> = HashMap::new();
        let mut artists: HashMap<String, (&ArtistRef, u32)> = HashMap::new();
        let mut s = Stats::default();
        let today = now / DAY;
        for p in self.plays.iter().filter(|p| p.t >= since) {
            s.plays += 1;
            s.listened_secs += u64::from(p.secs);
            tracks.entry(&p.track.video_id).or_insert((&p.track, 0)).1 += 1;
            for a in &p.track.artists {
                // an artist without an id is still told apart by name
                let key = a.id.clone().unwrap_or_else(|| a.name.to_lowercase());
                artists.entry(key).or_insert((a, 0)).1 += 1;
            }
            if let Some(back) = today.checked_sub(p.t / DAY).filter(|b| *b < 14) {
                s.per_day[13 - back as usize] += 1;
            }
        }
        s.unique_tracks = tracks.len();
        s.unique_artists = artists.len();
        s.top_tracks = ranked(
            tracks.into_values().map(|(t, n)| (t.clone(), n)),
            top,
            |t| format!("{}\0{}", t.title, t.video_id),
        );
        s.top_artists = ranked(
            artists.into_values().map(|(a, n)| (a.clone(), n)),
            top,
            |a| a.name.clone(),
        );
        s
    }
}

/// Highest count first; ties keep a stable order (by name) so the page does not shuffle.
fn ranked<T>(
    items: impl Iterator<Item = (T, u32)>,
    top: usize,
    name: impl Fn(&T) -> String,
) -> Vec<(T, u32)> {
    let mut v: Vec<_> = items.collect();
    v.sort_by_cached_key(|(t, n)| (std::cmp::Reverse(*n), name(t)));
    v.truncate(top);
    v
}

fn line(play: &Play) -> Option<String> {
    let mut s = serde_json::to_string(play).ok()?;
    s.push('\n');
    Some(s)
}

/// Appends one line to the history file (creating it).
pub fn append(path: &Path, text: &str) -> std::io::Result<()> {
    use std::io::Write;
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?
        .write_all(text.as_bytes())
}

/// Measures how long the current track has really been heard and says when it counts as a play.
#[derive(Debug, Default)]
pub struct PlayTimer {
    video_id: String,
    listened: Duration,
    last: Option<Instant>,
    counted: bool,
}

impl PlayTimer {
    /// Call once per frame. `playing` is whether audio is running for `track`. Returns the track
    /// when this call is the one that makes it count.
    pub fn tick(&mut self, now: Instant, track: Option<&Track>, playing: bool) -> Option<Track> {
        let Some(track) = track else {
            *self = Self::default();
            return None;
        };
        if self.video_id != track.video_id {
            *self = Self {
                video_id: track.video_id.clone(),
                ..Self::default()
            };
        }
        let last = self.last.replace(now);
        if !playing {
            self.last = None;
            return None;
        }
        // Frames are sparse while the window is idle; a long gap is a sleep, not listening.
        if let Some(last) = last {
            self.listened += now
                .saturating_duration_since(last)
                .min(Duration::from_secs(1));
        }
        let length = track
            .duration_secs
            .map(|d| Duration::from_secs(u64::from(d)));
        if self.counted {
            // repeat-one: the track went round again
            if length.is_some_and(|l| l > Duration::ZERO && self.listened >= l) {
                self.listened = Duration::ZERO;
                self.counted = false;
            }
            return None;
        }
        let needed = length.map_or(COUNT_AFTER, |l| COUNT_AFTER.min(l / 2));
        if self.listened >= needed {
            self.counted = true;
            return Some(track.clone());
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(id: &str, artist: &str, secs: u32) -> Track {
        Track {
            video_id: id.into(),
            title: format!("song {id}"),
            artists: vec![ArtistRef {
                name: artist.into(),
                id: Some(format!("UC{artist}")),
            }],
            duration_secs: Some(secs),
            ..Track::default()
        }
    }

    fn play(t: u64, id: &str, artist: &str) -> Play {
        Play {
            t,
            secs: 200,
            track: track(id, artist, 200),
        }
    }

    #[test]
    fn stats_rank_and_window() {
        let now = 100 * DAY;
        let mut h = History::default();
        h.record(play(now - 40 * DAY, "old", "A")); // outside the month
        h.record(play(now - 3 * DAY, "x", "A"));
        h.record(play(now - 2 * DAY, "x", "A"));
        h.record(play(now - DAY, "y", "B"));
        h.record(play(now, "z", "B"));
        h.record(play(now, "z", "B"));
        h.record(play(now, "z", "B"));

        let month = h.stats(Period::Month, now, 5);
        assert_eq!(month.plays, 6);
        assert_eq!(month.listened_secs, 1200);
        assert_eq!(month.unique_tracks, 3);
        assert_eq!(month.unique_artists, 2);
        assert_eq!(month.top_tracks[0].0.video_id, "z");
        assert_eq!(month.top_tracks[0].1, 3);
        assert_eq!(month.top_artists[0].0.name, "B");
        assert_eq!(month.per_day[13], 3, "today is last");
        assert_eq!(month.per_day[12], 1);
        assert_eq!(month.per_day[10], 1);

        assert_eq!(h.stats(Period::All, now, 5).plays, 7);
        assert_eq!(h.stats(Period::Week, now, 5).plays, 6);
        assert_eq!(h.stats(Period::Month, now, 1).top_tracks.len(), 1);
    }

    #[test]
    fn recent_tracks_are_distinct_newest_first() {
        let mut h = History::default();
        for (t, id) in [(1, "a"), (2, "b"), (3, "a"), (4, "c")] {
            h.record(play(t, id, "A"));
        }
        let ids: Vec<_> = h
            .recent_tracks(10)
            .into_iter()
            .map(|t| t.video_id)
            .collect();
        assert_eq!(ids, ["c", "a", "b"]);
        assert_eq!(h.recent_tracks(2).len(), 2);
    }

    #[test]
    fn file_round_trip_skips_bad_lines_and_trims() {
        let dir = std::env::temp_dir().join(format!("tunebox-history-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("history.jsonl");
        let _ = std::fs::remove_file(&path);
        assert!(History::load(&path).is_empty(), "missing file");
        let mut h = History::default();
        for i in 0..3 {
            let l = h.record(play(i, "a", "A")).unwrap();
            append(&path, &l).unwrap();
        }
        append(&path, "not json\n").unwrap();
        let back = History::load(&path);
        assert_eq!(back.plays, h.plays);

        // over the cap: trimmed on load, newest kept
        let mut body = String::new();
        for i in 0..(KEEP as u64 + 5) {
            body.push_str(&line(&play(i, "a", "A")).unwrap());
        }
        std::fs::write(&path, body).unwrap();
        let trimmed = History::load(&path);
        assert_eq!(trimmed.plays.len(), KEEP);
        assert_eq!(trimmed.plays[0].t, 5);
        assert_eq!(History::load(&path).plays.len(), KEEP, "file was rewritten");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_play_counts_after_thirty_seconds_or_half_a_short_track() {
        let t0 = Instant::now();
        let long = track("long", "A", 200);
        let mut timer = PlayTimer::default();
        let mut counted = None;
        for s in 0..=40 {
            if let Some(t) = timer.tick(t0 + Duration::from_secs(s), Some(&long), true) {
                counted = Some((s, t));
            }
        }
        let (at, t) = counted.expect("counted");
        assert_eq!((at, t.video_id.as_str()), (30, "long"));

        // a 20 s jingle counts at 10 s, and only once
        let short = track("short", "A", 20);
        let mut timer = PlayTimer::default();
        let hits: Vec<u64> = (0..=19)
            .filter(|s| {
                timer
                    .tick(t0 + Duration::from_secs(*s), Some(&short), true)
                    .is_some()
            })
            .collect();
        assert_eq!(hits, [10]);
    }

    #[test]
    fn pausing_and_gaps_do_not_count_and_a_new_track_resets() {
        let t0 = Instant::now();
        let a = track("a", "A", 200);
        let mut timer = PlayTimer::default();
        // 25 s of listening, then paused for ages, then the window sleeps for a minute
        for s in 0..25 {
            assert!(timer
                .tick(t0 + Duration::from_secs(s), Some(&a), true)
                .is_none());
        }
        assert!(timer
            .tick(t0 + Duration::from_secs(5000), Some(&a), false)
            .is_none());
        assert!(timer
            .tick(t0 + Duration::from_secs(5060), Some(&a), true)
            .is_none());
        // 1 s credited at most for the 60 s gap: still short of 30 s
        assert!(timer
            .tick(t0 + Duration::from_secs(5061), Some(&a), true)
            .is_none());
        // another track starts from zero
        let b = track("b", "A", 200);
        assert!(timer
            .tick(t0 + Duration::from_secs(5062), Some(&b), true)
            .is_none());
        assert!(timer
            .tick(t0 + Duration::from_secs(5070), Some(&b), true)
            .is_none());
        // no track at all
        assert!(timer
            .tick(t0 + Duration::from_secs(5071), None, true)
            .is_none());
    }

    #[test]
    fn repeat_one_counts_each_lap() {
        let t0 = Instant::now();
        let a = track("a", "A", 40);
        let mut timer = PlayTimer::default();
        let hits = (0..=100)
            .filter(|s| {
                timer
                    .tick(t0 + Duration::from_secs(*s), Some(&a), true)
                    .is_some()
            })
            .count();
        assert!(hits >= 2, "got {hits}");
    }
}
