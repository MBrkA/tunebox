use tracing_subscriber::{fmt, prelude::*, EnvFilter};

/// Installs the global `tracing` subscriber. Honors `RUST_LOG`; defaults to
/// `info` for our crates. Safe to call more than once (later calls are no-ops).
///
/// Credentials must never be passed to tracing macros; callers log opaque
/// identifiers only.
pub fn init() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        EnvFilter::new("warn,ytm_core=info,ytm_api=info,ytm_player=info,ytm_app=info,tunebox=info")
    });
    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_target(true))
        .try_init();
}
