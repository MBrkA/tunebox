//! Response parsers. They walk the JSON defensively: anything that does not
//! look like what we expect is skipped rather than treated as an error, so a
//! partial layout change degrades the page instead of breaking it.

use serde_json::Value;

use crate::models::*;
use crate::nav::{find_all, find_first, parse_duration, path, runs_text, str_at};

const TYPE_LABELS: &[&str] = &[
    "Song", "Video", "Album", "Single", "EP", "Artist", "Playlist", "Podcast", "Episode", "Profile",
];

pub fn parse_search(resp: &Value) -> SearchPage {
    let mut items = Vec::new();

    // The "top result" card only exists on unfiltered searches. When it is an artist, the songs
    // listed in it leave the artist out of their subtitle ("Song • 5:38"): the card names it.
    let mut cards = Vec::new();
    find_all(resp, "musicCardShelfRenderer", &mut cards);
    let mut card_rows: Vec<(&Value, ArtistRef)> = Vec::new();
    for card in cards {
        let Some(item) = parse_card(card) else {
            continue;
        };
        if let (SearchItem::Artist(a), Some(contents)) = (&item, card.get("contents")) {
            let mut rows = Vec::new();
            find_all(contents, "musicResponsiveListItemRenderer", &mut rows);
            let artist = ArtistRef {
                name: a.name.clone(),
                id: Some(a.browse_id.clone()),
            };
            card_rows.extend(rows.into_iter().map(|r| (r, artist.clone())));
        }
        items.push(item);
    }

    let mut rows = Vec::new();
    find_all(resp, "musicResponsiveListItemRenderer", &mut rows);
    for row in rows {
        let Some(mut item) = parse_list_item(row) else {
            continue;
        };
        if let SearchItem::Track(t) = &mut item {
            if t.artists.is_empty() {
                let card_artist = card_rows.iter().find(|(r, _)| std::ptr::eq(*r, row));
                t.artists.extend(card_artist.map(|(_, a)| a.clone()));
            }
        }
        items.push(item);
    }

    SearchPage {
        items,
        continuation: parse_continuation(resp),
    }
}

pub fn parse_suggestions(resp: &Value) -> Vec<String> {
    let mut nodes = Vec::new();
    find_all(resp, "searchSuggestionRenderer", &mut nodes);
    nodes
        .into_iter()
        .filter_map(|n| path(n, &["navigationEndpoint", "searchEndpoint", "query"]))
        .filter_map(|q| q.as_str().map(str::to_owned))
        .collect()
}

pub fn parse_continuation(resp: &Value) -> Option<String> {
    for key in ["nextContinuationData", "nextRadioContinuationData"] {
        if let Some(t) = find_first(resp, key) {
            return str_at(t, &["continuation"]).map(str::to_owned);
        }
    }
    find_first(resp, "continuationCommand")
        .and_then(|c| str_at(c, &["token"]))
        .map(str::to_owned)
}

