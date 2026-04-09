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
/// SPEC-gateway: events are the gateway's lingua franca — every inbound action
/// is wrapped in an Event before entering the routing pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    /// A message from a user or channel.
    Message,
    /// Periodic health check / keepalive.
    Heartbeat,
    /// Scheduled trigger (cron, workflow timer).
    Cron,
    /// Inbound webhook payload.
    Webhook,
    /// Hook-generated event (from a hook chain result).
    Hook,
    /// System-level event (startup, shutdown, config change).
    System,
    /// HITL approval event (approval request or response).
    Approval,
    /// Workflow trigger (dreaming, scheduled task, etc.).
    Workflow,
}

/// Where the event originated.
/// SPEC-gateway: source determines routing rules and trust level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventSource {
    /// From a channel connector (Discord, Slack, etc.).
    Channel,
    /// From the cron scheduler or workflow timer.
    Scheduler,
    /// From the HTTP/WS API directly.
    Api,
    /// Internal system event.
    Internal,
    /// From an external agent via A2A protocol.
    #[serde(rename = "a2a")]
    A2A,
    /// From an external agent via ACP protocol.
    #[serde(rename = "acp")]
    ACP,
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
    /// Target agent for multi-agent routing (SPEC-gateway §6).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recipient: Option<String>,
    /// Optional idempotency key for deduplication (SPEC-gateway §4.1).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
    /// Approval spec if this event requires HITL approval (SPEC-hitl-approval).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requires_approval: Option<serde_json::Value>,
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
            recipient: None,
            idempotency_key: None,
            requires_approval: None,
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

    /// Create a heartbeat event for an agent session.
    pub fn heartbeat(agent_id: &str, session_key: &str) -> Self {
        Self {
            id: EventId::generate(),
            kind: EventKind::Heartbeat,
            source: EventSource::Internal,
            agent_id: agent_id.to_string(),
            session_key: session_key.to_string(),
            principal: PrincipalRef {
                id: crate::principal::PrincipalId::new("system"),
                kind: crate::principal::PrincipalKind::System,
            },
            text: None,
            recipient: None,
            idempotency_key: None,
            requires_approval: None,
            timestamp: chrono::Utc::now().to_rfc3339(),
            metadata: serde_json::Value::Null,
        }
    }

    /// Create a cron/scheduled event triggering a workflow.
    pub fn cron(
        agent_id: &str,
        session_key: &str,
        principal: PrincipalRef,
        workflow_name: &str,
    ) -> Self {
        Self {
            id: EventId::generate(),
            kind: EventKind::Cron,
            source: EventSource::Scheduler,
            agent_id: agent_id.to_string(),
            session_key: session_key.to_string(),
            principal,
            text: Some(workflow_name.to_string()),
            recipient: None,
            idempotency_key: None,
            requires_approval: None,
            timestamp: chrono::Utc::now().to_rfc3339(),
            metadata: serde_json::Value::Null,
        }
    }

    /// Create a webhook event with a JSON payload.
    pub fn webhook(
        agent_id: &str,
        session_key: &str,
        principal: PrincipalRef,
        payload: serde_json::Value,
    ) -> Self {
        Self {
            id: EventId::generate(),
            kind: EventKind::Webhook,
            source: EventSource::Api,
            agent_id: agent_id.to_string(),
            session_key: session_key.to_string(),
            principal,
            text: None,
            recipient: None,
            idempotency_key: None,
            requires_approval: None,
            timestamp: chrono::Utc::now().to_rfc3339(),
            metadata: payload,
        }
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

    #[test]
    fn event_kind_all_variants_serde() {
        let variants = vec![
            (EventKind::Message, "message"),
            (EventKind::Heartbeat, "heartbeat"),
            (EventKind::Cron, "cron"),
            (EventKind::Webhook, "webhook"),
            (EventKind::Hook, "hook"),
            (EventKind::System, "system"),
            (EventKind::Approval, "approval"),
            (EventKind::Workflow, "workflow"),
        ];
        for (kind, expected) in variants {
            let json = serde_json::to_string(&kind).unwrap();
            assert_eq!(json, format!("\"{expected}\""));
            let parsed: EventKind = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, kind);
        }
    }

    #[test]
    fn event_source_all_variants_serde() {
        let variants = vec![
            (EventSource::Channel, "channel"),
            (EventSource::Scheduler, "scheduler"),
            (EventSource::Api, "api"),
            (EventSource::Internal, "internal"),
            (EventSource::A2A, "a2a"),
            (EventSource::ACP, "acp"),
        ];
        for (source, expected) in variants {
            let json = serde_json::to_string(&source).unwrap();
            assert_eq!(json, format!("\"{expected}\""));
            let parsed: EventSource = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, source);
        }
    }

    #[test]
    fn heartbeat_event() {
        let event = Event::heartbeat("sera", "agent:sera:main");
        assert_eq!(event.kind, EventKind::Heartbeat);
        assert_eq!(event.source, EventSource::Internal);
        assert!(event.text.is_none());
    }

    #[test]
    fn cron_event() {
        let event = Event::cron("sera", "agent:sera:main", test_principal(), "dreaming");
        assert_eq!(event.kind, EventKind::Cron);
        assert_eq!(event.source, EventSource::Scheduler);
        assert_eq!(event.text.as_deref(), Some("dreaming"));
    }

    #[test]
    fn webhook_event() {
        let payload = serde_json::json!({"action": "push", "repo": "sera"});
        let event = Event::webhook("sera", "agent:sera:main", test_principal(), payload);
        assert_eq!(event.kind, EventKind::Webhook);
        assert_eq!(event.source, EventSource::Api);
        assert!(event.text.is_none());
        assert_eq!(event.metadata["action"], "push");
    }

    #[test]
    fn event_with_recipient() {
        let mut event = Event::message("sera", "agent:sera:main", test_principal(), "Hello");
        event.recipient = Some("agent:reviewer".to_string());
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("recipient"));
        let parsed: Event = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.recipient.as_deref(), Some("agent:reviewer"));
    }

    #[test]
    fn event_with_approval() {
        let mut event = Event::message("sera", "agent:sera:main", test_principal(), "rm -rf /");
        event.requires_approval = Some(serde_json::json!({"scope": "tool_call", "urgency": "high"}));
        let json = serde_json::to_string(&event).unwrap();
        let parsed: Event = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.requires_approval.unwrap()["urgency"], "high");
    }
}
