use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use calloop::channel::Sender;
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tracing::{info, warn};

/// Start watching the config file for changes.
/// Sends a message on the calloop channel when a reload is needed.
/// Returns the watcher handle (must be kept alive).
pub fn watch_config(calloop_tx: Sender<ConfigReloadEvent>) -> Result<RecommendedWatcher> {
    let config_dir = config_dir();
    if !config_dir.exists() {
        info!("config directory does not exist, skipping file watcher");
        return Ok(create_noop_watcher()?);
    }

    let mut debounce_last = Instant::now() - Duration::from_secs(10);
    let debounce_interval = Duration::from_millis(200);

    let mut watcher =
        notify::recommended_watcher(move |res: Result<notify::Event, notify::Error>| {
            match res {
                Ok(event) => {
                    let dominated_by_debounce = debounce_last.elapsed() < debounce_interval;
                    if dominated_by_debounce {
                        return;
                    }

                    match event.kind {
                        EventKind::Modify(_) | EventKind::Create(_) => {
                            let is_lua = event
                                .paths
                                .iter()
                                .any(|p| p.extension().is_some_and(|e| e == "lua"));
                            if is_lua {
                                debounce_last = Instant::now();
                                info!("config file changed, triggering reload");
                                let _ = calloop_tx.send(ConfigReloadEvent);
                            }
                        }
                        _ => {}
                    }
                }
                Err(e) => {
                    warn!("file watcher error: {e}");
                }
            }
        })
        .context("failed to create file watcher")?;

    watcher
        .watch(&config_dir, RecursiveMode::NonRecursive)
        .context("failed to watch config directory")?;

    info!(path = %config_dir.display(), "watching config directory for changes");
    Ok(watcher)
}

/// Event sent when config file changes.
pub struct ConfigReloadEvent;

fn config_dir() -> PathBuf {
    let xdg = std::env::var("XDG_CONFIG_HOME").unwrap_or_else(|_| {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        format!("{home}/.config")
    });
    PathBuf::from(xdg).join("notiser")
}

fn create_noop_watcher() -> Result<RecommendedWatcher> {
    notify::recommended_watcher(|_: Result<notify::Event, notify::Error>| {})
        .context("failed to create noop watcher")
}
