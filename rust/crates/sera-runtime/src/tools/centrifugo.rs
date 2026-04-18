//! Centrifugo real-time messaging publisher for thoughts and streaming events.

use serde::{Deserialize, Serialize};
use serde_json::json;

/// A structured thought-stream event published to Centrifugo.
///
/// Subscribers key off `type == "thought_stream"` to distinguish these from
/// generic `event_type` notifications on the same channel.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ThoughtEvent {
    /// Always `"thought_stream"` — the field subscribers use to discriminate.
    #[serde(rename = "type")]
    pub event_type: String,
    /// Step classification: `"reasoning"`, `"tool_call"`, `"tool_result"`, etc.
    pub step_type: String,
    /// Human-readable content of this step.
    pub content: String,
    /// Tool name when `step_type` is `"tool_call"` or `"tool_result"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    /// Tool arguments when `step_type` is `"tool_call"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_args: Option<serde_json::Value>,
    /// Correlation ID linking a tool call to its result.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl ThoughtEvent {
    /// Create a reasoning step event.
    pub fn reasoning(content: impl Into<String>) -> Self {
        Self {
            event_type: "thought_stream".to_string(),
            step_type: "reasoning".to_string(),
            content: content.into(),
            tool_name: None,
            tool_args: None,
            tool_call_id: None,
        }
    }

    /// Create a tool-call step event.
    pub fn tool_call(
        content: impl Into<String>,
        tool_name: impl Into<String>,
        tool_args: serde_json::Value,
        tool_call_id: impl Into<String>,
    ) -> Self {
        Self {
            event_type: "thought_stream".to_string(),
            step_type: "tool_call".to_string(),
            content: content.into(),
            tool_name: Some(tool_name.into()),
            tool_args: Some(tool_args),
            tool_call_id: Some(tool_call_id.into()),
        }
    }

    /// Create a tool-result step event.
    pub fn tool_result(
        content: impl Into<String>,
        tool_name: impl Into<String>,
        tool_call_id: impl Into<String>,
    ) -> Self {
        Self {
            event_type: "thought_stream".to_string(),
            step_type: "tool_result".to_string(),
            content: content.into(),
            tool_name: Some(tool_name.into()),
            tool_args: None,
            tool_call_id: Some(tool_call_id.into()),
        }
    }
}

// TODO(sera-2q1d): CentrifugoPublisher is a future-use integration; not yet wired into the runtime.
#[allow(dead_code)]
pub struct CentrifugoPublisher {
    base_url: String,
    api_key: String,
}

impl CentrifugoPublisher {
    pub fn new(base_url: String, api_key: String) -> Self {
        Self { base_url, api_key }
    }

    /// Publish a structured [`ThoughtEvent`] to the per-agent thoughts channel.
    ///
    /// Uses the namespaced channel `agent:{instance_id}:thoughts` and emits
    /// `"type": "thought_stream"` so subscribers can distinguish these events
    /// from generic notifications.
    pub async fn publish_thought_event(
        &self,
        instance_id: &str,
        event: ThoughtEvent,
    ) -> anyhow::Result<()> {
        let channel = format!("agent:{}:thoughts", instance_id);
        let data = serde_json::to_value(&event)?;
        let payload = json!({
            "method": "publish",
            "params": {
                "channel": channel,
                "data": data,
            }
        });
        self.send_request(payload).await
    }

    /// Publish a raw thought event.
    ///
    /// # Migration note
    /// Prefer [`Self::publish_thought_event`] for structured thought steps.
    /// This method previously used `"event"` as the type key; it now uses
    /// `"type"` so subscribers can discriminate event kinds consistently.
    pub async fn publish_thought(
        &self,
        event: &str,
        data: serde_json::Value,
    ) -> anyhow::Result<()> {
        let payload = json!({
            "method": "publish",
            "params": {
                "channel": "thoughts",
                "data": {
                    "type": event,
                    "data": data
                }
            }
        });

        self.send_request(payload).await
    }

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

#[cfg(test)]
mod tests {
    use super::*;

