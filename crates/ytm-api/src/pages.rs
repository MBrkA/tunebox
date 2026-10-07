//! Parsers for browse-style pages: home, explore, artist, album, playlist,
//! up-next and lyrics. Same philosophy as `parse.rs`: walk defensively, skip
//! what is not understood.

use serde_json::Value;

use crate::models::*;
use crate::nav::{find_all, find_first, path, runs_text};
use crate::parse::{
    parse_continuation, parse_list_item, parse_subtitle, parse_thumbnails, parse_two_row,
    runs_with_endpoints,
};

const SHELF_KEYS: [&str; 4] = [
    "musicCarouselShelfRenderer",
    "musicShelfRenderer",
    "musicPlaylistShelfRenderer",
    // Some pages (e.g. Explore) lay their cards out in a grid rather than a carousel.
    "gridRenderer",
];

/// Shelves in document order (carousels and list shelves).
fn collect_shelves<'a>(v: &'a Value, out: &mut Vec<(&'static str, &'a Value)>) {
    match v {
        Value::Object(map) => {
            for (k, child) in map {
                if let Some(key) = SHELF_KEYS.iter().find(|s| **s == k) {
                    out.push((key, child));
                } else {
                    collect_shelves(child, out);
                }
            }
        }
        Value::Array(items) => items.iter().for_each(|c| collect_shelves(c, out)),
        _ => {}
    }
}

fn shelf_title(kind: &str, shelf: &Value) -> String {
    let node = if kind == "musicCarouselShelfRenderer" {
        path(
            shelf,
            &["header", "musicCarouselShelfBasicHeaderRenderer", "title"],
        )
    } else {
        shelf.get("title")
    };
    node.map(runs_text).unwrap_or_default()
}

/// Chart rows carry their rank in `customIndexColumn`.
fn chart_rank(row: &Value) -> Option<String> {
    let text = path(
        row,
        &[
            "customIndexColumn",
            "musicCustomIndexColumnRenderer",
            "text",
        ],
    )?;
    let rank = runs_text(text);
    (!rank.is_empty()).then_some(rank)
}

