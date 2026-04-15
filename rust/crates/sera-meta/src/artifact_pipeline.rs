//! Change-artifact pipeline — in-memory store plus `propose → evaluate →
//! approve → apply` lifecycle from SPEC-self-evolution §16.
//!
//! Two abstractions live here:
//!
//! - [`ChangeArtifactStore`] is a concurrency-safe keyed store over artifacts
//!   with pure state-machine transitions.
//! - [`ArtifactPipeline`] wraps the store, the policy engine, and the shadow
//!   session registry to gate each transition against the blast-radius
//!   approval matrix (see [`crate::approval_matrix`]).
//!
//! Tier-3 canary and two-generation-live orchestration is explicitly out of
//! scope for this module — those land in `sera-deployment` later.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use thiserror::Error;
use tokio::sync::RwLock;

use crate::approval_matrix::ApprovalRequirements;
use crate::policy::PolicyEngine;
use crate::shadow_session::{ShadowSessionHandle, ShadowSessionRegistry};
use crate::{ChangeArtifact, ChangeArtifactId, ChangeArtifactStatus};

/// Errors from `ChangeArtifactStore` operations.
#[derive(Debug, Error)]
pub enum ArtifactStoreError {
    #[error("artifact {0:?} not found")]
    NotFound(ChangeArtifactId),
    #[error("invalid status transition to {0:?}")]
    InvalidTransition(ChangeArtifactStatus),
}

/// In-memory, concurrency-safe store for `ChangeArtifact` records.
pub struct ChangeArtifactStore {
    inner: RwLock<HashMap<[u8; 32], ChangeArtifact>>,
}

impl ChangeArtifactStore {
    /// Create an empty store.
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(HashMap::new()),
        }
    }

    /// Submit a new artifact; returns its `ChangeArtifactId`.
    pub async fn submit(&self, artifact: ChangeArtifact) -> ChangeArtifactId {
        let id = artifact.id.clone();
        self.inner.write().await.insert(id.hash, artifact);
        id
    }

    /// Retrieve an artifact by id.
    pub async fn get(&self, id: &ChangeArtifactId) -> Option<ChangeArtifact> {
        self.inner.read().await.get(&id.hash).cloned()
    }

    /// List all artifacts with a given status.
    pub async fn list_by_status(&self, status: ChangeArtifactStatus) -> Vec<ChangeArtifact> {
        self.inner
            .read()
            .await
            .values()
            .filter(|a| a.status == status)
            .cloned()
            .collect()
    }

    /// Transition an artifact to a new status.
    ///
    /// Returns `Err(NotFound)` if the id is unknown, or
    /// `Err(InvalidTransition)` if the state machine rejects the transition.
    pub async fn transition(
        &self,
        id: &ChangeArtifactId,
        new_status: ChangeArtifactStatus,
    ) -> Result<(), ArtifactStoreError> {
        let mut guard = self.inner.write().await;
        let artifact = guard
            .get_mut(&id.hash)
            .ok_or_else(|| ArtifactStoreError::NotFound(id.clone()))?;
        if artifact.transition_to(new_status) {
            Ok(())
        } else {
            Err(ArtifactStoreError::InvalidTransition(new_status))
        }
    }
}

impl Default for ChangeArtifactStore {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Pipeline — propose → evaluate → approve → apply
// ---------------------------------------------------------------------------

/// Error type for `ArtifactPipeline` operations.
#[derive(Debug, Error)]
pub enum PipelineError {
    #[error("artifact {0} not found")]
    NotFound(String),
    #[error("artifact is in state {actual:?}; expected {expected:?}")]
    WrongState {
        actual: ChangeArtifactStatus,
        expected: ChangeArtifactStatus,
    },
    #[error("shadow-session dry-run failed: {0}")]
    DryRunFailed(String),
    #[error("policy rejection: {0}")]
    PolicyRejected(String),
    #[error("insufficient approvals: have {have}, need {need}")]
    InsufficientApprovals { have: u8, need: u8 },
    #[error("approver {0} already signed this artifact")]
    DuplicateApprover(String),
    #[error("proposer cannot self-approve their own artifact")]
    SelfApproval,
    #[error("operator offline key required but not supplied")]
    OperatorKeyMissing,
}

/// Outcome of the shadow-session dry-run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DryRunOutcome {
    Passed,
    Failed(String),
}

