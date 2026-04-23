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
pub struct ConstitutionalRuleEntry {
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

impl ConstitutionalRuleEntry {
    /// Create a new `ConstitutionalRuleEntry` with full metadata.
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
    inner: Arc<RwLock<HashMap<String, ConstitutionalRuleEntry>>>,
}

impl ConstitutionalRegistry {
    /// Create a new, empty registry.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register a new rule. Overwrites any existing rule with the same ID.
    pub async fn register(&self, rule: ConstitutionalRuleEntry) {
        self.inner.write().await.insert(rule.base.id.clone(), rule);
    }

    /// Remove a rule by ID. Returns the rule if it existed.
    pub async fn unregister(&self, rule_id: &str) -> Option<ConstitutionalRuleEntry> {
        self.inner.write().await.remove(rule_id)
    }

    /// Get a rule by ID.
    pub async fn get(&self, rule_id: &str) -> Option<ConstitutionalRuleEntry> {
        self.inner.read().await.get(rule_id).cloned()
    }

    /// Return all rules applicable at a given enforcement point.
    pub async fn rules_at(&self, ep: ConstitutionalEnforcementPoint) -> Vec<ConstitutionalRuleEntry> {
        self.inner
            .read()
            .await
            .values()
            .filter(|r| r.base.enforcement_point == ep)
            .cloned()
            .collect()
    }

    /// Return all registered rules.
    pub async fn all_rules(&self) -> Vec<ConstitutionalRuleEntry> {
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
        let applicable: Vec<ConstitutionalRuleEntry> = self
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
            capability_token: crate::CapabilityToken {
                id: "tok-1".to_string(),
                scopes: scopes.into_iter().collect(),
                expires_at: chrono::Utc::now(),
                max_proposals: 10,
                signature: [0u8; 64],
            },
        }
    }

    fn make_rule(id: &str, ep: ConstitutionalEnforcementPoint) -> ConstitutionalRuleEntry {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(b"rule content");
        let hash = hasher.finalize();
        let mut content_hash = [0u8; 32];
        content_hash.copy_from_slice(&hash[..32]);

        ConstitutionalRuleEntry {
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

    // ---- New edge-case tests ---------------------------------------------

    /// A rule that matches the scope/blast-radius pair is satisfied when the
    /// proposer already holds all required scopes.
    #[tokio::test]
    async fn evaluate_passes_when_proposer_holds_required_scopes() {
        let reg = ConstitutionalRegistry::new();
        let mut rule = make_rule("r-pass", ConstitutionalEnforcementPoint::PreApproval);
        rule.required_scopes.push(BlastRadius::AgentMemory);
        reg.register(rule).await;

        let result = reg
            .evaluate(
                ConstitutionalEnforcementPoint::PreApproval,
                &ChangeArtifactScope::AgentImprovement,
                &BlastRadius::AgentMemory,
                &make_proposer(vec![BlastRadius::AgentMemory]),
            )
            .await;

        assert!(result.is_ok());
    }

    /// Rules at a different enforcement point are NOT evaluated; a proposer
    /// missing required scopes passes when evaluated at the other point.
    #[tokio::test]
    async fn evaluate_ignores_rules_at_different_enforcement_point() {
        let reg = ConstitutionalRegistry::new();
        let mut rule = make_rule("r-pre-apply", ConstitutionalEnforcementPoint::PreApplication);
        rule.required_scopes.push(BlastRadius::AgentMemory);
        reg.register(rule).await;

        // Evaluating at PreProposal — the PreApply rule must be invisible here.
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

    /// When multiple rules apply, the first violation causes evaluate to return Err
    /// immediately (short-circuit behaviour).
    #[tokio::test]
    async fn evaluate_fails_on_first_violation_with_multiple_rules() {
        let reg = ConstitutionalRegistry::new();

        // Two rules both require scopes; the proposer satisfies neither.
        let mut r1 = make_rule("r1", ConstitutionalEnforcementPoint::PreApproval);
        r1.required_scopes.push(BlastRadius::AgentMemory);

        let mut r2 = make_rule("r2", ConstitutionalEnforcementPoint::PreApproval);
        r2.required_scopes.push(BlastRadius::AgentSkill);

        reg.register(r1).await;
        reg.register(r2).await;

        let result = reg
            .evaluate(
                ConstitutionalEnforcementPoint::PreApproval,
                &ChangeArtifactScope::AgentImprovement,
                &BlastRadius::AgentMemory,
                &make_proposer(vec![]),
            )
            .await;

        assert!(result.is_err());
    }

    /// is_applicable returns false when only the scope matches but the blast
    /// radius does not.
    #[test]
    fn is_applicable_requires_both_scope_and_blast_radius() {
        let rule = make_rule("r", ConstitutionalEnforcementPoint::PreProposal);
        // rule.scopes = [AgentImprovement], rule.blast_radii = [AgentMemory]
        assert!(rule.is_applicable(
            &ChangeArtifactScope::AgentImprovement,
            &BlastRadius::AgentMemory
        ));
        assert!(!rule.is_applicable(
            &ChangeArtifactScope::AgentImprovement,
            &BlastRadius::AgentSkill, // wrong blast radius
        ));
        assert!(!rule.is_applicable(
            &ChangeArtifactScope::ConfigEvolution, // wrong scope
            &BlastRadius::AgentMemory,
        ));
    }

    /// ConstitutionalViolation Display impl includes the rule id and reason.
    #[test]
    fn violation_display_contains_rule_id_and_reason() {
        let v = ConstitutionalViolation {
            rule_id: "rule-42".to_string(),
            rule_description: "desc".to_string(),
            enforcement_point: ConstitutionalEnforcementPoint::PreProposal,
            reason: "missing scope".to_string(),
        };
        let s = v.to_string();
        assert!(s.contains("rule-42"), "display missing rule_id: {s}");
        assert!(s.contains("missing scope"), "display missing reason: {s}");
    }

    /// all_rules returns every registered rule.
    #[tokio::test]
    async fn all_rules_returns_all_registered() {
        let reg = ConstitutionalRegistry::new();
        reg.register(make_rule("x1", ConstitutionalEnforcementPoint::PreProposal)).await;
        reg.register(make_rule("x2", ConstitutionalEnforcementPoint::PreApproval)).await;
        reg.register(make_rule("x3", ConstitutionalEnforcementPoint::PreApplication)).await;
        assert_eq!(reg.all_rules().await.len(), 3);
    }

    /// Overwriting a rule with the same id replaces it in the registry.
    #[tokio::test]
    async fn register_overwrites_existing_rule() {
        let reg = ConstitutionalRegistry::new();
        let mut r = make_rule("dup", ConstitutionalEnforcementPoint::PreProposal);
        reg.register(r.clone()).await;

        // Change the description and re-register under the same id.
        r.base.description = "updated".to_string();
        reg.register(r).await;

        assert_eq!(reg.all_rules().await.len(), 1);
        let fetched = reg.get("dup").await.unwrap();
        assert_eq!(fetched.base.description, "updated");
    }

    /// Unregistering a rule that does not exist returns None without error.
    #[tokio::test]
    async fn unregister_missing_rule_returns_none() {
        let reg = ConstitutionalRegistry::new();
        assert!(reg.unregister("ghost").await.is_none());
    }
}
