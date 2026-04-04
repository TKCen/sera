//! HTTP API client for sera-core.

use anyhow::{anyhow, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;

/// Represents an agent instance from the API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Agent {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub display_name: Option<String>,
    pub template_ref: String,
    pub status: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
}

/// Request payload for the chat endpoint.
#[derive(Debug, Serialize)]
pub struct ChatRequest {
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_instance_id: Option<String>,
    pub stream: bool,
}

/// Response from the chat endpoint.
#[derive(Debug, Deserialize)]
pub struct ChatResponse {
    pub session_id: String,
    pub message_id: String,
}

/// A log entry from the agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub timestamp: String,
    pub level: String,
    pub message: String,
}

/// HTTP client for SERA API.
pub struct ApiClient {
    base_url: String,
    api_key: String,
    client: Client,
}

impl ApiClient {
    /// Create a new API client with the given base URL and API key.
    pub fn new(base_url: String, api_key: String) -> Self {
        Self {
            base_url,
            api_key,
            client: Client::new(),
        }
    }

    /// Make an authenticated GET request.
    async fn get<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T> {
        let url = format!("{}{}", self.base_url, path);
        let resp = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(anyhow!("API error: {}", resp.status()));
        }

        Ok(resp.json().await?)
    }

    /// Make an authenticated POST request.
    async fn post<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        body: serde_json::Value,
    ) -> Result<T> {
        let url = format!("{}{}", self.base_url, path);
        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(anyhow!("API error: {}", resp.status()));
        }

        Ok(resp.json().await?)
    }

    /// List all agent instances.
    pub async fn list_agents(&self) -> Result<Vec<Agent>> {
        self.get("/api/agents/instances").await
    }

    /// Get a specific agent by ID.
    pub async fn get_agent(&self, id: &str) -> Result<Agent> {
        self.get(&format!("/api/agents/{}", id)).await
    }

    /// Get logs for an agent.
    pub async fn get_agent_logs(&self, id: &str) -> Result<Vec<LogEntry>> {
        // This is a placeholder — the actual endpoint may differ
        // For now, return an empty list
        Ok(Vec::new())
    }

    /// Send a chat message to an agent.
    pub async fn send_chat(&self, agent_id: &str, message: &str) -> Result<ChatResponse> {
        let body = json!({
            "message": message,
            "agent_instance_id": agent_id,
            "stream": true,
        });
        self.post("/api/chat", body).await
    }
}
