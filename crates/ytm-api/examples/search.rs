//! `cargo run -p ytm-api --example search -- "daft punk one more time"`
//! Prints search results and the resolved stream URL of the first track.

use std::sync::Arc;

use ytm_api::{ChainResolver, InnerTube, MusicApi, SearchFilter, SearchItem, StreamResolver};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    ytm_core::logging::init();
    let query = std::env::args().skip(1).collect::<Vec<_>>().join(" ");
    anyhow::ensure!(!query.is_empty(), "usage: search <query>");
    let cfg = ytm_core::Config::load()?;
    let tube = Arc::new(InnerTube::from_config(&cfg)?);

    println!("suggestions: {:?}", tube.search_suggestions(&query).await?);
    let page = tube.search(&query, SearchFilter::All).await?;
    let mut first_track = None;
    for item in &page.items {
        match item {
            SearchItem::Track(t) => {
                println!(
                    "[track]    {} — {} ({:?}s) {}",
                    t.title,
                    t.artist_line(),
                    t.duration_secs,
                    t.video_id
                );
                first_track.get_or_insert(t);
            }
            SearchItem::Album(a) => println!("[album]    {} ({}) {}", a.title, a.kind, a.browse_id),
            SearchItem::Artist(a) => println!("[artist]   {} {}", a.name, a.browse_id),
            SearchItem::Playlist(p) => println!("[playlist] {} {}", p.title, p.playlist_id),
        }
    }
    if let Some(t) = first_track {
        let info = ChainResolver::standard(tube.clone(), &cfg)
            .resolve(&t.video_id)
            .await?;
        println!(
            "\nstream for {:?}: itag {} {} {:?} bytes\n{}",
            t.title, info.itag, info.mime_type, info.content_length, info.url
        );
    }
    Ok(())
}
