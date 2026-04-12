//! SERA Runtime — standalone agent harness with CLI and NDJSON interfaces.
//!
//! The runtime is fully self-contained: it owns the LLM client, tool registry,
//! tool dispatch, context engine, and turn loop. No gateway required.
//!
//! Two modes:
//! - **Interactive** (default when stdin is a TTY): human-friendly chat REPL
//! - **NDJSON** (default when stdin is piped, or `--ndjson`): machine-readable
//!   Submission/Event protocol for gateway integration

use std::collections::HashMap;
use std::sync::Arc;

use clap::Parser;
use sera_runtime::config::RuntimeConfig;
use sera_runtime::context_engine::pipeline::ContextPipeline;
use sera_runtime::default_runtime::DefaultRuntime;
use sera_runtime::health;
use sera_runtime::llm_client::LlmClient;
use sera_runtime::tools::ToolRegistry;
use sera_runtime::tools::dispatcher::RegistryDispatcher;
use sera_types::runtime::{AgentRuntime, TurnContext, TurnOutcome};
use serde::{Deserialize, Serialize};

// ── CLI ──────────────────────────────────────────────────────────────────────

/// SERA Runtime — standalone agent harness
#[derive(Parser, Debug)]
#[command(name = "sera-runtime", about = "SERA agent runtime — standalone LLM + tool execution")]
struct Cli {
    /// LLM API base URL (OpenAI-compatible)
    #[arg(long, env = "LLM_BASE_URL")]
    llm_url: Option<String>,

    /// Model name
    #[arg(long, short, env = "LLM_MODEL")]
    model: Option<String>,

    /// API key for the LLM endpoint
    #[arg(long, env = "LLM_API_KEY")]
    api_key: Option<String>,

    /// Max tokens for LLM responses
    #[arg(long, env = "MAX_TOKENS")]
    max_tokens: Option<u32>,

    /// Agent identifier
    #[arg(long, env = "AGENT_ID", default_value = "sera-local")]
    agent_id: String,

    /// System prompt prepended to every conversation
    #[arg(long, short)]
    system: Option<String>,

    /// Force NDJSON mode (even when stdin is a TTY)
    #[arg(long)]
    ndjson: bool,

    /// Disable the health check HTTP server
    #[arg(long)]
    no_health: bool,

    /// Health server port (0 = disabled)
    #[arg(long, env = "AGENT_CHAT_PORT", default_value = "0")]
    health_port: u16,
}

impl Cli {
    /// Merge CLI args over env-var defaults to produce a RuntimeConfig.
    fn into_config(self) -> RuntimeConfig {
        let mut config = RuntimeConfig::from_env();
        if let Some(url) = self.llm_url {
            config.llm_base_url = url;
        }
        if let Some(model) = self.model {
            config.llm_model = model;
        }
        if let Some(key) = self.api_key {
            config.llm_api_key = key;
        }
        if let Some(max) = self.max_tokens {
            config.max_tokens = max;
        }
        config.agent_id = self.agent_id;
        config.chat_port = if self.no_health { 0 } else { self.health_port };
        config.lifecycle_mode = "task".to_string();
        config
    }
}

// ── NDJSON envelope types ────────────────────────────────────────────────────

/// Local NDJSON submission type — serde-compatible with sera-gateway's Submission.
/// Defined locally to avoid a cyclic dependency (sera-gateway depends on sera-runtime).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Submission {
    id: uuid::Uuid,
    op: Op,
}

/// Local operation enum — mirrors sera-gateway's Op.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Op {
    UserTurn {
        items: Vec<serde_json::Value>,
        #[serde(default)]
        model_override: Option<String>,
    },
    Steer {
        items: Vec<serde_json::Value>,
    },
    Interrupt,
    System(SystemOp),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "system_op", rename_all = "snake_case")]
enum SystemOp {
    Shutdown,
    HealthCheck,
}

/// Local NDJSON event type — serde-compatible with sera-gateway's Event.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Event {
    id: uuid::Uuid,
    submission_id: uuid::Uuid,
    msg: EventMsg,
    timestamp: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum EventMsg {
    TurnStarted { turn_id: uuid::Uuid },
    TurnCompleted { turn_id: uuid::Uuid },
    StreamingDelta { delta: String },
    Error { code: String, message: String },
}

