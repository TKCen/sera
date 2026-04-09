//! Event model — the unit of work flowing through the SERA gateway.
//!
//! Events are the gateway's lingua franca: every inbound message, webhook,
//! cron trigger, or system action is wrapped in an Event before entering
//! the routing pipeline. See SPEC-gateway and the PRD event model.

use serde::{Deserialize, Serialize};

use crate::principal::PrincipalRef;

/// Unique event identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EventId(pub String);

impl EventId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Generate a new random event ID.
    pub fn generate() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }
}

impl std::fmt::Display for EventId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// The kind of event flowing through the system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    /// A message from a user or channel.
    Message,
    /// System-level event (startup, shutdown, config change).
    System,
}

/// Where the event originated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventSource {
    /// From a channel connector (Discord, Slack, etc.).
    Channel,
    /// From the HTTP/WS API directly.
    Api,
    /// Internal system event.
    Internal,
}

/// An event flowing through the SERA gateway.
///
/// MVS scope: Message and System events only. No webhooks, no cron triggers,
/// no approval events. Queue modes are simple FIFO.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub id: EventId,
    pub kind: EventKind,
    pub source: EventSource,
    /// The agent this event is targeted at.
    pub agent_id: String,
    /// The session key for routing (e.g., "agent:sera:main").
    pub session_key: String,
    /// The acting principal who generated this event.
    pub principal: PrincipalRef,
    /// The message text (for Message events).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Optional idempotency key for deduplication.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
    /// Timestamp in ISO 8601 format.
    pub timestamp: String,
    /// Arbitrary metadata.
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub metadata: serde_json::Value,
}

impl Event {
    /// Create a new message event from a channel.
    pub fn message(
        agent_id: &str,
        session_key: &str,
        principal: PrincipalRef,
        text: &str,
    ) -> Self {
        Self {
            id: EventId::generate(),
            kind: EventKind::Message,
            source: EventSource::Channel,
            agent_id: agent_id.to_string(),
            session_key: session_key.to_string(),
            principal,
            text: Some(text.to_string()),
            idempotency_key: None,
            timestamp: chrono::Utc::now().to_rfc3339(),
            metadata: serde_json::Value::Null,
        }
    }

    /// Create a new message event from the API.
    pub fn api_message(
        agent_id: &str,
        session_key: &str,
        principal: PrincipalRef,
        text: &str,
    ) -> Self {
        let mut event = Self::message(agent_id, session_key, principal, text);
        event.source = EventSource::Api;
        event
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::principal::{PrincipalId, PrincipalKind};

    fn test_principal() -> PrincipalRef {
        PrincipalRef {
            id: PrincipalId::new("test-user"),
            kind: PrincipalKind::Human,
        }
    }

    #[test]
    fn message_event_construction() {
        let event = Event::message("sera", "agent:sera:main", test_principal(), "Hello");
        assert_eq!(event.kind, EventKind::Message);
        assert_eq!(event.source, EventSource::Channel);
        assert_eq!(event.agent_id, "sera");
        assert_eq!(event.text.as_deref(), Some("Hello"));
        assert!(!event.id.0.is_empty());
    }

    #[test]
    fn api_message_event() {
        let event = Event::api_message("sera", "agent:sera:main", test_principal(), "Hi");
        assert_eq!(event.source, EventSource::Api);
    }

    #[test]
    fn event_roundtrip() {
        let event = Event::message("sera", "agent:sera:main", test_principal(), "test");
        let json = serde_json::to_string(&event).unwrap();
        let parsed: Event = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.agent_id, "sera");
        assert_eq!(parsed.text.as_deref(), Some("test"));
    }

    #[test]
    fn event_id_generate_unique() {
        let a = EventId::generate();
        let b = EventId::generate();
        assert_ne!(a, b);
    }

    #[test]
    fn event_kind_serde() {
        let json = serde_json::to_string(&EventKind::Message).unwrap();
        assert_eq!(json, "\"message\"");
    }
}
