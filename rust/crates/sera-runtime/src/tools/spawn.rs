//! Spawn ephemeral subagent tool.
//!
//! Native `Tool` trait implementation (bead sera-ttrm-5) with
//! [`RiskLevel::Execute`] — spawning a subagent can run arbitrary tasks.

use std::collections::HashMap;

use async_trait::async_trait;
use sera_types::tool::{
    ExecutionTarget, FunctionParameters, ParameterSchema, RiskLevel, Tool, ToolContext, ToolError,
    ToolInput, ToolMetadata, ToolOutput, ToolSchema, ToolScope,
};

pub struct SpawnEphemeral;

#[async_trait]
impl Tool for SpawnEphemeral {
    fn metadata(&self) -> ToolMetadata {
        ToolMetadata {
            name: "spawn-ephemeral".to_string(),
            description: "Spawn an ephemeral subagent to execute a task (requires core_url and identity_token)"
                .to_string(),
            version: "1.0.0".to_string(),
            author: None,
            risk_level: RiskLevel::Execute,
            execution_target: ExecutionTarget::Remote("sera-core".to_string()),
            tags: vec!["subagent".to_string()],
            // sera-u4gj: spawn-ephemeral is a sub-agent dispatch tool, so
            // `Agent` scope per design report §5.4.
            scope: ToolScope::Agent,
        }
    }

    fn schema(&self) -> ToolSchema {
        let mut properties: HashMap<String, ParameterSchema> = HashMap::new();
        properties.insert(
            "task".to_string(),
            ParameterSchema {
                schema_type: "string".to_string(),
                description: Some("Task prompt for the subagent".to_string()),
                enum_values: None,
                default: None,
            },
        );
        properties.insert(
            "agent_template".to_string(),
            ParameterSchema {
                schema_type: "string".to_string(),
                description: Some("Agent template name to use (optional)".to_string()),
                enum_values: None,
                default: None,
            },
        );
        ToolSchema {
            parameters: FunctionParameters {
                schema_type: "object".to_string(),
                properties,
                required: vec!["task".to_string()],
            },
        }
    }

    async fn execute(
        &self,
        input: ToolInput,
        _ctx: ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let args = &input.arguments;
        let task = args["task"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidInput("Missing 'task'".to_string()))?;
        let agent_template = args["agent_template"].as_str();

        let core_url = std::env::var("SERA_CORE_URL")
            .unwrap_or_else(|_| "http://sera-core:3000".to_string());
        let token = std::env::var("SERA_IDENTITY_TOKEN").unwrap_or_default();

        let mut payload = serde_json::json!({
            "task": task
        });

        if let Some(template) = agent_template {
            payload["agent_template"] = serde_json::Value::String(template.to_string());
        }

        // sera-rdg4: pre-flight `core_url` through the internal-service
        // validator (allowlist-relaxed for `sera-core` etc., strict
        // fallback for unsafe overrides; cloud-metadata always blocked)
        // and pin the resolved addrs on the client.
        let client = crate::tools::safe_client::build_internal_service_client(&core_url)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        let resp = client
            .post(format!("{}/api/sandbox/subagent", core_url))
            .bearer_auth(&token)
            .json(&payload)
            .send()
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("request failed: {e}")))?;

        if resp.status().is_success() {
            let body = resp
                .text()
                .await
                .map_err(|e| ToolError::ExecutionFailed(format!("read body: {e}")))?;
            Ok(ToolOutput::success(body))
        } else {
            Ok(ToolOutput::success(format!(
                "Failed to spawn subagent: {}",
                resp.status()
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_risk_level_is_execute() {
        assert_eq!(SpawnEphemeral.metadata().risk_level, RiskLevel::Execute);
        assert_eq!(SpawnEphemeral.metadata().name, "spawn-ephemeral");
    }

    /// sera-u4gj — spawn-ephemeral dispatches sub-agents, so it carries
    /// `ToolScope::Agent` per design report §5.4.
    #[test]
    fn metadata_scope_is_agent() {
        assert_eq!(SpawnEphemeral.metadata().scope, ToolScope::Agent);
    }
}
