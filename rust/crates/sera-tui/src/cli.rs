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

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::Cli;

    fn parse(args: &[&str]) -> Cli {
        Cli::parse_from(args)
    }

    fn try_parse(args: &[&str]) -> Result<Cli, clap::Error> {
        Cli::try_parse_from(args)
    }

    // --- Default flag values ---

    #[test]
    fn defaults_when_no_flags_given() {
        let cli = parse(&["sera"]);
        assert_eq!(cli.api_url, "http://localhost:3001");
        assert_eq!(cli.api_key, "sera_bootstrap_dev_123");
        assert!(cli.command.is_none());
    }

    // --- --api-url / --api-key overrides ---

    #[test]
    fn api_url_flag_overrides_default() {
        let cli = parse(&["sera", "--api-url", "http://example.com:9000"]);
        assert_eq!(cli.api_url, "http://example.com:9000");
    }

    #[test]
    fn api_key_flag_overrides_default() {
        let cli = parse(&["sera", "--api-key", "custom_key_abc"]);
        assert_eq!(cli.api_key, "custom_key_abc");
    }

    // --- Subcommand parsing ---

    #[test]
    fn health_subcommand_parsed() {
        use super::Commands;
        let cli = parse(&["sera", "health"]);
        assert!(matches!(cli.command, Some(Commands::Health)));
    }

    #[test]
    fn tui_subcommand_parsed() {
        use super::Commands;
        let cli = parse(&["sera", "tui"]);
        assert!(matches!(cli.command, Some(Commands::Tui)));
    }

    #[test]
    fn chat_subcommand_captures_agent() {
        use super::Commands;
        let cli = parse(&["sera", "chat", "agent-42"]);
        assert!(matches!(cli.command, Some(Commands::Chat { agent }) if agent == "agent-42"));
    }

    #[test]
    fn agent_list_subcommand_parsed() {
        use super::{AgentCommands, Commands};
        let cli = parse(&["sera", "agent", "list"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Agent { subcommand: AgentCommands::List })
        ));
    }

    #[test]
    fn agent_show_captures_id() {
        use super::{AgentCommands, Commands};
        let cli = parse(&["sera", "agent", "show", "my-agent-id"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Agent { subcommand: AgentCommands::Show { id } }) if id == "my-agent-id"
        ));
    }

    #[test]
    fn agent_start_captures_id() {
        use super::{AgentCommands, Commands};
        let cli = parse(&["sera", "agent", "start", "start-id"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Agent { subcommand: AgentCommands::Start { id } }) if id == "start-id"
        ));
    }

    #[test]
    fn agent_stop_captures_id() {
        use super::{AgentCommands, Commands};
        let cli = parse(&["sera", "agent", "stop", "stop-id"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Agent { subcommand: AgentCommands::Stop { id } }) if id == "stop-id"
        ));
    }

    #[test]
    fn session_list_subcommand_parsed() {
        use super::{Commands, SessionCommands};
        let cli = parse(&["sera", "session", "list"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Session { subcommand: SessionCommands::List })
        ));
    }

    #[test]
    fn session_show_captures_id() {
        use super::{Commands, SessionCommands};
        let cli = parse(&["sera", "session", "show", "sess-99"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Session { subcommand: SessionCommands::Show { id } }) if id == "sess-99"
        ));
    }

    #[test]
    fn config_get_captures_key() {
        use super::{Commands, ConfigCommands};
        let cli = parse(&["sera", "config", "get", "timeout"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Config { subcommand: ConfigCommands::Get { key } }) if key == "timeout"
        ));
    }

    #[test]
    fn config_set_captures_key_and_value() {
        use super::{Commands, ConfigCommands};
        let cli = parse(&["sera", "config", "set", "timeout", "30"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Config { subcommand: ConfigCommands::Set { key, value } })
                if key == "timeout" && value == "30"
        ));
    }

    // --- Error cases ---

    #[test]
    fn unknown_subcommand_is_error() {
        assert!(try_parse(&["sera", "nonexistent"]).is_err());
    }

    #[test]
    fn agent_show_missing_id_is_error() {
        assert!(try_parse(&["sera", "agent", "show"]).is_err());
    }

    #[test]
    fn config_set_missing_value_is_error() {
        assert!(try_parse(&["sera", "config", "set", "key-only"]).is_err());
    }
}
