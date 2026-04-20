//! SecurityAnalyzer trait + ActionSecurityRisk.
//!
//! SPEC-hitl-approval §2a. Per-action risk classification runs before the
//! static approval matrix. Pluggable backends (Invariant, GraySwan, Heuristic)
//! return `ActionSecurityRisk` that feeds into routing decisions and the
//! `confirmation_mode` hold-pending pattern.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::types::ApprovalScope;

/// Coarse risk classification emitted by a [`SecurityAnalyzer`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionSecurityRisk {
    /// Bypass approval unless policy forces it.
    Low,
    /// Route through the standard approval chain.
    Medium,
    /// Escalate; require meta-quorum for change scopes.
    High,
}

/// A proposed action passed to a [`SecurityAnalyzer`] for risk scoring.
///
/// Intentionally small: analyzers can read the scope and optional tool args,
/// and add whatever context they need via `extra`. The wider runtime context
/// (principal, session) is kept elsewhere to avoid cycles with sera-runtime.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProposedAction {
    pub scope: ApprovalScope,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_args: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra: Option<serde_json::Value>,
}

/// Errors returned by a [`SecurityAnalyzer`] implementation.
#[derive(Debug, Error)]
pub enum AnalyzerError {
    #[error("analyzer backend error: {0}")]
    Backend(String),
    #[error("analyzer timed out")]
    Timeout,
    #[error("analyzer invalid input: {0}")]
    InvalidInput(String),
}

/// Pluggable risk analyzer called before the approval chain resolves.
///
/// SPEC-hitl-approval §2a. Reference backends live outside this crate
/// (`InvariantAnalyzer`, `GraySwanAnalyzer`, `HeuristicAnalyzer`).
// TODO P1-INTEGRATION: wire into sera-runtime turn pipeline + gateway
//                      confirmation_mode hold-pending state.
#[async_trait]
pub trait SecurityAnalyzer: Send + Sync {
    async fn security_risk(
        &self,
        action: &ProposedAction,
    ) -> Result<ActionSecurityRisk, AnalyzerError>;

    fn name(&self) -> &str;
}

#[cfg(test)]
mod tests {
    use super::*;
    use sera_types::tool::RiskLevel;

    struct FixedAnalyzer(ActionSecurityRisk);

    #[async_trait]
    impl SecurityAnalyzer for FixedAnalyzer {
        async fn security_risk(
            &self,
            _action: &ProposedAction,
        ) -> Result<ActionSecurityRisk, AnalyzerError> {
            Ok(self.0)
        }
        fn name(&self) -> &str {
            "fixed"
        }
    }

    fn action() -> ProposedAction {
        ProposedAction {
            scope: ApprovalScope::ToolCall {
                tool_name: "shell".to_string(),
                risk_level: RiskLevel::Execute,
            },
            tool_args: None,
            extra: None,
        }
    }

    #[tokio::test]
    async fn analyzer_returns_fixed_risk() {
        let a = FixedAnalyzer(ActionSecurityRisk::High);
        let r = a.security_risk(&action()).await.unwrap();
        assert_eq!(r, ActionSecurityRisk::High);
        assert_eq!(a.name(), "fixed");
    }

    #[test]
    fn action_security_risk_serde_roundtrip() {
        for (risk, expected) in [
            (ActionSecurityRisk::Low, "\"low\""),
            (ActionSecurityRisk::Medium, "\"medium\""),
            (ActionSecurityRisk::High, "\"high\""),
        ] {
            let json = serde_json::to_string(&risk).unwrap();
            assert_eq!(json, expected);
            let parsed: ActionSecurityRisk = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, risk);
        }
    }
}
