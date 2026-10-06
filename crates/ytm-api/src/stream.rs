//! Audio stream resolution.
//!
//! Primary: call the `player` endpoint as the `VISIONOS` client. Its URLs are
//! plain (no signature cipher) and are not gated by a PO token, so no JS
//! runtime is needed. Fallback: shell out to `yt-dlp`. See `docs/DECISIONS.md`.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::process::Command;

use crate::client::InnerTube;
use crate::error::{ApiError, Result};
use crate::models::StreamInfo;
use crate::StreamResolver;

const PLAYER_URL: &str = "https://www.youtube.com/youtubei/v1/player?prettyPrint=false";
const VISION_UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 15_7_3) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.0 Safari/605.1.15";
const VISION_VERSION: &str = "1.02";

/// Picks the best format from `adaptiveFormats` according to `preferred` itag order.
pub fn pick_format<'a>(formats: &'a [Value], preferred: &[u32]) -> Option<&'a Value> {
    let usable = |f: &&Value| f.get("url").and_then(Value::as_str).is_some();
    preferred.iter().find_map(|itag| {
        formats
            .iter()
            .filter(usable)
            .find(|f| f.get("itag").and_then(Value::as_u64) == Some(u64::from(*itag)))
    })
}

fn num(v: &Value, key: &str) -> Option<u64> {
    match v.get(key)? {
        Value::String(s) => s.parse().ok(),
        other => other.as_u64(),
    }
}

/// Turns a `player` response into a [`StreamInfo`].
pub fn parse_player_response(resp: &Value, preferred: &[u32]) -> Result<StreamInfo> {
    let status = resp
        .pointer("/playabilityStatus/status")
        .and_then(Value::as_str)
        .unwrap_or("UNKNOWN");
    if status != "OK" {
        let reason = resp
            .pointer("/playabilityStatus/reason")
            .and_then(Value::as_str)
            .unwrap_or(status);
        return Err(ApiError::Unplayable(reason.to_owned()));
    }
    let formats = resp
        .pointer("/streamingData/adaptiveFormats")
        .and_then(Value::as_array)
        .ok_or(ApiError::NoStream)?;
    let f = pick_format(formats, preferred).ok_or(ApiError::NoStream)?;
    Ok(StreamInfo {
        url: f["url"].as_str().unwrap_or_default().to_owned(),
        itag: num(f, "itag").unwrap_or(0) as u32,
        mime_type: f
            .get("mimeType")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        content_length: num(f, "contentLength"),
        duration_ms: num(f, "approxDurationMs"),
        user_agent: VISION_UA.to_owned(),
        expires_in_secs: resp
            .pointer("/streamingData/expiresInSeconds")
            .and_then(Value::as_str)
            .and_then(|s| s.parse().ok()),
        loudness_db: f
            .get("loudnessDb")
            .and_then(Value::as_f64)
            .map(|v| v as f32),
    })
}

/// Native resolver using the `VISIONOS` InnerTube client.
pub struct InnerTubeResolver {
    tube: Arc<InnerTube>,
    preferred: Vec<u32>,
}

impl InnerTubeResolver {
    pub fn new(tube: Arc<InnerTube>, preferred_itags: Vec<u32>) -> Self {
        Self {
            tube,
            preferred: preferred_itags,
        }
    }

    async fn fetch(&self, video_id: &str, visitor: &str) -> Result<Value> {
        let body = json!({
            "context": {"client": {
                "clientName": "VISIONOS",
                "clientVersion": VISION_VERSION,
                "deviceMake": "Apple",
                "deviceModel": "RealityDevice17,1",
                "osName": "visionOS",
                "osVersion": "26.5.23O471",
                "hl": "en",
                "visitorData": visitor,
            }},
            "videoId": video_id,
            "contentCheckOk": true,
            "racyCheckOk": true,
        });
        let resp = self
            .tube
            .http()
            .post(PLAYER_URL)
            .header("User-Agent", VISION_UA)
            .header("Origin", "https://www.youtube.com")
            .header("X-YouTube-Client-Name", "101")
            .header("X-YouTube-Client-Version", VISION_VERSION)
            .header("X-Goog-Visitor-Id", visitor)
            .json(&body)
            .send()
            .await?
            .error_for_status()?;
        Ok(resp.json().await?)
    }
}

#[async_trait]
impl StreamResolver for InnerTubeResolver {
    async fn resolve(&self, video_id: &str) -> Result<StreamInfo> {
        let mut last = ApiError::NoStream;
        // A stale/rejected visitor id shows up as "Sign in to confirm…"; get a fresh one once.
        for attempt in 0..2 {
            let visitor = self.tube.visitor_data().await?;
            let resp = self.fetch(video_id, &visitor).await?;
            match parse_player_response(&resp, &self.preferred) {
                Ok(info) => return Ok(info),
                Err(e @ ApiError::Unplayable(_)) if attempt == 0 => {
                    tracing::debug!(error = %e, "player rejected visitor, refreshing");
                    self.tube.reset_visitor();
                    last = e;
                }
                Err(e) => return Err(e),
            }
        }
        Err(last)
    }
}

/// Fallback resolver that runs `yt-dlp --dump-single-json`.
pub struct YtDlpResolver {
    binary: PathBuf,
}

impl YtDlpResolver {
    pub fn new(binary: Option<PathBuf>) -> Self {
        Self {
            binary: binary.unwrap_or_else(|| "yt-dlp".into()),
        }
    }
}