// ── Main ─────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let interactive = !cli.ndjson && atty::is(atty::Stream::Stdin);
    let system_prompt = cli.system.clone();
    let config = cli.into_config();

    // In interactive mode, only show warnings (no info spam); NDJSON mode uses RUST_LOG
    if interactive {
        tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
            )
            .with_writer(std::io::stderr)
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
            )
            .init();
    }

    // Start health server in background (unless disabled)
    if config.chat_port > 0 {
        let health_port = config.chat_port;
        tokio::spawn(async move {
            if let Err(e) = health::serve(health_port).await {
                tracing::error!("Health server error: {e}");
            }
        });
    }

    // Build tool registry and dispatcher
    let registry = Arc::new(ToolRegistry::new());
    let dispatcher = RegistryDispatcher::new(Arc::clone(&registry));

    // Pre-compute tool definitions for the LLM via serde round-trip
    let tool_defs: Vec<sera_types::tool::ToolDefinition> = registry
        .definitions()
        .iter()
        .filter_map(|d| {
            let value = serde_json::to_value(d).ok()?;
            serde_json::from_value(value).ok()
        })
        .collect();

    // Build the DefaultRuntime
    let context_engine = Box::new(ContextPipeline::new());
    let llm_client = Box::new(LlmClient::new(&config));
    let runtime = DefaultRuntime::new(context_engine)
        .with_llm(llm_client)
        .with_tool_dispatcher(Box::new(dispatcher));

    if interactive {
        run_interactive(&config, &runtime, &tool_defs, system_prompt.as_deref()).await
    } else {
        tracing::info!(
            agent_id = %config.agent_id,
            model = %config.llm_model,
            tool_count = tool_defs.len(),
            "sera-runtime starting (NDJSON transport)"
        );
        run_ndjson_loop(&config, &runtime, &tool_defs).await
    }
}

// ── Interactive REPL ─────────────────────────────────────────────────────────

async fn run_interactive(
    config: &RuntimeConfig,
    runtime: &DefaultRuntime,
    tool_defs: &[sera_types::tool::ToolDefinition],
    system_prompt: Option<&str>,
) -> anyhow::Result<()> {
    use std::io::{BufRead, Write};

    eprintln!("sera-runtime — interactive mode");
    eprintln!("  model:  {}", config.llm_model);
    eprintln!("  llm:    {}", config.llm_base_url);
    eprintln!("  tools:  {} available", tool_defs.len());
    eprintln!("  type 'exit' or Ctrl-D to quit\n");

    let stdin = std::io::stdin();
    let mut reader = stdin.lock();
    let mut conversation: Vec<serde_json::Value> = Vec::new();

    // Add system prompt if provided
    if let Some(sys) = system_prompt {
        conversation.push(serde_json::json!({"role": "system", "content": sys}));
    }

    loop {
        // Print prompt
        eprint!("> ");
        std::io::stderr().flush()?;

        let mut input = String::new();
        let n = reader.read_line(&mut input)?;
        if n == 0 {
            // EOF (Ctrl-D)
            eprintln!();
            break;
        }

        let trimmed = input.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed == "exit" || trimmed == "quit" {
            break;
        }

        // Add user message to conversation
        conversation.push(serde_json::json!({"role": "user", "content": trimmed}));

        // Build TurnContext with full conversation history
        let turn_ctx = TurnContext {
            event_id: uuid::Uuid::new_v4().to_string(),
            agent_id: config.agent_id.clone(),
            session_key: format!("session:{}:interactive", config.agent_id),
            messages: conversation.clone(),
            available_tools: tool_defs.to_vec(),
            metadata: HashMap::new(),
            change_artifact: None,
        };

        let outcome = runtime.execute_turn(turn_ctx).await;

        match outcome {
            Ok(TurnOutcome::FinalOutput { response, .. }) => {
                println!("{response}\n");
                // Add assistant response to conversation history
                conversation.push(serde_json::json!({"role": "assistant", "content": response}));
            }
            Ok(TurnOutcome::Interruption { reason, .. }) => {
                eprintln!("[interrupted: {reason}]\n");
            }
            Ok(TurnOutcome::Handoff { target_agent_id, .. }) => {
                eprintln!("[handoff -> {target_agent_id}]\n");
            }
            Ok(TurnOutcome::WaitingForApproval { ticket_id, .. }) => {
                eprintln!("[waiting for approval: {ticket_id}]\n");
            }
            Ok(other) => {
                eprintln!("[{other:?}]\n");
            }
            Err(e) => {
                eprintln!("[error: {e:?}]\n");
            }
        }
    }

    Ok(())
}

