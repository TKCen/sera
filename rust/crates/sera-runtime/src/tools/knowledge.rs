//! Knowledge store and query tools.

use super::ToolExecutor;

/// Store knowledge blocks in the agent's memory.
pub struct KnowledgeStore;

#[async_trait::async_trait]
impl ToolExecutor for KnowledgeStore {
    fn name(&self) -> &str { "knowledge-store" }
    fn description(&self) -> &str { "Store a knowledge block in the agent's memory (requires core_url and identity_token)" }
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "key": { "type": "string", "description": "Knowledge key identifier" },
                "content": { "type": "string", "description": "Knowledge content to store" },
                "scope": { "type": "string", "description": "Scope: agent or circle", "enum": ["agent", "circle"] }
            },
            "required": ["key", "content", "scope"]
        })
    }

    async fn execute(&self, args: &serde_json::Value) -> anyhow::Result<String> {
        let key = args["key"].as_str().ok_or_else(|| anyhow::anyhow!("Missing 'key'"))?;
        let content = args["content"].as_str().ok_or_else(|| anyhow::anyhow!("Missing 'content'"))?;
        let scope = args["scope"].as_str().unwrap_or("agent");

        // Note: In the actual implementation, core_url and identity_token would come from config/env
        let core_url = std::env::var("SERA_CORE_URL").unwrap_or_else(|_| "http://sera-core:3000".to_string());
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
            .await?;

        if resp.status().is_success() {
            Ok(format!("Knowledge '{}' stored successfully", key))
        } else {
            Ok(format!("Failed to store knowledge: {}", resp.status()))
        }
    }
}

/// Query knowledge blocks from the agent's memory.
pub struct KnowledgeQuery;

#[async_trait::async_trait]
impl ToolExecutor for KnowledgeQuery {
    fn name(&self) -> &str { "knowledge-query" }
    fn description(&self) -> &str { "Query knowledge blocks from the agent's memory (requires core_url and identity_token)" }
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Search query" },
                "limit": { "type": "integer", "description": "Maximum results (default 10)" },
                "scope": { "type": "string", "description": "Scope to search: agent or circle" }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, args: &serde_json::Value) -> anyhow::Result<String> {
        let query = args["query"].as_str().ok_or_else(|| anyhow::anyhow!("Missing 'query'"))?;
        let limit = args["limit"].as_u64().unwrap_or(10);
        let scope = args["scope"].as_str();

        let core_url = std::env::var("SERA_CORE_URL").unwrap_or_else(|_| "http://sera-core:3000".to_string());
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
            .await?;

        if resp.status().is_success() {
            let text = resp.text().await?;
            Ok(text)
        } else {
            Ok(format!("Failed to query knowledge: {}", resp.status()))
        }
    }
}