pub fn parse_ytdlp_json(v: &Value) -> Result<StreamInfo> {
    let url = v
        .get("url")
        .and_then(Value::as_str)
        .ok_or(ApiError::NoStream)?;
    let ua = v
        .pointer("/http_headers/User-Agent")
        .and_then(Value::as_str)
        .unwrap_or(VISION_UA);
    let ext = v.get("ext").and_then(Value::as_str).unwrap_or("m4a");
    Ok(StreamInfo {
        url: url.to_owned(),
        itag: v
            .get("format_id")
            .and_then(Value::as_str)
            .and_then(|s| s.parse().ok())
            .unwrap_or(0),
        mime_type: format!("audio/{ext}"),
        content_length: v
            .get("filesize")
            .or_else(|| v.get("filesize_approx"))
            .and_then(Value::as_u64),
        duration_ms: v
            .get("duration")
            .and_then(Value::as_f64)
            .map(|d| (d * 1000.0) as u64),
        user_agent: ua.to_owned(),
        expires_in_secs: None,
        loudness_db: None,
    })
}

#[async_trait]
impl StreamResolver for YtDlpResolver {
    async fn resolve(&self, video_id: &str) -> Result<StreamInfo> {
        let mut cmd = Command::new(&self.binary);
        // No flashing console window when launched from the GUI app on Windows.
        #[cfg(windows)]
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        let out = cmd
            .args([
                "--dump-single-json",
                "--no-playlist",
                "--no-warnings",
                "-f",
                "140/bestaudio[ext=m4a]",
            ])
            .arg(format!("https://music.youtube.com/watch?v={video_id}"))
            .kill_on_drop(true)
            .output()
            .await
            .map_err(|e| {
                ApiError::External(format!("cannot run {}: {e}", self.binary.display()))
            })?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            return Err(ApiError::External(
                err.lines().last().unwrap_or("yt-dlp failed").to_owned(),
            ));
        }
        parse_ytdlp_json(&serde_json::from_slice(&out.stdout)?)
    }
}

/// Tries each resolver in order, returning the first success.
pub struct ChainResolver {
    resolvers: Vec<Box<dyn StreamResolver>>,
}

impl ChainResolver {
    pub fn new(resolvers: Vec<Box<dyn StreamResolver>>) -> Self {
        Self { resolvers }
    }

    /// Native resolver first, `yt-dlp` as fallback.
    pub fn standard(tube: Arc<InnerTube>, cfg: &ytm_core::Config) -> Self {
        Self::new(vec![
            Box::new(InnerTubeResolver::new(tube, cfg.preferred_itags.clone())),
            Box::new(YtDlpResolver::new(cfg.ytdlp_path.clone())),
        ])
    }
}

#[async_trait]
impl StreamResolver for ChainResolver {
    async fn resolve(&self, video_id: &str) -> Result<StreamInfo> {
        let mut last = ApiError::NoStream;
        for r in &self.resolvers {
            match r.resolve(video_id).await {
                Ok(info) => return Ok(info),
                Err(e) => {
                    tracing::warn!(error = %e, "stream resolver failed, trying next");
                    last = e;
                }
            }
        }
        Err(last)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefers_listed_itag_order_and_skips_cipher_only() {
        let formats = vec![
            json!({"itag": 251, "signatureCipher": "x"}),
            json!({"itag": 140, "url": "u140", "mimeType": "audio/mp4", "contentLength": "100"}),
            json!({"itag": 139, "url": "u139"}),
        ];
        let f = pick_format(&formats, &[251, 140, 139]).unwrap();
        assert_eq!(f["itag"], 140);
        assert!(pick_format(&formats, &[18]).is_none());
    }

    #[test]
    fn player_response_ok_and_unplayable() {
        let ok = json!({
            "playabilityStatus": {"status": "OK"},
            "streamingData": {
                "expiresInSeconds": "21540",
                "adaptiveFormats": [{
                    "itag": 140, "url": "https://x/y", "mimeType": "audio/mp4; codecs=\"mp4a.40.2\"",
                    "contentLength": "5185422", "approxDurationMs": "320293", "loudnessDb": -3.5
                }]
            }
        });
        let info = parse_player_response(&ok, &[251, 140]).unwrap();
        assert_eq!(info.itag, 140);
        assert_eq!(info.content_length, Some(5185422));
        assert_eq!(info.duration_ms, Some(320293));
        assert_eq!(info.expires_in_secs, Some(21540));
        assert_eq!(info.loudness_db, Some(-3.5));

        let bad = json!({"playabilityStatus": {"status": "LOGIN_REQUIRED", "reason": "Sign in"}});
        assert!(matches!(
            parse_player_response(&bad, &[140]),
            Err(ApiError::Unplayable(r)) if r == "Sign in"
        ));
        let empty = json!({"playabilityStatus": {"status": "OK"}});
        assert!(matches!(
            parse_player_response(&empty, &[140]),
            Err(ApiError::NoStream)
        ));
    }

    #[test]
    fn ytdlp_json() {
        let v = json!({"url": "https://g/v", "format_id": "140", "ext": "m4a",
            "filesize": 99, "duration": 12.5, "http_headers": {"User-Agent": "UA"}});
        let i = parse_ytdlp_json(&v).unwrap();
        assert_eq!(
            (i.itag, i.content_length, i.duration_ms),
            (140, Some(99), Some(12500))
        );
        assert_eq!(i.user_agent, "UA");
        assert!(parse_ytdlp_json(&json!({})).is_err());
    }
}