/// An approver's signature on a change artifact.
#[derive(Debug, Clone)]
pub struct ApprovalSignature {
    pub approver_principal: String,
    pub signed_at: chrono::DateTime<chrono::Utc>,
}

/// Internal tracking record: the artifact plus its accumulated approvals and
/// (optional) shadow-session handle.
#[derive(Debug, Clone)]
struct TrackedArtifact {
    artifact: ChangeArtifact,
    approvals: Vec<ApprovalSignature>,
    shadow: Option<ShadowSessionHandle>,
    operator_key_supplied: bool,
}

/// The change-artifact pipeline.
///
/// Holds in-flight artifacts, the shadow-session registry, and a reference to
/// the policy engine. Designed to be wrapped in `Arc` and shared across async
/// tasks.
#[derive(Debug)]
pub struct ArtifactPipeline {
    artifacts: Arc<RwLock<HashMap<String, TrackedArtifact>>>,
    shadow_registry: ShadowSessionRegistry,
    policy_engine: PolicyEngine,
}

impl ArtifactPipeline {
    /// Build a new pipeline with the provided shadow registry and policy engine.
    pub fn new(shadow_registry: ShadowSessionRegistry, policy_engine: PolicyEngine) -> Self {
        Self {
            artifacts: Arc::new(RwLock::new(HashMap::new())),
            shadow_registry,
            policy_engine,
        }
    }

    /// Build a pipeline with empty registry and default policy engine.
    pub fn with_defaults() -> Self {
        Self::new(ShadowSessionRegistry::new(), PolicyEngine::new())
    }

    /// Stage 1: propose an artifact.
    ///
    /// Evaluates the artifact against the policy engine. If the engine rejects
    /// (e.g. missing scopes on the proposer's token) the artifact is not
    /// tracked and `PipelineError::PolicyRejected` is returned.
    pub async fn propose(&self, artifact: ChangeArtifact) -> Result<ChangeArtifactId, PipelineError> {
        let eval = self
            .policy_engine
            .evaluate(&artifact)
            .await
            .map_err(|e| PipelineError::PolicyRejected(e.to_string()))?;

        if !eval.approved {
            return Err(PipelineError::PolicyRejected(eval.summary));
        }

        let id = artifact.id.clone();
        let key = id.to_string();

        let shadow = if ApprovalRequirements::for_blast_radius(artifact.blast_radius)
            .requires_shadow_replay
        {
            Some(self.shadow_registry.create(artifact.clone()).await)
        } else {
            None
        };

        self.artifacts.write().await.insert(
            key,
            TrackedArtifact {
                artifact,
                approvals: Vec::new(),
                shadow,
                operator_key_supplied: false,
            },
        );
        Ok(id)
    }

    /// Stage 2: evaluate — run the supplied dry-run closure against the
    /// artifact and transition it to `Approved` (pending signatures) or
    /// `Rejected`.
    ///
    /// For scopes whose matrix row does not require a shadow replay (Tier 1),
    /// the dry-run closure is skipped entirely.
    pub async fn evaluate<F>(
        &self,
        id: &ChangeArtifactId,
        dry_run: F,
    ) -> Result<DryRunOutcome, PipelineError>
    where
        F: FnOnce(&ChangeArtifact) -> DryRunOutcome,
    {
        let key = id.to_string();
        let mut guard = self.artifacts.write().await;
        let tracked = guard
            .get_mut(&key)
            .ok_or_else(|| PipelineError::NotFound(key.clone()))?;

        if tracked.artifact.status != ChangeArtifactStatus::Proposed {
            return Err(PipelineError::WrongState {
                actual: tracked.artifact.status,
                expected: ChangeArtifactStatus::Proposed,
            });
        }

        let requirements = ApprovalRequirements::for_blast_radius(tracked.artifact.blast_radius);
        tracked.artifact.transition_to(ChangeArtifactStatus::Evaluating);

        let outcome = if requirements.requires_shadow_replay {
            if let Some(shadow) = &tracked.shadow {
                shadow.start().await;
            }
            let outcome = dry_run(&tracked.artifact);
            if let Some(shadow) = &tracked.shadow {
                match &outcome {
                    DryRunOutcome::Passed => {
                        shadow
                            .checkpoint("dry-run", true, "replay passed", serde_json::json!({}))
                            .await;
                        shadow.pass().await;
                    }
                    DryRunOutcome::Failed(reason) => {
                        shadow
                            .checkpoint("dry-run", false, reason.clone(), serde_json::json!({}))
                            .await;
                        shadow.fail().await;
                    }
                }
            }
            outcome
        } else {
            DryRunOutcome::Passed
        };

        match &outcome {
            DryRunOutcome::Passed => {
                tracked.artifact.transition_to(ChangeArtifactStatus::Approved);
            }
            DryRunOutcome::Failed(_reason) => {
                tracked.artifact.transition_to(ChangeArtifactStatus::Rejected);
            }
        }
        Ok(outcome)
    }

