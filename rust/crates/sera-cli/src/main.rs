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
    /// Manage and run agent instances
    Agent {
        #[command(subcommand)]
        command: AgentCommand,
    },
    /// Interactive streaming REPL against an agent
    Chat(ChatArgs),
    /// Manage LLM provider configurations
    Provider {
        #[command(subcommand)]
        command: ProviderCommand,
    },
    /// Alias for `provider select <name>` (one-shot dispatch).
    Model {
        /// Provider instance name to set as default
        name: String,
    },
    /// First-run bootstrap wizard — produces a working SERA config
    Init {
        /// Ingest a YAML manifest set non-interactively
        #[arg(long, value_name = "PATH")]
        from_file: Option<PathBuf>,
        /// Defaults-only local bootstrap (CI mode)
        #[arg(long)]
        non_interactive: bool,
        /// Overwrite existing config.yaml without prompting
        #[arg(long)]
        force: bool,
    },
    /// Inspect or mutate workflows via the gateway admin port
    Workflow {
        #[command(subcommand)]
        command: WorkflowCommand,
    },
    /// Manage capability policies
    Policy {
        #[command(subcommand)]
        command: PolicyCommand,
    },
    /// Inspect / cancel active sessions
    Session {
        #[command(subcommand)]
        command: SessionCommand,
    },
    /// Arm / disarm / inspect the gateway killswitch
    Killswitch {
        #[command(subcommand)]
        command: KillswitchCommand,
    },
    /// Read or modify config.yaml locally (no gateway round-trip)
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
}

#[derive(Subcommand)]
enum WorkflowCommand {
    /// List workflows
    List,
    /// Show one workflow by id
    Show {
        id: String,
    },
    /// Create a workflow from a manifest file (deferred to L.5)
    Create {
        file: PathBuf,
    },
    /// Delete a workflow by id (deferred to L.5)
    Delete {
        id: String,
    },
}

#[derive(Subcommand)]
enum PolicyCommand {
    /// List capability policies
    List,
    /// Show one policy by name
    Show {
        name: String,
    },
    /// Reload policies from disk
    Reload,
    /// Validate every policy YAML
    Validate,
}

#[derive(Subcommand)]
enum SessionCommand {
    /// List active sessions
    List,
    /// Show one session by id
    Show {
        id: String,
    },
    /// Cancel a session
    Kill {
        id: String,
    },
    /// Paginated browser (TTY-aware)
    Browse,
}

#[derive(Subcommand)]
enum KillswitchCommand {
    /// Arm the killswitch
    Arm,
    /// Disarm the killswitch
    Disarm,
    /// Print killswitch status
    Status,
}

#[derive(Subcommand)]
enum ConfigCommand {
    /// Read a value by dotted path
    Get {
        key: String,
    },
    /// Write a string value at a dotted path
    Set {
        key: String,
        value: String,
    },
    /// Open config.yaml in $EDITOR
    Edit,
    /// Print the resolved config.yaml path
    Path,
}

#[derive(Subcommand)]
enum ProviderCommand {
    /// List built-in providers and configured instances
    List,
    /// Add a provider instance interactively
    Add {
        /// Registry id (e.g. openrouter, anthropic, lm-studio)
        id: Option<String>,
        /// Instance name (default: same as id)
        #[arg(long, value_name = "NAME")]
        name: Option<String>,
    },
    /// Remove a configured provider instance
    Remove {
        /// Instance name to remove
        name: String,
    },
    /// Set a provider instance as the CLI default
    Select {
        /// Instance name
        name: String,
    },
    /// Re-run the wizard for an existing provider
    Configure {
        /// Instance name
        name: String,
    },
}

#[derive(clap::Args)]
struct ChatArgs {
    /// Session to resume.  If absent, --agent must be given.
    session_id: Option<String>,
    /// Agent to open a new session against.
    #[arg(long)]
    agent: Option<String>,
    /// Gateway base URL (overrides config endpoint)
    #[arg(long, short = 'e', value_name = "URL")]
    endpoint: Option<String>,
    /// Alias for --endpoint (kept for parity with the bead spec)
    #[arg(long, value_name = "URL")]
    api_url: Option<String>,
}

