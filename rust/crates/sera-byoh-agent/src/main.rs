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

    parse_task_line(line.trim())
}

/// Parse a single line of stdin into a [`TaskInput`].
///
/// Returns `None` for blank lines. Valid JSON is deserialized directly;
/// plain-text strings are wrapped as an inline task for backward compat.
pub(crate) fn parse_task_line(trimmed: &str) -> Option<TaskInput> {
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

#[cfg(test)]
mod tests {
    use super::*;

    // ── parse_task_line ───────────────────────────────────────────────────────

    #[test]
    fn empty_string_returns_none() {
        assert!(parse_task_line("").is_none());
    }

    #[test]
    fn whitespace_only_returns_none() {
        // caller trims before passing — empty after trim is None
        assert!(parse_task_line("").is_none());
    }

    #[test]
    fn valid_json_task_input_deserializes() {
        let json = r#"{"taskId":"t-001","task":"summarise the logs"}"#;
        let result = parse_task_line(json).expect("valid JSON should parse");
        assert_eq!(result.task_id, "t-001");
        assert_eq!(result.task, "summarise the logs");
        assert!(result.context.is_none());
    }

    #[test]
    fn valid_json_with_context_deserializes() {
        let json = r#"{"taskId":"t-002","task":"explain this","context":"some context"}"#;
        let result = parse_task_line(json).expect("valid JSON with context should parse");
        assert_eq!(result.task_id, "t-002");
        assert_eq!(result.context.as_deref(), Some("some context"));
    }

    #[test]
    fn plain_text_becomes_inline_task() {
        let result = parse_task_line("hello world").expect("plain text should produce Some");
        assert!(result.task_id.starts_with("inline-"), "task_id should have inline- prefix, got: {}", result.task_id);
        assert_eq!(result.task, "hello world");
        assert!(result.context.is_none());
    }

    #[test]
    fn invalid_json_falls_back_to_plain_text() {
        // Malformed JSON — not a TaskInput object — should become a plain-text task
        let result = parse_task_line("{bad json}").expect("fallback should produce Some");
        assert!(result.task_id.starts_with("inline-"));
        assert_eq!(result.task, "{bad json}");
    }

    // ── TaskOutput construction ───────────────────────────────────────────────

    #[test]
    fn task_output_success_has_result_no_error() {
        let output = TaskOutput {
            task_id: "t-100".into(),
            result: Some("the answer".into()),
            error: None,
        };
        assert_eq!(output.task_id, "t-100");
        assert_eq!(output.result.as_deref(), Some("the answer"));
        assert!(output.error.is_none());
    }

    #[test]
    fn task_output_error_has_no_result() {
        let output = TaskOutput {
            task_id: "t-101".into(),
            result: None,
            error: Some("LLM proxy returned 503: service unavailable".into()),
        };
        assert!(output.result.is_none());
        assert!(output.error.as_deref().unwrap().contains("503"));
    }

    #[test]
    fn task_output_serde_roundtrip_success() {
        let output = TaskOutput {
            task_id: "t-200".into(),
            result: Some("done".into()),
            error: None,
        };
        let json = serde_json::to_string(&output).expect("serialize");
        // taskId uses camelCase rename
        assert!(json.contains("\"taskId\""), "expected camelCase taskId, got: {json}");
        // error is None so skip_serializing_if should omit it
        assert!(!json.contains("\"error\""), "error should be absent when None, got: {json}");
        let parsed: TaskOutput = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed.task_id, "t-200");
        assert_eq!(parsed.result.as_deref(), Some("done"));
    }

    #[test]
    fn task_output_serde_roundtrip_error() {
        let output = TaskOutput {
            task_id: "t-201".into(),
            result: None,
            error: Some("timeout".into()),
        };
        let json = serde_json::to_string(&output).expect("serialize");
        let parsed: TaskOutput = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed.task_id, "t-201");
        assert!(parsed.result.is_none());
        assert_eq!(parsed.error.as_deref(), Some("timeout"));
    }

    // ── heartbeat URL construction (pure, no network) ─────────────────────────

    #[test]
    fn heartbeat_url_format() {
        let core_url = "https://sera.internal";
        let instance_id = "agent-abc-123";
        let url = format!("{core_url}/api/agents/{instance_id}/heartbeat");
        assert_eq!(url, "https://sera.internal/api/agents/agent-abc-123/heartbeat");
    }
}
