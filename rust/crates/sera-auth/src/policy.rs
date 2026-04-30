//! `PrincipalPolicy` — the per-principal authorization envelope (sera-fhcr).
//!
//! Resolved at gateway admission and (in PR3 / sera-u4gj) threaded through
//! `TurnContext` into every `ToolContext`. PR2 only lands the type, the
//! per-kind defaults, the in-memory store, and the narrowing helper used by
//! the future sub-agent dispatch path. The runtime dispatcher gate itself is
//! deliberately not wired here.
//!
//! Per-kind defaults follow the design report
//! (`artifacts/reports/claude-burn/principal-policy-tool-scope-design-2026-04-30.md`,
//! §5.3):
//!
//! | Kind            | scopes                                   | depth | spawn / meta / code |
//! |-----------------|------------------------------------------|-------|---------------------|
//! | Human           | all                                       | 5     | true / true / true  |
//! | Agent           | Read, Write, Execute, Network, Vcs, Agent | 2     | true / false / true |
//! | ExternalAgent   | empty                                     | 0     | false / false / false |
//! | Service         | empty                                     | 0     | false / false / false |
//! | System          | all                                       | 5     | true / true / true  |
//!
//! `required_trust_level` mirrors `PrincipalKind::default_trust_level`:
//! Human / Agent / Service / System default to [`TrustLevel::FirstParty`];
//! `ExternalAgent` defaults to [`TrustLevel::Unverified`] so the policy
//! "accepts" any external caller while the empty scope set is the actual
//! barrier (default-deny).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use sera_config::principal_policy::{PrincipalPoliciesConfig, PrincipalPolicySpec};
use sera_types::evolution::BlastRadius;
use sera_types::principal::{Principal, PrincipalKind, TrustLevel};
use sera_types::tool::ToolScope;

/// Per-principal authorization envelope. Resolved once at gateway admission
/// and propagated immutably through the turn (see design report §7.1 on
/// per-turn vs per-call resolution).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PrincipalPolicy {
    /// Maximum TTL for any capability token issued under this principal.
    /// On-disk format is seconds (`max_token_ttl_secs`).
    #[serde(with = "duration_secs", rename = "max_token_ttl_secs")]
    pub max_token_ttl: Duration,

    /// Coarse tool-category gate. Empty set = no tool calls allowed.
    pub allowed_tool_scopes: HashSet<ToolScope>,

    /// Maximum capability-token delegation chain depth. The runtime
    /// enforcement point lives in `CapabilityToken::narrow()` (sera-s64j);
    /// the field lands here so policies are correctly shaped from the start.
    pub max_delegation_depth: u32,

    /// Self-evolution proposals: which BlastRadii this principal may target.
    /// Empty set = no proposals.
    pub allowed_blast_radii: HashSet<BlastRadius>,

    /// Whether this principal may spawn sub-agents (delegate / ask /
    /// background). Independent of `ToolScope::Agent` because Agent-scope
    /// tools may exist for purposes other than dispatch.
    pub allow_subagent_spawn: bool,

    /// Whether this principal may originate meta-changes (skill updates,
    /// hook chain edits, persona mutation). Required even when the tool
    /// in question carries `ToolScope::Admin`.
    pub allow_meta_change: bool,

    /// Whether this principal may originate code changes via the
    /// change-proposer path. Decoupled from `allow_meta_change` so a
    /// code-editor agent can edit files without editing tier policies.
    pub allow_code_change: bool,

    /// Minimum trust level required for the principal under this policy.
    /// Higher levels (`FirstParty`) are stricter than lower (`Unverified`).
    /// The narrowing helper picks the stricter of two policies' values.
    pub required_trust_level: TrustLevel,
}

impl PrincipalPolicy {
    /// Conservative empty policy: empty scopes / blast radii, depth 0, all
    /// privileged booleans false, [`TrustLevel::FirstParty`] required, 5-min
    /// TTL. Use [`PrincipalPolicy::for_kind`] for the kind-aware defaults.
    pub fn empty() -> Self {
        Self {
            max_token_ttl: Duration::from_secs(300),
            allowed_tool_scopes: HashSet::new(),
            max_delegation_depth: 0,
            allowed_blast_radii: HashSet::new(),
            allow_subagent_spawn: false,
            allow_meta_change: false,
            allow_code_change: false,
            required_trust_level: TrustLevel::FirstParty,
        }
    }

