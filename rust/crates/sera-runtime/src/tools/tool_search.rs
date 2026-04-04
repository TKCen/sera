//! Tool and skill discovery tools.

use super::ToolExecutor;

pub struct ToolSearch;

#[async_trait::async_trait]
impl ToolExecutor for ToolSearch {
    fn name(&self) -> &str { "tool-search" }
    fn description(&self) -> &str { "Search for available tools by name or description (requires core_url and identity_token)" }
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Search query for tools" }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, args: &serde_json::Value) -> anyhow::Result<String> {
        let query = args["query"].as_str().ok_or_else(|| anyhow::anyhow!("Missing 'query'"))?;

        let core_url = std::env::var("SERA_CORE_URL").unwrap_or_else(|_| "http://sera-core:3000".to_string());
        let token = std::env::var("SERA_IDENTITY_TOKEN").unwrap_or_default();

        let client = reqwest::Client::new();
        let resp = client
            .get(format!("{}/api/tools/catalog", core_url))
            .bearer_auth(&token)
            .send()
            .await?;

        if resp.status().is_success() {
            let catalog: serde_json::Value = resp.json().await?;

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
                Ok(serde_json::to_string_pretty(&matches)?)
            } else {
                Ok("No tools found".to_string())
            }
        } else {
            Ok(format!("Failed to query tools: {}", resp.status()))
        }
    }
}

pub struct SkillSearch;

#[async_trait::async_trait]
impl ToolExecutor for SkillSearch {
    fn name(&self) -> &str { "skill-search" }
    fn description(&self) -> &str { "Search for available skills by name (requires core_url and identity_token)" }
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Search query for skills" }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, args: &serde_json::Value) -> anyhow::Result<String> {
        let query = args["query"].as_str().ok_or_else(|| anyhow::anyhow!("Missing 'query'"))?;

        let core_url = std::env::var("SERA_CORE_URL").unwrap_or_else(|_| "http://sera-core:3000".to_string());
        let token = std::env::var("SERA_IDENTITY_TOKEN").unwrap_or_default();

        let client = reqwest::Client::new();
        let resp = client
            .get(format!("{}/api/skills", core_url))
            .bearer_auth(&token)
            .send()
            .await?;

        if resp.status().is_success() {
            let catalog: serde_json::Value = resp.json().await?;

            // Filter by query
            if let Some(skills) = catalog.as_array() {
                let mut matches = Vec::new();
                for skill in skills {
                    let name = skill.get("name").and_then(|n| n.as_str()).unwrap_or("");

                    if name.contains(query) {
                        matches.push(skill.clone());
                    }
                }
                Ok(serde_json::to_string_pretty(&matches)?)
            } else {
                Ok("No skills found".to_string())
            }
        } else {
            Ok(format!("Failed to query skills: {}", resp.status()))
        }
    }
}
