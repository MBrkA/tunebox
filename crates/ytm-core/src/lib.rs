//! Shared plumbing: configuration and logging.

pub mod config;
pub mod logging;

pub use config::{Config, ConfigError};
