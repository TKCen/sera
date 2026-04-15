//! Model response types.

use serde::{Deserialize, Serialize};

/// A response from a model provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelResponse {
    /// The generated message content.
    pub content: String,
    /// The reason the model stopped generating.
    pub finish_reason: FinishReason,
    /// Usage statistics for this completion.
    pub usage: Usage,
    /// Any tool calls the model made.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
}

/// Reason the model stopped generating.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    Stop,
    Length,
    ContentFilter,
    ToolCalls,
}

/// A tool call made by the model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    /// The tool call ID.
    pub id: String,
    /// The name of the tool.
    pub name: String,
    /// The arguments to the tool (JSON).
    pub arguments: serde_json::Value,
}

/// Usage statistics for a completion.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}
