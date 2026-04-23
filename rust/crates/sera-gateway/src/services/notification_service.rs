//! Notification service — multi-channel event dispatch.

use sera_telemetry::centrifugo::CentrifugoClient;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use thiserror::Error;

/// Notification event payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationEvent {
    /// Event type identifier.
    pub event_type: String,
    /// Optional agent ID associated with the event.
    pub agent_id: Option<String>,
    /// Event payload data.
    pub payload: serde_json::Value,
    /// Channels to dispatch this event to.
    pub channels: Vec<NotificationChannel>,
}

/// Notification channel configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NotificationChannel {
    /// Centrifugo real-time pub/sub channel.
    Centrifugo { channel: String },
    /// Structured logging output.
    Log,
    /// HTTP webhook delivery.
    Webhook { url: String },
}

/// Notification service errors.
#[derive(Debug, Error)]
pub enum NotificationError {
    /// Centrifugo client error.
    #[error("Centrifugo error: {0}")]
    Centrifugo(String),

    /// Webhook delivery error.
    #[error("Webhook error: {0}")]
    Webhook(String),

    /// IO or serialization error.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Service for dispatching notifications to multiple channels.
pub struct NotificationService {
    centrifugo: Option<Arc<CentrifugoClient>>,
    http_client: reqwest::Client,
}

impl NotificationService {
    /// Create a new NotificationService.
    ///
    /// # Arguments
    /// * `centrifugo` — optional Centrifugo client for real-time dispatch
    /// * `http_client` — HTTP client for webhook delivery
    pub fn new(centrifugo: Option<Arc<CentrifugoClient>>, http_client: reqwest::Client) -> Self {
        Self {
            centrifugo,
            http_client,
        }
    }

    /// Dispatch a notification event to all configured channels.
    ///
    /// # Arguments
    /// * `event` — the notification event to dispatch
    ///
    /// # Returns
    /// * `Ok(())` if dispatch succeeded
    /// * `Err(NotificationError)` if any channel dispatch fails
    pub async fn notify(&self, event: NotificationEvent) -> Result<(), NotificationError> {
        for channel in &event.channels {
            match channel {
                NotificationChannel::Centrifugo { channel: ch } => {
                    self.dispatch_centrifugo(ch, &event).await?;
                }
                NotificationChannel::Log => {
                    self.dispatch_log(&event);
                }
                NotificationChannel::Webhook { url } => {
                    self.dispatch_webhook(url, &event).await?;
                }
            }
        }
        Ok(())
    }

    /// Dispatch to Centrifugo via fire-and-forget.
    async fn dispatch_centrifugo(
        &self,
        channel: &str,
        event: &NotificationEvent,
    ) -> Result<(), NotificationError> {
        if let Some(client) = &self.centrifugo {
            let payload = serde_json::json!({
                "event_type": event.event_type,
                "agent_id": event.agent_id,
                "payload": event.payload,
            });

            let client = Arc::clone(client);
            let channel = channel.to_string();

            tokio::spawn(async move {
                if let Err(e) = client.publish(&channel, payload).await {
                    tracing::error!("Centrifugo publish failed: {}", e);
                }
            });
        }

        Ok(())
    }

    /// Dispatch to structured logging.
    fn dispatch_log(&self, event: &NotificationEvent) {
        tracing::info!(
            event_type = %event.event_type,
            agent_id = ?event.agent_id,
            payload = ?event.payload,
            "Notification dispatched"
        );
    }

    /// Dispatch to webhook via fire-and-forget.
    async fn dispatch_webhook(
        &self,
        url: &str,
        event: &NotificationEvent,
    ) -> Result<(), NotificationError> {
        let payload = serde_json::json!({
            "event_type": event.event_type,
            "agent_id": event.agent_id,
            "payload": event.payload,
        });

        let client = self.http_client.clone();
        let url = url.to_string();

        tokio::spawn(async move {
            match client.post(&url).json(&payload).send().await {
                Ok(response) => {
                    if !response.status().is_success() {
                        tracing::warn!(
                            "Webhook returned non-success status: {}",
                            response.status()
                        );
                    }
                }
                Err(e) => {
                    tracing::error!("Webhook delivery failed: {}", e);
                }
            }
        });

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notification_event_serializes() {
        let event = NotificationEvent {
            event_type: "agent_started".to_string(),
            agent_id: Some("agent-123".to_string()),
            payload: serde_json::json!({"status": "running"}),
            channels: vec![
                NotificationChannel::Log,
                NotificationChannel::Centrifugo {
                    channel: "agents".to_string(),
                },
            ],
        };

        let serialized = serde_json::to_string(&event).expect("should serialize");
        let deserialized: NotificationEvent =
            serde_json::from_str(&serialized).expect("should deserialize");

        assert_eq!(deserialized.event_type, event.event_type);
        assert_eq!(deserialized.agent_id, event.agent_id);
        assert_eq!(deserialized.channels.len(), 2);
    }

    #[test]
    fn notification_channel_variants() {
        let log_ch = NotificationChannel::Log;
        let centrifugo_ch = NotificationChannel::Centrifugo {
            channel: "test".to_string(),
        };
        let webhook_ch = NotificationChannel::Webhook {
            url: "https://example.com/webhook".to_string(),
        };

        let serialized_log = serde_json::to_string(&log_ch).expect("should serialize Log");
        let serialized_centrifugo =
            serde_json::to_string(&centrifugo_ch).expect("should serialize Centrifugo");
        let serialized_webhook =
            serde_json::to_string(&webhook_ch).expect("should serialize Webhook");

        let _: NotificationChannel =
            serde_json::from_str(&serialized_log).expect("should deserialize Log");
        let _: NotificationChannel =
            serde_json::from_str(&serialized_centrifugo).expect("should deserialize Centrifugo");
        let _: NotificationChannel =
            serde_json::from_str(&serialized_webhook).expect("should deserialize Webhook");
    }

    #[tokio::test]
    async fn service_creation() {
        let http_client = reqwest::Client::new();
        let service = NotificationService::new(None, http_client);

        assert!(service.centrifugo.is_none());
    }

    #[tokio::test]
    async fn dispatch_log_works() {
        let http_client = reqwest::Client::new();
        let service = NotificationService::new(None, http_client);

        let event = NotificationEvent {
            event_type: "test_event".to_string(),
            agent_id: Some("agent-1".to_string()),
            payload: serde_json::json!({"data": "test"}),
            channels: vec![NotificationChannel::Log],
        };

        let result = service.notify(event).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn dispatch_webhook_fires_and_forgets() {
        let http_client = reqwest::Client::new();
        let service = NotificationService::new(None, http_client);

        let event = NotificationEvent {
            event_type: "webhook_test".to_string(),
            agent_id: None,
            payload: serde_json::json!({}),
            channels: vec![NotificationChannel::Webhook {
                url: "https://example.com/webhook".to_string(),
            }],
        };

        let result = service.notify(event).await;
        assert!(result.is_ok());
    }

    #[test]
    fn notification_error_display() {
        let centrifugo_err = NotificationError::Centrifugo("test error".to_string());
        let webhook_err = NotificationError::Webhook("test error".to_string());

        assert!(centrifugo_err.to_string().contains("Centrifugo"));
        assert!(webhook_err.to_string().contains("Webhook"));
    }
}
