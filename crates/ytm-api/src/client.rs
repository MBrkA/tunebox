use std::sync::Mutex;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::error::{ApiError, Result};
use crate::models::*;
use crate::MusicApi;
use crate::{pages, parse};

const BASE: &str = "https://music.youtube.com/youtubei/v1";
const WEB_REMIX_VERSION: &str = "1.20260101.01.00";
const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64; rv:128.0) Gecko/20100101 Firefox/128.0";

/// Low-level InnerTube client speaking as the `WEB_REMIX` (music.youtube.com) client.
pub struct InnerTube {
    http: reqwest::Client,
    base: String,
    hl: Mutex<String>,
    visitor: Mutex<Option<String>>,
}

impl InnerTube {
    pub fn new(hl: impl Into<String>) -> Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(Duration::from_secs(20))
            .build()?;
        Ok(Self {
            http,
            base: BASE.to_owned(),
            hl: Mutex::new(hl.into()),
            visitor: Mutex::new(None),
        })
    }

    /// Points the client at a different InnerTube base URL (tests, proxies).
    pub fn with_base_url(mut self, base: impl Into<String>) -> Self {
        self.base = base.into();
        self
    }

    pub fn from_config(cfg: &ytm_core::Config) -> Result<Self> {
        Self::new(cfg.language.clone())
    }

    pub(crate) fn http(&self) -> &reqwest::Client {
        &self.http
    }

    fn context(&self) -> Value {
        json!({"client": {
            "clientName": "WEB_REMIX",
            "clientVersion": WEB_REMIX_VERSION,
            "hl": *self.hl.lock().unwrap_or_else(|e| e.into_inner()),
        }})
    }

    /// POSTs `body` (merged with the client context) to `endpoint` and returns the JSON.
    pub async fn post(&self, endpoint: &str, body: Value, query: &[(&str, &str)]) -> Result<Value> {
        let mut payload = json!({ "context": self.context() });
        if let (Some(dst), Value::Object(src)) = (payload.as_object_mut(), body) {
            dst.extend(src);
        }
        let request = self
            .http
            .post(format!("{}/{endpoint}", self.base))
            .query(&[("prettyPrint", "false")])
            .query(query)
            .header("Origin", "https://music.youtube.com")
            .json(&payload);
        let resp = request.send().await?.error_for_status()?;
        let value: Value = resp.json().await?;
        if let Some(v) = value
            .pointer("/responseContext/visitorData")
            .and_then(Value::as_str)
        {
            if let Ok(mut slot) = self.visitor.lock() {
                slot.get_or_insert_with(|| v.to_owned());
            }
        }
        Ok(value)
    }

    /// An anonymous visitor id, required by some other InnerTube clients.
    /// Obtained from any response's `responseContext`; cached after the first call.
    pub async fn visitor_data(&self) -> Result<String> {
        if let Some(v) = self.visitor.lock().ok().and_then(|g| g.clone()) {
            return Ok(v);
        }
        self.post("music/get_search_suggestions", json!({"input": "a"}), &[])
            .await?;
        self.visitor
            .lock()
            .ok()
            .and_then(|g| g.clone())
            .ok_or_else(|| ApiError::Parse("no visitorData in response".into()))
    }

    /// Forget the cached visitor id (e.g. after the server rejected it).
    pub fn reset_visitor(&self) {
        if let Ok(mut slot) = self.visitor.lock() {
            *slot = None;
        }
    }
}

#[async_trait]
impl MusicApi for InnerTube {
    fn set_language(&self, hl: &str) {
        *self.hl.lock().unwrap_or_else(|e| e.into_inner()) = hl.to_owned();
    }

    async fn search(&self, query: &str, filter: SearchFilter) -> Result<SearchPage> {
        let mut body = json!({ "query": query });
        if let Some(params) = filter.params() {
            body["params"] = json!(params);
        }
        let resp = self.post("search", body, &[]).await?;
        Ok(parse::parse_search(&resp))
    }

    async fn search_continue(&self, continuation: &str) -> Result<SearchPage> {
        let resp = self
            .post("search", json!({}), &continuation_query(continuation))
            .await?;
        Ok(parse::parse_search(&resp))
    }

    async fn home(&self) -> Result<HomePage> {
        let resp = self
            .post("browse", json!({"browseId": "FEmusic_home"}), &[])
            .await?;
        Ok(pages::parse_home(&resp))
    }

    async fn home_continue(&self, continuation: &str) -> Result<HomePage> {
        let resp = self
            .post("browse", json!({}), &continuation_query(continuation))
            .await?;
        Ok(pages::parse_home(&resp))
    }

