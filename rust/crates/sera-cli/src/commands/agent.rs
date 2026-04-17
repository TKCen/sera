//! `sera agent` — list, show, and run agents.
//!
//! Three subcommands:
//! - [`AgentListCommand`]  — `GET /api/agents`, tabular output
//! - [`AgentShowCommand`]  — `GET /api/agents/:id`, formatted detail block
//! - [`AgentRunCommand`]   — `POST /api/chat`, synchronous turn, prints reply
//!
//! All three accept `--endpoint` to override the gateway URL.
//! List and show accept `--json` for machine-readable output.
//!
//! Token injection: callers must pass a `reqwest::Client` built with
//! [`crate::http::build_client_with_token`] as the `"client"` arg is not
//! feasible through `CommandArgs` (strings only).  Instead, each command
//! reads the token from the store it was constructed with, builds the
//! authenticated client internally, and returns exit-code 2 when no token
//! is present (matching the bead spec).

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::json;

use sera_commands::{
    Command, CommandArgSchema, CommandArgs, CommandCategory, CommandContext, CommandDescription,
    CommandError, CommandResult,
};

use crate::http::build_client_with_token;
use crate::token_store::{best_available_store, TokenStore};

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Exit code used when no token is stored.
const EXIT_NO_TOKEN: i32 = 2;
/// Exit code used when the resource was not found.
const EXIT_NOT_FOUND: i32 = 4;

fn no_token_error() -> CommandError {
    CommandError::Execution(
        "not authenticated — run `sera auth login` first".into(),
    )
}

/// Format a `serde_json::Value` field as a human-readable string,
/// collapsing null/missing to an empty string.
fn field_str<'a>(v: &'a serde_json::Value, key: &str) -> &'a str {
    v.get(key)
        .and_then(|f| f.as_str())
        .unwrap_or("")
}

// ---------------------------------------------------------------------------
// AgentListCommand
// ---------------------------------------------------------------------------

/// `sera agent list` — fetch and display all agent instances.
pub struct AgentListCommand {
    store: Arc<dyn TokenStore>,
}

impl AgentListCommand {
    pub fn new() -> Self {
        Self {
            store: Arc::from(best_available_store()),
        }
    }

    pub fn with_store(store: Arc<dyn TokenStore>) -> Self {
        Self { store }
    }
}

impl Default for AgentListCommand {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Command for AgentListCommand {
    fn name(&self) -> &str {
        "agent:list"
    }

    fn describe(&self) -> CommandDescription {
        CommandDescription {
            summary: "List all agent instances (GET /api/agents)".into(),
            help: "Fetches and displays all agent instances from the gateway. \
                   Use --json for machine-readable output."
                .into(),
            category: CommandCategory::System,
        }
    }

    fn argument_schema(&self) -> CommandArgSchema {
        CommandArgSchema(
            clap::Command::new("list")
                .about("List all agent instances")
                .arg(
                    clap::Arg::new("endpoint")
                        .long("endpoint")
                        .short('e')
                        .help("Gateway base URL (overrides config)")
                        .value_name("URL"),
                )
                .arg(
                    clap::Arg::new("json")
                        .long("json")
                        .help("Output raw JSON array")
                        .action(clap::ArgAction::SetTrue),
                ),
        )
    }

    async fn execute(
        &self,
        args: CommandArgs,
        _ctx: &CommandContext,
    ) -> Result<CommandResult, CommandError> {
        let endpoint = args
            .get("endpoint")
            .unwrap_or("http://localhost:8080")
            .trim_end_matches('/')
            .to_owned();
        let json_mode = args.get("json").map(|v| v == "true").unwrap_or(false);

        let token = self
            .store
            .load()
            .map_err(|e| CommandError::Execution(format!("failed to load token: {e}")))?
            .ok_or_else(no_token_error)?;

        let client = build_client_with_token(&token)
            .map_err(|e| CommandError::Execution(e.to_string()))?;

        let url = format!("{endpoint}/api/agents");
        let response = client
            .get(&url)
            .send()
            .await
            .map_err(|e| CommandError::Execution(format!("request failed: {e}")))?;

        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            return Err(CommandError::Execution(
                "token rejected — run `sera auth login` again".into(),
            ));
        }
        if !response.status().is_success() {
            return Err(CommandError::Execution(format!(
                "gateway returned HTTP {}",
                response.status()
            )));
        }

        let body: serde_json::Value = response
            .json()
            .await
            .map_err(|e| CommandError::Execution(format!("failed to parse response: {e}")))?;

        if json_mode {
            println!("{}", serde_json::to_string_pretty(&body).unwrap_or_default());
        } else {
            print_agents_table(&body);
        }

