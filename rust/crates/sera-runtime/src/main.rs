//! SERA Runtime — the agent worker process that runs inside containers.
//!
//! Replaces the TypeScript agent-runtime (core/agent-runtime/).
//! Communicates via NDJSON on stdin/stdout using Submission/Event envelope types.

use std::collections::HashMap;
use std::sync::Arc;

use sera_runtime::config::RuntimeConfig;
use sera_runtime::context_engine::pipeline::ContextPipeline;
use sera_runtime::default_runtime::DefaultRuntime;
use sera_runtime::health;
use sera_runtime::llm_client::LlmClient;
use sera_runtime::tools::ToolRegistry;
use sera_runtime::tools::dispatcher::RegistryDispatcher;
use sera_types::runtime::{AgentRuntime, TurnContext, TurnOutcome};
use serde::{Deserialize, Serialize};

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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let config = RuntimeConfig::from_env();
    tracing::info!(
        agent_id = %config.agent_id,
        mode = %config.lifecycle_mode,
        "sera-runtime-rs starting (NDJSON transport)"
    );

    // Start health server in background
    let health_port = config.chat_port;
    let health_handle = tokio::spawn(async move {
        if let Err(e) = health::serve(health_port).await {
            tracing::error!("Health server error: {e}");
        }
    });

    // Start heartbeat for persistent agents
    if config.lifecycle_mode == "persistent" {
        let hb_config = config.clone();
        tokio::spawn(async move {
            run_heartbeat(&hb_config).await;
        });
    }

    // Build tool registry and dispatcher
    let registry = Arc::new(ToolRegistry::new());
    let dispatcher = RegistryDispatcher::new(Arc::clone(&registry));

    // Pre-compute tool definitions for the LLM via serde round-trip:
    // crate::types::ToolDefinition (Value-based) → sera_types::tool::ToolDefinition (typed)
    let tool_defs: Vec<sera_types::tool::ToolDefinition> = registry
        .definitions()
        .iter()
        .filter_map(|d| {
            let value = serde_json::to_value(d).ok()?;
            serde_json::from_value(value).ok()
        })
        .collect();
    tracing::info!(tool_count = tool_defs.len(), "tool definitions loaded for LLM");

    // Build the DefaultRuntime with context engine, LLM client, and tool dispatcher
    let context_engine = Box::new(ContextPipeline::new());
    let llm_client = Box::new(LlmClient::new(&config));
    let runtime = DefaultRuntime::new(context_engine)
        .with_llm(llm_client)
        .with_tool_dispatcher(Box::new(dispatcher));

    // NDJSON Submission/Event loop on stdin/stdout
    let result = run_ndjson_loop(&config, &runtime, &tool_defs).await;

    match result {
        Ok(()) => tracing::info!("NDJSON loop exited cleanly"),
        Err(ref e) => {
            if config.lifecycle_mode == "persistent" {
                tracing::info!("Persistent mode — waiting for chat requests on port {health_port}");
                health_handle.await?;
            } else {
                tracing::error!("NDJSON loop failed: {e}");
                std::process::exit(1);
            }
        }
    }

    Ok(())
}

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
            // stdin closed
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
