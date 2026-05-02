//! HITL P1-INTEGRATION wiring (sera-k600).
//!
//! sera-hitl carries three "TODO P1-INTEGRATION" hooks that need a real
//! production caller in the runtime. This module is that caller surface:
//!
//! * `analyze_tool_calls` — invoke a [`sera_hitl::SecurityAnalyzer`] for each
//!   tool call the model proposed, returning the list of [`sera_types::envelope::EventMsg::GuardianAssessment`]
//!   events the runtime should fan out before the HITL approval gate runs.
//! * `rejection_feedback_message` — translate a `ToolResult::Rejected { feedback }`
//!   coming back from a HITL-rejected ticket into the synthetic `tool` role
//!   message the runtime injects on the next turn so the LLM can self-correct
//!   in-loop (the opencode `CorrectedError` pattern, SPEC-hitl-approval §5b).
//!
//! Both helpers are deliberately free-standing: the runtime's `turn::act`
//! loop is the heaviest test surface in the crate and we keep it unchanged.
//! Callers (`default_runtime`, gateway adapters) opt in by threading these
//! helpers around their existing dispatch site.

use sera_hitl::{
    ActionSecurityRisk, AnalyzerError, ProposedAction, SecurityAnalyzer, ToolResult,
};
use sera_types::envelope::EventMsg;
use sera_types::tool::RiskLevel;

/// Build a [`ProposedAction`] for `tool_call`. Returns `None` when the call
/// shape is not the standard `{"function": {"name": ...}, ...}` envelope —
/// in that case there is nothing meaningful to score.
fn proposed_action_from_tool_call(
    tool_call: &serde_json::Value,
    risk_level: RiskLevel,
) -> Option<ProposedAction> {
    let tool_name = tool_call
        .get("function")
        .and_then(|f| f.get("name"))
        .and_then(|n| n.as_str())?
        .to_owned();
    let tool_args = tool_call
        .get("function")
        .and_then(|f| f.get("arguments"))
        .cloned();
    Some(ProposedAction {
        scope: sera_hitl::ApprovalScope::ToolCall {
            tool_name,
            risk_level,
        },
        tool_args,
        extra: None,
    })
}

/// Map an [`ActionSecurityRisk`] to the `GuardianAssessment` JSON payload the
/// runtime emits. The mapping is intentionally direct: Low → low,
/// Medium → medium, High → high, with `recommended_action` derived from the
/// same scale (Low → auto_approve, Medium → surface_to_user, High → block).
///
/// Kept as a `serde_json::Value` so this crate does not need a public
/// dependency on `sera_hitl::GuardianAssessment` from `sera-types`.
fn assessment_payload(name: &str, risk: ActionSecurityRisk) -> serde_json::Value {
    let (risk_level, recommended_action) = match risk {
        ActionSecurityRisk::Low => ("low", "auto_approve"),
        ActionSecurityRisk::Medium => ("medium", "surface_to_user"),
        ActionSecurityRisk::High => ("high", "block"),
    };
    serde_json::json!({
        "risk_level": risk_level,
        "rationale": format!("{name} returned {risk_level}"),
        "recommended_action": recommended_action,
    })
}

/// Score every tool call in `tool_calls` with `analyzer` and return one
/// [`EventMsg::GuardianAssessment`] per scored call.
///
/// Analyzer failures are mapped to `EventMsg::Error { code:
/// "guardian_analyzer_failed", … }` so the gateway has a structured signal
/// rather than a silently dropped event. Tool calls that don't match the
/// standard envelope shape are skipped (no assessment emitted).
///
/// Caller is responsible for fanning the returned events out to whatever
/// transport carries `EventMsg` to the gateway. This helper does not own a
/// channel — that keeps the seam testable without spinning up runtime state.
pub async fn analyze_tool_calls(
    analyzer: &dyn SecurityAnalyzer,
    tool_calls: &[serde_json::Value],
    risk_level: RiskLevel,
) -> Vec<EventMsg> {
    let mut events = Vec::with_capacity(tool_calls.len());
    let analyzer_name = analyzer.name().to_string();
    for tc in tool_calls {
        let action = match proposed_action_from_tool_call(tc, risk_level) {
            Some(a) => a,
            None => continue,
        };
        match analyzer.security_risk(&action).await {
            Ok(risk) => events.push(EventMsg::GuardianAssessment {
                analyzer: analyzer_name.clone(),
                assessment: assessment_payload(&analyzer_name, risk),
            }),
            Err(e) => events.push(EventMsg::Error {
                code: "guardian_analyzer_failed".to_string(),
                message: format!("{analyzer_name}: {}", display_analyzer_error(&e)),
            }),
        }
    }
    events
}

