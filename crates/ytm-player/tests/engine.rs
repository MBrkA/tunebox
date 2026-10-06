//! End-to-end engine tests: mock resolver + local range server + WAV audio,
//! silent null output. No network, no sound device.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use ytm_api::{ApiError, StreamInfo, StreamResolver, Track};
use ytm_player::{Command, OutputKind, Player, PlayerOptions, Repeat, Status};

/// 16-bit stereo 44.1 kHz WAV containing `secs` of a quiet tone.
fn wav(secs: f32) -> Vec<u8> {
    let frames = (44_100.0 * secs) as usize;
    let mut pcm = Vec::with_capacity(frames * 4);
    for i in 0..frames {
        let s = ((i as f32 * 0.05).sin() * 3000.0) as i16;
        pcm.extend(s.to_le_bytes());
        pcm.extend(s.to_le_bytes());
    }
    let mut out = b"RIFF".to_vec();
    out.extend((36 + pcm.len() as u32).to_le_bytes());
    out.extend(b"WAVEfmt ");
    out.extend(16u32.to_le_bytes());
    out.extend(1u16.to_le_bytes()); // PCM
    out.extend(2u16.to_le_bytes());
    out.extend(44_100u32.to_le_bytes());
    out.extend((44_100u32 * 4).to_le_bytes());
    out.extend(4u16.to_le_bytes());
    out.extend(16u16.to_le_bytes());
    out.extend(b"data");
    out.extend((pcm.len() as u32).to_le_bytes());
    out.extend(pcm);
    out
}

/// Serves `/<name>` as a WAV of the length encoded in the name (`/t2.5` = 2.5 s);
/// `/bad` answers 403.
fn serve() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            std::thread::spawn(move || {
                let mut stream = stream;
                let mut rd = BufReader::new(stream.try_clone().unwrap());
                let mut first = String::new();
                rd.read_line(&mut first).unwrap();
                let path = first.split_whitespace().nth(1).unwrap_or("/").to_owned();
                let mut range = None;
                loop {
                    let mut l = String::new();
                    if rd.read_line(&mut l).unwrap_or(0) == 0 || l == "\r\n" {
                        break;
                    }
                    if let Some(r) = l.to_lowercase().strip_prefix("range: bytes=") {
                        let (a, b) = r.trim().split_once('-').unwrap();
                        range = Some((a.parse::<usize>().unwrap(), b.parse::<usize>().unwrap()));
                    }
                }
                if path.starts_with("/bad") {
                    let _ = stream.write_all(
                        b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    );
                    return;
                }
                let secs: f32 = path.trim_start_matches("/t").parse().unwrap_or(1.0);
                let data = wav(secs);
                let (a, b) = range.unwrap_or((0, data.len() - 1));
                let b = b.min(data.len() - 1);
                let body = &data[a..=b];
                let head = format!(
                    "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {a}-{b}/{}\r\nConnection: close\r\n\r\n",
                    body.len(),
                    data.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(body);
            });
        }
    });
    base
}

struct Mock {
    base: String,
}

#[async_trait]
impl StreamResolver for Mock {
    async fn resolve(&self, video_id: &str) -> Result<StreamInfo, ApiError> {
        // video ids look like "t2.5" or "bad1"
        if video_id.starts_with("bad") {
            return Err(ApiError::Unplayable("nope".into()));
        }
        let secs: f32 = video_id.trim_start_matches('t').parse().unwrap_or(1.0);
        Ok(StreamInfo {
            url: format!("{}/{video_id}", self.base),
            itag: 0,
            mime_type: "audio/wav".into(),
            content_length: Some(wav(secs).len() as u64),
            duration_ms: Some((secs * 1000.0) as u64),
            user_agent: "test".into(),
            expires_in_secs: None,
            loudness_db: None,
        })
    }
}

fn track(id: &str) -> Track {
    Track {
        video_id: id.into(),
        title: id.into(),
        ..Track::default()
    }
}

fn player() -> Player {
    let resolver = Arc::new(Mock { base: serve() });
    Player::spawn(
        &tokio::runtime::Handle::current(),
        resolver,
        PlayerOptions {
            output: OutputKind::Null,
            buffer_secs: 1.0,
            ..PlayerOptions::default()
        },
    )
    .unwrap()
}