fn shelf_items(shelf: &Value) -> Vec<SearchItem> {
    shelf
        .get("contents")
        .or_else(|| shelf.get("items"))
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|it| {
                    if let Some(n) = it.get("musicTwoRowItemRenderer") {
                        parse_two_row(n)
                    } else {
                        let row = it.get("musicResponsiveListItemRenderer")?;
                        let mut item = parse_list_item(row)?;
                        // Ranked rows (chart artists): show the position in the subtitle.
                        if let (Some(rank), SearchItem::Artist(a)) = (chart_rank(row), &mut item) {
                            a.subtitle = if a.subtitle.is_empty() {
                                format!("#{rank}")
                            } else {
                                format!("#{rank} · {}", a.subtitle)
                            };
                        }
                        Some(item)
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn parse_sections(resp: &Value) -> Vec<Section> {
    let mut shelves = Vec::new();
    collect_shelves(resp, &mut shelves);
    shelves
        .into_iter()
        .filter_map(|(kind, shelf)| {
            let items = shelf_items(shelf);
            (!items.is_empty()).then(|| Section {
                title: shelf_title(kind, shelf),
                items,
            })
        })
        .collect()
}

pub fn parse_home(resp: &Value) -> HomePage {
    HomePage {
        sections: parse_sections(resp),
        continuation: parse_continuation(resp),
        moods: parse_moods(resp),
    }
}

/// `browseId` of mood/genre category pages.
pub const MOOD_CATEGORY_ID: &str = "FEmusic_moods_and_genres_category";

fn mood_button(node: &Value) -> Option<MoodCategory> {
    let endpoint = path(node, &["clickCommand", "browseEndpoint"])?;
    if endpoint.get("browseId")?.as_str()? != MOOD_CATEGORY_ID {
        return None; // e.g. "New releases", "Charts"
    }
    let title = runs_text(node.get("buttonText")?);
    let params = endpoint.get("params")?.as_str()?.to_owned();
    (!title.is_empty() && !params.is_empty()).then(|| MoodCategory {
        title,
        params,
        color: path(node, &["solid", "leftStripeColor"])
            .and_then(Value::as_u64)
            .and_then(|c| u32::try_from(c).ok()),
    })
}

/// Every mood/genre button anywhere in the response, without duplicates.
pub fn parse_moods(resp: &Value) -> Vec<MoodCategory> {
    let mut nodes = Vec::new();
    find_all(resp, "musicNavigationButtonRenderer", &mut nodes);
    let mut out: Vec<MoodCategory> = Vec::new();
    for c in nodes.into_iter().filter_map(mood_button) {
        if !out.iter().any(|o| o.params == c.params) {
            out.push(c);
        }
    }
    out
}

/// The "Moods & genres" page: groups such as "Moods & moments" and "Genres".
pub fn parse_moods_page(resp: &Value) -> MoodsPage {
    let mut grids = Vec::new();
    find_all(resp, "gridRenderer", &mut grids);
    let groups = grids
        .into_iter()
        .filter_map(|g| {
            let categories: Vec<MoodCategory> = g
                .get("items")?
                .as_array()?
                .iter()
                .filter_map(|i| i.get("musicNavigationButtonRenderer").and_then(mood_button))
                .collect();
            let title = path(g, &["header", "gridHeaderRenderer", "title"])
                .map(runs_text)
                .unwrap_or_default();
            (!categories.is_empty()).then_some(MoodGroup { title, categories })
        })
        .collect();
    MoodsPage { groups }
}

/// Decodes standard base64 (padding optional); `None` on invalid input.
fn base64_decode(input: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(input.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in input.trim_end_matches('=').bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => return None,
        };
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// The country code hidden in a menu item's `formItemEntityKey`
/// (base64 of a protobuf containing `explore_charts_country_menu_<id><CC>`).
fn country_code(entity_key: &str) -> Option<String> {
    let key = entity_key
        .replace("%3D", "=")
        .replace("%2B", "+")
        .replace("%2F", "/");
    let bytes = base64_decode(&key)?;
    let text = String::from_utf8_lossy(&bytes);
    let tail = text.split("explore_charts_country_menu_").nth(1)?;
    let digits = tail.chars().take_while(char::is_ascii_digit).count();
    let code: String = tail.chars().skip(digits).take(2).collect();
    (code.len() == 2 && code.chars().all(|c| c.is_ascii_uppercase())).then_some(code)
}

/// The Charts page, with its country selector.
pub fn parse_charts(resp: &Value) -> ChartsPage {
    let mut items = Vec::new();
    find_all(resp, "musicMultiSelectMenuItemRenderer", &mut items);
    let countries: Vec<ChartCountry> = items
        .into_iter()
        .filter_map(|i| {
            Some(ChartCountry {
                code: country_code(i.get("formItemEntityKey")?.as_str()?)?,
                name: i.get("title").map(runs_text)?,
            })
        })
        .collect();
    // The selector button shows the country the charts are for.
    let current_name = find_first(resp, "musicSortFilterButtonRenderer")
        .and_then(|b| b.get("title"))
        .map(runs_text);
    let country = current_name.and_then(|name| countries.iter().find(|c| c.name == name).cloned());
    ChartsPage {
        country,
        countries,
        sections: parse_sections(resp),
    }
}

/// One category's page (title + rows of playlists).
pub fn parse_mood_category(resp: &Value) -> CategoryPage {
    CategoryPage {
        title: path(resp, &["header", "musicHeaderRenderer", "title"])
            .map(runs_text)
            .unwrap_or_default(),
        sections: parse_sections(resp),
    }
}

fn as_tracks(items: Vec<SearchItem>) -> Vec<Track> {
    items
        .into_iter()
        .filter_map(|i| match i {
            SearchItem::Track(t) => Some(t),
            _ => None,
        })
        .collect()
}

pub fn parse_artist(resp: &Value, browse_id: &str) -> ArtistPage {
    let header = path(resp, &["header", "musicImmersiveHeaderRenderer"])
        .or_else(|| path(resp, &["header", "musicVisualHeaderRenderer"]));
    let text = |key: &str| {
        header
            .and_then(|h| h.get(key))
            .map(runs_text)
            .unwrap_or_default()
    };
    let mut page = ArtistPage {
        browse_id: browse_id.to_owned(),
        name: text("title"),
        description: text("description"),
        thumbnails: header
            .and_then(|h| h.get("thumbnail"))
            .map(parse_thumbnails)
            .unwrap_or_default(),
        listeners: header
            .and_then(|h| h.get("monthlyListenerCount"))
            .map(runs_text)
            .filter(|t| !t.is_empty()),
        subscribers: header
            .and_then(|h| find_first(h, "subscriberCountText"))
            .map(runs_text)
            .filter(|t| !t.is_empty()),
        radio_id: header
            .and_then(|h| h.get("startRadioButton"))
            .and_then(|b| find_first(b, "playlistId"))
            .and_then(Value::as_str)
            .map(str::to_owned),
        ..ArtistPage::default()
    };
    let mut sections = parse_sections(resp);
    // The first all-songs list shelf is "Top songs".
    if let Some(i) = sections
        .iter()
        .position(|s| s.items.iter().all(|i| matches!(i, SearchItem::Track(_))))
    {
        page.top_songs = as_tracks(sections.remove(i).items);
    }
    page.sections = sections;
    page
}

pub fn parse_album(resp: &Value, browse_id: &str) -> AlbumPage {
    let header = find_first(resp, "musicResponsiveHeaderRenderer");
    let mut page = AlbumPage {
        browse_id: browse_id.to_owned(),
        ..AlbumPage::default()
    };
    if let Some(h) = header {
        page.title = h.get("title").map(runs_text).unwrap_or_default();
        page.thumbnails = h.get("thumbnail").map(parse_thumbnails).unwrap_or_default();
        page.description = h.get("description").map(runs_text).unwrap_or_default();
        page.stats = h.get("secondSubtitle").map(runs_text).unwrap_or_default();
        let sub = parse_subtitle(&runs_with_endpoints(h.get("subtitle")));
        page.kind = sub.kind.unwrap_or_else(|| "Album".into());
        page.year = sub.year;
        page.artists = parse_subtitle(&runs_with_endpoints(h.get("straplineTextOne"))).artists;
    }
    let mut shelves = Vec::new();
    collect_shelves(resp, &mut shelves);
    let rows = shelves
        .iter()
        .find(|(k, _)| *k == "musicShelfRenderer")
        .map(|(_, s)| as_tracks(shelf_items(s)))
        .unwrap_or_default();
    page.tracks = rows
        .into_iter()
        .map(|mut t| {
            if t.artists.is_empty() {
                t.artists = page.artists.clone();
            }
            if t.album.is_none() {
                t.album = Some(AlbumRef {
                    name: page.title.clone(),
                    id: Some(browse_id.to_owned()),
                });
            }
            if t.thumbnails.is_empty() {
                t.thumbnails = page.thumbnails.clone();
            }
            t
        })
        .collect();
    page
}

pub fn parse_playlist(resp: &Value, playlist_id: &str) -> PlaylistPage {
    let mut page = PlaylistPage {
        playlist_id: playlist_id.to_owned(),
        continuation: parse_continuation(resp),
        ..PlaylistPage::default()
    };
    if let Some(h) = find_first(resp, "musicResponsiveHeaderRenderer") {
        page.title = h.get("title").map(runs_text).unwrap_or_default();
        page.thumbnails = h.get("thumbnail").map(parse_thumbnails).unwrap_or_default();
        page.stats = h.get("secondSubtitle").map(runs_text).unwrap_or_default();
        page.author = h
            .get("facepile")
            .and_then(|f| find_first(f, "content"))
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or_else(|| h.get("straplineTextOne").map(runs_text))
            .unwrap_or_default();
    }
    page.tracks = playlist_rows(resp);
    page
}

/// Track rows of a playlist page or of a playlist continuation response.
pub fn playlist_rows(resp: &Value) -> Vec<Track> {
    let mut rows = Vec::new();
    find_all(resp, "musicResponsiveListItemRenderer", &mut rows);
    rows.into_iter()
        .filter_map(parse_list_item)
        .filter_map(|i| match i {
            SearchItem::Track(t) => Some(t),
            _ => None,
        })
        .collect()
}

pub fn parse_up_next(resp: &Value) -> UpNext {
    let mut nodes = Vec::new();
    find_all(resp, "playlistPanelVideoRenderer", &mut nodes);
    let mut tracks: Vec<Track> = Vec::new();
    for n in nodes {
        let Some(video_id) = n.get("videoId").and_then(Value::as_str) else {
            continue;
        };
        if tracks.iter().any(|t| t.video_id == video_id) {
            continue; // counterpart (video/audio) duplicates
        }
        let sub = parse_subtitle(&runs_with_endpoints(n.get("longBylineText")));
        let mut artists = sub.artists;
        if artists.is_empty() {
            artists.extend(crate::parse::fallback_artist(&sub.plain));
        }
        tracks.push(Track {
            video_id: video_id.to_owned(),
            title: n.get("title").map(runs_text).unwrap_or_default(),
            artists,
            album: sub.album,
            duration_secs: n
                .get("lengthText")
                .and_then(|t| crate::nav::parse_duration(&runs_text(t))),
            thumbnails: n.get("thumbnail").map(parse_thumbnails).unwrap_or_default(),
        });
    }
    UpNext {
        tracks,
        continuation: parse_continuation(resp),
    }
}

/// browseId of the "Lyrics" tab in a `next` response, if the track has lyrics.
pub fn parse_lyrics_browse_id(next: &Value) -> Option<String> {
    let mut ids = Vec::new();
    find_all(next, "browseId", &mut ids);
    ids.into_iter()
        .filter_map(Value::as_str)
        .find(|id| id.starts_with("MPLY"))
        .map(str::to_owned)
}

pub fn parse_lyrics(resp: &Value) -> Option<Lyrics> {
    let shelf = find_first(resp, "musicDescriptionShelfRenderer")?;
    let text = shelf.get("description").map(runs_text)?;
    if text.trim().is_empty() {
        return None;
    }
    Some(Lyrics {
        text,
        source: shelf
            .get("footer")
            .map(runs_text)
            .filter(|s| !s.trim().is_empty()),
        synced: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Value {
        let p = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
        serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
    }

    #[test]
    fn home_has_sections_and_continuation() {
        let home = parse_home(&fixture("home.json"));
        assert!(home.sections.len() >= 2, "{}", home.sections.len());
        assert!(home.continuation.is_some());
        let s = &home.sections[0];
        assert!(!s.title.is_empty());
        assert!(s.items.len() >= 5);
        assert!(s.items.iter().any(|i| matches!(i, SearchItem::Playlist(p) if !p.title.is_empty() && !p.thumbnails.is_empty())));
    }

    #[test]
    fn explore_has_albums_and_trending_songs() {
        let page = parse_home(&fixture("explore.json"));
        let titles: Vec<_> = page.sections.iter().map(|s| s.title.as_str()).collect();
        assert!(titles.contains(&"New albums & singles"), "{titles:?}");
        let albums = page
            .sections
            .iter()
            .find(|s| s.title.starts_with("New albums"))
            .unwrap();
        assert!(albums
            .items
            .iter()
            .all(|i| matches!(i, SearchItem::Album(a) if a.browse_id.starts_with("MPRE"))));
        let trending = page
            .sections
            .iter()
            .find(|s| s.title == "Trending")
            .unwrap();
        assert!(trending
            .items
            .iter()
            .all(|i| matches!(i, SearchItem::Track(_))));
        // "Moods & genres" are navigation buttons we do not model: no empty section is emitted
        assert!(page.sections.iter().all(|s| !s.items.is_empty()));
    }

    #[test]
    fn explore_exposes_mood_shortcuts_but_not_new_releases_or_charts() {
        let page = parse_home(&fixture("explore.json"));
        let titles: Vec<_> = page.moods.iter().map(|m| m.title.as_str()).collect();
        assert!(page.moods.len() >= 20, "{titles:?}");
        assert!(
            titles.contains(&"Chill") && titles.contains(&"Focus"),
            "{titles:?}"
        );
        assert!(!titles.contains(&"New releases") && !titles.contains(&"Charts"));
        assert!(
            !titles.contains(&"Moods & genres"),
            "the link to the page is not a category"
        );
        let chill = page.moods.iter().find(|m| m.title == "Chill").unwrap();
        assert_eq!(chill.params, "ggMPOg1uX1JOQWZFeDByc2Jm");
        assert_eq!(
            chill.color,
            Some(4288988671),
            "stripe colour is kept (0xAARRGGBB)"
        );
        let mut seen = std::collections::HashSet::new();
        assert!(
            page.moods.iter().all(|m| seen.insert(&m.params)),
            "no duplicates"
        );
    }

    #[test]
    fn moods_and_genres_page_has_groups() {
        let page = parse_moods_page(&fixture("moods.json"));
        let titles: Vec<_> = page.groups.iter().map(|g| g.title.as_str()).collect();
        assert_eq!(titles, ["Moods & moments", "Genres"]);
        assert_eq!(page.groups[0].categories.len(), 11);
        assert_eq!(page.groups[1].categories.len(), 26);
        assert!(page.groups[0].categories.iter().any(|c| c.title == "Chill"));
        assert!(page.groups[1]
            .categories
            .iter()
            .all(|c| !c.params.is_empty()));
    }

    #[test]
    fn mood_category_page_has_playlist_rows() {
        let page = parse_mood_category(&fixture("mood_category.json"));
        assert_eq!(page.title, "Chill");
        assert_eq!(page.sections.len(), 3);
        assert_eq!(page.sections[0].title, "Coffee shop blends");
        assert!(page.sections.iter().all(|s| !s.items.is_empty()
            && s.items
                .iter()
                .all(|i| matches!(i, SearchItem::Playlist(p) if !p.playlist_id.is_empty()))));
    }

    #[test]
    fn charts_page_has_country_selector_and_ranked_artists() {
        let page = parse_charts(&fixture("charts.json"));
        assert!(page.countries.len() >= 60, "{}", page.countries.len());
        let code = |name: &str| {
            page.countries
                .iter()
                .find(|c| c.name == name)
                .map(|c| c.code.as_str())
        };
        assert_eq!(code("Turkey"), Some("TR"));
        assert_eq!(code("Global"), Some("ZZ"));
        assert_eq!(code("United States"), Some("US"));
        assert_eq!(
            page.country.as_ref().map(|c| c.code.as_str()),
            Some("TR"),
            "current country"
        );
        let titles: Vec<_> = page.sections.iter().map(|s| s.title.as_str()).collect();
        assert!(
            titles.contains(&"Video charts") && titles.contains(&"Top artists"),
            "{titles:?}"
        );
        let artists = page
            .sections
            .iter()
            .find(|s| s.title == "Top artists")
            .unwrap();
        let SearchItem::Artist(first) = &artists.items[0] else {
            panic!("{:?}", artists.items[0])
        };
        assert!(
            first.subtitle.starts_with("#1 · "),
            "rank prefix: {:?}",
            first.subtitle
        );
        assert!(
            page.sections.iter().all(|s| !s.items.is_empty()),
            "the selector shelf is not a section"
        );
    }

    #[test]
    fn charts_for_another_country_report_that_country() {
        let page = parse_charts(&fixture("charts_ZZ.json"));
        assert_eq!(page.country.map(|c| c.code), Some("ZZ".to_string()));
    }

    #[test]
    fn country_codes_come_from_the_entity_key() {
        assert_eq!(
            country_code("EidleHBsb3JlX2NoYXJ0c19jb3VudHJ5X21lbnVfMzE2NzY2NTY3VFIgkQEoAQ%3D%3D")
                .as_deref(),
            Some("TR")
        );
        assert_eq!(country_code("not base64 !!"), None);
        assert_eq!(country_code("AAAA"), None);
        assert_eq!(base64_decode("aGk=").unwrap(), b"hi");
        assert_eq!(base64_decode("aGk").unwrap(), b"hi", "padding optional");
    }

    #[test]
    fn new_releases_page_has_albums_and_videos() {
        let page = parse_home(&fixture("new_releases.json"));
        let titles: Vec<_> = page.sections.iter().map(|s| s.title.as_str()).collect();
        assert!(titles.contains(&"Albums & singles"), "{titles:?}");
        let albums = page
            .sections
            .iter()
            .find(|s| s.title == "Albums & singles")
            .unwrap();
        assert!(albums
            .items
            .iter()
            .all(|i| matches!(i, SearchItem::Album(_))));
        assert!(albums.items.len() >= 10);
    }

    #[test]
    fn mood_parsers_tolerate_junk() {
        assert_eq!(
            parse_charts(&serde_json::json!({"x": 1})),
            ChartsPage::default()
        );
        let junk = serde_json::json!({"contents": [1, null, {"musicNavigationButtonRenderer": {"buttonText": 5}}]});
        assert!(parse_moods(&junk).is_empty());
        assert!(parse_moods_page(&junk).groups.is_empty());
        assert!(parse_mood_category(&junk).sections.is_empty());
    }

    #[test]
    fn artist_page() {
        let a = parse_artist(&fixture("artist.json"), "UCRr1xG_2WIDs18a6cIiCxeA");
        assert_eq!(a.name, "Daft Punk");
        assert!(a.description.starts_with("Daft Punk were"));
        assert!(a.listeners.is_some() || a.subscribers.is_some());
        assert!(!a.thumbnails.is_empty());
        assert_eq!(a.top_songs.len(), 5);
        assert!(a
            .top_songs
            .iter()
            .all(|t| t.video_id.len() == 11 && !t.artists.is_empty()));
        let titles: Vec<_> = a.sections.iter().map(|s| s.title.as_str()).collect();
        assert!(titles.contains(&"Albums"), "{titles:?}");
        assert!(a.radio_id.as_deref().is_some_and(|r| r.starts_with("RD")));
    }

    #[test]
    fn album_page_fills_missing_track_fields() {
        let a = parse_album(&fixture("album.json"), "MPREb_7ltM34kr0mH");
        assert_eq!(a.title, "Discovery");
        assert_eq!(a.kind, "Album");
        assert_eq!(a.year.as_deref(), Some("2001"));
        assert_eq!(a.artists[0].name, "Daft Punk");
        assert_eq!(a.tracks.len(), 14);
        assert_eq!(a.tracks[0].title, "One More Time");
        assert_eq!(
            a.tracks[0].artists[0].name, "Daft Punk",
            "inherited from album"
        );
        assert_eq!(a.tracks[0].album.as_ref().unwrap().name, "Discovery");
        assert!(
            a.tracks.iter().all(|t| t.duration_secs.is_some()),
            "durations from fixed columns"
        );
        assert!(a.stats.contains("14 songs"));
    }

    #[test]
    fn playlist_page() {
        let p = parse_playlist(
            &fixture("playlist.json"),
            "PLSdoVPM5WnnfbGVqQTCXjRnZd8hYLY0Cd",
        );
        assert_eq!(p.title, "Daft Punk - Official Videos");
        assert_eq!(p.tracks.len(), 32);
        assert!(p.stats.contains("32 tracks"));
        assert!(!p.thumbnails.is_empty());
        assert!(p.tracks[0].artists.iter().any(|a| a.name == "Daft Punk"));
    }

    #[test]
    fn up_next_queue() {
        let n = parse_up_next(&fixture("next.json"));
        assert!(n.tracks.len() >= 20, "{}", n.tracks.len());
        let t = &n.tracks[1];
        assert_eq!(
            t.title,
            "Get Lucky (feat. Pharrell Williams and Nile Rodgers)"
        );
        assert_eq!(t.artists.len(), 3);
        assert_eq!(t.album.as_ref().unwrap().name, "Random Access Memories");
        assert_eq!(t.duration_secs, Some(6 * 60 + 10));
        assert!(n.continuation.is_some());
        let ids: std::collections::HashSet<_> = n.tracks.iter().map(|t| &t.video_id).collect();
        assert_eq!(ids.len(), n.tracks.len(), "no duplicates");
    }

    #[test]
    fn lyrics_roundtrip() {
        let id = parse_lyrics_browse_id(&fixture("next.json")).unwrap();
        assert!(id.starts_with("MPLYt_"));
        let l = parse_lyrics(&fixture("lyrics.json")).unwrap();
        assert!(l.text.contains("I didn't want to be the one to forget"));
        assert!(l.source.is_some());
        assert!(parse_lyrics(&serde_json::json!({})).is_none());
    }

    #[test]
    fn garbage_is_harmless() {
        let junk = serde_json::json!({"contents": [1, null, {"x": []}]});
        assert!(parse_home(&junk).sections.is_empty());
        assert!(parse_artist(&junk, "x").top_songs.is_empty());
        assert!(parse_album(&junk, "x").tracks.is_empty());
        assert!(parse_up_next(&junk).tracks.is_empty());
        assert!(parse_lyrics_browse_id(&junk).is_none());
    }
}