    /// Stage 3a: record a `MetaApprover` signature.
    ///
    /// Enforces SPEC-self-evolution §7: no self-approval and deduplicated
    /// signers. Returns the new signature count.
    pub async fn approve(
        &self,
        id: &ChangeArtifactId,
        approver_principal: impl Into<String>,
    ) -> Result<u8, PipelineError> {
        let approver = approver_principal.into();
        let key = id.to_string();
        let mut guard = self.artifacts.write().await;
        let tracked = guard
            .get_mut(&key)
            .ok_or_else(|| PipelineError::NotFound(key.clone()))?;

        if tracked.artifact.status != ChangeArtifactStatus::Approved {
            return Err(PipelineError::WrongState {
                actual: tracked.artifact.status,
                expected: ChangeArtifactStatus::Approved,
            });
        }

        if approver == tracked.artifact.proposer.principal_id {
            return Err(PipelineError::SelfApproval);
        }

        if tracked
            .approvals
            .iter()
            .any(|s| s.approver_principal == approver)
        {
            return Err(PipelineError::DuplicateApprover(approver));
        }

        tracked.approvals.push(ApprovalSignature {
            approver_principal: approver,
            signed_at: chrono::Utc::now(),
        });
        Ok(tracked.approvals.len() as u8)
    }

    /// Stage 3b: mark the operator offline key as supplied.
    ///
    /// The four Tier-3 meta-change scopes require this before `apply` will
    /// run. Cryptographic verification lives in `sera-auth`.
    pub async fn supply_operator_key(&self, id: &ChangeArtifactId) -> Result<(), PipelineError> {
        let key = id.to_string();
        let mut guard = self.artifacts.write().await;
        let tracked = guard
            .get_mut(&key)
            .ok_or_else(|| PipelineError::NotFound(key.clone()))?;
        tracked.operator_key_supplied = true;
        Ok(())
    }

    /// Stage 4: apply the change.
    ///
    /// Verifies all approval-matrix requirements before transitioning the
    /// artifact to `Applied`.
    pub async fn apply(&self, id: &ChangeArtifactId) -> Result<ChangeArtifact, PipelineError> {
        let key = id.to_string();
        let mut guard = self.artifacts.write().await;
        let tracked = guard
            .get_mut(&key)
            .ok_or_else(|| PipelineError::NotFound(key.clone()))?;

        if tracked.artifact.status != ChangeArtifactStatus::Approved {
            return Err(PipelineError::WrongState {
                actual: tracked.artifact.status,
                expected: ChangeArtifactStatus::Approved,
            });
        }

        let requirements = ApprovalRequirements::for_blast_radius(tracked.artifact.blast_radius);
        let have = tracked.approvals.len() as u8;
        if have < requirements.approvers_required {
            return Err(PipelineError::InsufficientApprovals {
                have,
                need: requirements.approvers_required,
            });
        }

        if requirements.requires_operator_offline_key && !tracked.operator_key_supplied {
            return Err(PipelineError::OperatorKeyMissing);
        }

        tracked.artifact.transition_to(ChangeArtifactStatus::Applied);
        Ok(tracked.artifact.clone())
    }

