#[cfg(feature = "legacy-v2")]
mod bridge;
#[cfg(feature = "legacy-v2")]
mod candidate;
mod cli;
mod config;
mod daemon;
mod hot;
mod mcp;
mod pipeline;
mod project;
mod runtime;
mod service;

use clap::Parser;
use cli::{Cli, Command};
use tracing::info;

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Parse CLI; if no subcommand, default to MCP for backwards compat (stdio)
    let cli = Cli::parse();

    // For MCP, stdout must be pure JSON-RPC — tracing to stderr only
    // For CLI, tracing also to stderr; json stdout clean is enforced in cli.rs
    // Initialize tracing early but allow cli.rs to handle debug flag; init here with env filter
    // We defer init to cli.rs for search paths, but for MCP we need it now
    match &cli.command {
        Some(Command::Mcp) | None => {
            // MCP mode: init tracing to stderr
            let _ = tracing_subscriber::fmt()
                .with_env_filter(
                    tracing_subscriber::EnvFilter::try_from_default_env()
                        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
                )
                .with_writer(std::io::stderr)
                .try_init();
            info!(version = %VERSION, pid = %std::process::id(), "starting contextd mcp");
            // Use new native MCP adapter (service-based)
            // If invoked without subcommand, treat as MCP for backwards compat
            let root = cli.root.clone();
            mcp::run_mcp(root).await?;
            return Ok(());
        }
        _ => {
            // CLI mode handled by cli::run_cli which does its own tracing init (try_init)
            cli::run_cli(cli).await?;
        }
    }
    Ok(())
}