    /// Per-`PrincipalKind` defaults from the design report (§5.3).
    pub fn for_kind(kind: PrincipalKind) -> Self {
        match kind {
            PrincipalKind::Human => Self {
                max_token_ttl: Duration::from_secs(3600),
                allowed_tool_scopes: all_tool_scopes(),
                max_delegation_depth: 5,
                allowed_blast_radii: all_blast_radii(),
                allow_subagent_spawn: true,
                allow_meta_change: true,
                allow_code_change: true,
                required_trust_level: PrincipalKind::Human.default_trust_level(),
            },
            PrincipalKind::Agent => Self {
                max_token_ttl: Duration::from_secs(1800),
                allowed_tool_scopes: HashSet::from([
                    ToolScope::Read,
                    ToolScope::Write,
                    ToolScope::Execute,
                    ToolScope::Network,
                    ToolScope::Vcs,
                    ToolScope::Agent,
                ]),
                max_delegation_depth: 2,
                allowed_blast_radii: agent_blast_radii(),
                allow_subagent_spawn: true,
                allow_meta_change: false,
                allow_code_change: true,
                required_trust_level: PrincipalKind::Agent.default_trust_level(),
            },
            PrincipalKind::ExternalAgent => Self {
                max_token_ttl: Duration::from_secs(300),
                allowed_tool_scopes: HashSet::new(),
                max_delegation_depth: 0,
                allowed_blast_radii: HashSet::new(),
                allow_subagent_spawn: false,
                allow_meta_change: false,
                allow_code_change: false,
                required_trust_level: PrincipalKind::ExternalAgent.default_trust_level(),
            },
            PrincipalKind::Service => Self {
                max_token_ttl: Duration::from_secs(900),
                allowed_tool_scopes: HashSet::new(),
                max_delegation_depth: 0,
                allowed_blast_radii: HashSet::new(),
                allow_subagent_spawn: false,
                allow_meta_change: false,
                allow_code_change: false,
                required_trust_level: PrincipalKind::Service.default_trust_level(),
            },
            PrincipalKind::System => Self {
                max_token_ttl: Duration::from_secs(3600),
                allowed_tool_scopes: all_tool_scopes(),
                max_delegation_depth: 5,
                allowed_blast_radii: all_blast_radii(),
                allow_subagent_spawn: true,
                allow_meta_change: true,
                allow_code_change: true,
                required_trust_level: PrincipalKind::System.default_trust_level(),
            },
        }
    }

    /// Apply override fields from a [`PrincipalPolicySpec`] on top of `self`.
    /// Fields left unset on the spec preserve `self`'s values; this is the
    /// merge semantics used by [`InMemoryPrincipalPolicyStore::from_config`].
    pub fn with_spec(mut self, spec: PrincipalPolicySpec) -> Self {
        if let Some(v) = spec.max_token_ttl {
            self.max_token_ttl = v;
        }
        if let Some(v) = spec.allowed_tool_scopes {
            self.allowed_tool_scopes = v;
        }
        if let Some(v) = spec.max_delegation_depth {
            self.max_delegation_depth = v;
        }
        if let Some(v) = spec.allowed_blast_radii {
            self.allowed_blast_radii = v;
        }
        if let Some(v) = spec.allow_subagent_spawn {
            self.allow_subagent_spawn = v;
        }
        if let Some(v) = spec.allow_meta_change {
            self.allow_meta_change = v;
        }
        if let Some(v) = spec.allow_code_change {
            self.allow_code_change = v;
        }
        if let Some(v) = spec.required_trust_level {
            self.required_trust_level = v;
        }
        self
    }
}

