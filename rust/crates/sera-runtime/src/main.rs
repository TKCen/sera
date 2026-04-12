//! SERA Runtime — the agent worker process that runs inside containers.
//!
//! Replaces the TypeScript agent-runtime (core/agent-runtime/).
//! Communicates via NDJSON on stdin/stdout using Submission/Event envelope types.

mod config;
mod context;
mod error;
mod health;
mod llm_client;
mod manifest;
mod session_manager;
mod tools;
mod types;

// New P0-6 modules (available via lib.rs for library consumers)
mod compaction;
mod context_engine;
mod default_runtime;
mod handoff;
mod harness;
mod subagent;
mod turn;

use config::RuntimeConfig;
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

    // NDJSON Submission/Event loop on stdin/stdout
    let result = run_ndjson_loop(&config).await;

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
async fn run_ndjson_loop(_config: &RuntimeConfig) -> anyhow::Result<()> {
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

        // Process the submission (P0 stub — returns placeholder)
        let response_delta = match &submission.op {
            Op::UserTurn { .. } => "[turn executed - model call pending]".to_string(),
            Op::Steer { .. } => "[steer acknowledged]".to_string(),
            Op::Interrupt => "[interrupt acknowledged]".to_string(),
            Op::System(SystemOp::HealthCheck) => "[healthy]".to_string(),
            Op::System(SystemOp::Shutdown) => unreachable!(),
        };

        // Emit StreamingDelta with the response
        let delta = Event {
            id: uuid::Uuid::new_v4(),
            submission_id: submission.id,
            msg: EventMsg::StreamingDelta { delta: response_delta },
            timestamp: chrono::Utc::now(),
        };
        json = serde_json::to_string(&delta)?;
        json.push('\n');
        stdout.write_all(json.as_bytes()).await?;

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