    /// Roll back an already-`Applied` artifact.
    pub async fn rollback(&self, id: &ChangeArtifactId) -> Result<(), PipelineError> {
        let key = id.to_string();
        let mut guard = self.artifacts.write().await;
        let tracked = guard
            .get_mut(&key)
            .ok_or_else(|| PipelineError::NotFound(key.clone()))?;
        if tracked.artifact.status != ChangeArtifactStatus::Applied {
            return Err(PipelineError::WrongState {
                actual: tracked.artifact.status,
                expected: ChangeArtifactStatus::Applied,
            });
        }
        tracked.artifact.transition_to(ChangeArtifactStatus::RolledBack);
        Ok(())
    }

    /// Fetch a snapshot of the tracked artifact.
    pub async fn get(&self, id: &ChangeArtifactId) -> Option<ChangeArtifact> {
        self.artifacts
            .read()
            .await
            .get(&id.to_string())
            .map(|t| t.artifact.clone())
    }

    /// Principals who have signed the artifact.
    pub async fn signers(&self, id: &ChangeArtifactId) -> HashSet<String> {
        self.artifacts
            .read()
            .await
            .get(&id.to_string())
            .map(|t| t.approvals.iter().map(|a| a.approver_principal.clone()).collect())
            .unwrap_or_default()
    }
}

impl Default for ArtifactPipeline {
    fn default() -> Self {
        Self::with_defaults()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BlastRadius, ChangeArtifactScope, ChangeProposer};
    use sera_types::evolution::CapabilityToken;

    fn make_proposer(id: &str, scopes: Vec<BlastRadius>) -> ChangeProposer {
        ChangeProposer {
            principal_id: id.to_string(),
            capability_token: CapabilityToken {
                id: format!("tok-{id}"),
                scopes: scopes.into_iter().collect(),
                expires_at: chrono::Utc::now(),
                max_proposals: 10,
                signature: [0u8; 64],
            },
        }
    }

    fn make_artifact(description: &str) -> ChangeArtifact {
        ChangeArtifact::new(
            description.to_string(),
            ChangeArtifactScope::AgentImprovement,
            BlastRadius::AgentMemory,
            make_proposer("test-user", vec![BlastRadius::AgentMemory]),
            serde_json::json!({ "key": description }),
        )
    }

    fn make_tier2_artifact(proposer_id: &str) -> ChangeArtifact {
        ChangeArtifact::new(
            "tier 2 change".to_string(),
            ChangeArtifactScope::ConfigEvolution,
            BlastRadius::SingleHookConfig,
            make_proposer(proposer_id, vec![BlastRadius::SingleHookConfig]),
            serde_json::json!({ "hook": "on_turn_start" }),
        )
    }

    // ---- ChangeArtifactStore ---------------------------------------------

    #[tokio::test]
    async fn submit_and_get_roundtrip() {
        let store = ChangeArtifactStore::new();
        let artifact = make_artifact("roundtrip test");
        let id = store.submit(artifact.clone()).await;
        let retrieved = store.get(&id).await.expect("artifact must be present");
        assert_eq!(retrieved.description, "roundtrip test");
        assert_eq!(retrieved.status, ChangeArtifactStatus::Proposed);
    }

    #[tokio::test]
    async fn list_by_status_filters_correctly() {
        let store = ChangeArtifactStore::new();
        let a1 = make_artifact("a1");
        // sleep a microsecond so a2's created_at differs — the content-addressed
        // ID includes created_at, so this guarantees distinct keys even when
        // descriptions collide at sub-millisecond resolution.
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        let a2 = make_artifact("a2");
        let id1 = store.submit(a1).await;
        store.submit(a2).await;

        store
            .transition(&id1, ChangeArtifactStatus::Evaluating)
            .await
            .unwrap();

        let proposed = store.list_by_status(ChangeArtifactStatus::Proposed).await;
        let evaluating = store.list_by_status(ChangeArtifactStatus::Evaluating).await;
        assert_eq!(proposed.len(), 1);
        assert_eq!(evaluating.len(), 1);
    }

    #[tokio::test]
    async fn transition_invalid_returns_error() {
        let store = ChangeArtifactStore::new();
        let artifact = make_artifact("invalid transition");
        let id = store.submit(artifact).await;
        let result = store.transition(&id, ChangeArtifactStatus::Applied).await;
        assert!(matches!(
            result,
            Err(ArtifactStoreError::InvalidTransition(_))
        ));
    }