mod duration_secs {
    use std::time::Duration;

    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(d: &Duration, s: S) -> Result<S::Ok, S::Error> {
        d.as_secs().serialize(s)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Duration, D::Error> {
        u64::deserialize(d).map(Duration::from_secs)
    }
}

fn all_tool_scopes() -> HashSet<ToolScope> {
    [
        ToolScope::Read,
        ToolScope::Write,
        ToolScope::Execute,
        ToolScope::Network,
        ToolScope::Agent,
        ToolScope::Vcs,
        ToolScope::Admin,
    ]
    .into_iter()
    .collect()
}

fn all_blast_radii() -> HashSet<BlastRadius> {
    [
        BlastRadius::AgentMemory,
        BlastRadius::AgentPersonaMutable,
        BlastRadius::AgentSkill,
        BlastRadius::AgentExperiencePool,
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
        BlastRadius::RuntimeCrate,
        BlastRadius::GatewayCore,
        BlastRadius::ProtocolSchema,
        BlastRadius::DbMigration,
        BlastRadius::ConstitutionalRuleSet,
        BlastRadius::KillSwitchProtocol,
        BlastRadius::AuditLogBackend,
        BlastRadius::SelfEvolutionPipeline,
    ]
    .into_iter()
    .collect()
}

fn agent_blast_radii() -> HashSet<BlastRadius> {
    [
        BlastRadius::AgentMemory,
        BlastRadius::AgentPersonaMutable,
        BlastRadius::AgentSkill,
        BlastRadius::AgentExperiencePool,
    ]
    .into_iter()
    .collect()
}

/// Resolves the [`PrincipalPolicy`] for a given [`Principal`].
///
/// Implementations may layer kind defaults, harness-profile policies, or
/// per-id overrides; PR2 ships only the in-memory kind-default store.
#[async_trait]
pub trait PrincipalPolicyStore: Send + Sync + std::fmt::Debug {
    async fn resolve(&self, principal: &Principal) -> Arc<PrincipalPolicy>;
}

/// In-memory `PrincipalPolicyStore` seeded with per-kind defaults from
/// [`PrincipalPolicy::for_kind`]. Use [`Self::from_config`] to apply
/// overrides loaded from `sera-config`.
#[derive(Debug, Clone)]
pub struct InMemoryPrincipalPolicyStore {
    by_kind: HashMap<PrincipalKind, Arc<PrincipalPolicy>>,
}

const ALL_PRINCIPAL_KINDS: [PrincipalKind; 5] = [
    PrincipalKind::Human,
    PrincipalKind::Agent,
    PrincipalKind::ExternalAgent,
    PrincipalKind::Service,
    PrincipalKind::System,
];

impl InMemoryPrincipalPolicyStore {
    /// Seed the store with the per-kind defaults verbatim.
    pub fn new() -> Self {
        let mut by_kind = HashMap::with_capacity(ALL_PRINCIPAL_KINDS.len());
        for kind in ALL_PRINCIPAL_KINDS {
            by_kind.insert(kind, Arc::new(PrincipalPolicy::for_kind(kind)));
        }
        Self { by_kind }
    }

    /// Build a store from a config block: per-kind overrides are applied on
    /// top of the runtime defaults; kinds absent from the config keep the
    /// default verbatim.
    pub fn from_config(config: &PrincipalPoliciesConfig) -> Self {
        let mut store = Self::new();
        for (kind, spec) in &config.policies {
            let merged = PrincipalPolicy::for_kind(*kind).with_spec(spec.clone());
            store.by_kind.insert(*kind, Arc::new(merged));
        }
        store
    }

