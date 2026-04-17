//! `ping` — hit `GET /api/health` on the gateway and report latency.

use std::time::Instant;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::json;

use sera_commands::{
    Command, CommandArgSchema, CommandArgs, CommandCategory, CommandContext, CommandDescription,
    CommandError, CommandResult,
};

use crate::http::build_client;

/// CLI `ping` subcommand — calls `GET /api/health` and prints "OK" + latency.
pub struct PingCommand;

#[async_trait]
impl Command for PingCommand {
    fn name(&self) -> &str {
        "ping"
    }

    fn describe(&self) -> CommandDescription {
        CommandDescription {
            summary: "Check gateway liveness by hitting GET /api/health".into(),
            help: "Sends a GET /api/health request to the configured gateway endpoint and \
                   prints OK plus round-trip latency.  Use --endpoint to override the \
                   target URL for this invocation."
                .into(),
            category: CommandCategory::Diagnostic,
        }
    }

    fn argument_schema(&self) -> CommandArgSchema {
        CommandArgSchema(
            clap::Command::new("ping")
                .about("Check gateway liveness (GET /api/health)")
                .arg(
                    clap::Arg::new("endpoint")
                        .long("endpoint")
                        .short('e')
                        .help("Gateway base URL (overrides config)")
                        .value_name("URL"),
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
        let url = format!("{endpoint}/api/health");

        let client = build_client().map_err(|e| CommandError::Execution(e.to_string()))?;

        let start = Instant::now();
        let response = client
            .get(&url)
            .send()
            .await
            .map_err(|e| CommandError::Execution(format!("request failed: {e}")))?;
        let latency_ms = start.elapsed().as_millis();

        let status = response.status();
        if !status.is_success() {
            return Err(CommandError::Execution(format!(
                "gateway returned HTTP {status}"
            )));
        }

        let body: serde_json::Value = response
            .json()
            .await
            .map_err(|e| CommandError::Execution(format!("failed to parse response: {e}")))?;

        let gateway_status = body
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");

        println!("OK  status={gateway_status}  latency={latency_ms}ms  endpoint={endpoint}");

        Ok(CommandResult::ok(json!({
            "status": gateway_status,
            "latency_ms": latency_ms,
            "endpoint": endpoint,
        })))
    }
}

/// Execute a ping against `endpoint` and return `(status, latency_ms)`.
///
/// Extracted as a free function so the integration test can call it directly.
pub async fn do_ping(endpoint: &str) -> Result<(String, u128)> {
    let url = format!("{}/api/health", endpoint.trim_end_matches('/'));
    let client = build_client()?;
    let start = Instant::now();
    let resp = client.get(&url).send().await?;
    let latency_ms = start.elapsed().as_millis();
    resp.error_for_status_ref()?;
    let body: serde_json::Value = resp.json().await?;
    let status = body
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_owned();
    Ok((status, latency_ms))
}