    #[tokio::test]
    async fn get_missing_returns_none() {
        let store = ChangeArtifactStore::new();
        let phantom_id = ChangeArtifactId { hash: [0u8; 32] };
        assert!(store.get(&phantom_id).await.is_none());
    }

    // ---- ArtifactPipeline ------------------------------------------------

    #[tokio::test]
    async fn tier1_propose_evaluate_apply_happy_path() {
        let pipeline = ArtifactPipeline::with_defaults();
        let artifact = make_artifact("tier 1 memory note");
        let id = pipeline.propose(artifact).await.unwrap();

        let outcome = pipeline
            .evaluate(&id, |_| DryRunOutcome::Passed)
            .await
            .unwrap();
        assert_eq!(outcome, DryRunOutcome::Passed);

        // Tier 1 needs zero approvers → apply is immediately allowed.
        let applied = pipeline.apply(&id).await.unwrap();
        assert_eq!(applied.status, ChangeArtifactStatus::Applied);
    }

    #[tokio::test]
    async fn tier2_requires_approver_before_apply() {
        let pipeline = ArtifactPipeline::with_defaults();
        let id = pipeline
            .propose(make_tier2_artifact("admin-1"))
            .await
            .unwrap();

        pipeline
            .evaluate(&id, |_| DryRunOutcome::Passed)
            .await
            .unwrap();

        let err = pipeline.apply(&id).await.unwrap_err();
        assert!(matches!(
            err,
            PipelineError::InsufficientApprovals { have: 0, need: 1 }
        ));

        pipeline.approve(&id, "approver-1").await.unwrap();
        let applied = pipeline.apply(&id).await.unwrap();
        assert_eq!(applied.status, ChangeArtifactStatus::Applied);
    }

    #[tokio::test]
    async fn self_approval_rejected() {
        let pipeline = ArtifactPipeline::with_defaults();
        let id = pipeline
            .propose(make_tier2_artifact("admin-1"))
            .await
            .unwrap();
        pipeline
            .evaluate(&id, |_| DryRunOutcome::Passed)
            .await
            .unwrap();

        let err = pipeline.approve(&id, "admin-1").await.unwrap_err();
        assert!(matches!(err, PipelineError::SelfApproval));
    }

    #[tokio::test]
    async fn failed_dry_run_rejects_artifact() {
        let pipeline = ArtifactPipeline::with_defaults();
        let id = pipeline
            .propose(make_tier2_artifact("admin-1"))
            .await
            .unwrap();

        let outcome = pipeline
            .evaluate(&id, |_| DryRunOutcome::Failed("bad state".to_string()))
            .await
            .unwrap();
        assert!(matches!(outcome, DryRunOutcome::Failed(_)));

        let artifact = pipeline.get(&id).await.unwrap();
        assert_eq!(artifact.status, ChangeArtifactStatus::Rejected);

        let err = pipeline.apply(&id).await.unwrap_err();
        assert!(matches!(err, PipelineError::WrongState { .. }));
    }

    #[tokio::test]
    async fn duplicate_approver_rejected() {
        let pipeline = ArtifactPipeline::with_defaults();
        let id = pipeline
            .propose(make_tier2_artifact("admin-1"))
            .await
            .unwrap();
        pipeline
            .evaluate(&id, |_| DryRunOutcome::Passed)
            .await
            .unwrap();

        pipeline.approve(&id, "approver-1").await.unwrap();
        let err = pipeline.approve(&id, "approver-1").await.unwrap_err();
        assert!(matches!(err, PipelineError::DuplicateApprover(_)));
    }

    #[tokio::test]
    async fn rollback_after_apply() {
        let pipeline = ArtifactPipeline::with_defaults();
        let id = pipeline
            .propose(make_tier2_artifact("admin-1"))
            .await
            .unwrap();
        pipeline
            .evaluate(&id, |_| DryRunOutcome::Passed)
            .await
            .unwrap();
        pipeline.approve(&id, "approver-1").await.unwrap();
        pipeline.apply(&id).await.unwrap();

        pipeline.rollback(&id).await.unwrap();
        let artifact = pipeline.get(&id).await.unwrap();
        assert_eq!(artifact.status, ChangeArtifactStatus::RolledBack);
    }
}