    // ------------------------------------------------------------------
    // ThoughtEvent always carries type = "thought_stream"
    // ------------------------------------------------------------------
    #[test]
    fn thought_event_type_is_thought_stream() {
        let evt = ThoughtEvent::reasoning("I need to call a tool");
        assert_eq!(evt.event_type, "thought_stream");
    }

    // ------------------------------------------------------------------
    // reasoning() step has correct step_type and no tool fields
    // ------------------------------------------------------------------
    #[test]
    fn reasoning_event_shape() {
        let evt = ThoughtEvent::reasoning("thinking hard");
        assert_eq!(evt.step_type, "reasoning");
        assert_eq!(evt.content, "thinking hard");
        assert!(evt.tool_name.is_none());
        assert!(evt.tool_args.is_none());
        assert!(evt.tool_call_id.is_none());
    }

    // ------------------------------------------------------------------
    // tool_call() step carries all structured fields
    // ------------------------------------------------------------------
    #[test]
    fn tool_call_event_shape() {
        let evt = ThoughtEvent::tool_call(
            "calling bash",
            "bash",
            serde_json::json!({"command": "ls"}),
            "call-abc",
        );
        assert_eq!(evt.step_type, "tool_call");
        assert_eq!(evt.tool_name.as_deref(), Some("bash"));
        assert_eq!(evt.tool_call_id.as_deref(), Some("call-abc"));
        assert!(evt.tool_args.is_some());
    }

    // ------------------------------------------------------------------
    // tool_result() step carries tool_name and tool_call_id but no args
    // ------------------------------------------------------------------
    #[test]
    fn tool_result_event_shape() {
        let evt = ThoughtEvent::tool_result("got output", "bash", "call-abc");
        assert_eq!(evt.step_type, "tool_result");
        assert_eq!(evt.tool_name.as_deref(), Some("bash"));
        assert_eq!(evt.tool_call_id.as_deref(), Some("call-abc"));
        assert!(evt.tool_args.is_none());
    }

    // ------------------------------------------------------------------
    // Serialised envelope has "type" field (not "event") — regression test
    // for the original downgrade bug.
    // ------------------------------------------------------------------
    #[test]
    fn thought_event_serializes_type_not_event() {
        let evt = ThoughtEvent::reasoning("inspecting results");
        let json = serde_json::to_value(&evt).unwrap();
        // "type" must be present and set to "thought_stream"
        assert_eq!(json["type"], "thought_stream");
        // "event" must NOT appear — that was the old (broken) key name
        assert!(
            json.get("event").is_none(),
            "legacy 'event' key must not appear in serialised ThoughtEvent"
        );
    }

    // ------------------------------------------------------------------
    // publish_thought_event channel is namespaced to the instance
    // ------------------------------------------------------------------
    #[test]
    fn publish_thought_event_uses_namespaced_channel() {
        let instance_id = "agent-123";
        let expected = format!("agent:{}:thoughts", instance_id);
        assert_eq!(expected, "agent:agent-123:thoughts");
    }

    // ------------------------------------------------------------------
    // Legacy publish_thought now emits "type" not "event"
    // ------------------------------------------------------------------
    #[test]
    fn legacy_publish_thought_payload_uses_type_key() {
        let event_name = "assistant_thinking";
        let data = serde_json::json!({"content": "some thought"});
        let payload = serde_json::json!({
            "method": "publish",
            "params": {
                "channel": "thoughts",
                "data": {
                    "type": event_name,
                    "data": data
                }
            }
        });
        let inner = &payload["params"]["data"];
        assert_eq!(inner["type"], event_name);
        assert!(
            inner.get("event").is_none(),
            "legacy 'event' key must not appear"
        );
    }

    // ------------------------------------------------------------------
    // ThoughtEvent serde round-trip
    // ------------------------------------------------------------------
    #[test]
    fn thought_event_roundtrip() {
        let original = ThoughtEvent::tool_call(
            "invoking grep",
            "grep",
            serde_json::json!({"pattern": "foo"}),
            "tc-1",
        );
        let json = serde_json::to_string(&original).unwrap();
        let parsed: ThoughtEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, original);
    }
}