// ── NDJSON transport ─────────────────────────────────────────────────────────

/// Read Submissions from stdin (NDJSON), process each, write Events to stdout.
async fn run_ndjson_loop(
    config: &RuntimeConfig,
    runtime: &DefaultRuntime,
    tool_defs: &[sera_types::tool::ToolDefinition],
) -> anyhow::Result<()> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let stdin = tokio::io::stdin();
    let mut reader = BufReader::new(stdin);
    let mut stdout = tokio::io::stdout();
    let mut line = String::new();

    loop {
        line.clear();
        let n = reader.read_line(&mut line).await?;
        if n == 0 {
            tracing::info!("stdin closed, exiting NDJSON loop");
            break;
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let submission: Submission = match serde_json::from_str(trimmed) {
            Ok(s) => s,
            Err(e) => {
                let err_event = Event {
                    id: uuid::Uuid::new_v4(),
                    submission_id: uuid::Uuid::nil(),
                    msg: EventMsg::Error {
                        code: "parse_error".to_string(),
                        message: format!("failed to parse submission: {e}"),
                    },
                    timestamp: chrono::Utc::now(),
                };
                let mut json = serde_json::to_string(&err_event)?;
                json.push('\n');
                stdout.write_all(json.as_bytes()).await?;
                stdout.flush().await?;
                continue;
            }
        };

        // Check for shutdown
        if matches!(&submission.op, Op::System(SystemOp::Shutdown)) {
            tracing::info!("received shutdown command, exiting");
            break;
        }

        let turn_id = uuid::Uuid::new_v4();

        // Emit TurnStarted
        let started = Event {
            id: uuid::Uuid::new_v4(),
            submission_id: submission.id,
            msg: EventMsg::TurnStarted { turn_id },
            timestamp: chrono::Utc::now(),
        };
        let mut json = serde_json::to_string(&started)?;
        json.push('\n');
        stdout.write_all(json.as_bytes()).await?;

        // Convert Submission to TurnContext and execute via DefaultRuntime
        let turn_ctx = submission_to_turn_context(&submission, &config.agent_id, turn_id, tool_defs);
        let outcome = runtime.execute_turn(turn_ctx).await;

        // Convert TurnOutcome to Event messages
        match outcome {
            Ok(TurnOutcome::FinalOutput { response, .. }) => {
                let delta = Event {
                    id: uuid::Uuid::new_v4(),
                    submission_id: submission.id,
                    msg: EventMsg::StreamingDelta { delta: response },
                    timestamp: chrono::Utc::now(),
                };
                json = serde_json::to_string(&delta)?;
                json.push('\n');
                stdout.write_all(json.as_bytes()).await?;
            }
            Ok(TurnOutcome::RunAgain { .. }) => {
                let delta = Event {
                    id: uuid::Uuid::new_v4(),
                    submission_id: submission.id,
                    msg: EventMsg::StreamingDelta {
                        delta: "[run_again — tool calls dispatched]".to_string(),
                    },
                    timestamp: chrono::Utc::now(),
                };
                json = serde_json::to_string(&delta)?;
                json.push('\n');
                stdout.write_all(json.as_bytes()).await?;
            }
            Ok(TurnOutcome::Handoff { target_agent_id, .. }) => {
                let delta = Event {
                    id: uuid::Uuid::new_v4(),
                    submission_id: submission.id,
                    msg: EventMsg::StreamingDelta {
                        delta: format!("[handoff -> {target_agent_id}]"),
                    },
                    timestamp: chrono::Utc::now(),
                };
                json = serde_json::to_string(&delta)?;
                json.push('\n');
                stdout.write_all(json.as_bytes()).await?;
            }
            Ok(TurnOutcome::Compact { .. }) => {
                let delta = Event {
                    id: uuid::Uuid::new_v4(),
                    submission_id: submission.id,
                    msg: EventMsg::StreamingDelta {
                        delta: "[compact — context condensed]".to_string(),
                    },
                    timestamp: chrono::Utc::now(),
                };
                json = serde_json::to_string(&delta)?;
                json.push('\n');
                stdout.write_all(json.as_bytes()).await?;
            }
            Ok(TurnOutcome::Interruption { reason, .. }) => {
                let delta = Event {
                    id: uuid::Uuid::new_v4(),
                    submission_id: submission.id,
                    msg: EventMsg::StreamingDelta {
                        delta: format!("[interrupted: {reason}]"),
                    },
                    timestamp: chrono::Utc::now(),
                };
                json = serde_json::to_string(&delta)?;
                json.push('\n');
                stdout.write_all(json.as_bytes()).await?;
            }
            Ok(TurnOutcome::Stop { summary, .. }) => {
                let delta = Event {
                    id: uuid::Uuid::new_v4(),
                    submission_id: submission.id,
                    msg: EventMsg::StreamingDelta {
                        delta: format!("[stop: {summary}]"),
                    },
                    timestamp: chrono::Utc::now(),
                };
                json = serde_json::to_string(&delta)?;
                json.push('\n');
                stdout.write_all(json.as_bytes()).await?;
            }
            Ok(TurnOutcome::WaitingForApproval { ticket_id, .. }) => {
                let delta = Event {
                    id: uuid::Uuid::new_v4(),
                    submission_id: submission.id,
                    msg: EventMsg::StreamingDelta {
                        delta: format!("[waiting_for_approval: ticket={ticket_id}]"),
                    },
                    timestamp: chrono::Utc::now(),
                };
                json = serde_json::to_string(&delta)?;
                json.push('\n');
                stdout.write_all(json.as_bytes()).await?;
            }
            Err(e) => {
                tracing::error!("execute_turn failed: {e:?}");
                let err_event = Event {
                    id: uuid::Uuid::new_v4(),
                    submission_id: submission.id,
                    msg: EventMsg::Error {
                        code: "turn_error".to_string(),
                        message: format!("{e:?}"),
                    },
                    timestamp: chrono::Utc::now(),
                };
                json = serde_json::to_string(&err_event)?;
                json.push('\n');
                stdout.write_all(json.as_bytes()).await?;
            }
        }

        // Emit TurnCompleted
        let completed = Event {
            id: uuid::Uuid::new_v4(),
            submission_id: submission.id,
            msg: EventMsg::TurnCompleted { turn_id },
            timestamp: chrono::Utc::now(),
        };
        json = serde_json::to_string(&completed)?;
        json.push('\n');
        stdout.write_all(json.as_bytes()).await?;
        stdout.flush().await?;
    }

    Ok(())
}

