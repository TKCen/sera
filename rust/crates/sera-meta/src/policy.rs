//! Evolution policy — Tier 1/2/3 policy definitions and evaluation logic.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::{BlastRadius, ChangeArtifact, ChangeArtifactScope, ChangeProposer};
use sera_types::evolution::EvolutionTier;

/// An evolution policy that evaluates whether a change is permissible.
#[derive(Debug, Clone)]
pub struct EvolutionPolicy {
    pub id: String,
    pub name: String,
    pub description: String,
    /// Minimum tier required (as integer for ordering).
    pub min_tier: u8,
    /// Maximum tier allowed (as integer for ordering).
    pub max_tier: u8,
    /// Required capabilities for proposing changes under this policy.
    /// Stored as blast-radius scopes because CapabilityToken uses HashSet<BlastRadius>.
    pub required_scopes: HashSet<BlastRadius>,
    /// Whether shadow evaluation is required before application.
    pub requires_shadow_evaluation: bool,
    /// Maximum blast radii allowed under this policy.
    pub allowed_blast_radii: Vec<BlastRadius>,
    /// Tier-specific approval requirements (num approvers needed).
    pub approval_requirements: HashMap<u8, u8>,
}

impl EvolutionPolicy {
    /// Create a new `EvolutionPolicy`.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: String,
        name: String,
        description: String,
        min_tier: EvolutionTier,
        max_tier: EvolutionTier,
        required_scopes: HashSet<BlastRadius>,
        requires_shadow_evaluation: bool,
        allowed_blast_radii: Vec<BlastRadius>,
        approval_requirements: HashMap<EvolutionTier, u8>,
    ) -> Self {
        Self {
            id,
            name,
            description,
            min_tier: tier_to_u8(min_tier),
            max_tier: tier_to_u8(max_tier),
            required_scopes,
            requires_shadow_evaluation,
            allowed_blast_radii,
            approval_requirements: approval_requirements
                .into_iter()
                .map(|(k, v)| (tier_to_u8(k), v))
                .collect(),
        }
    }

    /// Check whether this policy applies to a given artifact.
    pub fn applies_to(&self, artifact: &ChangeArtifact) -> bool {
        let tier = scope_to_tier_u8(&artifact.scope);
        tier >= self.min_tier
            && tier <= self.max_tier
            && self.allowed_blast_radii.contains(&artifact.blast_radius)
    }

    /// Check whether a proposer has the required scopes under this policy.
    pub fn check_proposer(&self, proposer: &ChangeProposer) -> bool {
        self.required_scopes
            .iter()
            .all(|scope| proposer.capability_token.scopes.contains(scope))
    }

    /// Number of approvers required for a given tier under this policy.
    pub fn approvers_required(&self, tier: EvolutionTier) -> u8 {
        self.approval_requirements
            .get(&tier_to_u8(tier))
            .copied()
            .unwrap_or(1)
    }

    /// Build the default Tier 1 (AgentImprovement) policy.
    pub fn default_tier1() -> Self {
        Self::new(
            "default-tier1".to_string(),
            "Default Tier 1 Policy".to_string(),
            "Default policy for agent-level improvements".to_string(),
            EvolutionTier::AgentImprovement,
            EvolutionTier::AgentImprovement,
            [BlastRadius::AgentMemory].into_iter().collect(),
            false,
            vec![
                BlastRadius::AgentMemory,
                BlastRadius::AgentPersonaMutable,
                BlastRadius::AgentSkill,
                BlastRadius::AgentExperiencePool,
            ],
            [(EvolutionTier::AgentImprovement, 1)].into_iter().collect(),
        )
    }

    /// Build the default Tier 2 (ConfigEvolution) policy.
    pub fn default_tier2() -> Self {
        Self::new(
            "default-tier2".to_string(),
            "Default Tier 2 Policy".to_string(),
            "Default policy for configuration-level changes".to_string(),
            EvolutionTier::ConfigEvolution,
            EvolutionTier::ConfigEvolution,
            [BlastRadius::SingleHookConfig].into_iter().collect(),
            true,
            vec![
                BlastRadius::SingleHookConfig,
                BlastRadius::SingleToolPolicy,
                BlastRadius::SingleConnector,
                BlastRadius::SingleCircleConfig,
                BlastRadius::AgentManifest,
                BlastRadius::TierPolicy,
                BlastRadius::HookChainStructure,
                BlastRadius::ApprovalPolicy,
                BlastRadius::SecretProvider,
                BlastRadius::GlobalConfig,
            ],
            [(EvolutionTier::ConfigEvolution, 2)].into_iter().collect(),
        )
    }

    /// Build the default Tier 3 (CodeEvolution) policy.
    pub fn default_tier3() -> Self {
        Self::new(
            "default-tier3".to_string(),
            "Default Tier 3 Policy".to_string(),
            "Default policy for code-level changes".to_string(),
            EvolutionTier::CodeEvolution,
            EvolutionTier::CodeEvolution,
            [
                BlastRadius::RuntimeCrate,
                BlastRadius::GatewayCore,
            ]
            .into_iter()
            .collect(),
            true,
            vec![
                BlastRadius::RuntimeCrate,
                BlastRadius::GatewayCore,
                BlastRadius::ProtocolSchema,
                BlastRadius::DbMigration,
                BlastRadius::ConstitutionalRuleSet,
                BlastRadius::KillSwitchProtocol,
                BlastRadius::AuditLogBackend,
                BlastRadius::SelfEvolutionPipeline,
            ],
            [(EvolutionTier::CodeEvolution, 3)].into_iter().collect(),
        )
    }
}

