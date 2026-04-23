//! Self-evolution machinery for SERA.
//!
//! This crate provides the core infrastructure for SERA's self-evolution system,
//! including:
//!
//! - **ConstitutionalRule registry**: Tracks and enforces the rules that govern
//!   what changes are permissible at each evolution tier.
//! - **ShadowSession**: Parallel evaluation sessions used to validate proposed changes
//!   before they are applied to production state.
//! - **EvolutionPolicy**: Tiered policies (Tier 1/2/3) that control how changes
//!   are proposed, evaluated, approved, and applied.
//! - **ChangeArtifact**: Content-addressed change records with blast-radius classification.
//!
//! ## Evolution Tiers
//!
//! - **Tier 1 (AgentImprovement)**: Low blast-radius changes to agent memory,
//!   persona, skills, and experience pool.
//! - **Tier 2 (ConfigEvolution)**: Medium blast-radius changes to single-component
//!   configuration, hook chains, tool policies, and connector configs.
//! - **Tier 3 (CodeEvolution)**: High blast-radius changes to runtime crates,
//!   gateway core, protocol schemas, and constitutional rule sets.

pub mod approval_matrix;
pub mod sera_errors;
pub mod artifact_pipeline;
pub mod constitutional;
pub mod interaction_scoring;
pub mod policy;
pub mod prompt_refinement;
pub mod prompt_versioning;
pub mod shadow_session;
pub mod validation;

pub use approval_matrix::ApprovalRequirements;
pub use interaction_scoring::{
    DimensionScore, InMemoryInteractionScorer, InteractionScore, InteractionScorer,
    ScoringDimension, ScoringError, ScoringMode, ScoringRequest,
};
pub use prompt_refinement::{
    InMemoryRefinementAnalyzer, PromptChange, RefinementAnalyzer, RefinementConfig,
    RefinementError, RefinementResult, ScoredTrace,
};
pub use prompt_versioning::{
    ActivationMode, InMemoryPromptVersionStore, PromptSection, PromptVersion,
    PromptVersionError, PromptVersionStore, MAX_SECTION_LENGTH,
};
pub use validation::{
    DriftAlert, ValidationConfig, ValidationError, ValidationManager, ValidationOutcome,
    ValidationWindow,
};

// Re-export key types for ergonomics. CapabilityToken and ChangeProposer
// live in sera-auth (the canonical home for capability-related types); the
// rest remain in sera-types.
pub use sera_auth::{CapabilityToken, ChangeProposer};
pub use sera_types::{AgentCapability};
pub use sera_types::evolution::{
    BlastRadius, ChangeArtifactId, ConstitutionalEnforcementPoint, ConstitutionalRule,
    EvolutionTier,
};

/// Lifecycle status of a change artifact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeArtifactStatus {
    /// Proposed but not yet evaluated.
    Proposed,
    /// Being evaluated in a shadow session.
    Evaluating,
    /// Evaluation passed; awaiting approval.
    Approved,
    /// Evaluation failed; change rejected.
    Rejected,
    /// Applied to production state.
    Applied,
    /// Rolled back after application.
    RolledBack,
}

impl serde::Serialize for ChangeArtifactStatus {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let name = match self {
            ChangeArtifactStatus::Proposed => "proposed",
            ChangeArtifactStatus::Evaluating => "evaluating",
            ChangeArtifactStatus::Approved => "approved",
            ChangeArtifactStatus::Rejected => "rejected",
            ChangeArtifactStatus::Applied => "applied",
            ChangeArtifactStatus::RolledBack => "rolled_back",
        };
        s.serialize_str(name)
    }
}

impl<'de> serde::Deserialize<'de> for ChangeArtifactStatus {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        match s.as_str() {
            "proposed" => Ok(ChangeArtifactStatus::Proposed),
            "evaluating" => Ok(ChangeArtifactStatus::Evaluating),
            "approved" => Ok(ChangeArtifactStatus::Approved),
            "rejected" => Ok(ChangeArtifactStatus::Rejected),
            "applied" => Ok(ChangeArtifactStatus::Applied),
            "rolled_back" => Ok(ChangeArtifactStatus::RolledBack),
            _ => Err(serde::de::Error::custom(format!("unknown variant: {s}"))),
        }
    }
}

/// The scope of a change artifact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeArtifactScope {
    /// Tier 1 — agent-level changes.
    AgentImprovement,
    /// Tier 2 — configuration-level changes.
    ConfigEvolution,
    /// Tier 3 — code-level changes.
    CodeEvolution,
}

impl serde::Serialize for ChangeArtifactScope {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let name = match self {
            ChangeArtifactScope::AgentImprovement => "agent_improvement",
            ChangeArtifactScope::ConfigEvolution => "config_evolution",
            ChangeArtifactScope::CodeEvolution => "code_evolution",
        };
        s.serialize_str(name)
    }
}

impl<'de> serde::Deserialize<'de> for ChangeArtifactScope {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        match s.as_str() {
            "agent_improvement" => Ok(ChangeArtifactScope::AgentImprovement),
            "config_evolution" => Ok(ChangeArtifactScope::ConfigEvolution),
            "code_evolution" => Ok(ChangeArtifactScope::CodeEvolution),
            _ => Err(serde::de::Error::custom(format!("unknown variant: {s}"))),
        }
    }
}

