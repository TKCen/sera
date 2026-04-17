//! `From` impls bridging sera-meta error types into [`SeraError`].

use sera_errors::{SeraError, SeraErrorCode};

use crate::artifact_pipeline::{ArtifactStoreError, PipelineError};
use crate::interaction_scoring::ScoringError;
use crate::prompt_refinement::RefinementError;
use crate::prompt_versioning::PromptVersionError;
use crate::validation::ValidationError;

impl From<ArtifactStoreError> for SeraError {
    fn from(err: ArtifactStoreError) -> Self {
        let code = match &err {
            ArtifactStoreError::NotFound(_) => SeraErrorCode::NotFound,
            ArtifactStoreError::InvalidTransition(_) => SeraErrorCode::InvalidInput,
        };
        SeraError::with_source(code, err.to_string(), err)
    }
}

impl From<PipelineError> for SeraError {
    fn from(err: PipelineError) -> Self {
        let code = match &err {
            PipelineError::NotFound(_) => SeraErrorCode::NotFound,
            PipelineError::WrongState { .. } => SeraErrorCode::InvalidInput,
            PipelineError::DryRunFailed(_) => SeraErrorCode::PreconditionFailed,
            PipelineError::PolicyRejected(_) => SeraErrorCode::Forbidden,
            PipelineError::InsufficientApprovals { .. } => SeraErrorCode::PreconditionFailed,
            PipelineError::DuplicateApprover(_) => SeraErrorCode::InvalidInput,
            PipelineError::SelfApproval => SeraErrorCode::Forbidden,
            PipelineError::OperatorKeyMissing => SeraErrorCode::PreconditionFailed,
        };
        SeraError::with_source(code, err.to_string(), err)
    }
}

impl From<ValidationError> for SeraError {
    fn from(err: ValidationError) -> Self {
        let code = match &err {
            ValidationError::WindowNotFound(_) => SeraErrorCode::NotFound,
            ValidationError::WindowExpired => SeraErrorCode::PreconditionFailed,
            ValidationError::InvalidScore(_) => SeraErrorCode::InvalidInput,
        };
        SeraError::with_source(code, err.to_string(), err)
    }
}

impl From<ScoringError> for SeraError {
    fn from(err: ScoringError) -> Self {
        let code = match &err {
            ScoringError::ScoringFailed(_) => SeraErrorCode::Internal,
            ScoringError::InvalidScore { .. } => SeraErrorCode::InvalidInput,
            ScoringError::ParseError(_) => SeraErrorCode::Serialization,
        };
        SeraError::with_source(code, err.to_string(), err)
    }
}

impl From<PromptVersionError> for SeraError {
    fn from(err: PromptVersionError) -> Self {
        let code = match &err {
            PromptVersionError::VersionNotFound { .. } => SeraErrorCode::NotFound,
            PromptVersionError::ContentTooLong { .. } => SeraErrorCode::InvalidInput,
            PromptVersionError::RationaleRequired => SeraErrorCode::InvalidInput,
        };
        SeraError::with_source(code, err.to_string(), err)
    }
}

impl From<RefinementError> for SeraError {
    fn from(err: RefinementError) -> Self {
        let code = match &err {
            RefinementError::InsufficientData { .. } => SeraErrorCode::PreconditionFailed,
            RefinementError::AnalysisFailed(_) => SeraErrorCode::Internal,
            RefinementError::VersionError(_) => SeraErrorCode::NotFound,
        };
        SeraError::with_source(code, err.to_string(), err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ChangeArtifactId;
    use crate::ChangeArtifactStatus;

    #[test]
    fn artifact_store_not_found_maps_to_not_found() {
        let id = ChangeArtifactId { hash: [0u8; 32] };
        let e: SeraError = ArtifactStoreError::NotFound(id).into();
        assert_eq!(e.code, SeraErrorCode::NotFound);
    }

    #[test]
    fn artifact_store_invalid_transition_maps_to_invalid_input() {
        let e: SeraError =
            ArtifactStoreError::InvalidTransition(ChangeArtifactStatus::Applied).into();
        assert_eq!(e.code, SeraErrorCode::InvalidInput);
    }

    #[test]
    fn pipeline_policy_rejected_maps_to_forbidden() {
        let e: SeraError = PipelineError::PolicyRejected("tier-3 blocked".into()).into();
        assert_eq!(e.code, SeraErrorCode::Forbidden);
        assert!(e.message.contains("tier-3 blocked"));
    }

    #[test]
    fn pipeline_not_found_maps_to_not_found() {
        let e: SeraError = PipelineError::NotFound("art-abc".into()).into();
        assert_eq!(e.code, SeraErrorCode::NotFound);
    }

    #[test]
    fn pipeline_self_approval_maps_to_forbidden() {
        let e: SeraError = PipelineError::SelfApproval.into();
        assert_eq!(e.code, SeraErrorCode::Forbidden);
    }

    #[test]
    fn validation_window_not_found_maps_to_not_found() {
        let e: SeraError = ValidationError::WindowNotFound("win-1".into()).into();
        assert_eq!(e.code, SeraErrorCode::NotFound);
    }

    #[test]
    fn scoring_invalid_score_maps_to_invalid_input() {
        let e: SeraError = ScoringError::InvalidScore { value: 1.5 }.into();
        assert_eq!(e.code, SeraErrorCode::InvalidInput);
    }

    #[test]
    fn prompt_version_not_found_maps_to_not_found() {
        use crate::prompt_versioning::PromptSection;
        let e: SeraError = PromptVersionError::VersionNotFound {
            version: 99,
            agent_id: "agent-x".into(),
            section: PromptSection::Role,
        }
        .into();
        assert_eq!(e.code, SeraErrorCode::NotFound);
    }

    #[test]
    fn refinement_insufficient_data_maps_to_precondition_failed() {
        let e: SeraError = RefinementError::InsufficientData {
            needed: 10,
            have: 3,
        }
        .into();
        assert_eq!(e.code, SeraErrorCode::PreconditionFailed);
    }
}
