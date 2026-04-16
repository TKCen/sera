//! CLI command handler implementations.

use anyhow::Result;

use crate::api::ApiClient;
use crate::cli::{AgentCommands, Commands, SessionCommands};

/// Dispatch a CLI command to the appropriate handler.
pub async fn dispatch(client: ApiClient, cmd: Commands) -> Result<()> {
    match cmd {
        Commands::Tui => unreachable!("Tui command dispatched to CLI handler"),
        Commands::Agent { subcommand } => match subcommand {
            AgentCommands::List => run_agent_list(&client).await,
            AgentCommands::Show { id } => run_agent_show(&client, &id).await,
            AgentCommands::Start { id } => {
                println!("Start agent {id}: not yet implemented");
                Ok(())
            }
            AgentCommands::Stop { id } => {
                println!("Stop agent {id}: not yet implemented");
                Ok(())
            }
        },
        Commands::Session { subcommand } => match subcommand {
            SessionCommands::List => run_session_list().await,
            SessionCommands::Show { id } => {
                println!("Session {id}: not yet implemented");
                Ok(())
            }
        },
        Commands::Health => run_health(&client).await,
        Commands::Chat { agent } => {
            println!("Chat with agent {agent}: not yet implemented");
            Ok(())
        }
        Commands::Config { subcommand } => match subcommand {
            crate::cli::ConfigCommands::Get { key } => {
                println!("Config get {key}: not yet implemented");
                Ok(())
            }
            crate::cli::ConfigCommands::Set { key, value } => {
                println!("Config set {key}={value}: not yet implemented");
                Ok(())
            }
        },
    }
}

/// List all agent instances, printing a table to stdout.
async fn run_agent_list(client: &ApiClient) -> Result<()> {
    let agents = client.list_agents().await?;
    if agents.is_empty() {
        println!("No agents found.");
        return Ok(());
    }
    println!("{:<36}  {:<24}  {:<12}  TEMPLATE", "ID", "NAME", "STATUS");
    println!("{}", "-".repeat(90));
    for agent in &agents {
        let name = agent.display_name.as_deref().unwrap_or(&agent.name);
        println!(
            "{:<36}  {:<24}  {:<12}  {}",
            agent.id, name, agent.status, agent.template_ref
        );
    }
    Ok(())
}

/// Show details for a single agent.
async fn run_agent_show(client: &ApiClient, id: &str) -> Result<()> {
    let agent = client.get_agent(id).await?;
    let name = agent.display_name.as_deref().unwrap_or(&agent.name);
    println!("ID:          {}", agent.id);
    println!("Name:        {}", agent.name);
    println!("Display:     {}", name);
    println!("Template:    {}", agent.template_ref);
    println!("Status:      {}", agent.status);
    println!("Created:     {}", agent.created_at);
    println!("Updated:     {}", agent.updated_at);
    Ok(())
}

/// Check API health.
async fn run_health(client: &ApiClient) -> Result<()> {
    let value = client.health().await?;
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}

/// List sessions (placeholder).
async fn run_session_list() -> Result<()> {
    println!("Session list: not yet implemented");
    Ok(())
}