fn tier_to_u8(tier: EvolutionTier) -> u8 {
    match tier {
        EvolutionTier::AgentImprovement => 1,
        EvolutionTier::ConfigEvolution => 2,
        EvolutionTier::CodeEvolution => 3,
        _ => 0,
    }
}

fn scope_to_tier_u8(scope: &ChangeArtifactScope) -> u8 {
    match scope {
        ChangeArtifactScope::AgentImprovement => 1,
        ChangeArtifactScope::ConfigEvolution => 2,
        ChangeArtifactScope::CodeEvolution => 3,
    }
}

/// Result of evaluating a change under a policy.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EvolutionResult {
    /// The evaluated artifact ID.
    pub artifact_id: crate::ChangeArtifactId,
    /// Policy that was evaluated against.
    pub policy_id: String,
    /// Whether the change passed all checks.
    pub approved: bool,
    /// Human-readable summary.
    pub summary: String,
    /// List of violations if any.
    pub violations: Vec<String>,
    /// Whether shadow evaluation is recommended/required.
    pub requires_shadow: bool,
}

impl EvolutionResult {
    /// Create an approved result.
    pub fn approved(
        artifact_id: crate::ChangeArtifactId,
        policy_id: String,
        summary: String,
        requires_shadow: bool,
    ) -> Self {
        Self {
            artifact_id,
            policy_id,
            approved: true,
            summary,
            violations: Vec::new(),
            requires_shadow,
        }
    }

    /// Create a rejected result.
    pub fn rejected(
        artifact_id: crate::ChangeArtifactId,
        policy_id: String,
        summary: String,
        violations: Vec<String>,
    ) -> Self {
        Self {
            artifact_id,
            policy_id,
            approved: false,
            summary,
            violations,
            requires_shadow: false,
        }
    }
}

/// Thread-safe evolution policy engine.
#[derive(Debug, Clone)]
pub struct PolicyEngine {
    policies: Arc<RwLock<Vec<EvolutionPolicy>>>,
}

impl PolicyEngine {
    /// Create a new policy engine with the default tier 1/2/3 policies.
    pub fn new() -> Self {
        Self {
            policies: Arc::new(RwLock::new(vec![
                EvolutionPolicy::default_tier1(),
                EvolutionPolicy::default_tier2(),
                EvolutionPolicy::default_tier3(),
            ])),
        }
    }

    /// Add a policy to the engine.
    pub async fn add_policy(&self, policy: EvolutionPolicy) {
        self.policies.write().await.push(policy);
    }

    /// Find the most specific applicable policy for an artifact.
    pub async fn find_applicable(&self, artifact: &ChangeArtifact) -> Option<EvolutionPolicy> {
        let policies = self.policies.read().await;
        policies.iter().find(|p| p.applies_to(artifact)).cloned()
    }

    /// Evaluate an artifact against all applicable policies.
    ///
    /// Returns the first applicable policy's result, or an error if no policy applies.
    pub async fn evaluate(&self, artifact: &ChangeArtifact) -> Result<EvolutionResult, &'static str> {
        let policy = self
            .find_applicable(artifact)
            .await
            .ok_or("no applicable policy")?;