    async fn explore(&self) -> Result<HomePage> {
        let resp = self
            .post("browse", json!({"browseId": "FEmusic_explore"}), &[])
            .await?;
        Ok(pages::parse_home(&resp))
    }

    async fn artist(&self, browse_id: &str) -> Result<ArtistPage> {
        let resp = self
            .post("browse", json!({"browseId": browse_id}), &[])
            .await?;
        let page = pages::parse_artist(&resp, browse_id);
        if page.name.is_empty() {
            return Err(ApiError::Parse("artist page has no header".into()));
        }
        Ok(page)
    }

    async fn album(&self, browse_id: &str) -> Result<AlbumPage> {
        let resp = self
            .post("browse", json!({"browseId": browse_id}), &[])
            .await?;
        let page = pages::parse_album(&resp, browse_id);
        if page.title.is_empty() {
            return Err(ApiError::Parse("album page has no header".into()));
        }
        Ok(page)
    }

    async fn playlist(&self, playlist_id: &str) -> Result<PlaylistPage> {
        let browse_id = if playlist_id.starts_with("VL") {
            playlist_id.to_owned()
        } else {
            format!("VL{playlist_id}")
        };
        let resp = self
            .post("browse", json!({"browseId": browse_id}), &[])
            .await?;
        let page = pages::parse_playlist(&resp, playlist_id.trim_start_matches("VL"));
        if page.title.is_empty() && page.tracks.is_empty() {
            return Err(ApiError::Parse("playlist page is empty".into()));
        }
        Ok(page)
    }

    async fn playlist_continue(&self, continuation: &str) -> Result<(Vec<Track>, Option<String>)> {
        let resp = self
            .post("browse", json!({}), &continuation_query(continuation))
            .await?;
        Ok((
            pages::playlist_rows(&resp),
            parse::parse_continuation(&resp),
        ))
    }

    async fn up_next(&self, video_id: &str) -> Result<UpNext> {
        let resp = self
            .post(
                "next",
                json!({
                    "videoId": video_id,
                    "playlistId": format!("RDAMVM{video_id}"),
                    "isAudioOnly": true,
                }),
                &[],
            )
            .await?;
        Ok(pages::parse_up_next(&resp))
    }

    async fn new_releases(&self) -> Result<HomePage> {
        let resp = self
            .post("browse", json!({"browseId": "FEmusic_new_releases"}), &[])
            .await?;
        let page = pages::parse_home(&resp);
        if page.sections.is_empty() {
            return Err(ApiError::Parse("new releases page is empty".into()));
        }
        Ok(page)
    }

    async fn charts(&self, country: Option<&str>) -> Result<ChartsPage> {
        let mut body = json!({"browseId": "FEmusic_charts"});
        if let Some(code) = country.filter(|c| !c.is_empty()) {
            body["formData"] = json!({"selectedValues": [code]});
        }
        let resp = self.post("browse", body, &[]).await?;
        let page = pages::parse_charts(&resp);
        if page.sections.is_empty() {
            return Err(ApiError::Parse("charts page is empty".into()));
        }
        Ok(page)
    }

    async fn moods_and_genres(&self) -> Result<MoodsPage> {
        let resp = self
            .post(
                "browse",
                json!({"browseId": "FEmusic_moods_and_genres"}),
                &[],
            )
            .await?;
        let page = pages::parse_moods_page(&resp);
        if page.groups.is_empty() {
            return Err(ApiError::Parse("moods & genres page is empty".into()));
        }
        Ok(page)
    }

    async fn mood_category(&self, params: &str) -> Result<CategoryPage> {
        let resp = self
            .post(
                "browse",
                json!({"browseId": pages::MOOD_CATEGORY_ID, "params": params}),
                &[],
            )
            .await?;
        let page = pages::parse_mood_category(&resp);
        if page.sections.is_empty() {
            return Err(ApiError::Parse("category has no playlists".into()));
        }
        Ok(page)
    }

    async fn lyrics(&self, video_id: &str) -> Result<Option<Lyrics>> {
        let next = self
            .post(
                "next",
                json!({"videoId": video_id, "isAudioOnly": true}),
                &[],
            )
            .await?;
        let Some(browse_id) = pages::parse_lyrics_browse_id(&next) else {
            return Ok(None);
        };
        let resp = self
            .post("browse", json!({"browseId": browse_id}), &[])
            .await?;
        Ok(pages::parse_lyrics(&resp))
    }

    async fn search_suggestions(&self, query: &str) -> Result<Vec<String>> {
        let resp = self
            .post("music/get_search_suggestions", json!({"input": query}), &[])
            .await?;
        Ok(parse::parse_suggestions(&resp))
    }
}

fn continuation_query(token: &str) -> [(&str, &str); 3] {
    [("ctoken", token), ("continuation", token), ("type", "next")]
}
