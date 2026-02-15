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

    let cli = Cli::parse();

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    rt.block_on(async {
        match cli.command {
            Commands::List => commands::list(&cli.format).await,
            Commands::Close { id } => commands::close(id).await,
            Commands::CloseAll => commands::close_all().await,
            Commands::Dnd => commands::dnd().await,
            Commands::Reload { hard } => commands::reload(hard).await,
            Commands::History { limit } => commands::history(limit, &cli.format).await,
            Commands::Inspect => commands::inspect().await,
        }
    })
}