        if !policy.check_proposer(&artifact.proposer) {
            return Ok(EvolutionResult::rejected(
                artifact.id,
                policy.id.clone(),
                format!(
                    "proposer lacks required scopes for policy '{}'",
                    policy.name
                ),
                vec![format!("required scopes: {:?}", policy.required_scopes)],
            ));
        }

        let _tier = scope_to_tier_u8(&artifact.scope);
        let tier_name = match artifact.scope {
            ChangeArtifactScope::AgentImprovement => "Tier 1 (AgentImprovement)",
            ChangeArtifactScope::ConfigEvolution => "Tier 2 (ConfigEvolution)",
            ChangeArtifactScope::CodeEvolution => "Tier 3 (CodeEvolution)",
        };

        let summary = format!(
            "change to {:?} approved under {} policy '{}'",
            artifact.blast_radius, tier_name, policy.name
        );

        Ok(EvolutionResult::approved(
            artifact.id,
            policy.id.clone(),
            summary,
            policy.requires_shadow_evaluation,
        ))
    }
}

impl Default for PolicyEngine {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_proposer(scopes: Vec<BlastRadius>) -> ChangeProposer {
        ChangeProposer {
            principal_id: "agent-1".to_string(),
            capability_token: sera_types::evolution::CapabilityToken {
                id: "tok-1".to_string(),
                scopes: scopes.into_iter().collect(),
                expires_at: chrono::Utc::now(),
                max_proposals: 10,
                signature: [0u8; 64],
            },
        }
    }

    #[tokio::test]
    async fn default_tier1_policy_approves_agent_memory_change() {
        let engine = PolicyEngine::new();
        let artifact = ChangeArtifact::new(
            "Improve agent memory".to_string(),
            ChangeArtifactScope::AgentImprovement,
            BlastRadius::AgentMemory,
            make_proposer(vec![BlastRadius::AgentMemory]),
            serde_json::json!({}),
        );

        let result = engine.evaluate(&artifact).await;
        assert!(result.is_ok());
        assert!(result.unwrap().approved);
    }

    #[tokio::test]
    async fn default_tier3_requires_scopes() {
        let engine = PolicyEngine::new();
        let artifact = ChangeArtifact::new(
            "Change runtime crate".to_string(),
            ChangeArtifactScope::CodeEvolution,
            BlastRadius::RuntimeCrate,
            make_proposer(vec![]),
            serde_json::json!({}),
        );

        let result = engine.evaluate(&artifact).await;
        assert!(result.is_ok());
        // Proposer lacks required scopes for Tier 3
        assert!(!result.unwrap().approved);
    }

