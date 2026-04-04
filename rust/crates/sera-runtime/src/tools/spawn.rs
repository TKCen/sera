//! Spawn ephemeral subagent tool.

use super::ToolExecutor;

pub struct SpawnEphemeral;

#[async_trait::async_trait]
impl ToolExecutor for SpawnEphemeral {
    fn name(&self) -> &str { "spawn-ephemeral" }
    fn description(&self) -> &str { "Spawn an ephemeral subagent to execute a task (requires core_url and identity_token)" }
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "task": { "type": "string", "description": "Task prompt for the subagent" },
                "agent_template": { "type": "string", "description": "Agent template name to use (optional)" }
            },
            "required": ["task"]
        })
    }

    async fn execute(&self, args: &serde_json::Value) -> anyhow::Result<String> {
        let task = args["task"].as_str().ok_or_else(|| anyhow::anyhow!("Missing 'task'"))?;
        let agent_template = args["agent_template"].as_str();

        let core_url = std::env::var("SERA_CORE_URL").unwrap_or_else(|_| "http://sera-core:3000".to_string());
        let token = std::env::var("SERA_IDENTITY_TOKEN").unwrap_or_default();

        let mut payload = serde_json::json!({
            "task": task
        });

        if let Some(template) = agent_template {
            payload["agent_template"] = serde_json::Value::String(template.to_string());
        }

        let client = reqwest::Client::new();
        let resp = client
            .post(format!("{}/api/sandbox/subagent", core_url))
            .bearer_auth(&token)
            .json(&payload)
            .send()
            .await?;

        if resp.status().is_success() {
            let body = resp.text().await?;
            Ok(body)
        } else {
            Ok(format!("Failed to spawn subagent: {}", resp.status()))
        }
    }
}
