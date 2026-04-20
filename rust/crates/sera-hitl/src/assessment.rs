//! Guardian pre-approval LLM risk assessment.
//!
//! SPEC-hitl-approval §2b. An optional pre-gate that adds LLM-informed
//! context to `ApprovalEvidence` without replacing the downstream chain.

use serde::{Deserialize, Serialize};

/// Risk level returned by the Guardian LLM assessor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GuardianRiskLevel {
    Low,
    Medium,
    High,
}

/// What the Guardian recommends the runtime do with the proposed action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GuardianRecommendation {
    AutoApprove,
    SurfaceToUser,
    Block,
}

/// Structured LLM assessment attached to `ApprovalEvidence`.
///
/// Emitted as `EventMsg::GuardianAssessment` on the EQ channel so clients
/// can display the reasoning inline when the approval is surfaced.
// TODO P1-INTEGRATION: EventMsg::GuardianAssessment variant lives in sera-types
//                      envelope; wire emission from runtime turn pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianAssessment {
    pub risk_level: GuardianRiskLevel,
    pub rationale: String,
    pub recommended_action: GuardianRecommendation,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guardian_assessment_serde_roundtrip() {
        let ga = GuardianAssessment {
            risk_level: GuardianRiskLevel::High,
            rationale: "destructive filesystem operation".to_string(),
            recommended_action: GuardianRecommendation::SurfaceToUser,
        };
        let json = serde_json::to_string(&ga).unwrap();
        let parsed: GuardianAssessment = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.risk_level, GuardianRiskLevel::High);
        assert_eq!(parsed.rationale, ga.rationale);
        assert_eq!(
            parsed.recommended_action,
            GuardianRecommendation::SurfaceToUser
        );
    }

    #[test]
    fn guardian_risk_level_snake_case() {
        assert_eq!(
            serde_json::to_string(&GuardianRiskLevel::Medium).unwrap(),
            "\"medium\""
        );
        assert_eq!(
            serde_json::to_string(&GuardianRecommendation::AutoApprove).unwrap(),
            "\"auto_approve\""
        );
    }
}
