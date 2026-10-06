//! Audio playback: HTTP streaming source, decoding, output and queue.
//!
//! The entry point is [`Player`]: spawn it once, send it [`Command`]s and read
//! [`PlayerState`] snapshots. See `engine.rs` for the threading model.

pub mod decode;
pub mod engine;
pub mod fmp4;
pub mod output;
pub mod queue;
pub mod remote;
pub mod resample;

pub use engine::{Command, Player, PlayerEvent, PlayerOptions, PlayerState, Status};
pub use output::OutputKind;
pub use queue::{Queue, RemoveOutcome, Repeat};

#[derive(Debug, Clone, thiserror::Error)]
pub enum PlayerError {
    #[error("could not resolve stream: {0}")]
    Resolve(String),
    #[error("network/io error: {0}")]
    Io(String),
    #[error("decode error: {0}")]
    Decode(String),
    #[error("audio output error: {0}")]
    Output(String),
}

impl From<ytm_api::ApiError> for PlayerError {
    fn from(e: ytm_api::ApiError) -> Self {
        Self::Resolve(e.to_string())
    }
}
