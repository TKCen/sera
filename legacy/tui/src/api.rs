use crate::models::{AgentInstance, SyncChatResponse};
use serde_json::json;
use std::io::Read;

pub struct ApiClient {
    pub base_url: String,
    pub api_key: String,
}

impl ApiClient {
    pub fn new(base_url: String, api_key: String) -> Self {
        Self { base_url, api_key }
    }

    /// GET /health — returns true if the server responds with status "ok".
    pub fn health(&self) -> Result<bool, Box<dyn std::error::Error>> {
        let url = format!("{}/health", self.base_url);
        let body: serde_json::Value = ureq::get(&url)
            .call()
            .map_err(|e| format!("GET /health: {}", e))?
            .into_json()?;
        Ok(body.get("status").and_then(|s| s.as_str()) == Some("ok"))
    }

    /// Returns a hardcoded list containing the single "sera" agent.
    /// MVS has no agent-listing endpoint; the agent name comes from sera.yaml.
    pub fn get_instances(&self) -> Result<Vec<AgentInstance>, Box<dyn std::error::Error>> {
        // Confirm the server is reachable first.
        self.health()?;
        Ok(vec![AgentInstance {
            id: "sera".to_owned(),
            name: "sera".to_owned(),
            status: "running".to_owned(),
        }])
    }

    /// POST /api/chat with stream:false — returns the full sync response.
    pub fn send_chat_sync(
        &self,
        message: &str,
        agent: &str,
        _session_id: Option<&str>,
    ) -> Result<SyncChatResponse, Box<dyn std::error::Error>> {
        let url = format!("{}/api/chat", self.base_url);
        let body = json!({
            "message": message,
            "agent": agent,
            "stream": false,
        });
        let resp: SyncChatResponse = ureq::post(&url)
            .set("Authorization", &format!("Bearer {}", self.api_key))
            .set("Content-Type", "application/json")
            .send_json(body)
            .map_err(|e| format!("POST /api/chat (sync): {}", e))?
            .into_json()?;
        Ok(resp)
    }

    /// POST /api/chat with stream:true — returns the SSE response body as a reader.
    /// The caller is responsible for reading and parsing SSE lines.
    pub fn send_chat_stream(
        &self,
        message: &str,
        agent: &str,
        _session_id: Option<&str>,
    ) -> Result<Box<dyn Read + Send>, Box<dyn std::error::Error>> {
        let url = format!("{}/api/chat", self.base_url);
        let body = json!({
            "message": message,
            "agent": agent,
            "stream": true,
        });
        let resp = ureq::post(&url)
            .set("Authorization", &format!("Bearer {}", self.api_key))
            .set("Content-Type", "application/json")
            .send_json(body)
            .map_err(|e| format!("POST /api/chat (stream): {}", e))?;
        Ok(Box::new(resp.into_reader()))
    }
}
