//! CLI command definitions using clap derive.

/// SERA agent platform CLI.
#[derive(clap::Parser)]
#[command(name = "sera", about = "SERA agent platform CLI")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// API server URL
    #[arg(long, env = "SERA_API_URL", default_value = "http://localhost:3001")]
    pub api_url: String,

    /// API key
    #[arg(long, env = "SERA_API_KEY", default_value = "sera_bootstrap_dev_123")]
    pub api_key: String,
}

/// Top-level subcommands.
#[derive(clap::Subcommand)]
pub enum Commands {
    /// Launch interactive TUI (default when no command given)
    Tui,
    /// Manage agent instances
    Agent {
        #[command(subcommand)]
        subcommand: AgentCommands,
    },
    /// Manage sessions
    Session {
        #[command(subcommand)]
        subcommand: SessionCommands,
    },
    /// Check API health
    Health,
    /// Start a chat session with an agent (placeholder)
    Chat {
        /// Agent ID or name
        agent: String,
    },
    /// Manage configuration
    Config {
        #[command(subcommand)]
        subcommand: ConfigCommands,
    },
}

/// Agent subcommands.
#[derive(clap::Subcommand)]
pub enum AgentCommands {
    /// List all agent instances
    List,
    /// Show details for a specific agent
    Show {
        /// Agent ID
        id: String,
    },
    /// Start an agent instance
    Start {
        /// Agent ID
        id: String,
    },
    /// Stop an agent instance
    Stop {
        /// Agent ID
        id: String,
    },
}

/// Session subcommands.
#[derive(clap::Subcommand)]
pub enum SessionCommands {
    /// List all sessions
    List,
    /// Show details for a specific session
    Show {
        /// Session ID
        id: String,
    },
}

/// Config subcommands.
#[derive(clap::Subcommand)]
pub enum ConfigCommands {
    /// Get a configuration value
    Get {
        /// Configuration key
        key: String,
    },
    /// Set a configuration value
    Set {
        /// Configuration key
        key: String,
        /// Configuration value
        value: String,
    },
}
