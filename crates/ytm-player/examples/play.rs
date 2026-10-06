//! Interactive CLI player.
//!
//!   cargo run -p ytm-player --example play -- "radiohead" [--null]
//!
//! Searches for songs, queues the results and plays them. Commands on stdin:
//!   space/p pause·resume   n next   b prev   s <secs> seek   + / - volume
//!   r repeat-cycle   z shuffle   l list queue   q quit

use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, BufReader};
use ytm_api::{ChainResolver, InnerTube, MusicApi, SearchFilter, SearchItem};
use ytm_player::{Command, OutputKind, Player, PlayerOptions, Status};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    ytm_core::logging::init();
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let null = args
        .iter()
        .position(|a| a == "--null")
        .map(|i| args.remove(i))
        .is_some();
    let query = args.join(" ");
    anyhow::ensure!(!query.is_empty(), "usage: play <query> [--null]");

    let cfg = ytm_core::Config::load()?;
    let tube = Arc::new(InnerTube::from_config(&cfg)?);
    let tracks: Vec<_> = tube
        .search(&query, SearchFilter::Songs)
        .await?
        .items
        .into_iter()
        .filter_map(|i| {
            if let SearchItem::Track(t) = i {
                Some(t)
            } else {
                None
            }
        })
        .take(10)
        .collect();
    anyhow::ensure!(!tracks.is_empty(), "no results");

    let resolver = Arc::new(ChainResolver::standard(tube, &cfg));
    let player = Player::spawn(
        &tokio::runtime::Handle::current(),
        resolver,
        PlayerOptions {
            volume: cfg.volume,
            output: if null {
                OutputKind::Null
            } else {
                OutputKind::Device
            },
            ..PlayerOptions::default()
        },
    )?;
    player.send(Command::Play { tracks, start: 0 });

    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut tick = tokio::time::interval(Duration::from_millis(500));
    let mut last_version = 0;
    loop {
        tokio::select! {
            _ = tick.tick() => {
                let st = player.state();
                if st.version != last_version {
                    last_version = st.version;
                    if let Some(t) = st.current_track() {
                        println!("\n▶ {} — {}  [{:?}]", t.title, t.artist_line(), st.status);
                    }
                    if let Some(e) = &st.error { println!("  error: {e}"); }
                }
                if st.status != Status::Idle {
                    let pos = player.position().as_secs();
                    let dur = st.duration.map(|d| d.as_secs()).unwrap_or(0);
                    print!("\r  {:>2}:{:02} / {:>2}:{:02}  vol {:>3.0}%  buffered {:>4.1}s  underruns {}   ",
                        pos / 60, pos % 60, dur / 60, dur % 60, st.volume * 100.0, player.buffered().as_secs_f32(), player.underruns());
                    use std::io::Write; std::io::stdout().flush().ok();
                }
            }
            line = lines.next_line() => {
                let Some(line) = line? else { break };
                let st = player.state();
                let mut it = line.split_whitespace();
                match it.next() {
                    Some("q") => break,
                    Some("p") | Some(" ") | None => player.send(Command::Toggle),
                    Some("n") => player.send(Command::Next),
                    Some("b") => player.send(Command::Prev),
                    Some("s") => if let Some(s) = it.next().and_then(|v| v.parse().ok()) { player.send(Command::Seek(Duration::from_secs(s))) },
                    Some("+") => player.send(Command::SetVolume(st.volume + 0.1)),
                    Some("-") => player.send(Command::SetVolume(st.volume - 0.1)),
                    Some("r") => player.send(Command::SetRepeat(st.repeat.cycle())),
                    Some("z") => player.send(Command::SetShuffle(!st.shuffle)),
                    Some("l") => for (i, t) in st.tracks.iter().enumerate() {
                        println!("{}{i:>2} {} — {}", if st.current == Some(i) { "▶" } else { " " }, t.title, t.artist_line());
                    },
                    Some(other) => println!("unknown command {other:?}"),
                }
            }
        }
    }
    Ok(())
}