/// Convert a local `Submission` into a `TurnContext` for the runtime.
fn submission_to_turn_context(
    submission: &Submission,
    agent_id: &str,
    turn_id: uuid::Uuid,
    tool_defs: &[sera_types::tool::ToolDefinition],
) -> TurnContext {
    let messages = match &submission.op {
        Op::UserTurn { items, .. } => items.clone(),
        Op::Steer { items } => items.clone(),
        Op::Interrupt | Op::System(_) => vec![],
    };

    TurnContext {
        event_id: turn_id.to_string(),
        agent_id: agent_id.to_string(),
        session_key: format!("session:{agent_id}:{}", submission.id),
        messages,
        available_tools: tool_defs.to_vec(),
        metadata: HashMap::new(),
        change_artifact: None,
    }
}

/// Send periodic heartbeats to sera-core.
#[allow(dead_code)]
async fn run_heartbeat(config: &RuntimeConfig) {
    let client = reqwest::Client::new();
    let url = format!("{}/api/agents/{}/heartbeat", config.core_url, config.agent_id);

    loop {
        tokio::time::sleep(std::time::Duration::from_secs(30)).await;

        match client
            .post(&url)
            .header("Authorization", format!("Bearer {}", config.api_key))
            .json(&serde_json::json!({"status": "running"}))
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                tracing::debug!("Heartbeat sent");
            }
            Ok(resp) => {
                tracing::warn!("Heartbeat returned HTTP {}", resp.status());
            }
            Err(e) => {
                tracing::warn!("Heartbeat failed: {e}");
            }
        }
    }
}
