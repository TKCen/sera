//! Web fetch tool for retrieving web page content.

use super::ToolExecutor;

pub struct WebFetch;

#[async_trait::async_trait]
impl ToolExecutor for WebFetch {
    fn name(&self) -> &str { "web-fetch" }
    fn description(&self) -> &str { "Fetch the content of a web page and return text (truncated to max_length)" }
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "url": { "type": "string", "description": "URL to fetch" },
                "max_length": { "type": "integer", "description": "Maximum content length in bytes (default 50000)" }
            },
            "required": ["url"]
        })
    }

    async fn execute(&self, args: &serde_json::Value) -> anyhow::Result<String> {
        let url = args["url"].as_str().ok_or_else(|| anyhow::anyhow!("Missing 'url'"))?;
        let max_length = args["max_length"].as_u64().unwrap_or(50_000) as usize;

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .user_agent("SERA-Agent/1.0")
            .build()?;

        let resp = client.get(url).send().await?;

        if !resp.status().is_success() {
            return Ok(format!("HTTP {}: Failed to fetch {}", resp.status(), url));
        }

        let mut content = resp.text().await.unwrap_or_default();
        if content.len() > max_length {
            content.truncate(max_length);
            content.push_str("\n[... truncated ...]");
        }

        Ok(content)
    }
}
