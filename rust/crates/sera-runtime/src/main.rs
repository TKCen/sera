//! SERA Runtime — the agent worker process that runs inside containers.
//!
//! Replaces the TypeScript agent-runtime (core/agent-runtime/).
//! Handles: reasoning loop, tool execution, LLM proxy calls, context management.

mod config;
mod context;
mod error;
mod health;
mod llm_client;
mod manifest;
mod reasoning_loop;
mod tool_loop_detector;
mod tools;
mod types;

use config::RuntimeConfig;
use types::{TaskInput, TaskOutput};

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
        "sera-runtime-rs starting"
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

    // Read TaskInput from stdin
    let input = read_task_input().await;

    match input {
        Ok(task) => {
            tracing::info!(task_id = %task.task_id, "Processing task");
            let output = reasoning_loop::run(&config, task).await?;
            write_task_output(&output)?;
        }
        Err(e) => {
            // In persistent mode, no stdin task is expected — run health server only
            if config.lifecycle_mode == "persistent" {
                tracing::info!("Persistent mode — waiting for chat requests on port {health_port}");
                health_handle.await?;
            } else {
                tracing::error!("Failed to read task input: {e}");
                std::process::exit(1);
            }
        }
    }

    Ok(())
}

/// Read TaskInput from stdin as a JSON line.
async fn read_task_input() -> anyhow::Result<TaskInput> {
    use tokio::io::AsyncBufReadExt;

    let stdin = tokio::io::stdin();
    let mut reader = tokio::io::BufReader::new(stdin);
    let mut line = String::new();

    // Read with a timeout — persistent agents may not receive stdin
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        reader.read_line(&mut line),
    )
    .await;

    match result {
        Ok(Ok(0)) => anyhow::bail!("Empty stdin"),
        Ok(Ok(_)) => {
            let input: TaskInput = serde_json::from_str(line.trim())?;
            Ok(input)
        }
        Ok(Err(e)) => anyhow::bail!("Stdin read error: {e}"),
        Err(_) => anyhow::bail!("Stdin timeout — no task input received"),
    }
}

/// Write TaskOutput to stdout as JSON.
fn write_task_output(output: &TaskOutput) -> anyhow::Result<()> {
    let json = serde_json::to_string(output)?;
    println!("{json}");
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
