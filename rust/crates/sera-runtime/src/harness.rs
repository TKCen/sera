//! Agent harness — coordinates submission processing and event emission.

use serde::{Deserialize, Serialize};

/// Harness support level for a given context.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "support", rename_all = "snake_case")]
pub enum HarnessSupport {
    Supported,
    Unsupported { reason: String },
    RequiresUpgrade { required_tier: String },
}

/// Context for checking whether the harness can serve a given agent.
///
/// Populated by the gateway before dispatching a turn so `DefaultHarness`
/// (or any custom harness) can decide whether to accept, reject, or require
/// an upgrade.
#[derive(Debug, Clone)]
pub struct HarnessSupportContext {
    /// Agent identifier this turn belongs to.
    pub agent_id: String,
    /// Sandbox tier requested for the agent (e.g. `"tier-1"`, `"tier-2"`).
    pub tier: String,
    /// Model identifier the agent intends to invoke.
    pub model_id: String,
    /// Capability flags the agent needs (e.g. `"tools"`, `"streaming"`,
    /// `"vision"`, `"long_context"`). The harness matches these against
    /// its own `supported_capabilities` set.
    pub required_capabilities: Vec<String>,
    /// Runtime features the agent expects (e.g. `"hooks"`, `"subagents"`).
    pub runtime_features: Vec<String>,
}

impl HarnessSupportContext {
    /// Minimal constructor — callers typically build via field literals.
    pub fn new(agent_id: impl Into<String>, tier: impl Into<String>) -> Self {
        Self {
            agent_id: agent_id.into(),
            tier: tier.into(),
            model_id: String::new(),
            required_capabilities: Vec::new(),
            runtime_features: Vec::new(),
        }
    }
}

/// Parameters for compaction.
#[derive(Debug, Clone)]
pub struct CompactionParams {
    pub session_key: String,
    pub trigger: CompactionTrigger,
}

/// Compaction triggers.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompactionTrigger {
    Manual,
    AutoThreshold,
    OverflowRetry,
    TimeoutRetry,
}

/// Result of compaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactionResult {
    pub tokens_before: u32,
    pub tokens_after: u32,
    pub messages_removed: u32,
}

/// Parameters for reset.
#[derive(Debug, Clone)]
pub struct ResetParams {
    pub session_key: String,
}

/// Harness errors.
#[derive(Debug, thiserror::Error)]
pub enum HarnessError {
    #[error("harness error: {0}")]
    Internal(String),
    #[error("not supported: {0}")]
    NotSupported(String),
}

/// The default harness implementation.
///
/// Tracks the sandbox tiers and capabilities it can serve; missing
/// capabilities yield [`HarnessSupport::Unsupported`], higher tier
/// requirements yield [`HarnessSupport::RequiresUpgrade`].
pub struct DefaultHarness {
    supported_tiers: Vec<String>,
    supported_capabilities: Vec<String>,
}

impl DefaultHarness {
    pub fn new() -> Self {
        Self {
            supported_tiers: vec!["tier-1".into(), "tier-2".into(), "tier-3".into()],
            supported_capabilities: vec![
                "tools".into(),
                "streaming".into(),
                "hooks".into(),
                "subagents".into(),
            ],
        }
    }

    /// Override the set of tiers this harness will accept.
    pub fn with_tiers(mut self, tiers: Vec<String>) -> Self {
        self.supported_tiers = tiers;
        self
    }

    /// Override the set of capabilities this harness advertises.
    pub fn with_capabilities(mut self, caps: Vec<String>) -> Self {
        self.supported_capabilities = caps;
        self
    }

    pub fn supports(&self, ctx: &HarnessSupportContext) -> HarnessSupport {
        if !self.supported_tiers.iter().any(|t| t == &ctx.tier) {
            return HarnessSupport::RequiresUpgrade {
                required_tier: ctx.tier.clone(),
            };
        }
        for cap in &ctx.required_capabilities {
            if !self.supported_capabilities.contains(cap) {
                return HarnessSupport::Unsupported {
                    reason: format!("capability '{cap}' is not provided by this harness"),
                };
            }
        }
        HarnessSupport::Supported
    }
}

impl Default for DefaultHarness {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supports_defaults_accept_known_tier_and_no_caps() {
        let h = DefaultHarness::new();
        let ctx = HarnessSupportContext::new("agent-1", "tier-2");
        assert!(matches!(h.supports(&ctx), HarnessSupport::Supported));
    }

    #[test]
    fn supports_rejects_unknown_tier_as_upgrade() {
        let h = DefaultHarness::new();
        let ctx = HarnessSupportContext::new("agent-1", "tier-99");
        match h.supports(&ctx) {
            HarnessSupport::RequiresUpgrade { required_tier } => {
                assert_eq!(required_tier, "tier-99");
            }
            other => panic!("expected RequiresUpgrade, got {:?}", other),
        }
    }

    #[test]
    fn supports_rejects_missing_capability_as_unsupported() {
        let h = DefaultHarness::new();
        let mut ctx = HarnessSupportContext::new("agent-1", "tier-1");
        ctx.required_capabilities = vec!["vision".into()];
        match h.supports(&ctx) {
            HarnessSupport::Unsupported { reason } => {
                assert!(reason.contains("vision"));
            }
            other => panic!("expected Unsupported, got {:?}", other),
        }
    }

    #[test]
    fn supports_with_explicit_capability_allowlist() {
        let h = DefaultHarness::new().with_capabilities(vec!["vision".into(), "tools".into()]);
        let mut ctx = HarnessSupportContext::new("agent-1", "tier-1");
        ctx.required_capabilities = vec!["vision".into(), "tools".into()];
        assert!(matches!(h.supports(&ctx), HarnessSupport::Supported));
    }
}
