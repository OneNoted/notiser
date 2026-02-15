use anyhow::Result;
use tracing::info;

pub fn run() -> Result<()> {
    info!("notiser daemon starting up");
    // Phase 1 will fill this in with calloop event loop
    Ok(())
}
