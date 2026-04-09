//! Core types for the sera-runtime agent worker.

use serde::{Deserialize, Serialize};

/// Input received from stdin — describes the task to execute.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskInput {
    pub task_id: String,
    pub prompt: String,
    #[serde(default)]
    pub context: Vec<ChatMessage>,
    #[allow(dead_code)]
    pub agent_id: Option<String>,
    #[allow(dead_code)]
    pub session_id: Option<String>,
    pub max_iterations: Option<u32>,
}

/// Output written to stdout — the result of task execution.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskOutput {
    pub task_id: String,
    pub status: String,
    pub result: Option<String>,
    pub error: Option<String>,
    pub messages: Vec<ChatMessage>,
    pub tool_calls: Vec<ToolCallRecord>,
    pub usage: UsageStats,
}

/// A chat message in the conversation.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// An LLM tool call request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: ToolCallFunction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallFunction {
    pub name: String,
    pub arguments: String,
}

/// Record of a tool execution for the output.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallRecord {
    pub tool_name: String,
    pub arguments: serde_json::Value,
    pub result: String,
    pub duration_ms: u64,
}

/// Token usage statistics.
#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageStats {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub cache_creation_tokens: u32,
    pub cache_read_tokens: u32,
    pub total_tokens: u32,
    pub iterations: u32,
}

/// LLM response from the chat completions endpoint.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct LlmResponse {
    pub choices: Vec<LlmChoice>,
    #[serde(default)]
    pub usage: Option<LlmUsage>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct LlmChoice {
    pub message: ChatMessage,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[allow(dead_code)]
pub struct LlmUsage {
    #[serde(default)]
    pub prompt_tokens: u32,
    #[serde(default)]
    pub completion_tokens: u32,
    #[serde(default)]
    #[allow(dead_code)]
    pub cache_creation_tokens: u32,
    #[serde(default)]
    #[allow(dead_code)]
    pub cache_read_tokens: u32,
    #[serde(default)]
    #[allow(dead_code)]
    pub total_tokens: u32,
}

/// Tool definition sent to the LLM.
#[derive(Debug, Clone, Serialize)]
pub struct ToolDefinition {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: FunctionDefinition,
}

#[derive(Debug, Clone, Serialize)]
pub struct FunctionDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}
