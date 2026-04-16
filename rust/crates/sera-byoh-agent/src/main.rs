//! Minimal BYOH Rust Agent — proof-of-concept for SERA V1.
//!
//! Implements the BYOH contract: reads TaskInput from stdin, calls
//! the LLM proxy, writes TaskOutput to stdout. Serves /health on
//! AGENT_CHAT_PORT. Sends heartbeats for persistent mode.

mod health;
mod heartbeat;
mod llm;

use sera_config::SeraConfig;
use sera_types::{TaskInput, TaskOutput};
use std::io::{self, BufRead};
use tokio::signal;
use tracing::{error, info};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let config = match SeraConfig::from_env() {
        Ok(c) => c,
        Err(e) => {
            error!("Configuration error: {e}");
            std::process::exit(1);
        }
    };

    info!(
        agent = %config.agent_name,
        instance = %config.instance_id,
        "SERA BYOH Rust Agent starting"
    );

    // Start health server
    let health_handle = tokio::spawn(async move {
        if let Err(e) = health::serve(config.chat_port).await {
            error!("Health server failed: {e}");
        }
    });

    // Start heartbeat for persistent mode
    let heartbeat_handle = if config.lifecycle_mode == "persistent" {
        Some(tokio::spawn(heartbeat::run(
            config.core_url.clone(),
            config.instance_id.clone(),
            config.identity_token.clone(),
            config.heartbeat_interval_ms,
        )))
    } else {
        None
    };

    // Read task from stdin
    let task_input = read_stdin_task();

    if let Some(input) = task_input {
        info!(task_id = %input.task_id, "Processing task");
        let output = process_task(&config, input).await;

        // Write result to stdout
        if let Ok(json) = serde_json::to_string(&output) {
            println!("{json}");
        }
    } else {
        info!("No task on stdin");
        if config.lifecycle_mode == "persistent" {
            info!("Persistent mode — waiting for SIGTERM");
            signal::ctrl_c().await.ok();
        }
    }

    // Cleanup
    health_handle.abort();
    if let Some(h) = heartbeat_handle {
        h.abort();
    }

    info!("Shutdown complete");
}

fn read_stdin_task() -> Option<TaskInput> {
    let stdin = io::stdin();
    let mut line = String::new();

    // Non-blocking: if stdin is empty/closed, return None
    if stdin.lock().read_line(&mut line).ok()? == 0 {
        return None;
    }

    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }

    match serde_json::from_str::<TaskInput>(trimmed) {
        Ok(input) => Some(input),
        Err(_) => {
            // Treat as plain text task (backward compat)
            Some(TaskInput {
                task_id: format!("inline-{}", std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis()),
                task: trimmed.to_string(),
                context: None,
            })
        }
    }
}

async fn process_task(config: &SeraConfig, input: TaskInput) -> TaskOutput {
    match llm::chat(config, &input.task).await {
        Ok(response) => TaskOutput {
            task_id: input.task_id,
            result: Some(response),
            error: None,
        },
        Err(e) => TaskOutput {
            task_id: input.task_id,
            result: None,
            error: Some(e.to_string()),
        },
    }
}
