//! `sera` binary entry point.
//!
//! Parses the top-level CLI flags (`--config`, `--verbose`), initialises
//! tracing, loads config, then dispatches to the appropriate subcommand.

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use sera_commands::{CommandArgs, CommandContext};

use sera_cli::config::CliConfig;
use sera_cli::token_store::best_available_store;

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
    /// Manage authentication (login, whoami, logout)
    Auth {
        #[command(subcommand)]
        command: AuthCommand,
    },
}

#[derive(Subcommand)]
enum AuthCommand {
    /// Authenticate and store a token
    Login {
        /// Gateway base URL (overrides config endpoint)
        #[arg(long, short = 'e', value_name = "URL")]
        endpoint: Option<String>,
        /// Supply token non-interactively (for scripts/tests)
        #[arg(long, value_name = "TOKEN")]
        token: Option<String>,
    },
    /// Print the currently authenticated principal
    Whoami {
        /// Gateway base URL (overrides config endpoint)
        #[arg(long, short = 'e', value_name = "URL")]
        endpoint: Option<String>,
    },
    /// Remove the stored token
    Logout,
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
    let config_path = cli.config.unwrap_or_else(CliConfig::default_path);
    let config = CliConfig::load(&config_path)
        .with_context(|| format!("loading config from {}", config_path.display()))?;
    tracing::debug!(?config_path, "config loaded");

    // Attempt to load the stored token and populate caller_id.
    let ctx = {
        let store = best_available_store();
        match store.load() {
            Ok(Some(token)) => {
                tracing::debug!("loaded stored token");
                // We don't have the sub yet — caller_id will be refined after /api/auth/me
                // when needed.  For now use a sentinel that indicates "authenticated".
                let _ = token; // token is threaded into the HTTP client per-command
                CommandContext::with_caller("authenticated")
            }
            Ok(None) => CommandContext::new(),
            Err(e) => {
                tracing::debug!("could not load token: {e}");
                CommandContext::new()
            }
        }
    };

    let registry = sera_cli::build_registry();

    match cli.command {
        Commands::Ping { endpoint } => {
            let mut args = CommandArgs::new();
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

        Commands::Auth { command } => match command {
            AuthCommand::Login { endpoint, token } => {
                let mut args = CommandArgs::new();
                let resolved = endpoint.unwrap_or_else(|| config.endpoint.clone());
                args.insert("endpoint", resolved);
                if let Some(t) = token {
                    args.insert("token", t);
                }
                let cmd = registry
                    .get("auth:login")
                    .context("auth:login command not registered")?;
                let result = cmd
                    .execute(args, &ctx)
                    .await
                    .map_err(|e| anyhow::anyhow!("{e}"))?;
                if result.exit_code != 0 {
                    std::process::exit(result.exit_code);
                }
            }

            AuthCommand::Whoami { endpoint } => {
                let mut args = CommandArgs::new();
                let resolved = endpoint.unwrap_or_else(|| config.endpoint.clone());
                args.insert("endpoint", resolved);
                let cmd = registry
                    .get("auth:whoami")
                    .context("auth:whoami command not registered")?;
                let result = cmd
                    .execute(args, &ctx)
                    .await
                    .map_err(|e| anyhow::anyhow!("{e}"))?;
                if result.exit_code != 0 {
                    std::process::exit(result.exit_code);
                }
            }

            AuthCommand::Logout => {
                let cmd = registry
                    .get("auth:logout")
                    .context("auth:logout command not registered")?;
                let result = cmd
                    .execute(CommandArgs::new(), &ctx)
                    .await
                    .map_err(|e| anyhow::anyhow!("{e}"))?;
                if result.exit_code != 0 {
                    std::process::exit(result.exit_code);
                }
            }
        },
    }

    Ok(())
}