fn display_analyzer_error(e: &AnalyzerError) -> String {
    match e {
        AnalyzerError::Backend(msg) => format!("backend: {msg}"),
        AnalyzerError::Timeout => "timeout".to_string(),
        AnalyzerError::InvalidInput(msg) => format!("invalid_input: {msg}"),
    }
}

/// Translate a [`ToolResult::Rejected`] into the synthetic `tool` role
/// message the runtime injects for the LLM's next think pass.
///
/// `tool_call_id` is the id of the original `tool_calls[*]` entry the model
/// emitted — the model expects the corresponding tool result on the next
/// turn or it stalls. `feedback` is the human-supplied rejection reason and
/// becomes the message body so the model can revise its plan in-loop. When
/// the result is not `Rejected` (Ok / Err), `None` is returned so callers
/// can keep using their normal tool-result construction path.
pub fn rejection_feedback_message(
    tool_call_id: &str,
    result: &ToolResult,
) -> Option<serde_json::Value> {
    let feedback = match result {
        ToolResult::Rejected { feedback } => feedback,
        _ => return None,
    };
    Some(serde_json::json!({
        "role": "tool",
        "tool_call_id": tool_call_id,
        "content": format!("[hitl rejected: {feedback}]"),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;

    struct FixedAnalyzer {
        name: &'static str,
        risk: ActionSecurityRisk,
    }

    #[async_trait]
    impl SecurityAnalyzer for FixedAnalyzer {
        async fn security_risk(
            &self,
            _action: &ProposedAction,
        ) -> Result<ActionSecurityRisk, AnalyzerError> {
            Ok(self.risk)
        }
        fn name(&self) -> &str {
            self.name
        }
    }

    struct FailingAnalyzer;

    #[async_trait]
    impl SecurityAnalyzer for FailingAnalyzer {
        async fn security_risk(
            &self,
            _action: &ProposedAction,
        ) -> Result<ActionSecurityRisk, AnalyzerError> {
            Err(AnalyzerError::Backend("upstream 500".to_string()))
        }
        fn name(&self) -> &str {
            "failing"
        }
    }

    fn shell_call() -> serde_json::Value {
        serde_json::json!({
            "id": "call-1",
            "function": {
                "name": "shell",
                "arguments": "{\"cmd\":\"ls\"}",
            },
        })
    }

    #[tokio::test]
    async fn analyze_emits_one_event_per_call() {
        let analyzer = FixedAnalyzer {
            name: "heuristic",
            risk: ActionSecurityRisk::High,
        };
        let calls = vec![shell_call(), shell_call()];
        let events = analyze_tool_calls(&analyzer, &calls, RiskLevel::Execute).await;
        assert_eq!(events.len(), 2);
        for event in &events {
            match event {
                EventMsg::GuardianAssessment { analyzer, assessment } => {
                    assert_eq!(analyzer, "heuristic");
                    assert_eq!(assessment["risk_level"], "high");
                    assert_eq!(assessment["recommended_action"], "block");
                }
                other => panic!("expected GuardianAssessment, got {other:?}"),
            }
        }
    }

    #[tokio::test]
    async fn analyze_skips_malformed_tool_calls() {
        let analyzer = FixedAnalyzer {
            name: "heuristic",
            risk: ActionSecurityRisk::Low,
        };
        let calls = vec![serde_json::json!({"not_a_function": true})];
        let events = analyze_tool_calls(&analyzer, &calls, RiskLevel::Execute).await;
        assert!(events.is_empty());
    }

    #[tokio::test]
    async fn analyze_emits_error_event_on_backend_failure() {
        let analyzer = FailingAnalyzer;
        let events = analyze_tool_calls(&analyzer, &[shell_call()], RiskLevel::Execute).await;
        assert_eq!(events.len(), 1);
        match &events[0] {
            EventMsg::Error { code, message } => {
                assert_eq!(code, "guardian_analyzer_failed");
                assert!(message.contains("failing"));
                assert!(message.contains("upstream 500"));
            }
            other => panic!("expected Error, got {other:?}"),
        }
    }

    #[test]
    fn rejection_feedback_translates_rejected_result() {
        let result = ToolResult::Rejected {
            feedback: "use git status instead".to_string(),
        };
        let msg = rejection_feedback_message("call-7", &result).unwrap();
        assert_eq!(msg["role"], "tool");
        assert_eq!(msg["tool_call_id"], "call-7");
        assert!(
            msg["content"]
                .as_str()
                .unwrap()
                .contains("use git status instead")
        );
    }

    #[test]
    fn rejection_feedback_returns_none_for_ok_or_err_results() {
        let ok = ToolResult::Ok {
            output: serde_json::json!({}),
        };
        assert!(rejection_feedback_message("call-1", &ok).is_none());
        let err = ToolResult::Err {
            error: "boom".to_string(),
        };
        assert!(rejection_feedback_message("call-2", &err).is_none());
    }
}
