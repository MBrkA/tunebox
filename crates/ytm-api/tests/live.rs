//! Hits the real service. Run with `cargo test -p ytm-api --features live-tests`.
#![cfg(feature = "live-tests")]

use std::sync::Arc;

use ytm_api::*;

#[tokio::test]
async fn search_and_resolve_stream() {
    let tube = Arc::new(InnerTube::new("en").unwrap());
    let page = tube
        .search("daft punk one more time", SearchFilter::Songs)
        .await
        .unwrap();
    let track = page
        .items
        .iter()
        .find_map(|i| {
            if let SearchItem::Track(t) = i {
                Some(t)
            } else {
                None
            }
        })
        .expect("at least one track");

    let resolver = InnerTubeResolver::new(tube.clone(), vec![140]);
    let info = resolver.resolve(&track.video_id).await.unwrap();
    assert_eq!(info.itag, 140);

    // The URL must really serve audio past the first MiB (the PO-token gate).
    let resp = reqwest::Client::new()
        .get(&info.url)
        .header("User-Agent", &info.user_agent)
        .header("Range", "bytes=2097152-2162687")
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status().as_u16(),
        206,
        "range past 2 MiB for {} ({:?} bytes)",
        track.video_id,
        info.content_length
    );
}

fn client() -> InnerTube {
    InnerTube::new("en").unwrap()
}

#[tokio::test]
async fn browse_pages() {
    let tube = client();
    let home = tube.home().await.unwrap();
    assert!(!home.sections.is_empty());
    if let Some(token) = &home.continuation {
        // Anonymous Home returns no extra sections; it must simply not error.
        tube.home_continue(token).await.unwrap();
    }
    let explore = tube.explore().await.unwrap();
    assert!(explore.sections.iter().any(|s| s.title.contains("albums")));

    let artist = tube.artist("UCRr1xG_2WIDs18a6cIiCxeA").await.unwrap();
    assert_eq!(artist.name, "Daft Punk");
    assert!(!artist.top_songs.is_empty());

    let album = tube.album("MPREb_7ltM34kr0mH").await.unwrap();
    assert_eq!(album.title, "Discovery");
    assert!(album.tracks.len() >= 10);

    let playlist = tube
        .playlist("PLSdoVPM5WnnfbGVqQTCXjRnZd8hYLY0Cd")
        .await
        .unwrap();
    assert!(!playlist.tracks.is_empty());
}

#[tokio::test]
async fn up_next_and_lyrics() {
    let tube = client();
    let next = tube.up_next("khnokW3Mw24").await.unwrap();
    assert!(next.tracks.len() > 5);
    if let Some(token) = &next.continuation {
        // radio continuation is best-effort; it must at least not error
        let _ = tube.search_continue(token).await;
    }
    let lyrics = tube.lyrics("4D7u5KF7SP8").await.unwrap();
    assert!(lyrics.is_some_and(|l| l.text.len() > 100));
    // instrumental / no-lyrics tracks must be Ok(None), not an error
    let _ = tube.lyrics("wU26xVT_vBU").await.unwrap();
}

#[tokio::test]
async fn moods_and_genres() {
    let tube = client();
    let explore = tube.explore().await.unwrap();
    assert!(explore.moods.len() >= 10, "Explore lists mood shortcuts");

    let page = tube.moods_and_genres().await.unwrap();
    assert!(page.groups.iter().any(|g| g.title.contains("Genres")));
    let jazz_or_first = page
        .groups
        .iter()
        .flat_map(|g| &g.categories)
        .next()
        .expect("at least one category");

    let category = tube.mood_category(&jazz_or_first.params).await.unwrap();
    assert!(!category.title.is_empty());
    assert!(category.sections.iter().any(|s| !s.items.is_empty()));
}

#[tokio::test]
async fn new_releases_and_charts() {
    let tube = client();
    let releases = tube.new_releases().await.unwrap();
    assert!(releases.sections.iter().any(|s| !s.items.is_empty()));

    let default = tube.charts(None).await.unwrap();
    assert!(default.countries.len() > 30);
    assert!(
        default.country.is_some(),
        "YouTube reports which country it picked"
    );

    // Choosing a country changes which country the page is for.
    let global = tube.charts(Some("ZZ")).await.unwrap();
    assert_eq!(global.country.as_ref().map(|c| c.code.as_str()), Some("ZZ"));
    let us = tube.charts(Some("US")).await.unwrap();
    assert_eq!(us.country.as_ref().map(|c| c.code.as_str()), Some("US"));
}
