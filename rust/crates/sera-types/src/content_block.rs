use serde::{Deserialize, Serialize};
use std::fmt;

/// Unique identifier for an action within a turn.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ActionId(pub String);

impl fmt::Display for ActionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for ActionId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl From<String> for ActionId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for ActionId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// A single block of content within a conversation message or transcript.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text { text: String },
    ToolUse { id: String, name: String, input: serde_json::Value },
    ToolResult { tool_use_id: String, content: String, is_error: bool },
    Image { media_type: String, data: String },
    Thinking { thinking: String },
}

/// Role of a participant in a conversation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConversationRole {
    User,
    Assistant,
    System,
    Tool,
}

/// A single message in a conversation history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationMessage {
    pub role: ConversationRole,
    pub content: Vec<ContentBlock>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<crate::runtime::TokenUsage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cause_by: Option<ActionId>,
}
