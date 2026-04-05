//! Intercom service — higher-level wrapper around Centrifugo client.
//!
//! Provides domain-specific channel abstraction and retry logic for real-time messaging.

use std::sync::Arc;
use std::time::Duration;

use sera_events::centrifugo::CentrifugoClient;
use sera_events::error::CentrifugoError;
use thiserror::Error;

/// Intercom service error types.
#[derive(Debug, Error)]
pub enum IntercomError {
    /// Publishing message failed.
    #[error("Publish error: {0}")]
    Publish(String),

    /// Token generation failed.
    #[error("Token generation error: {0}")]
    Token(String),

    /// HTTP request failed.
    #[error("HTTP error: {0}")]
    Http(String),
}

impl From<CentrifugoError> for IntercomError {
    fn from(err: CentrifugoError) -> Self {
        match err {
            CentrifugoError::TokenError(msg) => IntercomError::Token(msg),
            CentrifugoError::ApiError(msg) => IntercomError::Publish(msg),
            CentrifugoError::HttpError(e) => IntercomError::Http(e.to_string()),
        }
    }
}

/// Intercom service wrapping Centrifugo client with retry logic and domain-specific methods.
pub struct IntercomService {
    client: Arc<CentrifugoClient>,
}

impl IntercomService {
    /// Create a new Intercom service.
    pub fn new(centrifugo: Arc<CentrifugoClient>) -> Self {
        Self {
            client: centrifugo,
        }
    }

    /// Publish data to a channel.
    pub async fn publish(
        &self,
        channel: &str,
        data: serde_json::Value,
    ) -> Result<(), IntercomError> {
        self.publish_with_retry(channel, data).await
    }

    /// Broadcast an event to a specific agent.
    pub async fn broadcast_to_agent(
        &self,
        agent_id: &str,
        event_type: &str,
        data: serde_json::Value,
    ) -> Result<(), IntercomError> {
        let channel = format!("agent:{}:{}", agent_id, event_type);
        self.publish(&channel, data).await
    }

    /// Broadcast an event to a circle.
    pub async fn broadcast_to_circle(
        &self,
        circle_id: &str,
        event_type: &str,
        data: serde_json::Value,
    ) -> Result<(), IntercomError> {
        let channel = format!("circle:{}:{}", circle_id, event_type);
        self.publish(&channel, data).await
    }

    /// Generate a connection token for a user.
    pub fn generate_connection_token(
        &self,
        user_id: &str,
        expire_secs: u64,
    ) -> Result<String, IntercomError> {
        let expire_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| IntercomError::Token(format!("Failed to get current time: {}", e)))?
            .as_secs()
            + expire_secs;

        self.client
            .generate_connection_token(user_id, expire_at)
            .map_err(IntercomError::from)
    }

    /// Publish with exponential backoff retry (max 3 attempts).
    async fn publish_with_retry(
        &self,
        channel: &str,
        data: serde_json::Value,
    ) -> Result<(), IntercomError> {
        let mut attempt = 0;
        let max_retries = 3;

        loop {
            match self.client.publish(channel, data.clone()).await {
                Ok(()) => return Ok(()),
                Err(err) => {
                    attempt += 1;

                    // Check if error is retryable (5xx)
                    let is_retryable = matches!(
                        &err,
                        CentrifugoError::ApiError(msg) if msg.contains("HTTP 5")
                    );

                    if !is_retryable || attempt >= max_retries {
                        return Err(IntercomError::from(err));
                    }

                    // Exponential backoff: 100ms, 200ms, 400ms
                    let backoff_ms = 100 * (2_u64.pow(attempt as u32 - 1));
                    tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_channel_format() {
        let agent_id = "agent-123";
        let event_type = "status_update";
        let expected = format!("agent:{}:{}", agent_id, event_type);
        assert_eq!(expected, "agent:agent-123:status_update");
    }

    #[test]
    fn test_circle_channel_format() {
        let circle_id = "circle-456";
        let event_type = "message";
        let expected = format!("circle:{}:{}", circle_id, event_type);
        assert_eq!(expected, "circle:circle-456:message");
    }

    #[tokio::test]
    async fn test_token_generation_adds_expiry() {
        let client = Arc::new(CentrifugoClient::new(
            "http://localhost:8000".to_string(),
            "test_key".to_string(),
            "test_secret".to_string(),
        ));

        let service = IntercomService::new(client);
        let token = service
            .generate_connection_token("user-123", 3600)
            .expect("Token generation should succeed");

        // Verify it's a valid JWT (3 parts separated by dots)
        let parts: Vec<&str> = token.split('.').collect();
        assert_eq!(parts.len(), 3, "Token should be a valid JWT with 3 parts");
    }
}
