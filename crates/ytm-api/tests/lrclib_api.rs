//! `synced_lyrics` against a local fake LRCLIB: request shape and the get → search fallback.

use std::sync::{Arc, Mutex};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use ytm_api::{AlbumRef, ArtistRef, InnerTube, MusicApi, Track};

const RECORD: &str = include_str!("fixtures/lrclib_get.json");

/// Serves `routes` (path → (status, body)) and records every request target.
async fn serve(routes: Vec<(&'static str, u16, String)>) -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            let mut buf = vec![0u8; 8192];
            let n = sock.read(&mut buf).await.unwrap_or(0);
            let head = String::from_utf8_lossy(&buf[..n]).to_string();
            let target = head
                .lines()
                .next()
                .and_then(|l| l.split(' ').nth(1))
                .unwrap_or("")
                .to_owned();
            log.lock().unwrap().push(target.clone());
            let path = target.split('?').next().unwrap_or("");
            let (status, body) = routes
                .iter()
                .find(|(p, _, _)| *p == path)
                .map_or((404, String::new()), |(_, s, b)| (*s, b.clone()));
            let reply = format!(
                "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = sock.write_all(reply.as_bytes()).await;
        }
    });
    (base, seen)
}

fn track() -> Track {
    Track {
        video_id: "v".into(),
        title: "Blinding Lights (Official Video)".into(),
        artists: vec![ArtistRef {
            name: "The Weeknd".into(),
            id: None,
        }],
        album: Some(AlbumRef {
            name: "After Hours".into(),
            id: None,
        }),
        duration_secs: Some(200),
        ..Track::default()
    }
}

#[tokio::test]
async fn exact_match_is_used_and_sends_title_artist_album_duration() {
    let (base, seen) = serve(vec![("/get", 200, RECORD.to_owned())]).await;
    let api = InnerTube::new("en").unwrap().with_lrclib_url(base);
    let l = api.synced_lyrics(&track()).await.unwrap().expect("lyrics");
    assert!(l.synced.len() > 30);
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 1);
    for part in [
        "track_name=Blinding+Lights+%28Official+Video%29",
        "artist_name=The+Weeknd",
        "album_name=After+Hours",
        "duration=200",
    ] {
        assert!(seen[0].contains(part), "{part} missing in {}", seen[0]);
    }
}

#[tokio::test]
async fn falls_back_to_a_search_with_the_cleaned_title() {
    let results = format!("[{RECORD}]");
    let (base, seen) = serve(vec![("/search", 200, results)]).await;
    let api = InnerTube::new("en").unwrap().with_lrclib_url(base);
    let l = api.synced_lyrics(&track()).await.unwrap().expect("lyrics");
    assert!(!l.synced.is_empty());
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 2, "get (404) then search: {seen:?}");
    assert!(seen[1].starts_with("/search?"));
    assert!(
        seen[1].contains("track_name=Blinding+Lights&"),
        "{}",
        seen[1]
    );
    assert!(!seen[1].contains("album_name"));
}

#[tokio::test]
async fn nothing_found_and_server_errors() {
    let (base, _) = serve(vec![]).await;
    let api = InnerTube::new("en").unwrap().with_lrclib_url(base);
    assert!(api.synced_lyrics(&track()).await.unwrap().is_none());

    let (base, _) = serve(vec![("/get", 500, "oops".into())]).await;
    let api = InnerTube::new("en").unwrap().with_lrclib_url(base);
    assert!(
        api.synced_lyrics(&track()).await.is_err(),
        "5xx is an error, not 'no lyrics'"
    );

    // no artist: nothing to look up, and no request is made
    let (base, seen) = serve(vec![]).await;
    let api = InnerTube::new("en").unwrap().with_lrclib_url(base);
    let t = Track {
        artists: vec![],
        ..track()
    };
    assert!(api.synced_lyrics(&t).await.unwrap().is_none());
    assert!(seen.lock().unwrap().is_empty());
}
