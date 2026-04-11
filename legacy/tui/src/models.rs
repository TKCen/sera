use serde::{Deserialize, Serialize};

/// A single agent shown in the agent list.
/// MVS exposes no agent-listing endpoint, so the TUI hardcodes one "sera" agent.
#[derive(Debug, Clone, Deserialize)]
pub struct AgentInstance {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub status: String,
}

/// Sync (non-streaming) response from POST /api/chat with stream:false.
#[derive(Debug, Deserialize)]
pub struct SyncChatResponse {
    /// The assistant's reply text.
    #[serde(default)]
    pub response: String,
    #[serde(rename = "session_id", default)]
    #[allow(dead_code)]
    pub session_id: String,
}

/// One SSE `event: message` data payload from the streaming response.
#[derive(Debug, Deserialize)]
pub struct SseDelta {
    #[serde(default)]
    pub delta: String,
    /// Retained for future session tracking; currently unused by the TUI.
    #[serde(default)]
    #[allow(dead_code)]
    pub session_id: String,
}

/// Thought events — kept for UI compatibility (thoughts panel stays visible).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ThoughtEvent {
    #[serde(rename = "stepType", default)]
    pub step_type: String,
    #[serde(default)]
    pub content: String,
    #[serde(rename = "agentDisplayName", default)]
    pub agent_display_name: String,
}

pub struct ChatMessage {
    pub sender: String,
    pub text: String,
}

pub enum WsEvent {
    Token(String),
    Done,
    Thought(ThoughtEvent),
    Error(String),
}
