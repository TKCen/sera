//! sera-agui — AG-UI (Agent-User Interaction) streaming protocol.
//!
//! Vendored event types from the [AG-UI protocol](https://github.com/ag-ui-protocol/ag-ui)
//! (CopilotKit). Defines the 17 canonical event types as a serde-tagged enum
//! and provides stream builder utilities for the gateway.
//!
//! The crate exposes both the **full event stream** (for `sera-web` and
//! compatible frontends) and a **thin client stream** (for HMIs and
//! embedded clients).
//!
//! See SPEC-interop §6 for the full protocol specification.

use serde::{Deserialize, Serialize};
use sera_errors::{SeraError, SeraErrorCode};
use thiserror::Error;


// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum AgUiError {
    #[error("stream closed")]
    StreamClosed,
    #[error("serialization error: {reason}")]
    Serialization { reason: String },
    #[error("invalid event: {reason}")]
    InvalidEvent { reason: String },
}

impl From<AgUiError> for SeraError {
    fn from(err: AgUiError) -> Self {
        let code = match &err {
            AgUiError::StreamClosed => SeraErrorCode::Unavailable,
            AgUiError::Serialization { .. } => SeraErrorCode::Serialization,
            AgUiError::InvalidEvent { .. } => SeraErrorCode::InvalidInput,
        };
        SeraError::new(code, err.to_string())
    }
}

// ---------------------------------------------------------------------------
// AG-UI Event Types (17 canonical events)
// ---------------------------------------------------------------------------

/// The 17 canonical AG-UI event types.
///
/// Each variant carries the payload for one event kind. The enum is
/// `#[serde(tag = "type")]` so that serialised JSON includes a `"type"`
/// discriminator matching the AG-UI spec's event names.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum AgUiEvent {
    // ---- Run lifecycle ----
    #[serde(rename = "RUN_STARTED")]
    RunStarted {
        thread_id: String,
        run_id: String,
    },

    #[serde(rename = "RUN_FINISHED")]
    RunFinished {
        thread_id: String,
        run_id: String,
    },

    #[serde(rename = "RUN_ERROR")]
    RunError {
        thread_id: String,
        run_id: String,
        message: String,
        code: Option<String>,
    },

    // ---- Text messages ----
    #[serde(rename = "TEXT_MESSAGE_START")]
    TextMessageStart {
        message_id: String,
        role: AgUiRole,
    },

    #[serde(rename = "TEXT_MESSAGE_CONTENT")]
    TextMessageContent {
        message_id: String,
        delta: String,
    },

    #[serde(rename = "TEXT_MESSAGE_END")]
    TextMessageEnd {
        message_id: String,
    },

    // ---- Tool calls ----
    #[serde(rename = "TOOL_CALL_START")]
    ToolCallStart {
        tool_call_id: String,
        tool_call_name: String,
        parent_message_id: Option<String>,
    },

    #[serde(rename = "TOOL_CALL_ARGS")]
    ToolCallArgs {
        tool_call_id: String,
        delta: String,
    },

    #[serde(rename = "TOOL_CALL_END")]
    ToolCallEnd {
        tool_call_id: String,
    },

    // ---- Tool results ----
    #[serde(rename = "TOOL_CALL_RESULT")]
    ToolCallResult {
        tool_call_id: String,
        result: String,
    },

    // ---- State management ----
    #[serde(rename = "STATE_SNAPSHOT")]
    StateSnapshot {
        snapshot: serde_json::Value,
    },

    #[serde(rename = "STATE_DELTA")]
    StateDelta {
        delta: Vec<serde_json::Value>,
    },

    // ---- Messages (complete) ----
    #[serde(rename = "MESSAGES_SNAPSHOT")]
    MessagesSnapshot {
        messages: Vec<serde_json::Value>,
    },

    // ---- Step lifecycle ----
    #[serde(rename = "STEP_STARTED")]
    StepStarted {
        step_name: String,
        #[serde(default)]
        metadata: serde_json::Value,
    },

    #[serde(rename = "STEP_FINISHED")]
    StepFinished {
        step_name: String,
        #[serde(default)]
        metadata: serde_json::Value,
    },

    // ---- Custom events ----
    #[serde(rename = "CUSTOM")]
    Custom {
        name: String,
        value: serde_json::Value,
    },

    // ---- Raw SSE passthrough ----
    #[serde(rename = "RAW")]
    Raw {
        event: String,
        data: String,
    },
}

