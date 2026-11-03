use rust_apps_core::SebrusLoggerLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

/// Install the global tracing subscriber. A no-op when a subscriber is
/// already installed, so library consumers may call their own setup first.
pub fn init_tracing() {
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    let _ = tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_target(true)
                .with_thread_ids(true)
                .with_line_number(true),
        )
        .with(SebrusLoggerLayer::new())
        .with(env_filter)
        .try_init();
}