    /// Direct lookup by kind, falling back to the per-kind default if the
    /// store has been mutated to remove an entry. Used by the gateway
    /// resolution seam (sera-gateway::policy_resolution).
    pub fn policy_for(&self, kind: PrincipalKind) -> Arc<PrincipalPolicy> {
        self.by_kind
            .get(&kind)
            .cloned()
            .unwrap_or_else(|| Arc::new(PrincipalPolicy::for_kind(kind)))
    }
}

impl Default for InMemoryPrincipalPolicyStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl PrincipalPolicyStore for InMemoryPrincipalPolicyStore {
    async fn resolve(&self, principal: &Principal) -> Arc<PrincipalPolicy> {
        self.policy_for(principal.kind)
    }
}

/// Narrow a parent policy by intersection with a child policy for sub-agent
/// dispatch (design report §6.4 / §7.2).
///
/// Semantics:
/// - `allowed_tool_scopes` = parent ∩ child
/// - `allowed_blast_radii` = parent ∩ child
/// - `max_delegation_depth` = `min(parent.depth - 1, child.depth)` (saturating)
/// - `allow_*` flags are AND-reduced
/// - `required_trust_level` is the stricter of the two
/// - `max_token_ttl` is the shorter of the two
///
/// PR2 ships only the helper. PR3 (sera-u4gj) wires it into
/// `AgentToolRegistry` so sub-agent dispatch can never launder past the
/// caller's scope set.
pub fn narrow_for_subagent(
    parent: &PrincipalPolicy,
    child: &PrincipalPolicy,
) -> PrincipalPolicy {
    PrincipalPolicy {
        max_token_ttl: parent.max_token_ttl.min(child.max_token_ttl),
        allowed_tool_scopes: parent
            .allowed_tool_scopes
            .intersection(&child.allowed_tool_scopes)
            .copied()
            .collect(),
        max_delegation_depth: parent
            .max_delegation_depth
            .saturating_sub(1)
            .min(child.max_delegation_depth),
        allowed_blast_radii: parent
            .allowed_blast_radii
            .intersection(&child.allowed_blast_radii)
            .copied()
            .collect(),
        allow_subagent_spawn: parent.allow_subagent_spawn && child.allow_subagent_spawn,
        allow_meta_change: parent.allow_meta_change && child.allow_meta_change,
        allow_code_change: parent.allow_code_change && child.allow_code_change,
        required_trust_level: stricter_trust_level(
            parent.required_trust_level,
            child.required_trust_level,
        ),
    }
}

fn trust_strictness(level: TrustLevel) -> u8 {
    match level {
        TrustLevel::Unverified => 0,
        TrustLevel::VerifiedThirdParty => 1,
        TrustLevel::FirstParty => 2,
    }
}

/// Pick the stricter (numerically higher) of two trust requirements.
fn stricter_trust_level(a: TrustLevel, b: TrustLevel) -> TrustLevel {
    if trust_strictness(a) >= trust_strictness(b) {
        a
    } else {
        b
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Per-kind defaults ──────────────────────────────────────────────────

    #[test]
    fn default_human_policy_has_all_scopes_depth_5() {
        let p = PrincipalPolicy::for_kind(PrincipalKind::Human);
        assert_eq!(p.max_delegation_depth, 5);
        assert_eq!(p.allowed_tool_scopes, all_tool_scopes());
        assert!(p.allow_subagent_spawn);
        assert!(p.allow_meta_change);
        assert!(p.allow_code_change);
    }

    #[test]
    fn default_agent_policy_excludes_admin_depth_2() {
        let p = PrincipalPolicy::for_kind(PrincipalKind::Agent);
        assert_eq!(p.max_delegation_depth, 2);
        assert!(p.allowed_tool_scopes.contains(&ToolScope::Read));
        assert!(p.allowed_tool_scopes.contains(&ToolScope::Write));
        assert!(p.allowed_tool_scopes.contains(&ToolScope::Execute));
        assert!(p.allowed_tool_scopes.contains(&ToolScope::Network));
        assert!(p.allowed_tool_scopes.contains(&ToolScope::Vcs));
        assert!(p.allowed_tool_scopes.contains(&ToolScope::Agent));
        assert!(
            !p.allowed_tool_scopes.contains(&ToolScope::Admin),
            "Agent policy must never carry the Admin scope"
        );
        assert!(p.allow_subagent_spawn);
        assert!(!p.allow_meta_change);
        assert!(p.allow_code_change);
    }

    #[test]
    fn default_external_agent_policy_is_default_deny_depth_0() {
        let p = PrincipalPolicy::for_kind(PrincipalKind::ExternalAgent);
        assert_eq!(p.max_delegation_depth, 0);
        assert!(p.allowed_tool_scopes.is_empty());
        assert!(p.allowed_blast_radii.is_empty());
        assert!(!p.allow_subagent_spawn);
        assert!(!p.allow_meta_change);
        assert!(!p.allow_code_change);
    }

    #[test]
    fn default_service_policy_is_default_deny_depth_0() {
        let p = PrincipalPolicy::for_kind(PrincipalKind::Service);
        assert_eq!(p.max_delegation_depth, 0);
        assert!(p.allowed_tool_scopes.is_empty());
        assert!(p.allowed_blast_radii.is_empty());
        assert!(!p.allow_subagent_spawn);
        assert!(!p.allow_meta_change);
        assert!(!p.allow_code_change);
    }

    #[test]
    fn default_system_policy_has_all_scopes_depth_5() {
        let p = PrincipalPolicy::for_kind(PrincipalKind::System);
        assert_eq!(p.max_delegation_depth, 5);
        assert_eq!(p.allowed_tool_scopes, all_tool_scopes());
        assert!(p.allow_subagent_spawn);
        assert!(p.allow_meta_change);
        assert!(p.allow_code_change);
    }

    /// `required_trust_level` mirrors `PrincipalKind::default_trust_level` —
    /// `ExternalAgent` keeps the conservative `Unverified` mapping; every
    /// other kind defaults to `FirstParty`. The empty scope set on
    /// `ExternalAgent` / `Service` is the actual default-deny barrier.
    #[test]
    fn required_trust_level_defaults_follow_principal_kind() {
        for kind in ALL_PRINCIPAL_KINDS {
            let p = PrincipalPolicy::for_kind(kind);
            assert_eq!(
                p.required_trust_level,
                kind.default_trust_level(),
                "kind {kind:?} drifted from its default trust level",
            );
        }
        // Pin the explicit values so a future change to
        // `default_trust_level` does not silently widen this policy.
        assert_eq!(
            PrincipalPolicy::for_kind(PrincipalKind::ExternalAgent).required_trust_level,
            TrustLevel::Unverified,
        );
        assert_eq!(
            PrincipalPolicy::for_kind(PrincipalKind::Human).required_trust_level,
            TrustLevel::FirstParty,
        );
    }

    // ── Store ──────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn policy_store_resolves_by_principal_kind() {
        let store = InMemoryPrincipalPolicyStore::new();

        let admin = Principal::default_admin();
        let admin_policy = store.resolve(&admin).await;
        assert_eq!(admin_policy.max_delegation_depth, 5);
        assert!(admin_policy.allow_meta_change);

        let agent = Principal::for_agent("a-1", "a");
        let agent_policy = store.resolve(&agent).await;
        assert_eq!(agent_policy.max_delegation_depth, 2);
        assert!(!agent_policy.allow_meta_change);

        let ext = Principal::external_agent("a2a", "bot");
        let ext_policy = store.resolve(&ext).await;
        assert_eq!(ext_policy.max_delegation_depth, 0);
        assert!(ext_policy.allowed_tool_scopes.is_empty());

        let svc = Principal::service("connector");
        let svc_policy = store.resolve(&svc).await;
        assert_eq!(svc_policy.max_delegation_depth, 0);
        assert!(svc_policy.allowed_tool_scopes.is_empty());

        let sys = Principal::system();
        let sys_policy = store.resolve(&sys).await;
        assert_eq!(sys_policy.max_delegation_depth, 5);
        assert!(sys_policy.allow_meta_change);
    }

    /// Empty config produces the same effective policies as the
    /// kind-default store — the "absence = defaults" invariant.
    #[test]
    fn empty_config_matches_in_memory_defaults() {
        let from_default = InMemoryPrincipalPolicyStore::new();
        let from_empty =
            InMemoryPrincipalPolicyStore::from_config(&PrincipalPoliciesConfig::default());
        for kind in ALL_PRINCIPAL_KINDS {
            assert_eq!(
                from_default.policy_for(kind),
                from_empty.policy_for(kind),
                "default vs empty-config drift on kind {kind:?}",
            );
        }
    }

    /// A single field in a per-kind spec must override only that field.
    #[test]
    fn config_override_only_replaces_specified_fields() {
        let mut config = PrincipalPoliciesConfig::default();
        let spec = PrincipalPolicySpec {
            max_delegation_depth: Some(1),
            ..PrincipalPolicySpec::default()
        };
        config.policies.insert(PrincipalKind::Agent, spec);

        let store = InMemoryPrincipalPolicyStore::from_config(&config);
        let agent = store.policy_for(PrincipalKind::Agent);
        assert_eq!(agent.max_delegation_depth, 1);
        // Other fields remain at the kind default.
        assert!(agent.allowed_tool_scopes.contains(&ToolScope::Execute));
        assert!(!agent.allowed_tool_scopes.contains(&ToolScope::Admin));
        assert!(!agent.allow_meta_change);
    }

    // ── Narrowing ──────────────────────────────────────────────────────────

    #[test]
    fn policy_narrowing_intersects_scopes_and_decrements_depth() {
        let parent = PrincipalPolicy::for_kind(PrincipalKind::Human);
        let child = PrincipalPolicy::for_kind(PrincipalKind::Agent);
        let narrowed = narrow_for_subagent(&parent, &child);

        // Human (all) ∩ Agent (no Admin) = Agent's set.
        assert!(narrowed.allowed_tool_scopes.contains(&ToolScope::Read));
        assert!(narrowed.allowed_tool_scopes.contains(&ToolScope::Execute));
        assert!(!narrowed.allowed_tool_scopes.contains(&ToolScope::Admin));

        // Depth: min(parent.depth - 1, child.depth) = min(4, 2) = 2.
        assert_eq!(narrowed.max_delegation_depth, 2);

        // AND-reduced flags: meta is off (child=false).
        assert!(narrowed.allow_subagent_spawn);
        assert!(!narrowed.allow_meta_change);
        assert!(narrowed.allow_code_change);
    }

    /// Narrowing into an `ExternalAgent` child collapses to default-deny
    /// regardless of the parent's permissions — the laundering attack
    /// described in §7.2 of the design report.
    #[test]
    fn policy_narrowing_into_external_agent_is_default_deny() {
        let parent = PrincipalPolicy::for_kind(PrincipalKind::Human);
        let child = PrincipalPolicy::for_kind(PrincipalKind::ExternalAgent);
        let narrowed = narrow_for_subagent(&parent, &child);

        assert!(narrowed.allowed_tool_scopes.is_empty());
        assert_eq!(narrowed.max_delegation_depth, 0);
        assert!(!narrowed.allow_subagent_spawn);
        assert!(!narrowed.allow_meta_change);
        assert!(!narrowed.allow_code_change);
    }

    /// Depth saturates at zero so a parent at depth 0 cannot underflow into
    /// a permissive child.
    #[test]
    fn policy_narrowing_depth_saturates_at_zero() {
        let mut parent = PrincipalPolicy::for_kind(PrincipalKind::ExternalAgent);
        parent.allowed_tool_scopes.insert(ToolScope::Read);
        let mut child = PrincipalPolicy::for_kind(PrincipalKind::Human);
        child.max_delegation_depth = 5;
        let narrowed = narrow_for_subagent(&parent, &child);

        assert_eq!(narrowed.max_delegation_depth, 0);
    }

    /// The narrowed policy keeps the stricter trust requirement.
    #[test]
    fn policy_narrowing_picks_stricter_trust_level() {
        let mut parent = PrincipalPolicy::for_kind(PrincipalKind::Agent);
        parent.required_trust_level = TrustLevel::Unverified;
        let mut child = PrincipalPolicy::for_kind(PrincipalKind::Agent);
        child.required_trust_level = TrustLevel::FirstParty;
        let narrowed = narrow_for_subagent(&parent, &child);
        assert_eq!(narrowed.required_trust_level, TrustLevel::FirstParty);

        let mut parent2 = PrincipalPolicy::for_kind(PrincipalKind::Agent);
        parent2.required_trust_level = TrustLevel::FirstParty;
        let mut child2 = PrincipalPolicy::for_kind(PrincipalKind::Agent);
        child2.required_trust_level = TrustLevel::Unverified;
        let narrowed2 = narrow_for_subagent(&parent2, &child2);
        assert_eq!(narrowed2.required_trust_level, TrustLevel::FirstParty);
    }

    // ── Serde roundtrip ────────────────────────────────────────────────────

    #[test]
    fn principal_policy_json_roundtrip() {
        let p = PrincipalPolicy::for_kind(PrincipalKind::Agent);
        let json = serde_json::to_string(&p).unwrap();
        let parsed: PrincipalPolicy = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, p);
    }
}