/// Message role in AG-UI events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgUiRole {
    User,
    Assistant,
    System,
    Tool,
}

// ---------------------------------------------------------------------------
// Stream filtering (full vs. thin client)
// ---------------------------------------------------------------------------

/// The MVS minimum event subset for thin clients (SPEC-interop §8).
pub const THIN_CLIENT_EVENTS: &[&str] = &[
    "RUN_STARTED",
    "RUN_FINISHED",
    "RUN_ERROR",
    "TEXT_MESSAGE_START",
    "TEXT_MESSAGE_CONTENT",
    "TEXT_MESSAGE_END",
    "TOOL_CALL_START",
    "TOOL_CALL_ARGS",
    "TOOL_CALL_END",
    "STATE_SNAPSHOT",
];

impl AgUiEvent {
    /// Returns the event type tag as a string.
    pub fn event_type(&self) -> &'static str {
        match self {
            Self::RunStarted { .. } => "RUN_STARTED",
            Self::RunFinished { .. } => "RUN_FINISHED",
            Self::RunError { .. } => "RUN_ERROR",
            Self::TextMessageStart { .. } => "TEXT_MESSAGE_START",
            Self::TextMessageContent { .. } => "TEXT_MESSAGE_CONTENT",
            Self::TextMessageEnd { .. } => "TEXT_MESSAGE_END",
            Self::ToolCallStart { .. } => "TOOL_CALL_START",
            Self::ToolCallArgs { .. } => "TOOL_CALL_ARGS",
            Self::ToolCallEnd { .. } => "TOOL_CALL_END",
            Self::ToolCallResult { .. } => "TOOL_CALL_RESULT",
            Self::StateSnapshot { .. } => "STATE_SNAPSHOT",
            Self::StateDelta { .. } => "STATE_DELTA",
            Self::MessagesSnapshot { .. } => "MESSAGES_SNAPSHOT",
            Self::StepStarted { .. } => "STEP_STARTED",
            Self::StepFinished { .. } => "STEP_FINISHED",
            Self::Custom { .. } => "CUSTOM",
            Self::Raw { .. } => "RAW",
        }
    }

    /// Whether this event is included in the thin-client subset.
    pub fn is_thin_client_event(&self) -> bool {
        THIN_CLIENT_EVENTS.contains(&self.event_type())
    }

    /// Serialise as an SSE `data:` line (newline-delimited JSON).
    pub fn to_sse_data(&self) -> Result<String, AgUiError> {
        serde_json::to_string(self).map_err(|e| AgUiError::Serialization {
            reason: e.to_string(),
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_serde_roundtrip_run_started() {
        let evt = AgUiEvent::RunStarted {
            thread_id: "t1".into(),
            run_id: "r1".into(),
        };
        let json = serde_json::to_string(&evt).unwrap();
        assert!(json.contains("\"type\":\"RUN_STARTED\""));
        let back: AgUiEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(back.event_type(), "RUN_STARTED");
    }

    #[test]
    fn event_serde_text_content() {
        let evt = AgUiEvent::TextMessageContent {
            message_id: "m1".into(),
            delta: "hello ".into(),
        };
        let json = serde_json::to_string(&evt).unwrap();
        assert!(json.contains("TEXT_MESSAGE_CONTENT"));
        assert!(json.contains("hello "));
    }

    #[test]
    fn event_serde_tool_call() {
        let evt = AgUiEvent::ToolCallStart {
            tool_call_id: "tc1".into(),
            tool_call_name: "search".into(),
            parent_message_id: None,
        };
        let json = serde_json::to_string(&evt).unwrap();
        assert!(json.contains("TOOL_CALL_START"));
        assert!(json.contains("search"));
    }

    #[test]
    fn event_type_tag() {
        let evt = AgUiEvent::StateSnapshot {
            snapshot: serde_json::json!({"key": "value"}),
        };
        assert_eq!(evt.event_type(), "STATE_SNAPSHOT");
    }

    #[test]
    fn thin_client_filtering() {
        let full = AgUiEvent::StateDelta {
            delta: vec![serde_json::json!({"op": "replace"})],
        };
        assert!(!full.is_thin_client_event());

        let thin = AgUiEvent::RunStarted {
            thread_id: "t".into(),
            run_id: "r".into(),
        };
        assert!(thin.is_thin_client_event());
    }

    #[test]
    fn sse_data_serialization() {
        let evt = AgUiEvent::RunError {
            thread_id: "t1".into(),
            run_id: "r1".into(),
            message: "boom".into(),
            code: Some("INTERNAL".into()),
        };
        let sse = evt.to_sse_data().unwrap();
        assert!(sse.contains("RUN_ERROR"));
        assert!(sse.contains("boom"));
    }

    #[test]
    fn role_serde() {
        let r = AgUiRole::Assistant;
        let json = serde_json::to_string(&r).unwrap();
        assert_eq!(json, "\"assistant\"");
    }

    #[test]
    fn custom_event() {
        let evt = AgUiEvent::Custom {
            name: "approval_prompt".into(),
            value: serde_json::json!({"tool": "bash", "command": "rm -rf /"}),
        };
        assert_eq!(evt.event_type(), "CUSTOM");
        assert!(!evt.is_thin_client_event());
    }

    #[test]
    fn agui_error_to_sera_error() {
        let err = AgUiError::StreamClosed;
        let sera: SeraError = err.into();
        assert_eq!(sera.code, SeraErrorCode::Unavailable);
    }

    #[test]
    fn all_17_event_types_covered() {
        // Verify we have exactly 17 variants by constructing one of each
        let events: Vec<AgUiEvent> = vec![
            AgUiEvent::RunStarted { thread_id: "t".into(), run_id: "r".into() },
            AgUiEvent::RunFinished { thread_id: "t".into(), run_id: "r".into() },
            AgUiEvent::RunError { thread_id: "t".into(), run_id: "r".into(), message: "e".into(), code: None },
            AgUiEvent::TextMessageStart { message_id: "m".into(), role: AgUiRole::Assistant },
            AgUiEvent::TextMessageContent { message_id: "m".into(), delta: "d".into() },
            AgUiEvent::TextMessageEnd { message_id: "m".into() },
            AgUiEvent::ToolCallStart { tool_call_id: "tc".into(), tool_call_name: "n".into(), parent_message_id: None },
            AgUiEvent::ToolCallArgs { tool_call_id: "tc".into(), delta: "d".into() },
            AgUiEvent::ToolCallEnd { tool_call_id: "tc".into() },
            AgUiEvent::ToolCallResult { tool_call_id: "tc".into(), result: "r".into() },
            AgUiEvent::StateSnapshot { snapshot: serde_json::json!({}) },
            AgUiEvent::StateDelta { delta: vec![] },
            AgUiEvent::MessagesSnapshot { messages: vec![] },
            AgUiEvent::StepStarted { step_name: "s".into(), metadata: serde_json::json!({}) },
            AgUiEvent::StepFinished { step_name: "s".into(), metadata: serde_json::json!({}) },
            AgUiEvent::Custom { name: "c".into(), value: serde_json::json!({}) },
            AgUiEvent::Raw { event: "e".into(), data: "d".into() },
        ];
        assert_eq!(events.len(), 17);
    }
}
