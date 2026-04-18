//! Knowledge store and query tools.
//!
//! Native `Tool` trait implementations (bead sera-ttrm-5):
//! - `KnowledgeStore`: [`RiskLevel::Write`] — persists a new block.
//! - `KnowledgeQuery`: [`RiskLevel::Read`] — read-only lookup.

use std::collections::HashMap;

use async_trait::async_trait;
use sera_types::tool::{
    ExecutionTarget, FunctionParameters, ParameterSchema, RiskLevel, Tool, ToolContext, ToolError,
    ToolInput, ToolMetadata, ToolOutput, ToolSchema,
};

// ── KnowledgeStore ──────────────────────────────────────────────────────────

/// Store knowledge blocks in the agent's memory.
pub struct KnowledgeStore;

#[async_trait]
impl Tool for KnowledgeStore {
    fn metadata(&self) -> ToolMetadata {
        ToolMetadata {
            name: "knowledge-store".to_string(),
            description:
                "Store a knowledge block in the agent's memory (requires core_url and identity_token)"
                    .to_string(),
            version: "1.0.0".to_string(),
            author: None,
            risk_level: RiskLevel::Write,
            execution_target: ExecutionTarget::Remote("sera-core".to_string()),
            tags: vec!["memory".to_string(), "knowledge".to_string()],
        }
    }

    fn schema(&self) -> ToolSchema {
        let mut properties: HashMap<String, ParameterSchema> = HashMap::new();
        properties.insert(
            "key".to_string(),
            ParameterSchema {
                schema_type: "string".to_string(),
                description: Some("Knowledge key identifier".to_string()),
                enum_values: None,
                default: None,
            },
        );
        properties.insert(
            "content".to_string(),
            ParameterSchema {
                schema_type: "string".to_string(),
                description: Some("Knowledge content to store".to_string()),
                enum_values: None,
                default: None,
            },
        );
        properties.insert(
            "scope".to_string(),
            ParameterSchema {
                schema_type: "string".to_string(),
                description: Some("Scope: agent or circle".to_string()),
                enum_values: Some(vec!["agent".to_string(), "circle".to_string()]),
                default: None,
            },
        );
        ToolSchema {
            parameters: FunctionParameters {
                schema_type: "object".to_string(),
                properties,
                required: vec!["key".to_string(), "content".to_string(), "scope".to_string()],
            },
        }
    }

    async fn execute(
        &self,
        input: ToolInput,
        _ctx: ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let args = &input.arguments;
        let key = args["key"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidInput("Missing 'key'".to_string()))?;
        let content = args["content"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidInput("Missing 'content'".to_string()))?;
        let scope = args["scope"].as_str().unwrap_or("agent");

        // Note: In the actual implementation, core_url and identity_token would come from config/env
        let core_url = std::env::var("SERA_CORE_URL")
            .unwrap_or_else(|_| "http://sera-core:3000".to_string());
        let token = std::env::var("SERA_IDENTITY_TOKEN").unwrap_or_default();

        let client = reqwest::Client::new();
        let payload = serde_json::json!({
            "key": key,
            "content": content,
            "scope": scope
        });

        let resp = client
            .post(format!("{}/api/memory/blocks", core_url))
            .bearer_auth(&token)
            .json(&payload)
            .send()
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("request failed: {e}")))?;

        if resp.status().is_success() {
            Ok(ToolOutput::success(format!(
                "Knowledge '{}' stored successfully",
                key
            )))
        } else {
            Ok(ToolOutput::success(format!(
                "Failed to store knowledge: {}",
                resp.status()
            )))
        }
    }
}

// ── KnowledgeQuery ──────────────────────────────────────────────────────────

/// Query knowledge blocks from the agent's memory.
pub struct KnowledgeQuery;

#[async_trait]
impl Tool for KnowledgeQuery {
    fn metadata(&self) -> ToolMetadata {
        ToolMetadata {
            name: "knowledge-query".to_string(),
            description:
                "Query knowledge blocks from the agent's memory (requires core_url and identity_token)"
                    .to_string(),
            version: "1.0.0".to_string(),
            author: None,
            risk_level: RiskLevel::Read,
            execution_target: ExecutionTarget::Remote("sera-core".to_string()),
            tags: vec!["memory".to_string(), "knowledge".to_string()],
        }
    }

    fn schema(&self) -> ToolSchema {
        let mut properties: HashMap<String, ParameterSchema> = HashMap::new();
        properties.insert(
            "query".to_string(),
            ParameterSchema {
                schema_type: "string".to_string(),
                description: Some("Search query".to_string()),
                enum_values: None,
                default: None,
            },
        );
        properties.insert(
            "limit".to_string(),
            ParameterSchema {
                schema_type: "integer".to_string(),
                description: Some("Maximum results (default 10)".to_string()),
                enum_values: None,
                default: None,
            },
        );
        properties.insert(
            "scope".to_string(),
            ParameterSchema {
                schema_type: "string".to_string(),
                description: Some("Scope to search: agent or circle".to_string()),
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
        let limit = args["limit"].as_u64().unwrap_or(10);
        let scope = args["scope"].as_str();

        let core_url = std::env::var("SERA_CORE_URL")
            .unwrap_or_else(|_| "http://sera-core:3000".to_string());
        let token = std::env::var("SERA_IDENTITY_TOKEN").unwrap_or_default();

        let mut payload = serde_json::json!({
            "query": query,
            "limit": limit
        });

        if let Some(s) = scope {
            payload["scope"] = serde_json::Value::String(s.to_string());
        }

        let client = reqwest::Client::new();
        let resp = client
            .post(format!("{}/api/knowledge/query", core_url))
            .bearer_auth(&token)
            .json(&payload)
            .send()
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("request failed: {e}")))?;

        if resp.status().is_success() {
            let text = resp
                .text()
                .await
                .map_err(|e| ToolError::ExecutionFailed(format!("read body: {e}")))?;
            Ok(ToolOutput::success(text))
        } else {
            Ok(ToolOutput::success(format!(
                "Failed to query knowledge: {}",
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
        assert_eq!(KnowledgeStore.metadata().risk_level, RiskLevel::Write);
        assert_eq!(KnowledgeQuery.metadata().risk_level, RiskLevel::Read);
    }
}
