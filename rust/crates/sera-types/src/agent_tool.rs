//! Agent-as-tool — types for invoking another agent as a callable tool.
//!
//! See bead `sera-8d1.1` (GH#144). Three agent-tool kinds expose the
//! cross-agent dispatch surface to the runtime:
//!
//! - [`AgentToolKind::DelegateTask`] — synchronous "do this task and return
//!   a structured result". The caller blocks until the target produces output.
//! - [`AgentToolKind::AskAgent`] — synchronous "answer this question". The
//!   caller blocks until the target responds with a single answer string.
//! - [`AgentToolKind::BackgroundTask`] — fire-and-forget "start this task
//!   in the background". Returns a `task_id` immediately; the caller can
//!   poll the task asynchronously.
//!
//! The companion runtime registry [`crate::tool::Tool`] implementations live
//! in `sera_runtime::agent_tool_registry` and the `tools/agent_tools` module.

use serde::{Deserialize, Serialize};

/// A callable agent-tool descriptor — pairs a kind with a target agent id.
///
/// This is the on-the-wire description of a registered agent-tool entry; the
/// runtime registry uses it to dispatch the correct kind of cross-agent call.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentTool {
    /// What kind of agent-tool dispatch this entry performs.
    pub kind: AgentToolKind,
    /// Stable identifier of the target agent (e.g. `"researcher"`,
    /// `"coder"`).
    pub target_agent: String,
}

/// The three supported agent-tool kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AgentToolKind {
    /// Synchronous task delegation — the caller blocks until the target
    /// returns structured output and a token usage tally.
    DelegateTask,
    /// Synchronous Q&A — the caller blocks until the target answers.
    AskAgent,
    /// Asynchronous fire-and-forget — the caller receives a task id
    /// immediately and may poll for completion separately.
    BackgroundTask,
}

// ── delegate-task ─────────────────────────────────────────────────────────────

/// Input for [`AgentToolKind::DelegateTask`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DelegateTaskInput {
    /// Free-text task description.
    pub task: String,
    /// Optional structured context payload forwarded to the target agent.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub context: Option<serde_json::Value>,
}

/// Output for [`AgentToolKind::DelegateTask`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DelegateTaskOutput {
    /// Structured result produced by the target agent.
    pub result: serde_json::Value,
    /// Total tokens consumed by the delegated work — counted against the
    /// caller's budget by the runtime registry.
    pub tokens_used: u64,
}

// ── ask-agent ─────────────────────────────────────────────────────────────────

/// Input for [`AgentToolKind::AskAgent`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AskAgentInput {
    /// Free-text question for the target agent.
    pub question: String,
}

/// Output for [`AgentToolKind::AskAgent`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AskAgentOutput {
    /// Free-text answer from the target agent.
    pub answer: String,
    /// Total tokens consumed by the answer — counted against the caller's
    /// budget by the runtime registry.
    pub tokens_used: u64,
}

// ── background-task ───────────────────────────────────────────────────────────

/// Input for [`AgentToolKind::BackgroundTask`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BackgroundTaskInput {
    /// Free-text task description handed to the background worker.
    pub task: String,
}

/// Output for [`AgentToolKind::BackgroundTask`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BackgroundTaskOutput {
    /// Identifier the caller can use to poll task status later.
    pub task_id: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn agent_tool_serde_roundtrip() {
        let tool = AgentTool {
            kind: AgentToolKind::DelegateTask,
            target_agent: "researcher".into(),
        };
        let json = serde_json::to_string(&tool).unwrap();
        let back: AgentTool = serde_json::from_str(&json).unwrap();
        assert_eq!(tool, back);
        assert!(json.contains("delegate-task"));
    }

    #[test]
    fn delegate_task_io_roundtrip() {
        let input = DelegateTaskInput {
            task: "summarise the report".into(),
            context: Some(json!({"doc_id": 42})),
        };
        let s = serde_json::to_string(&input).unwrap();
        assert_eq!(input, serde_json::from_str(&s).unwrap());

        let output = DelegateTaskOutput {
            result: json!({"summary": "ok"}),
            tokens_used: 1234,
        };
        let s = serde_json::to_string(&output).unwrap();
        assert_eq!(output, serde_json::from_str(&s).unwrap());
    }

    #[test]
    fn delegate_task_input_omits_missing_context() {
        let input = DelegateTaskInput {
            task: "hi".into(),
            context: None,
        };
        let s = serde_json::to_string(&input).unwrap();
        assert!(!s.contains("context"));
        let back: DelegateTaskInput = serde_json::from_str(&s).unwrap();
        assert!(back.context.is_none());
    }

    #[test]
    fn ask_agent_io_roundtrip() {
        let input = AskAgentInput {
            question: "what is 2+2?".into(),
        };
        let s = serde_json::to_string(&input).unwrap();
        assert_eq!(input, serde_json::from_str(&s).unwrap());

        let output = AskAgentOutput {
            answer: "4".into(),
            tokens_used: 7,
        };
        let s = serde_json::to_string(&output).unwrap();
        assert_eq!(output, serde_json::from_str(&s).unwrap());
    }

    #[test]
    fn background_task_io_roundtrip() {
        let input = BackgroundTaskInput {
            task: "rebuild the index".into(),
        };
        let s = serde_json::to_string(&input).unwrap();
        assert_eq!(input, serde_json::from_str(&s).unwrap());

        let output = BackgroundTaskOutput {
            task_id: "bg-abc-123".into(),
        };
        let s = serde_json::to_string(&output).unwrap();
        assert_eq!(output, serde_json::from_str(&s).unwrap());
    }

    #[test]
    fn agent_tool_kind_serde_kebab() {
        let s = serde_json::to_string(&AgentToolKind::AskAgent).unwrap();
        assert_eq!(s, "\"ask-agent\"");
        let back: AgentToolKind = serde_json::from_str("\"background-task\"").unwrap();
        assert_eq!(back, AgentToolKind::BackgroundTask);
    }
}