#[derive(Subcommand)]
enum AgentCommand {
    /// List all agent instances (GET /api/agents)
    List {
        /// Gateway base URL (overrides config endpoint)
        #[arg(long, short = 'e', value_name = "URL")]
        endpoint: Option<String>,
        /// Output raw JSON array
        #[arg(long)]
        json: bool,
    },
    /// Show full detail for an agent instance (GET /api/agents/:id)
    Show {
        /// Agent instance ID
        id: String,
        /// Gateway base URL (overrides config endpoint)
        #[arg(long, short = 'e', value_name = "URL")]
        endpoint: Option<String>,
        /// Output raw JSON
        #[arg(long)]
        json: bool,
    },
    /// Post a prompt to an agent and print the reply (POST /api/chat)
    Run {
        /// Agent instance ID or name
        id: String,
        /// Prompt to send to the agent
        prompt: String,
        /// Gateway base URL (overrides config endpoint)
        #[arg(long, short = 'e', value_name = "URL")]
        endpoint: Option<String>,
        /// Output raw JSON response for debugging
        #[arg(long)]
        raw: bool,
        /// Disable streaming; return the full reply in a single JSON response
        #[arg(long)]
        no_stream: bool,
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

        Commands::Agent { command } => match command {
            AgentCommand::List { endpoint, json } => {
                let mut args = CommandArgs::new();
                let resolved = endpoint.unwrap_or_else(|| config.endpoint.clone());
                args.insert("endpoint", resolved);
                if json {
                    args.insert("json", "true".to_string());
                }
                let cmd = registry
                    .get("agent:list")
                    .context("agent:list command not registered")?;
                let result = cmd
                    .execute(args, &ctx)
                    .await
                    .map_err(|e| anyhow::anyhow!("{e}"))?;
                if result.exit_code != 0 {
                    std::process::exit(result.exit_code);
                }
            }

            AgentCommand::Show { id, endpoint, json } => {
                let mut args = CommandArgs::new();
                let resolved = endpoint.unwrap_or_else(|| config.endpoint.clone());
                args.insert("endpoint", resolved);
                args.insert("id", id);
                if json {
                    args.insert("json", "true".to_string());
                }
                let cmd = registry
                    .get("agent:show")
                    .context("agent:show command not registered")?;
                let result = cmd
                    .execute(args, &ctx)
                    .await
                    .map_err(|e| anyhow::anyhow!("{e}"))?;
                if result.exit_code != 0 {
                    std::process::exit(result.exit_code);
                }
            }

            AgentCommand::Run { id, prompt, endpoint, raw, no_stream } => {
                let mut args = CommandArgs::new();
                let resolved = endpoint.unwrap_or_else(|| config.endpoint.clone());
                args.insert("endpoint", resolved);
                args.insert("id", id);
                args.insert("prompt", prompt);
                if raw {
                    args.insert("raw", "true".to_string());
                }
                if no_stream {
                    args.insert("no-stream", "true".to_string());
                }
                let cmd = registry
                    .get("agent:run")
                    .context("agent:run command not registered")?;
                let result = cmd
                    .execute(args, &ctx)
                    .await
                    .map_err(|e| anyhow::anyhow!("{e}"))?;
                if result.exit_code != 0 {
                    std::process::exit(result.exit_code);
                }
            }
        },

        Commands::Provider { command } => {
            let cmd_name = match &command {
                ProviderCommand::List => "provider:list",
                ProviderCommand::Add { .. } => "provider:add",
                ProviderCommand::Remove { .. } => "provider:remove",
                ProviderCommand::Select { .. } => "provider:select",
                ProviderCommand::Configure { .. } => "provider:configure",
            };

            let mut args = CommandArgs::new();
            match command {
                ProviderCommand::List => {}
                ProviderCommand::Add { id, name } => {
                    if let Some(v) = id {
                        args.insert("id", v);
                    }
                    if let Some(v) = name {
                        args.insert("name", v);
                    }
                }
                ProviderCommand::Remove { name } => {
                    args.insert("name", name);
                }
                ProviderCommand::Select { name } => {
                    args.insert("name", name);
                }
                ProviderCommand::Configure { name } => {
                    args.insert("name", name);
                }
            }

            let cmd = registry
                .get(cmd_name)
                .with_context(|| format!("{cmd_name} command not registered"))?;
            let result = cmd
                .execute(args, &ctx)
                .await
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            if result.exit_code != 0 {
                std::process::exit(result.exit_code);
            }
        }

        Commands::Init {
            from_file,
            non_interactive,
            force,
        } => {
            let mut args = CommandArgs::new();
            if let Some(p) = from_file {
                args.insert("from-file", p.display().to_string());
            }
            if non_interactive {
                args.insert("non-interactive", "true".to_string());
            }
            if force {
                args.insert("force", "true".to_string());
            }
            let cmd = registry
                .get("init")
                .context("init command not registered")?;
            let result = cmd
                .execute(args, &ctx)
                .await
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            if result.exit_code != 0 {
                std::process::exit(result.exit_code);
            }
        }

        Commands::Model { name } => {
            let mut args = CommandArgs::new();
            args.insert("name", name);
            let cmd = registry
                .get("provider:select")
                .context("provider:select command not registered")?;
            let result = cmd
                .execute(args, &ctx)
                .await
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            if result.exit_code != 0 {
                std::process::exit(result.exit_code);
            }
        }

        Commands::Workflow { command } => {
            let (cmd_name, mut args) = match command {
                WorkflowCommand::List => ("workflow:list", CommandArgs::new()),
                WorkflowCommand::Show { id } => {
                    let mut a = CommandArgs::new();
                    a.insert("id", id);
                    ("workflow:show", a)
                }
                WorkflowCommand::Create { file } => {
                    let mut a = CommandArgs::new();
                    a.insert("file", file.display().to_string());
                    ("workflow:create", a)
                }
                WorkflowCommand::Delete { id } => {
                    let mut a = CommandArgs::new();
                    a.insert("id", id);
                    ("workflow:delete", a)
                }
            };
            let _ = &mut args; // (kept mut for symmetry with other dispatch arms)
            let cmd = registry
                .get(cmd_name)
                .with_context(|| format!("{cmd_name} command not registered"))?;
            let result = cmd
                .execute(args, &ctx)
                .await
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            if result.exit_code != 0 {
                std::process::exit(result.exit_code);
            }
        }

        Commands::Policy { command } => {
            let (cmd_name, args) = match command {
                PolicyCommand::List => ("policy:list", CommandArgs::new()),
                PolicyCommand::Show { name } => {
                    let mut a = CommandArgs::new();
                    a.insert("name", name);
                    ("policy:show", a)
                }
                PolicyCommand::Reload => ("policy:reload", CommandArgs::new()),
                PolicyCommand::Validate => ("policy:validate", CommandArgs::new()),
            };
            let cmd = registry
                .get(cmd_name)
                .with_context(|| format!("{cmd_name} command not registered"))?;
            let result = cmd
                .execute(args, &ctx)
                .await
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            if result.exit_code != 0 {
                std::process::exit(result.exit_code);
            }
        }

        Commands::Session { command } => {
            let (cmd_name, args) = match command {
                SessionCommand::List => ("session:list", CommandArgs::new()),
                SessionCommand::Show { id } => {
                    let mut a = CommandArgs::new();
                    a.insert("id", id);
                    ("session:show", a)
                }
                SessionCommand::Kill { id } => {
                    let mut a = CommandArgs::new();
                    a.insert("id", id);
                    ("session:kill", a)
                }
                SessionCommand::Browse => ("session:browse", CommandArgs::new()),
            };
            let cmd = registry
                .get(cmd_name)
                .with_context(|| format!("{cmd_name} command not registered"))?;
            let result = cmd
                .execute(args, &ctx)
                .await
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            if result.exit_code != 0 {
                std::process::exit(result.exit_code);
            }
        }

        Commands::Killswitch { command } => {
            let cmd_name = match command {
                KillswitchCommand::Arm => "killswitch:arm",
                KillswitchCommand::Disarm => "killswitch:disarm",
                KillswitchCommand::Status => "killswitch:status",
            };
            let cmd = registry
                .get(cmd_name)
                .with_context(|| format!("{cmd_name} command not registered"))?;
            let result = cmd
                .execute(CommandArgs::new(), &ctx)
                .await
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            if result.exit_code != 0 {
                std::process::exit(result.exit_code);
            }
        }

        Commands::Config { command } => {
            let (cmd_name, args) = match command {
                ConfigCommand::Get { key } => {
                    let mut a = CommandArgs::new();
                    a.insert("key", key);
                    ("config:get", a)
                }
                ConfigCommand::Set { key, value } => {
                    let mut a = CommandArgs::new();
                    a.insert("key", key);
                    a.insert("value", value);
                    ("config:set", a)
                }
                ConfigCommand::Edit => ("config:edit", CommandArgs::new()),
                ConfigCommand::Path => ("config:path", CommandArgs::new()),
            };
            let cmd = registry
                .get(cmd_name)
                .with_context(|| format!("{cmd_name} command not registered"))?;
            let result = cmd
                .execute(args, &ctx)
                .await
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            if result.exit_code != 0 {
                std::process::exit(result.exit_code);
            }
        }

        Commands::Chat(chat_args) => {
            let mut args = CommandArgs::new();
            let resolved = chat_args
                .api_url
                .clone()
                .or_else(|| chat_args.endpoint.clone())
                .unwrap_or_else(|| config.endpoint.clone());
            args.insert("endpoint", resolved);
            if let Some(a) = chat_args.agent {
                args.insert("agent", a);
            }
            if let Some(s) = chat_args.session_id {
                args.insert("session", s);
            }
            let cmd = registry
                .get("chat")
                .context("chat command not registered")?;
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
