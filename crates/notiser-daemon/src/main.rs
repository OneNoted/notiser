mod animation;
mod app;
#[cfg(feature = "audio")]
mod audio;
mod config;
mod dbus;
mod monitor;
mod notification;
mod presentation;
mod wayland;

use anyhow::Result;
use tracing_subscriber::EnvFilter;

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("notiser=info".parse()?))
        .init();

    tracing::info!("starting notiser daemon");
    app::run()
}
