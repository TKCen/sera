//! Gateway-side principal policy resolution seam (sera-fhcr / PR2).
//!
//! PR2 plumbs the resolution path so the gateway can ask a
//! [`PrincipalPolicyStore`] for an `Arc<PrincipalPolicy>` given a
//! [`Principal`]. The runtime dispatcher gate (PR3 / sera-u4gj) will
//! consume the resolved policy via `TurnContext` — this module deliberately
//! stops short of that wiring so PR2 stays reviewable in isolation.
//!
//! Where this gets wired in PR3:
//!
//! - `chat_handler` (`sera-gateway/src/bin/sera.rs`) constructs the
//!   principal at admission, calls [`PolicyResolver::resolve`], and stamps
//!   the resulting `Arc<PrincipalPolicy>` onto the lane submission so every
//!   `ToolContext` for that turn carries the same value (turn-lifetime
//!   immutability — see design report §7.1).
//! - `chat_cancel_handler` does not need a policy (no tool dispatch), but
//!   should still resolve for audit parity.
//! - `EmbeddedRuntimeTransport::send_turn` extends `TurnContext` with the
//!   resolved policy field.
//!
//! Until that wiring lands, the resolver is a thin async forwarder over the
//! store with no per-turn caching.

use std::sync::Arc;

use sera_auth::policy::{
    InMemoryPrincipalPolicyStore, PrincipalPolicy, PrincipalPolicyStore,
};
use sera_types::principal::Principal;

/// Thin wrapper over a [`PrincipalPolicyStore`] that the gateway can pass
/// down to admission code in PR3. Today this just forwards `resolve` to the
/// store; PR3 can layer per-turn caching here without churning callers.
#[derive(Clone)]
pub struct PolicyResolver {
    store: Arc<dyn PrincipalPolicyStore>,
}

impl PolicyResolver {
    pub fn new(store: Arc<dyn PrincipalPolicyStore>) -> Self {
        Self { store }
    }

    /// Construct a resolver backed by a fresh
    /// [`InMemoryPrincipalPolicyStore`] seeded with per-kind defaults.
    pub fn in_memory() -> Self {
        Self::new(Arc::new(InMemoryPrincipalPolicyStore::new()))
    }

    /// Resolve the policy for a principal at gateway admission. The
    /// returned `Arc` is meant to be cloned cheaply onto the turn
    /// submission and propagated immutably to downstream `ToolContext`
    /// builders in PR3.
    pub async fn resolve(&self, principal: &Principal) -> Arc<PrincipalPolicy> {
        self.store.resolve(principal).await
    }
}

impl std::fmt::Debug for PolicyResolver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PolicyResolver").finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sera_types::tool::ToolScope;

    #[tokio::test]
    async fn resolver_returns_kind_default_for_admin_human() {
        let resolver = PolicyResolver::in_memory();
        let admin = Principal::default_admin();
        let policy = resolver.resolve(&admin).await;
        assert_eq!(policy.max_delegation_depth, 5);
        assert!(policy.allow_meta_change);
        assert!(policy.allowed_tool_scopes.contains(&ToolScope::Admin));
    }

    #[tokio::test]
    async fn resolver_returns_kind_default_for_agent() {
        let resolver = PolicyResolver::in_memory();
        let agent = Principal::for_agent("a-1", "a");
        let policy = resolver.resolve(&agent).await;
        assert_eq!(policy.max_delegation_depth, 2);
        assert!(!policy.allow_meta_change);
        assert!(!policy.allowed_tool_scopes.contains(&ToolScope::Admin));
    }

    #[tokio::test]
    async fn resolver_returns_default_deny_for_external_agent() {
        let resolver = PolicyResolver::in_memory();
        let ext = Principal::external_agent("a2a", "bot");
        let policy = resolver.resolve(&ext).await;
        assert_eq!(policy.max_delegation_depth, 0);
        assert!(policy.allowed_tool_scopes.is_empty());
        assert!(!policy.allow_subagent_spawn);
    }

    /// The resolver must return a fresh `Arc` reference per call — PR2
    /// does not yet cache, and depending on identity here would tie the
    /// gateway to a specific implementation. PR3 will layer turn-scoped
    /// caching at a higher level (the lane submission), not in the
    /// resolver itself.
    #[tokio::test]
    async fn resolver_returns_equal_policy_across_calls() {
        let resolver = PolicyResolver::in_memory();
        let agent = Principal::for_agent("a-1", "a");
        let p1 = resolver.resolve(&agent).await;
        let p2 = resolver.resolve(&agent).await;
        assert_eq!(*p1, *p2);
    }
}
