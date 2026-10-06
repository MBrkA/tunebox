//! YouTube Music API layer.
//!
//! Everything InnerTube-specific lives behind the [`MusicApi`] and
//! [`StreamResolver`] traits so the UI and player can be tested against mocks
//! and the protocol code can be patched in one place when YouTube changes it.

pub mod client;
pub mod error;
pub mod models;
pub mod nav;
pub mod pages;
pub mod parse;
pub mod stream;

use async_trait::async_trait;

pub use client::InnerTube;
pub use error::{ApiError, Result};
pub use models::*;
pub use stream::{ChainResolver, InnerTubeResolver, YtDlpResolver};

/// Metadata/catalogue operations.
#[async_trait]
pub trait MusicApi: Send + Sync {
    async fn search(&self, query: &str, filter: SearchFilter) -> Result<SearchPage>;
    async fn search_continue(&self, continuation: &str) -> Result<SearchPage>;
    async fn search_suggestions(&self, query: &str) -> Result<Vec<String>>;

    async fn home(&self) -> Result<HomePage>;
    async fn home_continue(&self, continuation: &str) -> Result<HomePage>;
    async fn explore(&self) -> Result<HomePage>;
    async fn artist(&self, browse_id: &str) -> Result<ArtistPage>;
    async fn album(&self, browse_id: &str) -> Result<AlbumPage>;
    async fn playlist(&self, playlist_id: &str) -> Result<PlaylistPage>;
    /// Next page of tracks of a long playlist, plus the following token.
    async fn playlist_continue(&self, continuation: &str) -> Result<(Vec<Track>, Option<String>)>;
    /// Radio / up-next queue seeded from a track.
    async fn up_next(&self, video_id: &str) -> Result<UpNext>;
    async fn new_releases(&self) -> Result<HomePage>;
    /// Charts for a country code (`ZZ` = Global); `None` lets YouTube pick from the user's location.
    async fn charts(&self, country: Option<&str>) -> Result<ChartsPage>;
    /// The full "Moods & genres" page.
    async fn moods_and_genres(&self) -> Result<MoodsPage>;
    /// One mood/genre category (`params` from [`MoodCategory`]).
    async fn mood_category(&self, params: &str) -> Result<CategoryPage>;
    /// Switch the content language (`hl`) used for every following request.
    fn set_language(&self, _hl: &str) {}
    /// `None` when the track has no lyrics.
    async fn lyrics(&self, video_id: &str) -> Result<Option<Lyrics>>;
}

/// Turns a video id into a fetchable audio stream.
#[async_trait]
pub trait StreamResolver: Send + Sync {
    async fn resolve(&self, video_id: &str) -> Result<StreamInfo>;
}
