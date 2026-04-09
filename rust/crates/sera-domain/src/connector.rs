//! Channel connector types — the outbound messaging layer of SERA.
//!
//! Channel connectors bridge the SERA gateway to external messaging platforms
//! (Discord, Slack, Telegram, Webhooks, etc.). Each connector maps to one agent
//! and handles both inbound event production (via the gateway event loop) and
//! outbound message delivery.
//!
//! See SPEC-gateway §8 for the full design.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ── ChannelType ───────────────────────────────────────────────────────────────

/// The type of external channel a connector integrates with.
/// SPEC-gateway §8: each connector kind has distinct auth and protocol requirements.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChannelType {
    Discord,
    Slack,
    Telegram,
    Webhook,
    #[serde(untagged)]
    Custom(String),
}

impl std::fmt::Display for ChannelType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Discord => write!(f, "discord"),
            Self::Slack => write!(f, "slack"),
            Self::Telegram => write!(f, "telegram"),
            Self::Webhook => write!(f, "webhook"),
            Self::Custom(name) => write!(f, "{name}"),
        }
    }
}

// ── ConnectorStatus ───────────────────────────────────────────────────────────

/// The current connection state of a channel connector.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase", tag = "status")]
pub enum ConnectorStatus {
    Connected,
    Disconnected,
    Connecting,
    #[serde(rename = "error")]
    Error(String),
}

// ── ConnectorIdentity ─────────────────────────────────────────────────────────

/// Maps a connector to the agent principal it routes messages to.
/// SPEC-gateway §8: identity determines routing from channel events to agent sessions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectorIdentity {
    /// The connector's unique name (matches manifest metadata.name).
    pub connector_name: String,
    /// The kind of external channel.
    pub channel_type: ChannelType,
    /// The agent this connector routes inbound messages to.
    pub agent_id: String,
    /// Optional account identifier (e.g., Discord bot token identifier, Slack workspace ID).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
}

// ── ConnectorError ────────────────────────────────────────────────────────────

/// Errors that can occur during connector operations.
#[derive(Debug, thiserror::Error)]
pub enum ConnectorError {
    #[error("connection failed: {0}")]
    ConnectionFailed(String),
    #[error("send failed: {0}")]
    SendFailed(String),
    #[error("authentication error: {0}")]
    AuthError(String),
    #[error("connector is not connected")]
    NotConnected,
    #[error("configuration error: {0}")]
    ConfigError(String),
}

// ── OutboundMessage ───────────────────────────────────────────────────────────

/// A message to be sent through a channel connector to an external platform.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutboundMessage {
    /// Target channel or DM identifier (platform-specific, e.g., Discord channel ID).
    pub channel: String,
    /// The message content to send.
    pub content: String,
    /// Optional platform-specific metadata (e.g., embeds, attachments, formatting).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, serde_json::Value>>,
}

// ── ChannelConnector trait ────────────────────────────────────────────────────

/// A channel connector bridges SERA to an external messaging platform.
///
/// Connectors are registered in the `ConnectorRegistry` and driven by the
/// gateway event loop. Each connector is responsible for:
/// - Authenticating with the external platform
/// - Receiving inbound messages and converting them to `Event`s
/// - Delivering outbound `OutboundMessage`s from agent responses
///
/// SPEC-gateway §8: connectors are `Send + Sync` so the registry can hold them
/// across async tasks.
#[async_trait]
pub trait ChannelConnector: Send + Sync {
    /// The connector's unique name (must match `ConnectorIdentity::connector_name`).
    fn name(&self) -> &str;

    /// The type of external channel this connector integrates with.
    fn channel_type(&self) -> ChannelType;

    /// Establish a connection to the external platform.
    /// Called once during startup or reconnection.
    async fn connect(&mut self) -> Result<(), ConnectorError>;

    /// Gracefully disconnect from the external platform.
    async fn disconnect(&mut self) -> Result<(), ConnectorError>;

    /// Send an outbound message through this connector.
    /// The connector must be in `Connected` state.
    async fn send(&self, message: OutboundMessage) -> Result<(), ConnectorError>;

    /// Current connection status.
    fn status(&self) -> ConnectorStatus;

    /// Identity mapping — connector name, channel type, and target agent.
    fn identity(&self) -> &ConnectorIdentity;
}

// ── ConnectorRegistry ─────────────────────────────────────────────────────────

/// Registry of all active channel connectors.
///
/// The gateway holds a single `ConnectorRegistry` and routes outbound messages
/// through it. `connect_all` is called during startup to bring all connectors online.
#[derive(Default)]
pub struct ConnectorRegistry {
    connectors: HashMap<String, Box<dyn ChannelConnector>>,
}