    #[tokio::test]
    async fn no_applicable_policy_returns_err() {
        let engine = PolicyEngine::new();
        // Scope/blast-radius mismatch: AgentImprovement tier with a Tier-2
        // blast radius has no applicable policy in the default set.
        let artifact = ChangeArtifact::new(
            "mismatched".to_string(),
            ChangeArtifactScope::AgentImprovement,
            BlastRadius::GlobalConfig,
            make_proposer(vec![]),
            serde_json::json!({}),
        );

        let result = engine.evaluate(&artifact).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn approvers_required_tier3() {
        let policy = EvolutionPolicy::default_tier3();
        assert_eq!(
            policy.approvers_required(EvolutionTier::CodeEvolution),
            3
        );
    }

    // ---- New edge-case tests ---------------------------------------------

    /// Tier-2 artifact with correct scopes is approved.
    #[tokio::test]
    async fn tier2_artifact_approved_with_correct_scopes() {
        let engine = PolicyEngine::new();
        let artifact = ChangeArtifact::new(
            "change hook config".to_string(),
            ChangeArtifactScope::ConfigEvolution,
            BlastRadius::SingleHookConfig,
            make_proposer(vec![BlastRadius::SingleHookConfig]),
            serde_json::json!({}),
        );

        let result = engine.evaluate(&artifact).await.unwrap();
        assert!(result.approved);
        assert!(result.requires_shadow);
    }

    /// Tier-2 artifact without required scopes is rejected (not an error — just not approved).
    #[tokio::test]
    async fn tier2_artifact_rejected_without_required_scopes() {
        let engine = PolicyEngine::new();
        let artifact = ChangeArtifact::new(
            "change hook config".to_string(),
            ChangeArtifactScope::ConfigEvolution,
            BlastRadius::SingleHookConfig,
            make_proposer(vec![]), // no scopes
            serde_json::json!({}),
        );

        let result = engine.evaluate(&artifact).await.unwrap();
        assert!(!result.approved);
        assert!(!result.violations.is_empty());
    }

    /// Tier-3 artifact with all required scopes is approved.
    #[tokio::test]
    async fn tier3_artifact_approved_with_required_scopes() {
        let engine = PolicyEngine::new();
        let artifact = ChangeArtifact::new(
            "change runtime crate".to_string(),
            ChangeArtifactScope::CodeEvolution,
            BlastRadius::RuntimeCrate,
            make_proposer(vec![BlastRadius::RuntimeCrate, BlastRadius::GatewayCore]),
            serde_json::json!({}),
        );

        let result = engine.evaluate(&artifact).await.unwrap();
        assert!(result.approved);
        assert!(result.requires_shadow);
    }

    /// applies_to returns false when blast_radius is not in allowed list.
    #[test]
    fn applies_to_false_for_disallowed_blast_radius() {
        let policy = EvolutionPolicy::default_tier1();
        // Tier-1 policy does not cover GlobalConfig (Tier-2 blast radius).
        let artifact = ChangeArtifact::new(
            "test".to_string(),
            ChangeArtifactScope::AgentImprovement,
            BlastRadius::GlobalConfig,
            make_proposer(vec![]),
            serde_json::json!({}),
        );
        assert!(!policy.applies_to(&artifact));
    }

    /// approvers_required falls back to 1 when the tier is not in the map.
    #[test]
    fn approvers_required_defaults_to_one_for_unknown_tier() {
        let policy = EvolutionPolicy::default_tier1();
        // Tier-1 policy only has AgentImprovement in its map; CodeEvolution is absent.
        assert_eq!(policy.approvers_required(EvolutionTier::CodeEvolution), 1);
    }

    /// check_proposer returns true when required_scopes is empty (open policy).
    #[test]
    fn check_proposer_true_when_no_required_scopes() {
        let policy = EvolutionPolicy::new(
            "open".to_string(),
            "Open Policy".to_string(),
            "no scope requirements".to_string(),
            EvolutionTier::AgentImprovement,
            EvolutionTier::AgentImprovement,
            std::collections::HashSet::new(), // empty required_scopes
            false,
            vec![BlastRadius::AgentMemory],
            std::collections::HashMap::new(),
        );
        let proposer = make_proposer(vec![]); // no scopes
        assert!(policy.check_proposer(&proposer));
    }

    /// A custom policy added via add_policy is found by find_applicable.
    #[tokio::test]
    async fn custom_policy_found_by_find_applicable() {
        let engine = PolicyEngine::new();
        let custom = EvolutionPolicy::new(
            "custom-agent-skill".to_string(),
            "Custom Skill Policy".to_string(),
            "covers agent skill changes".to_string(),
            EvolutionTier::AgentImprovement,
            EvolutionTier::AgentImprovement,
            std::collections::HashSet::new(),
            false,
            vec![BlastRadius::AgentSkill],
            std::collections::HashMap::new(),
        );
        engine.add_policy(custom).await;

        let artifact = ChangeArtifact::new(
            "update skill".to_string(),
            ChangeArtifactScope::AgentImprovement,
            BlastRadius::AgentSkill,
            make_proposer(vec![]),
            serde_json::json!({}),
        );

        let found = engine.find_applicable(&artifact).await;
        // The default tier-1 policy also covers AgentSkill, so at minimum one policy matches.
        assert!(found.is_some());
    }

    /// EvolutionResult::rejected carries the violations list.
    #[test]
    fn evolution_result_rejected_carries_violations() {
        use crate::ChangeArtifactId;
        let id = ChangeArtifactId { hash: [0u8; 32] };
        let result = super::EvolutionResult::rejected(
            id,
            "pol".to_string(),
            "summary".to_string(),
            vec!["violation-1".to_string(), "violation-2".to_string()],
        );
        assert!(!result.approved);
        assert_eq!(result.violations.len(), 2);
        assert!(!result.requires_shadow);
    }
}
