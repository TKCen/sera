//! Constitutional rule registry.
//!
//! Tracks the rules that govern what changes are permissible at each
//! evolution tier and enforcement point.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::{BlastRadius, ChangeArtifactScope, ChangeProposer};
use sera_types::evolution::ConstitutionalEnforcementPoint;
use sera_types::evolution::ConstitutionalRule as ConstitutionalRuleBase;

/// A constitutional rule with full applicability metadata (scopes, blast radii,
/// required scopes) — stored in the registry.
#[derive(Debug, Clone)]
pub struct ConstitutionalRule {
    /// The base rule from sera-types.
    pub base: ConstitutionalRuleBase,
    /// Scopes this rule applies to.
    pub scopes: Vec<ChangeArtifactScope>,
    /// Blast radii this rule applies to.
    pub blast_radii: Vec<BlastRadius>,
    /// Minimum blast-radius scopes required to propose under this rule.
    /// (Uses BlastRadius because CapabilityToken.scopes is HashSet<BlastRadius>.)
    pub required_scopes: Vec<BlastRadius>,
}

impl ConstitutionalRule {
    /// Create a new `ConstitutionalRule` with full metadata.
    pub fn new(
        base: ConstitutionalRuleBase,
        scopes: Vec<ChangeArtifactScope>,
        blast_radii: Vec<BlastRadius>,
        required_scopes: Vec<BlastRadius>,
    ) -> Self {
        Self {
            base,
            scopes,
            blast_radii,
            required_scopes,
        }
    }

    /// Check whether this rule is applicable to a given scope and blast radius.
    pub fn is_applicable(&self, scope: &ChangeArtifactScope, blast_radius: &BlastRadius) -> bool {
        self.scopes.iter().any(|s| s == scope) && self.blast_radii.contains(blast_radius)
    }

    /// Check whether a proposer satisfies the required scopes.
    /// Uses `capability_token.scopes` which is HashSet<BlastRadius>.
    pub fn check_proposer(&self, proposer: &ChangeProposer) -> bool {
        self.required_scopes
            .iter()
            .all(|req| proposer.capability_token.scopes.contains(req))
    }
}

/// Thread-safe registry of constitutional rules.
#[derive(Debug, Clone)]
pub struct ConstitutionalRegistry {
    inner: Arc<RwLock<HashMap<String, ConstitutionalRule>>>,
}

impl ConstitutionalRegistry {
    /// Create a new, empty registry.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register a new rule. Overwrites any existing rule with the same ID.
    pub async fn register(&self, rule: ConstitutionalRule) {
        self.inner.write().await.insert(rule.base.id.clone(), rule);
    }

    /// Remove a rule by ID. Returns the rule if it existed.
    pub async fn unregister(&self, rule_id: &str) -> Option<ConstitutionalRule> {
        self.inner.write().await.remove(rule_id)
    }

    /// Get a rule by ID.
    pub async fn get(&self, rule_id: &str) -> Option<ConstitutionalRule> {
        self.inner.read().await.get(rule_id).cloned()
    }

    /// Return all rules applicable at a given enforcement point.
    pub async fn rules_at(&self, ep: ConstitutionalEnforcementPoint) -> Vec<ConstitutionalRule> {
        self.inner
            .read()
            .await
            .values()
            .filter(|r| r.base.enforcement_point == ep)
            .cloned()
            .collect()
    }

    /// Return all registered rules.
    pub async fn all_rules(&self) -> Vec<ConstitutionalRule> {
        self.inner.read().await.values().cloned().collect()
    }

    /// Evaluate all applicable rules at `enforcement_point` for a proposed change.
    ///
    /// Returns `Ok(())` if all applicable rules pass, or `Err` listing the first failure.
    pub async fn evaluate(
        &self,
        enforcement_point: ConstitutionalEnforcementPoint,
        scope: &ChangeArtifactScope,
        blast_radius: &BlastRadius,
        proposer: &ChangeProposer,
    ) -> Result<(), ConstitutionalViolation> {
        let applicable: Vec<ConstitutionalRule> = self
            .inner
            .read()
            .await
            .values()
            .filter(|r| {
                r.base.enforcement_point == enforcement_point
                    && r.is_applicable(scope, blast_radius)
            })
            .cloned()
            .collect();

        for rule in applicable {
            if !rule.check_proposer(proposer) {
                return Err(ConstitutionalViolation {
                    rule_id: rule.base.id.clone(),
                    rule_description: rule.base.description.clone(),
                    enforcement_point,
                    reason: "proposer lacks required scopes".to_string(),
                });
            }
        }

        Ok(())
    }
}