impl ConnectorRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            connectors: HashMap::new(),
        }
    }

    /// Register a connector. The connector's `name()` becomes its registry key.
    /// If a connector with the same name already exists, it is replaced.
    pub fn register(&mut self, connector: Box<dyn ChannelConnector>) {
        let name = connector.name().to_string();
        self.connectors.insert(name, connector);
    }

    /// Look up a connector by name.
    pub fn get(&self, name: &str) -> Option<&dyn ChannelConnector> {
        self.connectors.get(name).map(|c| c.as_ref())
    }

    /// List all registered connectors: (name, channel_type, status).
    pub fn list(&self) -> Vec<(&str, ChannelType, ConnectorStatus)> {
        self.connectors
            .values()
            .map(|c| (c.name(), c.channel_type(), c.status()))
            .collect()
    }

    /// Connect all registered connectors concurrently.
    /// Returns one result per connector — failures do not abort others.
    pub async fn connect_all(&mut self) -> Vec<(String, Result<(), ConnectorError>)> {
        // Collect names first to avoid borrow issues
        let names: Vec<String> = self.connectors.keys().cloned().collect();
        let mut results = Vec::with_capacity(names.len());
        for name in names {
            if let Some(connector) = self.connectors.get_mut(&name) {
                let result = connector.connect().await;
                results.push((name, result));
            }
        }
        results
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Test double ──────────────────────────────────────────────────────────

    struct MockConnector {
        identity: ConnectorIdentity,
        status: ConnectorStatus,
    }

    impl MockConnector {
        fn new(name: &str, channel_type: ChannelType, agent_id: &str) -> Self {
            Self {
                identity: ConnectorIdentity {
                    connector_name: name.to_string(),
                    channel_type: channel_type.clone(),
                    agent_id: agent_id.to_string(),
                    account_id: None,
                },
                status: ConnectorStatus::Disconnected,
            }
        }
    }

    #[async_trait]
    impl ChannelConnector for MockConnector {
        fn name(&self) -> &str {
            &self.identity.connector_name
        }

        fn channel_type(&self) -> ChannelType {
            self.identity.channel_type.clone()
        }

        async fn connect(&mut self) -> Result<(), ConnectorError> {
            self.status = ConnectorStatus::Connected;
            Ok(())
        }

        async fn disconnect(&mut self) -> Result<(), ConnectorError> {
            self.status = ConnectorStatus::Disconnected;
            Ok(())
        }

        async fn send(&self, _message: OutboundMessage) -> Result<(), ConnectorError> {
            if self.status != ConnectorStatus::Connected {
                return Err(ConnectorError::NotConnected);
            }
            Ok(())
        }

        fn status(&self) -> ConnectorStatus {
            self.status.clone()
        }

        fn identity(&self) -> &ConnectorIdentity {
            &self.identity
        }
    }

    // ── ConnectorRegistry tests ──────────────────────────────────────────────

    #[test]
    fn registry_register_and_get() {
        let mut registry = ConnectorRegistry::new();
        let connector = MockConnector::new("discord-main", ChannelType::Discord, "sera");
        registry.register(Box::new(connector));

        let got = registry.get("discord-main");
        assert!(got.is_some());
        assert_eq!(got.unwrap().name(), "discord-main");

        assert!(registry.get("nonexistent").is_none());
    }

    #[test]
    fn registry_list() {
        let mut registry = ConnectorRegistry::new();
        registry.register(Box::new(MockConnector::new(
            "discord-main",
            ChannelType::Discord,
            "sera",
        )));
        registry.register(Box::new(MockConnector::new(
            "slack-ops",
            ChannelType::Slack,
            "ops-agent",
        )));

        let list = registry.list();
        assert_eq!(list.len(), 2);

        let names: Vec<&str> = list.iter().map(|(n, _, _)| *n).collect();
        assert!(names.contains(&"discord-main"));
        assert!(names.contains(&"slack-ops"));

        for (_, _, status) in &list {
            assert_eq!(*status, ConnectorStatus::Disconnected);
        }
    }

    #[tokio::test]
    async fn registry_connect_all() {
        let mut registry = ConnectorRegistry::new();
        registry.register(Box::new(MockConnector::new(
            "discord-main",
            ChannelType::Discord,
            "sera",
        )));
        registry.register(Box::new(MockConnector::new(
            "slack-ops",
            ChannelType::Slack,
            "ops-agent",
        )));

        let results = registry.connect_all().await;
        assert_eq!(results.len(), 2);
        for (name, result) in &results {
            assert!(result.is_ok(), "connect failed for {name}");
        }

        // Verify connectors are now connected
        for (_, _, status) in registry.list() {
            assert_eq!(status, ConnectorStatus::Connected);
        }
    }

    // ── ChannelType serde roundtrip ──────────────────────────────────────────

    #[test]
    fn channel_type_serde_roundtrip() {
        let cases = vec![
            (ChannelType::Discord, "\"discord\""),
            (ChannelType::Slack, "\"slack\""),
            (ChannelType::Telegram, "\"telegram\""),
            (ChannelType::Webhook, "\"webhook\""),
        ];

        for (channel_type, expected_json) in cases {
            let json = serde_json::to_string(&channel_type).unwrap();
            assert_eq!(json, expected_json);
            let parsed: ChannelType = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, channel_type);
        }
    }

    #[test]
    fn channel_type_custom_serde() {
        let custom = ChannelType::Custom("matrix".to_string());
        let json = serde_json::to_string(&custom).unwrap();
        let parsed: ChannelType = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, custom);
    }

    #[test]
    fn channel_type_display() {
        assert_eq!(ChannelType::Discord.to_string(), "discord");
        assert_eq!(ChannelType::Slack.to_string(), "slack");
        assert_eq!(ChannelType::Telegram.to_string(), "telegram");
        assert_eq!(ChannelType::Webhook.to_string(), "webhook");
        assert_eq!(ChannelType::Custom("irc".to_string()).to_string(), "irc");
    }

    // ── ConnectorStatus tests ────────────────────────────────────────────────

    #[test]
    fn connector_status_variants() {
        let statuses = vec![
            ConnectorStatus::Connected,
            ConnectorStatus::Disconnected,
            ConnectorStatus::Connecting,
            ConnectorStatus::Error("timeout".to_string()),
        ];

        for status in &statuses {
            // Clone and equality checks
            let cloned = status.clone();
            assert_eq!(&cloned, status);
        }
    }

    #[test]
    fn connector_status_error_message() {
        let status = ConnectorStatus::Error("auth failed".to_string());
        if let ConnectorStatus::Error(msg) = &status {
            assert_eq!(msg, "auth failed");
        } else {
            panic!("expected Error variant");
        }
    }

    // ── ConnectorIdentity tests ──────────────────────────────────────────────

    #[test]
    fn connector_identity_construction() {
        let identity = ConnectorIdentity {
            connector_name: "discord-main".to_string(),
            channel_type: ChannelType::Discord,
            agent_id: "sera".to_string(),
            account_id: Some("bot-token-id-123".to_string()),
        };

        assert_eq!(identity.connector_name, "discord-main");
        assert_eq!(identity.channel_type, ChannelType::Discord);
        assert_eq!(identity.agent_id, "sera");
        assert_eq!(identity.account_id.as_deref(), Some("bot-token-id-123"));
    }

    #[test]
    fn connector_identity_without_account_id() {
        let identity = ConnectorIdentity {
            connector_name: "webhook-ci".to_string(),
            channel_type: ChannelType::Webhook,
            agent_id: "ci-agent".to_string(),
            account_id: None,
        };

        let json = serde_json::to_string(&identity).unwrap();
        assert!(!json.contains("account_id"), "None fields should be skipped");

        let parsed: ConnectorIdentity = serde_json::from_str(&json).unwrap();
        assert!(parsed.account_id.is_none());
    }

    // ── ConnectorError tests ─────────────────────────────────────────────────

    #[test]
    fn connector_error_messages() {
        assert_eq!(
            ConnectorError::ConnectionFailed("timeout".to_string()).to_string(),
            "connection failed: timeout"
        );
        assert_eq!(
            ConnectorError::SendFailed("rate limited".to_string()).to_string(),
            "send failed: rate limited"
        );
        assert_eq!(
            ConnectorError::AuthError("invalid token".to_string()).to_string(),
            "authentication error: invalid token"
        );
        assert_eq!(ConnectorError::NotConnected.to_string(), "connector is not connected");
        assert_eq!(
            ConnectorError::ConfigError("missing token".to_string()).to_string(),
            "configuration error: missing token"
        );
    }

    // ── OutboundMessage tests ────────────────────────────────────────────────

    #[test]
    fn outbound_message_construction() {
        let msg = OutboundMessage {
            channel: "1234567890".to_string(),
            content: "Hello from SERA!".to_string(),
            metadata: None,
        };
        assert_eq!(msg.channel, "1234567890");
        assert_eq!(msg.content, "Hello from SERA!");
        assert!(msg.metadata.is_none());
    }

    #[test]
    fn outbound_message_with_metadata() {
        let mut meta = HashMap::new();
        meta.insert("embed_color".to_string(), serde_json::json!(0x5865F2));
        let msg = OutboundMessage {
            channel: "ch-1".to_string(),
            content: "test".to_string(),
            metadata: Some(meta),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: OutboundMessage = serde_json::from_str(&json).unwrap();
        assert!(parsed.metadata.is_some());
        assert_eq!(parsed.metadata.unwrap()["embed_color"], 0x5865F2);
    }
}