        Ok(CommandResult::ok(body))
    }
}

fn print_agents_table(agents: &serde_json::Value) {
    use comfy_table::{Table, presets::UTF8_FULL};

    let mut table = Table::new();
    table.load_preset(UTF8_FULL);
    table.set_header(["NAME", "PROVIDER/TEMPLATE", "MODEL/STATUS", "TOOLS/CIRCLE"]);

    if let Some(arr) = agents.as_array() {
        for agent in arr {
            // Support both the autonomous gateway shape {name, provider, model, has_tools}
            // and the full gateway shape {id, name, template_ref, status, circle}.
            let name = field_str(agent, "name");
            let provider_or_template = if !field_str(agent, "provider").is_empty() {
                field_str(agent, "provider")
            } else {
                field_str(agent, "template_ref")
            };
            let model_or_status = if !field_str(agent, "model").is_empty() {
                field_str(agent, "model")
            } else {
                field_str(agent, "status")
            };
            let tools_or_circle = {
                // has_tools is a bool — convert to "yes"/"no" if present.
                if let Some(ht) = agent.get("has_tools") {
                    if ht.as_bool().unwrap_or(false) { "yes" } else { "no" }
                } else {
                    field_str(agent, "circle")
                }
            };
            table.add_row([name, provider_or_template, model_or_status, tools_or_circle]);
        }
    }

    println!("{table}");
}

// ---------------------------------------------------------------------------
// AgentShowCommand
// ---------------------------------------------------------------------------

/// `sera agent show <id>` — fetch and display full agent detail.
pub struct AgentShowCommand {
    store: Arc<dyn TokenStore>,
}

impl AgentShowCommand {
    pub fn new() -> Self {
        Self {
            store: Arc::from(best_available_store()),
        }
    }

    pub fn with_store(store: Arc<dyn TokenStore>) -> Self {
        Self { store }
    }
}

impl Default for AgentShowCommand {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Command for AgentShowCommand {
    fn name(&self) -> &str {
        "agent:show"
    }

    fn describe(&self) -> CommandDescription {
        CommandDescription {
            summary: "Show full detail for an agent instance (GET /api/agents/:id)".into(),
            help: "Fetches a single agent instance by ID and displays its manifest, \
                   policies, and metadata. Use --json for machine-readable output."
                .into(),
            category: CommandCategory::System,
        }
    }

    fn argument_schema(&self) -> CommandArgSchema {
        CommandArgSchema(
            clap::Command::new("show")
                .about("Show agent instance detail")
                .arg(
                    clap::Arg::new("id")
                        .help("Agent instance ID")
                        .required(true)
                        .value_name("ID"),
                )
                .arg(
                    clap::Arg::new("endpoint")
                        .long("endpoint")
                        .short('e')
                        .help("Gateway base URL (overrides config)")
                        .value_name("URL"),
                )
                .arg(
                    clap::Arg::new("json")
                        .long("json")
                        .help("Output raw JSON")
                        .action(clap::ArgAction::SetTrue),
                ),
        )
    }

    async fn execute(
        &self,
        args: CommandArgs,
        _ctx: &CommandContext,
    ) -> Result<CommandResult, CommandError> {
        let id = args
            .get("id")
            .ok_or_else(|| CommandError::InvalidArgs("id is required".into()))?
            .to_owned();
        let endpoint = args
            .get("endpoint")
            .unwrap_or("http://localhost:8080")
            .trim_end_matches('/')
            .to_owned();
        let json_mode = args.get("json").map(|v| v == "true").unwrap_or(false);

        let token = self
            .store
            .load()
            .map_err(|e| CommandError::Execution(format!("failed to load token: {e}")))?
            .ok_or_else(no_token_error)?;

        let client = build_client_with_token(&token)
            .map_err(|e| CommandError::Execution(e.to_string()))?;

        let url = format!("{endpoint}/api/agents/{id}");
        let response = client
            .get(&url)
            .send()
            .await
            .map_err(|e| CommandError::Execution(format!("request failed: {e}")))?;

        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            return Err(CommandError::Execution(
                "token rejected — run `sera auth login` again".into(),
            ));
        }
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            eprintln!("agent not found: {id}");
            let mut result = CommandResult::ok(json!({"error": "not found", "id": id}));
            result.exit_code = EXIT_NOT_FOUND;
            return Ok(result);
        }
        if !response.status().is_success() {
            return Err(CommandError::Execution(format!(
                "gateway returned HTTP {}",
                response.status()
            )));
        }

        let body: serde_json::Value = response
            .json()
            .await
            .map_err(|e| CommandError::Execution(format!("failed to parse response: {e}")))?;

