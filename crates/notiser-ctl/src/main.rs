mod commands;
mod output;

use anyhow::Result;
use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "notiser-ctl", about = "Control tool for the notiser daemon")]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Output format
    #[arg(long, default_value = "text")]
    format: output::OutputFormat,
}

#[derive(Subcommand)]
enum Commands {
    /// List active notifications
    List,
    /// Close a notification by ID
    Close {
        /// Notification ID to close
        id: u32,
    },
    /// Close all notifications
    CloseAll,
    /// Toggle Do Not Disturb mode
    Dnd,
    /// Reload configuration
    Reload {
        /// Hard reload (rebuild surfaces)
        #[arg(long)]
        hard: bool,
    },
    /// Show notification history
    History {
        /// Maximum entries to show
        #[arg(short, long, default_value = "20")]
        limit: u32,
    },
    /// Show daemon status
    Inspect,
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let _cli = Cli::parse();

    // Phase 7 will implement the D-Bus client calls
    tracing::info!("notiser-ctl: not yet connected to daemon");
    Ok(())
}
