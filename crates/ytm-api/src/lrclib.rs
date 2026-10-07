//! Time-synced lyrics from LRCLIB (<https://lrclib.net>), a free, key-less lyrics database.
//! YouTube's own lyrics are plain text; this adds the timestamps. Only the song title, artist,
//! album and length are sent, and only when the user has synced lyrics switched on.

use serde_json::Value;

use crate::models::{LyricLine, Lyrics, Track};

pub const BASE: &str = "https://lrclib.net/api";
/// LRCLIB asks clients to identify themselves.
pub const USER_AGENT: &str = "Tunebox (https://github.com/tunebox)";
/// How far a candidate's length may differ from the playing track's, in seconds.
const DURATION_SLACK: f64 = 3.0;

/// Parses LRC text: `[mm:ss.xx] words`, one or more stamps per line; `[offset:±ms]` shifts all
/// lines; other `[tag:value]` headers are ignored. Lines come back sorted by time. Empty text is
/// kept (an instrumental gap), so the previous line does not stay highlighted through a solo.
pub fn parse_lrc(text: &str) -> Vec<LyricLine> {
    let mut offset: i64 = 0;
    let mut lines = Vec::new();
    for raw in text.lines() {
        let mut rest = raw.trim_start();
        let mut stamps = Vec::new();
        while let Some(body) = rest.strip_prefix('[') {
            let Some(end) = body.find(']') else { break };
            let tag = &body[..end];
            rest = body[end + 1..].trim_start();
            if let Some(ms) = parse_stamp(tag) {
                stamps.push(ms);
            } else if let Some(v) = tag.strip_prefix("offset:") {
                offset = v.trim().parse().unwrap_or(0);
            }
        }
        let words = rest.trim().to_owned();
        for ms in stamps {
            lines.push(LyricLine {
                time_ms: ms,
                text: words.clone(),
            });
        }
    }
    if offset != 0 {
        // LRC offsets are "positive = lyrics appear sooner"
        for l in &mut lines {
            l.time_ms = l.time_ms.saturating_add_signed(-offset);
        }
    }
    lines.sort_by_key(|l| l.time_ms);
    lines
}

/// `mm:ss`, `mm:ss.x`, `mm:ss.xx`, `mm:ss.xxx` → milliseconds. Not a stamp → `None`.
fn parse_stamp(tag: &str) -> Option<u64> {
    let (min, rest) = tag.split_once(':')?;
    let (sec, frac) = rest.split_once(['.', ':']).unwrap_or((rest, ""));
    if min.is_empty()
        || sec.is_empty()
        || ![min, sec, frac]
            .iter()
            .all(|p| p.bytes().all(|b| b.is_ascii_digit()))
    {
        return None;
    }
    let ms = match frac.len() {
        0 => 0,
        1 => frac.parse::<u64>().ok()? * 100,
        2 => frac.parse::<u64>().ok()? * 10,
        3 => frac.parse::<u64>().ok()?,
        _ => frac[..3].parse::<u64>().ok()?,
    };
    Some(min.parse::<u64>().ok()? * 60_000 + sec.parse::<u64>().ok()? * 1000 + ms)
}

/// Lyrics from one LRCLIB record (`/get` answer or one `/search` entry). `None` for instrumentals
/// and records with neither synced nor plain text.
pub fn parse_record(rec: &Value) -> Option<Lyrics> {
    if rec.get("instrumental").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    let text = |key: &str| {
        rec.get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
    };
    let synced = text("syncedLyrics").map(parse_lrc).unwrap_or_default();
    let plain = match text("plainLyrics") {
        Some(p) => p.to_owned(),
        // no plain text: the synced words without their stamps
        None => synced
            .iter()
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>()
            .join("\n"),
    };
    if synced.iter().all(|l| l.text.is_empty()) && plain.trim().is_empty() {
        return None;
    }
    let synced = if synced.iter().any(|l| !l.text.is_empty()) {
        synced
    } else {
        Vec::new()
    };
    Some(Lyrics {
        text: plain,
        source: Some("LRCLIB".into()),
        synced,
    })
}

/// The best `/search` entry: has synced lyrics and a length within a few seconds of `duration`
/// (the closest wins). Without a known length the first synced entry is taken.
pub fn pick_search_result(results: &Value, duration: Option<u32>) -> Option<&Value> {
    let synced = |r: &&Value| {
        r.get("syncedLyrics")
            .and_then(Value::as_str)
            .is_some_and(|s| !s.trim().is_empty())
    };
    let candidates = results.as_array()?.iter().filter(synced);
    let Some(want) = duration else {
        return candidates.into_iter().next();
    };
    let gap = |r: &&Value| {
        r.get("duration")
            .and_then(Value::as_f64)
            .map(|d| (d - f64::from(want)).abs())
            .unwrap_or(f64::MAX)
    };
    candidates
        .filter(|r| gap(r) <= DURATION_SLACK)
        .min_by(|a, b| gap(a).total_cmp(&gap(b)))
}

/// A title as a lyrics database knows it: without "(Official Video)", "[Remastered 2011]", … at
/// the end. Only the trailing bracketed groups with such words are dropped.
pub fn clean_title(title: &str) -> String {
    const NOISE: [&str; 9] = [
        "official",
        "video",
        "audio",
        "lyrics",
        "lyric",
        "visualizer",
        "remaster",
        "hd",
        "4k",
    ];
    let mut t = title.trim();
    while let Some(open) = t.rfind(['(', '[']) {
        let group = &t[open..];
        let closes = group.ends_with(')') || group.ends_with(']');
        let lower = group.to_lowercase();
        if closes && NOISE.iter().any(|w| lower.contains(w)) {
            t = t[..open].trim_end();
        } else {
            break;
        }
    }
    t.to_owned()
}