        if json_mode {
            println!("{}", serde_json::to_string_pretty(&body).unwrap_or_default());
        } else {
            print_agent_detail(&body);
        }

        Ok(CommandResult::ok(body))
    }
}

fn print_agent_detail(agent: &serde_json::Value) {
    println!("Name:           {}", field_str(agent, "name"));

    // Full gateway shape fields (may be absent in autonomous mode).
    let id = field_str(agent, "id");
    if !id.is_empty() {
        println!("ID:             {id}");
    }
    let display_name = field_str(agent, "display_name");
    if !display_name.is_empty() {
        println!("Display name:   {display_name}");
    }
    let template_ref = field_str(agent, "template_ref");
    if !template_ref.is_empty() {
        println!("Template:       {template_ref}");
    }
    let status = field_str(agent, "status");
    if !status.is_empty() {
        println!("Status:         {status}");
    }

    // Autonomous gateway shape fields.
    let provider = field_str(agent, "provider");
    if !provider.is_empty() {
        println!("Provider:       {provider}");
    }
    let model = field_str(agent, "model");
    if !model.is_empty() {
        println!("Model:          {model}");
    }
    if let Some(ht) = agent.get("has_tools") {
        println!("Has tools:      {}", if ht.as_bool().unwrap_or(false) { "yes" } else { "no" });
    }

    // Remaining full gateway fields.
    let lifecycle_mode = field_str(agent, "lifecycle_mode");
    if !lifecycle_mode.is_empty() {
        println!("Lifecycle mode: {lifecycle_mode}");
    }
    let circle = field_str(agent, "circle");
    if !circle.is_empty() {
        println!("Circle:         {circle}");
    }
    let workspace_path = field_str(agent, "workspace_path");
    if !workspace_path.is_empty() {
        println!("Workspace:      {workspace_path}");
    }
    let container_id = field_str(agent, "container_id");
    if !container_id.is_empty() {
        println!("Container:      {container_id}");
    }
    let last_heartbeat_at = field_str(agent, "last_heartbeat_at");
    if !last_heartbeat_at.is_empty() {
        println!("Last heartbeat: {last_heartbeat_at}");
    }
    let created_at = field_str(agent, "created_at");
    if !created_at.is_empty() {
        println!("Created:        {created_at}");
    }
    let updated_at = field_str(agent, "updated_at");
    if !updated_at.is_empty() {
        println!("Updated:        {updated_at}");
    }

    if let Some(cfg) = agent.get("resolved_config").filter(|v| !v.is_null()) {
        println!(
            "Resolved config:\n{}",
            serde_json::to_string_pretty(cfg).unwrap_or_default()
        );
    }
}

// ---------------------------------------------------------------------------
// AgentRunCommand
// ---------------------------------------------------------------------------

/// `sera agent run <id> <prompt>` — post a turn and stream output.
///
/// Uses `POST /api/chat` with `stream: false` (synchronous mode).
/// The gateway's SSE/stream endpoint routes via Centrifugo (WebSocket) and
/// is not directly accessible from a CLI without a Centrifugo subscription.
/// Synchronous mode returns the full reply in a single HTTP response.
pub struct AgentRunCommand {
    store: Arc<dyn TokenStore>,
}

impl AgentRunCommand {
    pub fn new() -> Self {
        Self {
            store: Arc::from(best_available_store()),
        }
    }

    pub fn with_store(store: Arc<dyn TokenStore>) -> Self {
        Self { store }
    }
}

impl Default for AgentRunCommand {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Command for AgentRunCommand {
    fn name(&self) -> &str {
        "agent:run"
    }

    fn describe(&self) -> CommandDescription {
        CommandDescription {
            summary: "Post a prompt to an agent and print the reply (POST /api/chat)".into(),
            help: "Sends a prompt to the specified agent instance via POST /api/chat \
                   (synchronous mode) and prints the reply. The gateway's SSE streaming \
                   path routes through Centrifugo WebSocket, which is not directly \
                   accessible from the CLI without a Centrifugo subscription; \
                   synchronous mode is used instead."
                .into(),
            category: CommandCategory::System,
        }
    }

    fn argument_schema(&self) -> CommandArgSchema {
        CommandArgSchema(
            clap::Command::new("run")
                .about("Post a prompt to an agent and print the reply")
                .arg(
                    clap::Arg::new("id")
                        .help("Agent instance ID or name")
                        .required(true)
                        .value_name("ID"),
                )
                .arg(
                    clap::Arg::new("prompt")
                        .help("Prompt to send to the agent")
                        .required(true)
                        .value_name("PROMPT"),
                )
                .arg(
                    clap::Arg::new("endpoint")
                        .long("endpoint")
                        .short('e')
                        .help("Gateway base URL (overrides config)")
                        .value_name("URL"),
                )
                .arg(
                    clap::Arg::new("raw")
                        .long("raw")
                        .help("Output raw JSON response for debugging")
                        .action(clap::ArgAction::SetTrue),
                ),
        )
    }

