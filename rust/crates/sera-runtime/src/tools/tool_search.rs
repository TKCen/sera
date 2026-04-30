//! Tool and skill discovery tools.
//!
//! Native `Tool` trait implementations (bead sera-ttrm-5). Both surfaces
//! query the core catalog — read-only observation, so
//! [`RiskLevel::Read`].

use std::collections::HashMap;

use async_trait::async_trait;
use sera_types::tool::{
    ExecutionTarget, FunctionParameters, ParameterSchema, RiskLevel, Tool, ToolContext, ToolError,
    ToolInput, ToolMetadata, ToolOutput, ToolSchema, ToolScope,
};

// ── ToolSearch ──────────────────────────────────────────────────────────────

pub struct ToolSearch;

#[async_trait]
impl Tool for ToolSearch {
    fn metadata(&self) -> ToolMetadata {
        ToolMetadata {
            name: "tool-search".to_string(),
            description:
                "Search for available tools by name or description (requires core_url and identity_token)"
                    .to_string(),
            version: "1.0.0".to_string(),
            author: None,
            risk_level: RiskLevel::Read,
            execution_target: ExecutionTarget::Remote("sera-core".to_string()),
            tags: vec!["discovery".to_string()],
            // sera-u4gj: catalog discovery is read-only metadata lookup —
            // no exfiltration risk distinct from other reads, so `Read`
            // (not `Network`) per design report §5.4. The transport
            // layer is internal-service-only by `safe_client`.
            scope: ToolScope::Read,
        }
    }

    fn schema(&self) -> ToolSchema {
        let mut properties: HashMap<String, ParameterSchema> = HashMap::new();
        properties.insert(
            "query".to_string(),
            ParameterSchema {
                schema_type: "string".to_string(),
                description: Some("Search query for tools".to_string()),
                enum_values: None,
                default: None,
            },
        );
        ToolSchema {
            parameters: FunctionParameters {
                schema_type: "object".to_string(),
                properties,
                required: vec!["query".to_string()],
            },
        }
    }

    async fn execute(
        &self,
        input: ToolInput,
        _ctx: ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let args = &input.arguments;
        let query = args["query"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidInput("Missing 'query'".to_string()))?;

        let core_url = std::env::var("SERA_CORE_URL")
            .unwrap_or_else(|_| "http://sera-core:3000".to_string());
        let token = std::env::var("SERA_IDENTITY_TOKEN").unwrap_or_default();

        // sera-rdg4: internal-service client (allowlisted internal SERA
        // hostnames are permitted to resolve to private IPs in Docker/k8s;
        // off-allowlist overrides are still rejected).
        let client = crate::tools::safe_client::build_internal_service_client(&core_url)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        let resp = client
            .get(format!("{}/api/tools/catalog", core_url))
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("request failed: {e}")))?;

        if resp.status().is_success() {
            let catalog: serde_json::Value = resp
                .json()
                .await
                .map_err(|e| ToolError::ExecutionFailed(format!("parse json: {e}")))?;

            // Filter by query
            if let Some(tools) = catalog.as_array() {
                let mut matches = Vec::new();
                for tool in tools {
                    let name = tool.get("name").and_then(|n| n.as_str()).unwrap_or("");
                    let desc = tool.get("description").and_then(|d| d.as_str()).unwrap_or("");

                    if name.contains(query) || desc.contains(query) {
                        matches.push(tool.clone());
                    }
                }
                let pretty = serde_json::to_string_pretty(&matches)
                    .map_err(|e| ToolError::ExecutionFailed(format!("serialize: {e}")))?;
                Ok(ToolOutput::success(pretty))
            } else {
                Ok(ToolOutput::success("No tools found".to_string()))
            }
        } else {
            Ok(ToolOutput::success(format!(
                "Failed to query tools: {}",
                resp.status()
            )))
        }
    }
}

// ── SkillSearch ─────────────────────────────────────────────────────────────

pub struct SkillSearch;

#[async_trait]
impl Tool for SkillSearch {
    fn metadata(&self) -> ToolMetadata {
        ToolMetadata {
            name: "skill-search".to_string(),
            description:
                "Search for available skills by name (requires core_url and identity_token)"
                    .to_string(),
            version: "1.0.0".to_string(),
            author: None,
            risk_level: RiskLevel::Read,
            execution_target: ExecutionTarget::Remote("sera-core".to_string()),
            tags: vec!["discovery".to_string()],
            // sera-u4gj: same rationale as `tool-search` — internal
            // catalog discovery, `Read` per design report §5.4.
            scope: ToolScope::Read,
        }
    }

    fn schema(&self) -> ToolSchema {
        let mut properties: HashMap<String, ParameterSchema> = HashMap::new();
        properties.insert(
            "query".to_string(),
            ParameterSchema {
                schema_type: "string".to_string(),
                description: Some("Search query for skills".to_string()),
                enum_values: None,
                default: None,
            },
        );
        ToolSchema {
            parameters: FunctionParameters {
                schema_type: "object".to_string(),
                properties,
                required: vec!["query".to_string()],
            },
        }
    }

    async fn execute(
        &self,
        input: ToolInput,
        _ctx: ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let args = &input.arguments;
        let query = args["query"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidInput("Missing 'query'".to_string()))?;

        let core_url = std::env::var("SERA_CORE_URL")
            .unwrap_or_else(|_| "http://sera-core:3000".to_string());
        let token = std::env::var("SERA_IDENTITY_TOKEN").unwrap_or_default();

        // sera-rdg4: internal-service client (allowlisted internal SERA
        // hostnames are permitted to resolve to private IPs in Docker/k8s;
        // off-allowlist overrides are still rejected).
        let client = crate::tools::safe_client::build_internal_service_client(&core_url)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        let resp = client
            .get(format!("{}/api/skills", core_url))
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("request failed: {e}")))?;

        if resp.status().is_success() {
            let catalog: serde_json::Value = resp
                .json()
                .await
                .map_err(|e| ToolError::ExecutionFailed(format!("parse json: {e}")))?;

            // Filter by query
            if let Some(skills) = catalog.as_array() {
                let mut matches = Vec::new();
                for skill in skills {
                    let name = skill.get("name").and_then(|n| n.as_str()).unwrap_or("");

                    if name.contains(query) {
                        matches.push(skill.clone());
                    }
                }
                let pretty = serde_json::to_string_pretty(&matches)
                    .map_err(|e| ToolError::ExecutionFailed(format!("serialize: {e}")))?;
                Ok(ToolOutput::success(pretty))
            } else {
                Ok(ToolOutput::success("No skills found".to_string()))
            }
        } else {
            Ok(ToolOutput::success(format!(
                "Failed to query skills: {}",
                resp.status()
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_risk_levels() {
        assert_eq!(ToolSearch.metadata().risk_level, RiskLevel::Read);
        assert_eq!(SkillSearch.metadata().risk_level, RiskLevel::Read);
    }

    /// sera-u4gj — both discovery tools are `ToolScope::Read`.
    #[test]
    fn metadata_scopes() {
        assert_eq!(ToolSearch.metadata().scope, ToolScope::Read);
        assert_eq!(SkillSearch.metadata().scope, ToolScope::Read);
    }
}