impl Default for ConstitutionalRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Alias used by SPEC-self-evolution §6; identical to [`ConstitutionalRegistry`].
pub type ConstitutionalRuleRegistry = ConstitutionalRegistry;

/// A constitutional rule was violated.
#[derive(Debug, Clone)]
pub struct ConstitutionalViolation {
    pub rule_id: String,
    pub rule_description: String,
    pub enforcement_point: ConstitutionalEnforcementPoint,
    pub reason: String,
}

impl std::fmt::Display for ConstitutionalViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "constitutional violation at {:?}: rule '{}' — {}",
            self.enforcement_point, self.rule_id, self.reason
        )
    }
}

impl std::error::Error for ConstitutionalViolation {}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_proposer(scopes: Vec<BlastRadius>) -> ChangeProposer {
        ChangeProposer {
            principal_id: "tester".to_string(),
            capability_token: sera_types::evolution::CapabilityToken {
                id: "tok-1".to_string(),
                scopes: scopes.into_iter().collect(),
                expires_at: chrono::Utc::now(),
                max_proposals: 10,
                signature: [0u8; 64],
            },
        }
    }

    fn make_rule(id: &str, ep: ConstitutionalEnforcementPoint) -> ConstitutionalRule {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(b"rule content");
        let hash = hasher.finalize();
        let mut content_hash = [0u8; 32];
        content_hash.copy_from_slice(&hash[..32]);

        ConstitutionalRule {
            base: sera_types::evolution::ConstitutionalRule {
                id: id.to_string(),
                description: format!("rule {id}"),
                enforcement_point: ep,
                content_hash,
            },
            scopes: vec![ChangeArtifactScope::AgentImprovement],
            blast_radii: vec![BlastRadius::AgentMemory],
            required_scopes: vec![],
        }
    }

    #[tokio::test]
    async fn register_and_retrieve() {
        let reg = ConstitutionalRegistry::new();
        let rule = make_rule("r1", ConstitutionalEnforcementPoint::PreApproval);
        reg.register(rule.clone()).await;
        assert_eq!(
            reg.get("r1").await.map(|r| r.base.id.clone()),
            Some("r1".to_string())
        );
    }

    #[tokio::test]
    async fn unregister() {
        let reg = ConstitutionalRegistry::new();
        reg.register(make_rule("r1", ConstitutionalEnforcementPoint::PreProposal))
            .await;
        assert!(reg.get("r1").await.is_some());
        let removed = reg.unregister("r1").await;
        assert!(removed.is_some());
        assert!(reg.get("r1").await.is_none());
    }

    #[tokio::test]
    async fn rules_at_filters_by_enforcement_point() {
        let reg = ConstitutionalRegistry::new();
        reg.register(make_rule("r1", ConstitutionalEnforcementPoint::PreProposal))
            .await;
        reg.register(make_rule("r2", ConstitutionalEnforcementPoint::PreApproval))
            .await;

        let at_pre_prop = reg.rules_at(ConstitutionalEnforcementPoint::PreProposal).await;
        assert_eq!(at_pre_prop.len(), 1);
        assert_eq!(at_pre_prop[0].base.id, "r1");
    }

    #[tokio::test]
    async fn evaluate_passes_when_no_applicable_rules() {
        let reg = ConstitutionalRegistry::new();
        let result = reg
            .evaluate(
                ConstitutionalEnforcementPoint::PreProposal,
                &ChangeArtifactScope::AgentImprovement,
                &BlastRadius::AgentMemory,
                &make_proposer(vec![]),
            )
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn evaluate_fails_when_scopes_insufficient() {
        let reg = ConstitutionalRegistry::new();
        let mut rule = make_rule("r1", ConstitutionalEnforcementPoint::PreApproval);
        rule.required_scopes.push(BlastRadius::AgentMemory);
        reg.register(rule).await;

        let result = reg
            .evaluate(
                ConstitutionalEnforcementPoint::PreApproval,
                &ChangeArtifactScope::AgentImprovement,
                &BlastRadius::AgentMemory,
                &make_proposer(vec![]),
            )
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.rule_id, "r1");
    }
}