    async fn execute(
        &self,
        args: CommandArgs,
        _ctx: &CommandContext,
    ) -> Result<CommandResult, CommandError> {
        let id = args
            .get("id")
            .ok_or_else(|| CommandError::InvalidArgs("id is required".into()))?
            .to_owned();
        let prompt = args
            .get("prompt")
            .ok_or_else(|| CommandError::InvalidArgs("prompt is required".into()))?
            .to_owned();
        let endpoint = args
            .get("endpoint")
            .unwrap_or("http://localhost:8080")
            .trim_end_matches('/')
            .to_owned();
        let raw = args.get("raw").map(|v| v == "true").unwrap_or(false);

        let token = self
            .store
            .load()
            .map_err(|e| CommandError::Execution(format!("failed to load token: {e}")))?
            .ok_or_else(no_token_error)?;

        let client = build_client_with_token(&token)
            .map_err(|e| CommandError::Execution(e.to_string()))?;

        // POST /api/chat — send both `agent` (autonomous gateway) and
        // `agentInstanceId` (full gateway) so the payload works against either.
        let payload = json!({
            "agent": id,
            "agentInstanceId": id,
            "message": prompt,
            "stream": false,
        });

        let url = format!("{endpoint}/api/chat");
        let response = client
            .post(&url)
            .json(&payload)
            .send()
            .await
            .map_err(|e| CommandError::Execution(format!("request failed: {e}")))?;

        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            return Err(CommandError::Execution(
                "token rejected — run `sera auth login` again".into(),
            ));
        }
        if !response.status().is_success() {
            let status = response.status();
            let body_text = response.text().await.unwrap_or_default();
            return Err(CommandError::Execution(format!(
                "gateway returned HTTP {status}: {body_text}"
            )));
        }

        let body: serde_json::Value = response
            .json()
            .await
            .map_err(|e| CommandError::Execution(format!("failed to parse response: {e}")))?;

        if raw {
            println!("{}", serde_json::to_string_pretty(&body).unwrap_or_default());
        } else {
            print_run_output(&body);
        }

        Ok(CommandResult::ok(body))
    }
}

fn print_run_output(body: &serde_json::Value) {
    // Print thought/tool-call context if present
    if let Some(thoughts) = body.get("thoughts").and_then(|t| t.as_array())
        && !thoughts.is_empty()
    {
        println!("--- tool calls / thoughts ---");
        for thought in thoughts {
            let step = thought.get("step").and_then(|s| s.as_str()).unwrap_or("?");
            let content = thought
                .get("content")
                .and_then(|c| c.as_str())
                .unwrap_or("");
            println!("  [{step}] {content}");
        }
        println!("--- end thoughts ---");
    }

    // Print the assistant reply — support both full gateway (`reply`) and
    // autonomous gateway (`response`) field names.
    if let Some(reply) = body.get("reply").and_then(|r| r.as_str())
        .or_else(|| body.get("response").and_then(|r| r.as_str()))
    {
        println!("{reply}");
    } else if let Some(thought) = body.get("thought").and_then(|t| t.as_str()) {
        // May be a status message (e.g. "queued behind active turn")
        println!("[{thought}]");
    } else {
        println!("[no reply]");
    }

    // Session info
    if let Some(session_id) = body.get("session_id").and_then(|s| s.as_str()) {
        eprintln!("session: {session_id}");
    }

    // Usage summary if present
    if let Some(usage) = body.get("usage").filter(|v| !v.is_null()) {
        let total = usage
            .get("totalTokens")
            .and_then(|t| t.as_u64())
            .unwrap_or(0);
        if total > 0 {
            eprintln!("tokens: {total}");
        }
    }
}

// ---------------------------------------------------------------------------
// No-token result helper (exit_code = 2)
// ---------------------------------------------------------------------------

/// Build a `CommandResult` with `exit_code = EXIT_NO_TOKEN` for use in dispatch
/// when we detect a missing token before executing the command.
pub fn no_token_result() -> CommandResult {
    eprintln!("not authenticated — run `sera auth login` first");
    let mut r = CommandResult::ok(json!({"error": "not authenticated"}));
    r.exit_code = EXIT_NO_TOKEN;
    r
}
