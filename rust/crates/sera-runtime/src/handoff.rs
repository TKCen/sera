//! Handoff — first-class agent-to-agent handoff as a tool.

use serde::{Deserialize, Serialize};

/// Handoff input filter.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandoffInputFilter {
    None,
    RemoveAllTools,
}

/// Handoff input data passed to the handoff callback.
#[derive(Debug, Clone)]
pub struct HandoffInputData {
    pub input_history: Vec<serde_json::Value>,
    pub pre_handoff_items: Vec<serde_json::Value>,
    pub new_items: Vec<serde_json::Value>,
}

/// Handoff definition — a tool that transfers control to another agent.
pub struct Handoff {
    pub tool_name: String,
    pub tool_description: String,
    pub input_json_schema: serde_json::Value,
    pub input_filter: Option<HandoffInputFilter>,
}

impl Handoff {
    /// Convert this handoff into a tool definition for the LLM.
    pub fn as_tool_definition(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "function",
            "function": {
                "name": self.tool_name,
                "description": self.tool_description,
                "parameters": self.input_json_schema,
            }
        })
    }
}
