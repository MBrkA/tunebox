#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("network error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("unexpected response shape: {0}")]
    Parse(String),
    #[error("track is not playable: {0}")]
    Unplayable(String),
    #[error("no suitable audio stream found")]
    NoStream,
    #[error("external resolver failed: {0}")]
    External(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, ApiError>;