pub fn parse_thumbnails(node: &Value) -> Vec<Thumbnail> {
    find_first(node, "thumbnails")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|t| {
                    Some(Thumbnail {
                        url: t.get("url")?.as_str()?.to_owned(),
                        width: t.get("width").and_then(Value::as_u64).unwrap_or(0) as u32,
                        height: t.get("height").and_then(Value::as_u64).unwrap_or(0) as u32,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

#[derive(Debug, PartialEq)]
pub(crate) enum Target {
    Track(String),
    Album(String),
    Artist(String),
    Playlist(String),
}

pub(crate) fn page_type(endpoint: &Value) -> Option<&str> {
    find_first(endpoint, "pageType").and_then(Value::as_str)
}

/// Classifies a navigation endpoint, if it points at something we can show.
pub(crate) fn target_of(endpoint: &Value) -> Option<Target> {
    if let Some(id) = path(endpoint, &["watchEndpoint", "videoId"]).and_then(Value::as_str) {
        return Some(Target::Track(id.to_owned()));
    }
    if let Some(id) = path(endpoint, &["browseEndpoint", "browseId"]).and_then(Value::as_str) {
        return match page_type(endpoint)? {
            "MUSIC_PAGE_TYPE_ALBUM" | "MUSIC_PAGE_TYPE_AUDIOBOOK" => {
                Some(Target::Album(id.to_owned()))
            }
            "MUSIC_PAGE_TYPE_ARTIST" | "MUSIC_PAGE_TYPE_USER_CHANNEL" => {
                Some(Target::Artist(id.to_owned()))
            }
            "MUSIC_PAGE_TYPE_PLAYLIST" => Some(Target::Playlist(
                id.strip_prefix("VL").unwrap_or(id).to_owned(),
            )),
            _ => None,
        };
    }
    if let Some(id) =
        path(endpoint, &["watchPlaylistEndpoint", "playlistId"]).and_then(Value::as_str)
    {
        return Some(Target::Playlist(id.to_owned()));
    }
    None
}

/// Runs of a flex column as `(text, endpoint)` pairs.
pub(crate) fn column_runs(item: &Value, col: usize) -> Vec<(&str, Option<&Value>)> {
    let idx = col.to_string();
    path(
        item,
        &[
            "flexColumns",
            &idx,
            "musicResponsiveListItemFlexColumnRenderer",
            "text",
            "runs",
        ],
    )
    .and_then(Value::as_array)
    .map(|runs| {
        runs.iter()
            .filter_map(|r| Some((r.get("text")?.as_str()?, r.get("navigationEndpoint"))))
            .collect()
    })
    .unwrap_or_default()
}

#[derive(Default)]
pub(crate) struct Subtitle {
    pub(crate) artists: Vec<ArtistRef>,
    pub(crate) album: Option<AlbumRef>,
    pub(crate) duration: Option<u32>,
    pub(crate) year: Option<String>,
    pub(crate) kind: Option<String>,
    /// Plain text groups between separators (used for subtitles we display verbatim).
    pub(crate) plain: Vec<String>,
}

pub(crate) fn parse_subtitle(runs: &[(&str, Option<&Value>)]) -> Subtitle {
    let mut sub = Subtitle::default();
    for (text, ep) in runs {
        let text = text.trim();
        if text.is_empty() || text == "•" || *text == *"&" || text == "," {
            continue;
        }
        match ep.and_then(target_of) {
            Some(Target::Artist(id)) => sub.artists.push(ArtistRef {
                name: text.to_owned(),
                id: Some(id),
            }),
            Some(Target::Album(id)) => {
                sub.album = Some(AlbumRef {
                    name: text.to_owned(),
                    id: Some(id),
                })
            }
            _ => {
                if let Some(d) = parse_duration(text) {
                    sub.duration = Some(d);
                } else if TYPE_LABELS.contains(&text) {
                    sub.kind = Some(text.to_owned());
                } else if text.len() == 4 && text.chars().all(|c| c.is_ascii_digit()) {
                    sub.year = Some(text.to_owned());
                } else {
                    sub.plain.push(text.to_owned());
                }
            }
        }
    }
    sub
}

/// Artists that have no browse id (e.g. "Various Artists") show up as plain
/// text; for tracks the first plain run that is not a play/view count is the artist.
pub(crate) fn fallback_artist(plain: &[String]) -> Option<ArtistRef> {
    plain
        .iter()
        .find(|t| {
            let l = t.to_lowercase();
            !(l.ends_with("plays") || l.ends_with("views") || l.ends_with("likes"))
        })
        .map(|name| ArtistRef {
            name: name.clone(),
            id: None,
        })
}

pub(crate) fn assemble(
    target: Target,
    title: &str,
    sub: Subtitle,
    thumbnails: Vec<Thumbnail>,
    fixed_duration: Option<u32>,
) -> SearchItem {
    match target {
        Target::Track(video_id) => {
            let mut artists = sub.artists;
            if artists.is_empty() {
                artists.extend(fallback_artist(&sub.plain));
            }
            SearchItem::Track(Track {
                video_id,
                title: title.to_owned(),
                artists,
                album: sub.album,
                duration_secs: sub.duration.or(fixed_duration),
                thumbnails,
            })
        }
        Target::Album(browse_id) => SearchItem::Album(AlbumSummary {
            browse_id,
            title: title.to_owned(),
            kind: sub.kind.unwrap_or_else(|| "Album".into()),
            artists: sub.artists,
            year: sub.year,
            thumbnails,
        }),
        Target::Artist(browse_id) => SearchItem::Artist(ArtistSummary {
            browse_id,
            name: title.to_owned(),
            subtitle: sub.plain.join(" · "),
            thumbnails,
        }),
        Target::Playlist(playlist_id) => SearchItem::Playlist(PlaylistSummary {
            playlist_id,
            title: title.to_owned(),
            subtitle: sub
                .artists
                .iter()
                .map(|a| a.name.clone())
                .chain(sub.plain)
                .collect::<Vec<_>>()
                .join(" · "),
            thumbnails,
            covers: Vec::new(),
        }),
    }
}

pub(crate) fn runs_with_endpoints(node: Option<&Value>) -> Vec<(&str, Option<&Value>)> {
    node.and_then(|n| n.get("runs"))
        .and_then(Value::as_array)
        .map(|runs| {
            runs.iter()
                .filter_map(|r| Some((r.get("text")?.as_str()?, r.get("navigationEndpoint"))))
                .collect()
        })
        .unwrap_or_default()
}

pub fn parse_list_item(item: &Value) -> Option<SearchItem> {
    let title_runs = column_runs(item, 0);
    let (title, title_ep) = *title_runs.first()?;
    let sub = parse_subtitle(&column_runs(item, 1));
    let thumbnails = item
        .get("thumbnail")
        .map(parse_thumbnails)
        .unwrap_or_default();
    let target = path(item, &["playlistItemData", "videoId"])
        .and_then(Value::as_str)
        .map(|id| Target::Track(id.to_owned()))
        .or_else(|| title_ep.and_then(target_of))
        .or_else(|| item.get("navigationEndpoint").and_then(target_of))?;
    let fixed = path(
        item,
        &[
            "fixedColumns",
            "0",
            "musicResponsiveListItemFixedColumnRenderer",
            "text",
        ],
    )
    .and_then(|t| parse_duration(&runs_text(t)));
    Some(assemble(target, title, sub, thumbnails, fixed))
}

/// Square/portrait cards used in carousels (`musicTwoRowItemRenderer`).
pub fn parse_two_row(item: &Value) -> Option<SearchItem> {
    let first = path(item, &["title", "runs", "0"])?;
    let title = first.get("text")?.as_str()?;
    let target = item
        .get("navigationEndpoint")
        .and_then(target_of)
        .or_else(|| first.get("navigationEndpoint").and_then(target_of))?;
    let sub = parse_subtitle(&runs_with_endpoints(item.get("subtitle")));
    let thumbnails = item
        .get("thumbnailRenderer")
        .map(parse_thumbnails)
        .unwrap_or_default();
    Some(assemble(target, title, sub, thumbnails, None))
}

/// The "top result" card on unfiltered searches.
fn parse_card(card: &Value) -> Option<SearchItem> {
    let first = path(card, &["title", "runs", "0"])?;
    let title = first.get("text")?.as_str()?;
    let target = first.get("navigationEndpoint").and_then(target_of)?;
    let sub = parse_subtitle(&runs_with_endpoints(card.get("subtitle")));
    let thumbnails = card
        .get("thumbnail")
        .map(parse_thumbnails)
        .unwrap_or_default();
    Some(assemble(target, title, sub, thumbnails, None))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Value {
        let p = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
        serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
    }

    #[test]
    fn songs_search() {
        let page = parse_search(&fixture("search_songs.json"));
        assert!(page.items.len() >= 10, "got {}", page.items.len());
        assert!(page.continuation.is_some());
        let SearchItem::Track(t) = &page.items[0] else {
            panic!("first item should be a track: {:?}", page.items[0]);
        };
        assert_eq!(t.video_id, "khnokW3Mw24");
        assert_eq!(t.title, "Instant Crush (feat. Julian Casablancas)");
        assert_eq!(t.artists.len(), 2);
        assert_eq!(t.artists[0].name, "Daft Punk");
        assert!(t.artists[0].id.is_some());
        assert_eq!(t.album.as_ref().unwrap().name, "Random Access Memories");
        assert_eq!(t.duration_secs, Some(5 * 60 + 38));
        assert!(!t.thumbnails.is_empty());
        // every song row must be a playable track with a title
        for item in &page.items {
            if let SearchItem::Track(t) = item {
                assert_eq!(t.video_id.len(), 11);
                assert!(!t.title.is_empty());
            }
        }
    }

    #[test]
    fn mixed_search_has_several_kinds() {
        let page = parse_search(&fixture("search_all.json"));
        let has = |f: fn(&SearchItem) -> bool| page.items.iter().any(f);
        assert!(has(|i| matches!(i, SearchItem::Track(_))));
        assert!(has(|i| matches!(i, SearchItem::Artist(_))));
        assert!(has(|i| matches!(i, SearchItem::Album(_))));
        // top card is the artist
        assert!(matches!(&page.items[0], SearchItem::Artist(a) if a.name == "Daft Punk"));
    }

    #[test]
    fn songs_in_the_artist_top_card_get_the_card_artist() {
        // Their subtitle is only "Song • 5:38"; the artist is the card's title.
        let page = parse_search(&fixture("search_all.json"));
        let crush = page
            .items
            .iter()
            .find_map(|i| match i {
                SearchItem::Track(t) if t.title.starts_with("Instant Crush") => Some(t),
                _ => None,
            })
            .expect("Instant Crush is in the top card");
        assert_eq!(crush.artist_line(), "Daft Punk");
        assert_eq!(
            crush.artists[0].id.as_deref(),
            Some("UCRr1xG_2WIDs18a6cIiCxeA")
        );
        assert_eq!(crush.duration_secs, Some(5 * 60 + 38));
        // no track in the mixed results is left without an artist
        for i in &page.items {
            if let SearchItem::Track(t) = i {
                assert!(!t.artists.is_empty(), "{} has no artist", t.title);
            }
        }
    }

    #[test]
    fn suggestions() {
        let s = parse_suggestions(&fixture("suggestions.json"));
        assert!(s.len() >= 3);
        assert!(s.iter().any(|q| q.contains("daft punk")));
    }

    #[test]
    fn garbage_yields_empty_not_panic() {
        let page = parse_search(&serde_json::json!({"contents": [1, "x", null]}));
        assert!(page.items.is_empty());
        assert!(parse_suggestions(&serde_json::json!(42)).is_empty());
    }
}
