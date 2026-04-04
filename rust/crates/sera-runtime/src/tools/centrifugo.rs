//! Centrifugo real-time messaging publisher for thoughts and streaming events.

use serde_json::json;

#[allow(dead_code)]
pub struct CentrifugoPublisher {
    base_url: String,
    api_key: String,
}

impl CentrifugoPublisher {
    #[allow(dead_code)]
    pub fn new(base_url: String, api_key: String) -> Self {
        Self { base_url, api_key }
    }

    #[allow(dead_code)]
    pub async fn publish_thought(&self, event: &str, data: serde_json::Value) -> anyhow::Result<()> {
        let payload = json!({
            "method": "publish",
            "params": {
                "channel": "thoughts",
                "data": {
                    "event": event,
                    "data": data
                }
            }
        });

        self.send_request(payload).await
    }

    #[allow(dead_code)]
    pub async fn publish_token_chunk(&self, chunk: &str, model: &str) -> anyhow::Result<()> {
        let payload = json!({
            "method": "publish",
            "params": {
                "channel": "tokens",
                "data": {
                    "chunk": chunk,
                    "model": model
                }
            }
        });

        self.send_request(payload).await
    }

    #[allow(dead_code)]
    pub async fn publish_tool_output(&self, tool_name: &str, output: &str) -> anyhow::Result<()> {
        let payload = json!({
            "method": "publish",
            "params": {
                "channel": "tools",
                "data": {
                    "tool": tool_name,
                    "output": output
                }
            }
        });

        self.send_request(payload).await
    }

    #[allow(dead_code)]
    async fn send_request(&self, payload: serde_json::Value) -> anyhow::Result<()> {
        let client = reqwest::Client::new();
        let resp = client
            .post(format!("{}/api/publish", self.base_url))
            .header("Authorization", format!("apikey {}", self.api_key))
            .json(&payload)
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(anyhow::anyhow!(
                "Centrifugo publish failed: {} {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            ));
        }

        Ok(())
    }
}