impl ChangeArtifactScope {
    /// Convert to the corresponding EvolutionTier.
    pub fn to_tier(&self) -> EvolutionTier {
        match self {
            ChangeArtifactScope::AgentImprovement => EvolutionTier::AgentImprovement,
            ChangeArtifactScope::ConfigEvolution => EvolutionTier::ConfigEvolution,
            ChangeArtifactScope::CodeEvolution => EvolutionTier::CodeEvolution,
        }
    }
}

/// A record of a proposed change to the system.
#[derive(Debug, Clone)]
pub struct ChangeArtifact {
    pub id: ChangeArtifactId,
    /// Human-readable description of the change.
    pub description: String,
    /// Scope of the change.
    pub scope: ChangeArtifactScope,
    /// Estimated blast radius.
    pub blast_radius: BlastRadius,
    /// Who/what proposed this change.
    pub proposer: ChangeProposer,
    /// Current status.
    pub status: ChangeArtifactStatus,
    /// JSON payload describing the change in detail.
    pub payload: serde_json::Value,
    /// When the artifact was created.
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// When the artifact was last updated.
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl ChangeArtifact {
    /// Create a new `ChangeArtifact` with `Proposed` status.
    pub fn new(
        description: String,
        scope: ChangeArtifactScope,
        blast_radius: BlastRadius,
        proposer: ChangeProposer,
        payload: serde_json::Value,
    ) -> Self {
        let now = chrono::Utc::now();
        let id = Self::compute_id(&scope, &blast_radius, &proposer.principal_id, &payload, now);
        Self {
            id,
            description,
            scope,
            blast_radius,
            proposer,
            status: ChangeArtifactStatus::Proposed,
            payload,
            created_at: now,
            updated_at: now,
        }
    }

    /// Compute the content-addressed ID per SPEC-self-evolution §8.
    ///
    /// The ID is the SHA-256 of the canonical serialization of
    /// `(tier, scope, content, proposer.principal_id, created_at)`. This makes
    /// IDs collision-resistant across agents and prevents silent mutation.
    fn compute_id(
        scope: &ChangeArtifactScope,
        blast_radius: &BlastRadius,
        proposer_principal: &str,
        payload: &serde_json::Value,
        created_at: chrono::DateTime<chrono::Utc>,
    ) -> ChangeArtifactId {
        use sha2::{Digest, Sha256};
        let canonical = serde_json::json!({
            "tier": scope.to_tier(),
            "scope": scope,
            "blast_radius": blast_radius,
            "content": payload,
            "proposer_principal": proposer_principal,
            "created_at": created_at.to_rfc3339(),
        });
        let mut hasher = Sha256::new();
        hasher.update(canonical.to_string().as_bytes());
        let result = hasher.finalize();
        let mut hash = [0u8; 32];
        hash.copy_from_slice(&result[..32]);
        ChangeArtifactId { hash }
    }

    /// Transition to a new status. Returns true if the transition was valid.
    pub fn transition_to(&mut self, new_status: ChangeArtifactStatus) -> bool {
        let valid = matches!(
            (self.status, new_status),
            (ChangeArtifactStatus::Proposed, ChangeArtifactStatus::Evaluating)
                | (ChangeArtifactStatus::Proposed, ChangeArtifactStatus::Rejected)
                | (ChangeArtifactStatus::Evaluating, ChangeArtifactStatus::Approved)
                | (ChangeArtifactStatus::Evaluating, ChangeArtifactStatus::Rejected)
                | (ChangeArtifactStatus::Approved, ChangeArtifactStatus::Applied)
                | (ChangeArtifactStatus::Applied, ChangeArtifactStatus::RolledBack)
        );
        if valid {
            self.status = new_status;
            self.updated_at = chrono::Utc::now();
        }
        valid
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn change_artifact_status_transitions() {
        let proposer = ChangeProposer {
            principal_id: "user-1".to_string(),
            capability_token: CapabilityToken {
                id: "tok-1".to_string(),
                scopes: Default::default(),
                expires_at: chrono::Utc::now(),
                max_proposals: 10,
                signature: [0u8; 64],
            },
        };
        let mut artifact = ChangeArtifact::new(
            "test change".to_string(),
            ChangeArtifactScope::AgentImprovement,
            BlastRadius::AgentMemory,
            proposer,
            serde_json::json!({}),
        );

        assert_eq!(artifact.status, ChangeArtifactStatus::Proposed);
        assert!(artifact.transition_to(ChangeArtifactStatus::Evaluating));
        assert!(artifact.transition_to(ChangeArtifactStatus::Approved));
        assert!(artifact.transition_to(ChangeArtifactStatus::Applied));
        assert!(artifact.transition_to(ChangeArtifactStatus::RolledBack));
    }

    #[test]
    fn invalid_status_transition_returns_false() {
        let proposer = ChangeProposer {
            principal_id: "user-1".to_string(),
            capability_token: CapabilityToken {
                id: "tok-1".to_string(),
                scopes: Default::default(),
                expires_at: chrono::Utc::now(),
                max_proposals: 10,
                signature: [0u8; 64],
            },
        };
        let mut artifact = ChangeArtifact::new(
            "test".to_string(),
            ChangeArtifactScope::ConfigEvolution,
            BlastRadius::GlobalConfig,
            proposer,
            serde_json::json!({}),
        );

        // Can't go directly from Proposed to Applied
        assert!(!artifact.transition_to(ChangeArtifactStatus::Applied));
    }
}
