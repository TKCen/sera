//! Chat and LLM conversation types.

use serde::{Deserialize, Serialize};

/// Role in an LLM conversation.
/// Maps from TS: 'user' | 'assistant' | 'system' | 'tool'
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChatRole {
    User,
    Assistant,
    System,
    Tool,
}

/// A single message in an LLM conversation.
/// Maps from TS: ChatMessage in agents/types.ts
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

/// An LLM tool call request.
/// Maps from TS: ToolCall in lib/llm/types.ts
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: ToolCallFunction,
}

/// The function portion of a tool call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallFunction {
    pub name: String,
    pub arguments: String,
}

/// A captured reasoning step from agent processing.
/// Maps from TS: CapturedThought in agents/types.ts
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapturedThought {
    pub timestamp: String,
    #[serde(rename = "stepType")]
    pub step_type: String,
    pub content: String,
}

/// Agent reasoning output — replaces the TS "bag of optionals" pattern
/// with an explicit enum for valid action states.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum AgentAction {
    #[serde(rename = "thinking")]
    Thinking { thought: String },
    #[serde(rename = "tool_call")]
    ToolCall {
        tool: String,
        args: serde_json::Value,
    },
    #[serde(rename = "delegation")]
    Delegation {
        agent_role: String,
        task: String,
    },
    #[serde(rename = "final_answer")]
    FinalAnswer {
        answer: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        thoughts: Option<Vec<CapturedThought>>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_role_roundtrip() {
        for role in [ChatRole::User, ChatRole::Assistant, ChatRole::System, ChatRole::Tool] {
            let json = serde_json::to_string(&role).unwrap();
            let parsed: ChatRole = serde_json::from_str(&json).unwrap();
            assert_eq!(role, parsed);
        }
    }

    #[test]
    fn chat_message_with_tool_call() {
        let msg = ChatMessage {
            role: ChatRole::Assistant,
            content: None,
            name: None,
            tool_calls: Some(vec![ToolCall {
                id: "call_1".to_string(),
                call_type: "function".to_string(),
                function: ToolCallFunction {
                    name: "shell-exec".to_string(),
                    arguments: r#"{"command":"ls"}"#.to_string(),
                },
            }]),
            tool_call_id: None,
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("shell-exec"));

        let parsed: ChatMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.role, ChatRole::Assistant);
        assert!(parsed.content.is_none());
        assert_eq!(parsed.tool_calls.unwrap().len(), 1);
    }

    #[test]
    fn agent_action_final_answer() {
        let action = AgentAction::FinalAnswer {
            answer: "Done".to_string(),
            thoughts: None,
        };
        let json = serde_json::to_string(&action).unwrap();
        assert!(json.contains("\"type\":\"final_answer\""));
        assert!(json.contains("\"answer\":\"Done\""));
    }
}
