//! `ConstitutionalGateHook` — bridges the [`ConstitutionalRegistry`] into the
//! [`HookPoint::ConstitutionalGate`] hook point (bead sera-0yh3).
//!
//! The registry is populated from YAML at gateway startup (see
//! `sera_gateway::constitutional_config::seed_registry_from_env`), but until
//! now no hook actually consulted the registry at the gate. This hook closes
//! that gap: on every `ConstitutionalGate` invocation it scans the turn
//! output (pulled from [`HookContext::event`]) for substrings matching any
//! registered rule's `description`. A match returns
//! [`HookResult::Reject`] — the executor surfaces this to the runtime as a
//! `TurnOutcome::Interruption` (see `turn::observe` / `turn::react`).
//!
//! # Matching semantics
//!
//! The bead's registry exposes an [`evaluate`] method, but its signature takes
//! `(enforcement_point, scope, blast_radius, proposer)` and is meant for
//! scope-checking change-artifact proposers — not for scanning free-form turn
//! output. The hook therefore falls back to the task's explicit guidance:
//! iterate [`ConstitutionalRegistry::all_rules`] and treat each rule's
//! `description` as a forbidden substring. Empty descriptions are skipped so
//! an unconfigured rule cannot trivially reject every turn.
//!
//! # Side effects
//!
//! On violation the hook logs at `warn!` level with the rule id — the OCSF
//! audit emit (`class_uid=6003`, `action=blocked`) belongs in the executor /
//! gateway audit layer, not here, because the [`Hook`] trait contract does
//! not give hooks direct access to audit sinks.
//!
//! [`evaluate`]: sera_meta::constitutional::ConstitutionalRegistry::evaluate
//! [`HookContext::event`]: sera_types::hook::HookContext::event
//! [`HookPoint::ConstitutionalGate`]: sera_types::hook::HookPoint::ConstitutionalGate
//! [`HookResult::Reject`]: sera_types::hook::HookResult::Reject

use std::sync::Arc;

use async_trait::async_trait;
use sera_hooks::{Hook, HookError, HookRegistry};
use sera_meta::constitutional::ConstitutionalRegistry;
use sera_types::hook::{HookContext, HookMetadata, HookPoint, HookResult};
use tracing::warn;

/// Stable name of the hook as registered with the [`HookRegistry`]. Used by
/// chain manifests to reference it via `hook_ref`.
pub const HOOK_NAME: &str = "constitutional-gate";

/// In-process hook that consults a shared [`ConstitutionalRegistry`] at the
/// [`HookPoint::ConstitutionalGate`] point and rejects the turn if the turn
/// output matches any registered rule.
pub struct ConstitutionalGateHook {
    registry: Arc<ConstitutionalRegistry>,
}

impl ConstitutionalGateHook {
    /// Build a hook backed by the given registry.
    pub fn new(registry: Arc<ConstitutionalRegistry>) -> Self {
        Self { registry }
    }

    /// Register `ConstitutionalGateHook` with `HOOK_NAME` into the given
    /// [`HookRegistry`]. Convenience for gateway startup.
    pub fn register_into(registry: &mut HookRegistry, reg: Arc<ConstitutionalRegistry>) {
        registry.register(Box::new(Self::new(reg)));
    }

    /// Render the hook context into a single scannable string. The turn
    /// lifecycle populates `event` as either `{"messages": [..]}` (on
    /// `observe`) or `{"response": ..}` (on `react`), so both shapes are
    /// flattened here via JSON serialization.
    fn scannable_text(ctx: &HookContext) -> String {
        // updated_input takes precedence — an upstream hook may have
        // transformed the payload.
        if let Some(v) = ctx.updated_input() {
            return v.to_string();
        }
        match &ctx.event {
            Some(v) => v.to_string(),
            None => String::new(),
        }
    }
}

#[async_trait]
impl Hook for ConstitutionalGateHook {
    fn metadata(&self) -> HookMetadata {
        HookMetadata {
            name: HOOK_NAME.to_string(),
            description: "Rejects turns whose output matches a registered ConstitutionalRule"
                .to_string(),
            version: "1.0.0".to_string(),
            supported_points: vec![HookPoint::ConstitutionalGate],
            author: Some("sera-runtime".to_string()),
        }
    }

    async fn init(&mut self, _config: serde_json::Value) -> Result<(), HookError> {
        Ok(())
    }

