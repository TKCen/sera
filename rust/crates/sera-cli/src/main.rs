//! `sera` binary entry point.
//!
//! Parses the top-level CLI flags (`--config`, `--verbose`), initialises
//! tracing, loads config, then dispatches to the appropriate subcommand.

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use sera_commands::{CommandArgs, CommandContext};

use sera_cli::config::CliConfig;

/// SERA — Sandboxed Extensible Reasoning Agent CLI
#[derive(Parser)]
#[command(
    name = "sera",
    about = "SERA CLI — interact with the SERA gateway",
    version
)]
struct Cli {
    /// Path to config file (default: ~/.sera/config.toml)
    #[arg(long, short = 'c', global = true, value_name = "PATH")]
    config: Option<PathBuf>,

    /// Enable verbose logging
    #[arg(long, short = 'v', global = true)]
    verbose: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Check gateway liveness (GET /api/health)
    Ping {
        /// Gateway base URL (overrides config endpoint)
        #[arg(long, short = 'e', value_name = "URL")]
        endpoint: Option<String>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialise tracing — verbose flag enables DEBUG, otherwise INFO.
    let filter = if cli.verbose { "debug" } else { "info" };
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(filter)),
        )
        .with_target(false)
        .init();

    // Load config (graceful if file absent).
    let config_path = cli
        .config
        .unwrap_or_else(CliConfig::default_path);
    let config = CliConfig::load(&config_path)
        .with_context(|| format!("loading config from {}", config_path.display()))?;
    tracing::debug!(?config_path, "config loaded");

    let registry = sera_cli::build_registry();
    let ctx = CommandContext::new();

    match cli.command {
        Commands::Ping { endpoint } => {
            let mut args = CommandArgs::new();
            // Precedence: --endpoint flag > config.endpoint
            let resolved = endpoint.unwrap_or_else(|| config.endpoint.clone());
            args.insert("endpoint", resolved);

            let cmd = registry
                .get("ping")
                .context("ping command not registered")?;
            let result = cmd
                .execute(args, &ctx)
                .await
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            if result.exit_code != 0 {
                std::process::exit(result.exit_code);
            }
        }
    }

    Ok(())
}