async fn wait_for(what: &str, timeout: Duration, mut cond: impl FnMut() -> bool) {
    let start = Instant::now();
    while !cond() {
        assert!(start.elapsed() < timeout, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn current_title(p: &Player) -> Option<String> {
    p.state().current_track().map(|t| t.title.clone())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn plays_and_advances_gaplessly_then_goes_idle() {
    let p = player();
    p.send(Command::Play {
        tracks: vec![track("t1.0"), track("t1.0"), track("t1.0")],
        start: 0,
    });
    wait_for("playing", Duration::from_secs(5), || {
        p.state().status == Status::Playing
    })
    .await;
    let mut seen_loading_after_start = false;
    let mut indices = vec![p.state().current.unwrap()];
    let start = Instant::now();
    while p.state().status != Status::Idle {
        assert!(start.elapsed() < Duration::from_secs(8), "never finished");
        let st = p.state();
        if st.status == Status::Loading {
            seen_loading_after_start = true;
        }
        if indices.last() != st.current.as_ref() {
            indices.push(st.current.unwrap());
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(indices, vec![0, 1, 2]);
    assert!(
        !seen_loading_after_start,
        "prefetch should make transitions gapless"
    );
    assert_eq!(p.position(), Duration::ZERO);
    assert_eq!(p.state().current, Some(2), "stays on the last track");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn seek_pause_and_volume() {
    let p = player();
    p.send(Command::Play {
        tracks: vec![track("t6.0")],
        start: 0,
    });
    wait_for("playing", Duration::from_secs(5), || {
        p.state().status == Status::Playing
    })
    .await;

    p.send(Command::Seek(Duration::from_secs(4)));
    wait_for("seek applied", Duration::from_secs(5), || {
        p.state().status == Status::Playing && p.position() >= Duration::from_millis(3900)
    })
    .await;
    assert!(p.position() < Duration::from_secs(5), "{:?}", p.position());

    p.send(Command::Pause);
    wait_for("paused", Duration::from_secs(2), || {
        p.state().status == Status::Paused
    })
    .await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    let frozen = p.position();
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(
        p.position(),
        frozen,
        "position must not advance while paused"
    );
    p.send(Command::Resume);
    wait_for("resumed", Duration::from_secs(2), || {
        p.state().status == Status::Playing
    })
    .await;

    p.send(Command::SetVolume(0.25));
    wait_for("volume", Duration::from_secs(2), || {
        (p.state().volume - 0.25).abs() < 1e-6
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn skip_prev_next_and_jump() {
    let p = player();
    p.send(Command::Play {
        tracks: vec![track("t30"), track("t31"), track("t32")],
        start: 0,
    });
    wait_for("t30", Duration::from_secs(5), || {
        p.state().status == Status::Playing
    })
    .await;
    p.send(Command::Next);
    wait_for("t31", Duration::from_secs(5), || {
        current_title(&p).as_deref() == Some("t31") && p.state().status == Status::Playing
    })
    .await;
    p.send(Command::Jump(2));
    wait_for("t32", Duration::from_secs(5), || {
        current_title(&p).as_deref() == Some("t32") && p.state().status == Status::Playing
    })
    .await;
    // Within the first 3 s, "previous" goes to the previous track
    p.send(Command::Prev);
    wait_for("back to t31", Duration::from_secs(5), || {
        current_title(&p).as_deref() == Some("t31")
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn repeat_one_loops_and_shuffle_keeps_current() {
    let p = player();
    p.send(Command::SetRepeat(Repeat::One));
    p.send(Command::Play {
        tracks: vec![track("t0.8"), track("t0.9")],
        start: 0,
    });
    wait_for("playing", Duration::from_secs(5), || {
        p.state().status == Status::Playing
    })
    .await;
    // Longer than the track: still on it, still playing.
    tokio::time::sleep(Duration::from_millis(2500)).await;
    assert_eq!(current_title(&p).as_deref(), Some("t0.8"));
    assert_eq!(p.state().status, Status::Playing);
    assert_eq!(p.state().repeat, Repeat::One);

    p.send(Command::SetShuffle(true));
    wait_for("shuffle", Duration::from_secs(2), || p.state().shuffle).await;
    assert_eq!(current_title(&p).as_deref(), Some("t0.8"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unplayable_tracks_are_skipped_and_reported() {
    let p = player();
    let mut events = p.subscribe();
    p.send(Command::Play {
        tracks: vec![track("bad1"), track("bad2"), track("t3.0")],
        start: 0,
    });
    wait_for("skipped to playable", Duration::from_secs(8), || {
        current_title(&p).as_deref() == Some("t3.0") && p.state().status == Status::Playing
    })
    .await;
    let mut errors = 0;
    while let Ok(ev) = events.try_recv() {
        if matches!(ev, ytm_player::PlayerEvent::Error(_)) {
            errors += 1;
        }
    }
    assert_eq!(errors, 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn all_tracks_failing_ends_idle_without_looping() {
    let p = player();
    p.send(Command::Play {
        tracks: vec![track("bad1"), track("bad2")],
        start: 0,
    });
    wait_for("idle with error", Duration::from_secs(8), || {
        let st = p.state();
        st.status == Status::Idle && st.error.is_some()
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn queue_edits_while_playing() {
    let p = player();
    p.send(Command::Play {
        tracks: vec![track("t20"), track("t21")],
        start: 0,
    });
    wait_for("playing", Duration::from_secs(5), || {
        p.state().status == Status::Playing
    })
    .await;
    p.send(Command::PlayNext(track("t22")));
    p.send(Command::Enqueue(vec![track("t23")]));
    wait_for("queue grew", Duration::from_secs(2), || {
        p.state().tracks.len() == 4
    })
    .await;
    let st = p.state();
    assert_eq!(st.upcoming.len(), 3);
    assert_eq!(
        st.tracks[st.upcoming[0]].title, "t22",
        "play-next goes first"
    );

    // removing the playing track moves on to its successor
    p.send(Command::Remove(0));
    wait_for("successor", Duration::from_secs(5), || {
        current_title(&p).as_deref() == Some("t22") && p.state().status == Status::Playing
    })
    .await;
    p.send(Command::Clear);
    wait_for("cleared", Duration::from_secs(2), || {
        let s = p.state();
        s.tracks.is_empty() && s.status == Status::Idle
    })
    .await;
}
