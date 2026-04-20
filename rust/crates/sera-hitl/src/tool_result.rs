//! `ToolResult::Rejected { feedback }` — the opencode `CorrectedError` pattern.
//!
//! SPEC-hitl-approval §5b. When a user rejects a tool call with a reason,
//! the reason is fed back to the LLM as a structured tool-result error so
//! the model can self-correct within the same turn (no turn restart).

use serde::{Deserialize, Serialize};

/// The outcome of a tool call as seen by the model.
///
/// `Rejected { feedback }` is the in-turn self-correction signal: the
/// runtime surfaces the user's rejection text as the tool-call error body
/// so the next model response can adapt without a turn boundary.
// TODO P1-INTEGRATION: wire ToolResult::Rejected through sera-runtime turn
//                      loop so the user's feedback reaches the model directly.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ToolResult {
    /// Successful tool invocation.
    Ok { output: serde_json::Value },
    /// Tool error — any non-user-rejection failure.
    Err { error: String },
    /// User rejected the call; `feedback` is surfaced to the model.
    Rejected { feedback: String },
}

impl ToolResult {
    /// `true` when this result should be fed back as a correction signal
    /// rather than a fatal error.
    pub fn is_rejected(&self) -> bool {
        matches!(self, Self::Rejected { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_result_rejected_serde_roundtrip() {
        let r = ToolResult::Rejected {
            feedback: "use git status instead".to_string(),
        };
        let json = serde_json::to_string(&r).unwrap();
        let parsed: ToolResult = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, r);
        assert!(parsed.is_rejected());
    }

    #[test]
    fn tool_result_ok_and_err_not_rejected() {
        assert!(!ToolResult::Ok { output: serde_json::json!({}) }.is_rejected());
        assert!(!ToolResult::Err { error: "boom".into() }.is_rejected());
    }
}