/// Query pairs for `track` (None when it has no artist to look up).
pub fn query_for(track: &Track, clean: bool) -> Option<Vec<(&'static str, String)>> {
    let artist = track.artists.first()?.name.clone();
    let title = if clean {
        clean_title(&track.title)
    } else {
        track.title.clone()
    };
    let mut q = vec![("track_name", title), ("artist_name", artist)];
    if let Some(a) = track.album.as_ref().filter(|a| !a.name.is_empty()) {
        q.push(("album_name", a.name.clone()));
    }
    Some(q)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture() -> Value {
        serde_json::from_str(include_str!("../tests/fixtures/lrclib_get.json")).unwrap()
    }

    #[test]
    fn real_response_parses_into_ordered_timed_lines() {
        let l = parse_record(&fixture()).expect("lyrics");
        assert_eq!(l.source.as_deref(), Some("LRCLIB"));
        assert!(l.synced.len() > 30);
        assert_eq!(l.synced[0].time_ms, 13_130);
        assert_eq!(l.synced[0].text, "Yeah");
        assert!(l.synced.windows(2).all(|w| w[0].time_ms <= w[1].time_ms));
        assert!(l.text.contains("I've been tryna call"));
    }

    #[test]
    fn lrc_stamps_offsets_and_junk() {
        let lines = parse_lrc(
            "[ar:Someone]\n[offset:+500]\n[00:01.5] one\n[01:02.25][01:10] two\n\nnot a line\n[00:05]\n[x:y] z",
        );
        let got: Vec<_> = lines.iter().map(|l| (l.time_ms, l.text.as_str())).collect();
        assert_eq!(
            got,
            [(1000, "one"), (4500, ""), (61_750, "two"), (69_500, "two"),]
        );
        assert!(parse_lrc("").is_empty());
        assert_eq!(parse_lrc("[00:00.123] a")[0].time_ms, 123);
        assert_eq!(
            parse_lrc("[10:00:50] a")[0].time_ms,
            600_500,
            "colon fraction"
        );
    }

    #[test]
    fn instrumentals_and_empty_records_have_no_lyrics() {
        assert!(parse_record(&json!({"instrumental": true, "plainLyrics": "x"})).is_none());
        assert!(parse_record(&json!({})).is_none());
        assert!(parse_record(&json!({"plainLyrics": "  ", "syncedLyrics": ""})).is_none());
        let plain = parse_record(&json!({"plainLyrics": "la la"})).unwrap();
        assert!(plain.synced.is_empty() && plain.text == "la la");
        let only_synced =
            parse_record(&json!({"syncedLyrics": "[00:01.00] hey\n[00:02.00] you"})).unwrap();
        assert_eq!(only_synced.text, "hey\nyou");
    }

    #[test]
    fn search_picks_the_closest_synced_length() {
        let results = json!([
            {"duration": 248.0, "syncedLyrics": "[00:01.00] a"},
            {"duration": 202.0, "syncedLyrics": null},
            {"duration": 203.0, "syncedLyrics": "[00:01.00] b"},
            {"duration": 201.0, "syncedLyrics": "[00:01.00] c"},
        ]);
        let pick = pick_search_result(&results, Some(201)).unwrap();
        assert_eq!(pick["syncedLyrics"], "[00:01.00] c");
        assert!(
            pick_search_result(&results, Some(100)).is_none(),
            "wrong song length"
        );
        assert_eq!(
            pick_search_result(&results, None).unwrap()["syncedLyrics"],
            "[00:01.00] a"
        );
        assert!(pick_search_result(&json!({}), Some(1)).is_none());
    }

    #[test]
    fn titles_lose_trailing_video_noise_only() {
        assert_eq!(
            clean_title("Blinding Lights (Official Video)"),
            "Blinding Lights"
        );
        assert_eq!(clean_title("Song [Remastered 2011] (Lyrics)"), "Song");
        assert_eq!(clean_title("Song (feat. Someone)"), "Song (feat. Someone)");
        assert_eq!(
            clean_title("(Don't Fear) The Reaper"),
            "(Don't Fear) The Reaper"
        );
    }

    #[test]
    fn current_line_follows_the_position() {
        let l = parse_record(&json!({"syncedLyrics": "[00:10.00] a\n[00:20.00]\n[00:30.00] c"}))
            .unwrap();
        assert_eq!(l.current_line(0), None, "before the first line");
        assert_eq!(l.current_line(9_999), None);
        assert_eq!(l.current_line(10_000), Some(0));
        assert_eq!(
            l.current_line(25_000),
            Some(1),
            "an empty gap line is current too"
        );
        assert_eq!(l.current_line(999_999), Some(2));
        assert_eq!(Lyrics::default().current_line(5), None);
    }

    #[test]
    fn query_needs_an_artist() {
        let mut t = Track {
            title: "T (Official Video)".into(),
            ..Track::default()
        };
        assert!(query_for(&t, true).is_none());
        t.artists = vec![crate::ArtistRef {
            name: "A".into(),
            id: None,
        }];
        let q = query_for(&t, true).unwrap();
        assert_eq!(q[0], ("track_name", "T".to_owned()));
        assert_eq!(q.len(), 2);
    }
}
