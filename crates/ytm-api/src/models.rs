//! Plain data types returned by the API layer. They are independent of the
//! InnerTube JSON shape so the UI never touches raw responses.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Thumbnail {
    pub url: String,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ArtistRef {
    pub name: String,
    pub id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct AlbumRef {
    pub name: String,
    pub id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Track {
    pub video_id: String,
    pub title: String,
    pub artists: Vec<ArtistRef>,
    pub album: Option<AlbumRef>,
    pub duration_secs: Option<u32>,
    pub thumbnails: Vec<Thumbnail>,
}

impl Track {
    pub fn artist_line(&self) -> String {
        self.artists
            .iter()
            .map(|a| a.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// Smallest thumbnail that is at least `min` pixels wide, else the largest.
    pub fn thumbnail_for(&self, min: u32) -> Option<&Thumbnail> {
        pick_thumbnail(&self.thumbnails, min)
    }
}

pub fn pick_thumbnail(thumbs: &[Thumbnail], min: u32) -> Option<&Thumbnail> {
    thumbs
        .iter()
        .filter(|t| t.width >= min)
        .min_by_key(|t| t.width)
        .or_else(|| thumbs.iter().max_by_key(|t| t.width))
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct AlbumSummary {
    pub browse_id: String,
    pub title: String,
    /// "Album", "Single", "EP", …
    pub kind: String,
    pub artists: Vec<ArtistRef>,
    pub year: Option<String>,
    pub thumbnails: Vec<Thumbnail>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ArtistSummary {
    pub browse_id: String,
    pub name: String,
    pub subtitle: String,
    pub thumbnails: Vec<Thumbnail>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct PlaylistSummary {
    pub playlist_id: String,
    pub title: String,
    pub subtitle: String,
    pub thumbnails: Vec<Thumbnail>,
    /// Cover URLs of the first songs (device playlists): drawn as a 2×2 mosaic when more than one.
    #[serde(default)]
    pub covers: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SearchItem {
    Track(Track),
    Album(AlbumSummary),
    Artist(ArtistSummary),
    Playlist(PlaylistSummary),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SearchFilter {
    #[default]
    All,
    Songs,
    Videos,
    Albums,
    Artists,
    Playlists,
}

impl SearchFilter {
    /// Opaque `params` blob understood by the search endpoint.
    pub fn params(self) -> Option<&'static str> {
        match self {
            Self::All => None,
            Self::Songs => Some("EgWKAQIIAWoMEA4QChADEAQQCRAF"),
            Self::Videos => Some("EgWKAQIQAWoMEA4QChADEAQQCRAF"),
            Self::Albums => Some("EgWKAQIYAWoMEA4QChADEAQQCRAF"),
            Self::Artists => Some("EgWKAQIgAWoMEA4QChADEAQQCRAF"),
            Self::Playlists => Some("EgeKAQQoAEABagwQDhAKEAMQBBAJEAU="),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SearchPage {
    pub items: Vec<SearchItem>,
    /// Pass to `MusicApi::search_continue` for more results.
    pub continuation: Option<String>,
}

/// A resolved, directly fetchable audio stream.
#[derive(Debug, Clone, PartialEq)]
pub struct StreamInfo {
    pub url: String,
    pub itag: u32,
    pub mime_type: String,
    pub content_length: Option<u64>,
    pub duration_ms: Option<u64>,
    /// User-Agent the URL was issued for; must be sent when fetching it.
    pub user_agent: String,
    pub expires_in_secs: Option<u64>,
    pub loudness_db: Option<f32>,
}

/// A titled row/carousel of items (Home, Explore, artist pages …).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Section {
    pub title: String,
    pub items: Vec<SearchItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct HomePage {
    pub sections: Vec<Section>,
    pub continuation: Option<String>,
    /// "Moods & genres" shortcuts found on the page (Explore has them).
    pub moods: Vec<MoodCategory>,
}

/// A mood or genre (Chill, Workout, Jazz …) that opens a page of playlists.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct MoodCategory {
    pub title: String,
    /// Opaque request parameter that selects the category.
    pub params: String,
    /// Accent colour as 0xAARRGGBB, when YouTube provides one.
    pub color: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct MoodGroup {
    pub title: String,
    pub categories: Vec<MoodCategory>,
}

/// The full "Moods & genres" page.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct MoodsPage {
    pub groups: Vec<MoodGroup>,
}

/// A country (or "Global") whose charts can be shown.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ChartCountry {
    /// ISO 3166 alpha-2 code, `ZZ` = Global.
    pub code: String,
    pub name: String,
}

/// The Charts page for one country.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ChartsPage {
    /// The country these charts are for (YouTube picks one from the user's location by default).
    pub country: Option<ChartCountry>,
    pub countries: Vec<ChartCountry>,
    pub sections: Vec<Section>,
}

/// What a single mood/genre category contains: titled rows of playlists.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct CategoryPage {
    pub title: String,
    pub sections: Vec<Section>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ArtistPage {
    pub browse_id: String,
    pub name: String,
    pub description: String,
    pub subscribers: Option<String>,
    pub listeners: Option<String>,
    pub thumbnails: Vec<Thumbnail>,
    /// Playlist id of the artist radio ("start radio").
    pub radio_id: Option<String>,
    pub top_songs: Vec<Track>,
    /// Albums, singles, videos, similar artists, …
    pub sections: Vec<Section>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct AlbumPage {
    pub browse_id: String,
    pub title: String,
    pub kind: String,
    pub year: Option<String>,
    pub artists: Vec<ArtistRef>,
    pub thumbnails: Vec<Thumbnail>,
    pub description: String,
    /// e.g. "14 songs • 1 hour, 1 minute"
    pub stats: String,
    pub tracks: Vec<Track>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct PlaylistPage {
    pub playlist_id: String,
    pub title: String,
    pub author: String,
    pub stats: String,
    pub thumbnails: Vec<Thumbnail>,
    pub tracks: Vec<Track>,
    pub continuation: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct UpNext {
    pub tracks: Vec<Track>,
    pub continuation: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Lyrics {
    pub text: String,
    pub source: Option<String>,
    /// Lines with their start time, when a synced source had them (empty = plain text only).
    #[serde(default)]
    pub synced: Vec<LyricLine>,
}

/// One line of time-synced lyrics.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct LyricLine {
    pub time_ms: u64,
    /// Empty for an instrumental gap.
    pub text: String,
}

impl Lyrics {
    /// Index of the line being sung at `position_ms`, if the first line has started.
    pub fn current_line(&self, position_ms: u64) -> Option<usize> {
        self.synced
            .partition_point(|l| l.time_ms <= position_ms)
            .checked_sub(1)
    }
}