    async fn execute(&self, ctx: &HookContext) -> Result<HookResult, HookError> {
        let text = Self::scannable_text(ctx);
        // Empty registry / empty text → pass-through.
        if text.is_empty() {
            return Ok(HookResult::pass());
        }

        let rules = self.registry.all_rules().await;
        for rule in rules {
            let needle = rule.base.description.trim();
            if needle.is_empty() {
                continue;
            }
            if text.contains(needle) {
                warn!(
                    rule_id = %rule.base.id,
                    "ConstitutionalGate: turn output matched rule — rejecting"
                );
                return Ok(HookResult::reject_with_code(
                    format!(
                        "constitutional violation: rule '{}' — {}",
                        rule.base.id, rule.base.description
                    ),
                    "constitutional_violation",
                ));
            }
        }

        Ok(HookResult::pass())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use sera_meta::constitutional::ConstitutionalRuleEntry;
    use sera_types::evolution::{ConstitutionalEnforcementPoint, ConstitutionalRule};

    fn make_rule(id: &str, description: &str) -> ConstitutionalRuleEntry {
        ConstitutionalRuleEntry {
            base: ConstitutionalRule {
                id: id.to_string(),
                description: description.to_string(),
                enforcement_point: ConstitutionalEnforcementPoint::PreApproval,
                content_hash: [0u8; 32],
            },
            scopes: vec![],
            blast_radii: vec![],
            required_scopes: vec![],
        }
    }

    fn ctx_with_event(event: serde_json::Value) -> HookContext {
        HookContext {
            event: Some(event),
            ..HookContext::new(HookPoint::ConstitutionalGate)
        }
    }

    #[tokio::test]
    async fn hook_no_op_when_registry_empty() {
        let registry = Arc::new(ConstitutionalRegistry::new());
        let hook = ConstitutionalGateHook::new(registry);
        let ctx = ctx_with_event(serde_json::json!({
            "response": "anything goes when the registry is empty"
        }));
        let result = hook.execute(&ctx).await.expect("hook must not error");
        assert!(
            result.is_continue(),
            "empty registry must pass through, got {result:?}"
        );
    }

    #[tokio::test]
    async fn hook_passes_when_no_rules_violated() {
        let registry = Arc::new(ConstitutionalRegistry::new());
        registry
            .register(make_rule("r1", "forbidden-phrase"))
            .await;
        let hook = ConstitutionalGateHook::new(registry);
        let ctx = ctx_with_event(serde_json::json!({
            "response": "everything is fine here"
        }));
        let result = hook.execute(&ctx).await.expect("hook must not error");
        assert!(
            result.is_continue(),
            "no rule substring present → pass, got {result:?}"
        );
    }

    #[tokio::test]
    async fn hook_blocks_on_violation() {
        let registry = Arc::new(ConstitutionalRegistry::new());
        registry
            .register(make_rule("r-classified", "classified"))
            .await;
        let hook = ConstitutionalGateHook::new(registry);
        let ctx = ctx_with_event(serde_json::json!({
            "response": "this reveals classified information"
        }));
        let result = hook.execute(&ctx).await.expect("hook must not error");
        match result {
            HookResult::Reject { reason, code } => {
                assert!(reason.contains("r-classified"), "reason missing rule id: {reason}");
                assert_eq!(code.as_deref(), Some("constitutional_violation"));
            }
            other => panic!("expected Reject, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn empty_description_rule_does_not_match() {
        // Unconfigured rules with empty descriptions must NOT trivially match
        // every turn — that would make every turn reject.
        let registry = Arc::new(ConstitutionalRegistry::new());
        registry.register(make_rule("r-empty", "")).await;
        let hook = ConstitutionalGateHook::new(registry);
        let ctx = ctx_with_event(serde_json::json!({"response": "x"}));
        let result = hook.execute(&ctx).await.expect("hook must not error");
        assert!(result.is_continue(), "empty description must not match");
    }

    #[tokio::test]
    async fn updated_input_takes_precedence_over_event() {
        let registry = Arc::new(ConstitutionalRegistry::new());
        registry.register(make_rule("r-bad", "BAD")).await;
        let hook = ConstitutionalGateHook::new(registry);

        let mut ctx = ctx_with_event(serde_json::json!({"response": "clean"}));
        // An upstream hook transformed the payload to one containing "BAD".
        ctx.set_updated_input(serde_json::json!({"response": "now BAD"}));

        let result = hook.execute(&ctx).await.expect("hook must not error");
        match result {
            HookResult::Reject { reason, .. } => {
                assert!(reason.contains("r-bad"), "reason missing id: {reason}");
            }
            other => panic!("expected Reject, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn register_into_exposes_hook_under_name() {
        let mut hreg = HookRegistry::new();
        let creg = Arc::new(ConstitutionalRegistry::new());
        ConstitutionalGateHook::register_into(&mut hreg, creg);
        assert!(hreg.contains(HOOK_NAME));
    }
}
