//! Wire format for `sera.auth.principal_policies` (sera-fhcr).
//!
//! `sera-config` deliberately stays free of any `sera-auth` dependency, so
//! the runtime [`PrincipalPolicy`] type lives in `sera-auth::policy`. This
//! module exposes the on-disk shadow ([`PrincipalPolicySpec`]) plus the
//! top-level config block ([`PrincipalPoliciesConfig`]) and converts on
//! the auth side via `PrincipalPolicy::with_spec`.
//!
//! Every spec field is optional. An absent field falls back to the per-kind
//! default produced by `PrincipalPolicy::for_kind`, so an empty config
//! produces the same effective policies as the in-memory store's defaults.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use sera_types::evolution::BlastRadius;
use sera_types::principal::{PrincipalKind, TrustLevel};
use sera_types::tool::ToolScope;

/// All-optional override of a runtime `PrincipalPolicy`. Only fields that
/// are explicitly set in YAML take effect; everything else falls back to
/// the per-kind default in `sera-auth`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PrincipalPolicySpec {
    #[serde(
        default,
        with = "duration_secs_opt",
        skip_serializing_if = "Option::is_none",
        rename = "max_token_ttl_secs"
    )]
    pub max_token_ttl: Option<Duration>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed_tool_scopes: Option<HashSet<ToolScope>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_delegation_depth: Option<u32>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed_blast_radii: Option<HashSet<BlastRadius>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow_subagent_spawn: Option<bool>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow_meta_change: Option<bool>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow_code_change: Option<bool>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required_trust_level: Option<TrustLevel>,
}

/// Top-level block under `sera.auth.principal_policies`.
///
/// The map is keyed by [`PrincipalKind`] (snake_case in YAML). Kinds absent
/// from the map use the runtime per-kind defaults verbatim.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PrincipalPoliciesConfig {
    #[serde(default)]
    pub policies: HashMap<PrincipalKind, PrincipalPolicySpec>,
}

impl PrincipalPoliciesConfig {
    pub fn is_empty(&self) -> bool {
        self.policies.is_empty()
    }
}

mod duration_secs_opt {
    use std::time::Duration;

    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(d: &Option<Duration>, s: S) -> Result<S::Ok, S::Error> {
        match d {
            Some(d) => d.as_secs().serialize(s),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Duration>, D::Error> {
        Option::<u64>::deserialize(d).map(|o| o.map(Duration::from_secs))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// sera-fhcr — a fully populated YAML block must roundtrip through
    /// `PrincipalPoliciesConfig` so operators can authoritatively pin every
    /// field of a per-kind policy.
    #[test]
    fn principal_policies_yaml_roundtrip_full() {
        let yaml = r#"
policies:
  human:
    max_token_ttl_secs: 7200
    allowed_tool_scopes: [read, write, execute, network, agent, vcs, admin]
    max_delegation_depth: 5
    allowed_blast_radii: [agent_memory, agent_skill]
    allow_subagent_spawn: true
    allow_meta_change: true
    allow_code_change: true
    required_trust_level: first_party
  external_agent:
    allowed_tool_scopes: []
    max_delegation_depth: 0
    allow_subagent_spawn: false
    allow_meta_change: false
    allow_code_change: false
    required_trust_level: unverified
"#;
        let parsed: PrincipalPoliciesConfig = serde_yaml::from_str(yaml).unwrap();
        let human = parsed.policies.get(&PrincipalKind::Human).unwrap();
        assert_eq!(human.max_token_ttl, Some(Duration::from_secs(7200)));
        assert_eq!(human.max_delegation_depth, Some(5));
        let scopes = human.allowed_tool_scopes.as_ref().unwrap();
        assert!(scopes.contains(&ToolScope::Admin));
        assert!(scopes.contains(&ToolScope::Read));
        assert_eq!(human.required_trust_level, Some(TrustLevel::FirstParty));

        let ext = parsed.policies.get(&PrincipalKind::ExternalAgent).unwrap();
        assert_eq!(ext.max_delegation_depth, Some(0));
        assert_eq!(ext.allow_subagent_spawn, Some(false));
        assert_eq!(ext.required_trust_level, Some(TrustLevel::Unverified));
        assert!(ext.allowed_tool_scopes.as_ref().unwrap().is_empty());

        // Round back to YAML and parse again to confirm symmetry.
        let yaml2 = serde_yaml::to_string(&parsed).unwrap();
        let parsed2: PrincipalPoliciesConfig = serde_yaml::from_str(&yaml2).unwrap();
        assert_eq!(parsed, parsed2);
    }

    /// sera-fhcr — partial overrides must leave omitted fields as `None`,
    /// signalling "fall back to the runtime default" to `with_spec`.
    #[test]
    fn principal_policies_yaml_partial_override() {
        let yaml = r#"
policies:
  agent:
    max_delegation_depth: 1
"#;
        let parsed: PrincipalPoliciesConfig = serde_yaml::from_str(yaml).unwrap();
        let agent = parsed.policies.get(&PrincipalKind::Agent).unwrap();
        assert_eq!(agent.max_delegation_depth, Some(1));
        assert!(agent.allowed_tool_scopes.is_none());
        assert!(agent.allow_meta_change.is_none());
    }

    /// sera-fhcr — an absent `principal_policies` block (the common case)
    /// deserializes to an empty map, and downstream callers must produce
    /// the same effective policies as `InMemoryPrincipalPolicyStore::new()`.
    #[test]
    fn principal_policies_yaml_absent_is_empty() {
        let yaml = "policies: {}";
        let parsed: PrincipalPoliciesConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(parsed.is_empty());

        // The default impl matches the empty-map shape too.
        assert_eq!(parsed, PrincipalPoliciesConfig::default());
    }

    /// Unknown keys must fail closed so a typo never silently disables a
    /// policy override (e.g. `max_delegation_dept` → 0 by default).
    #[test]
    fn principal_policies_yaml_rejects_unknown_fields() {
        let yaml = r#"
policies:
  agent:
    max_delegation_dept: 1
"#;
        let res: Result<PrincipalPoliciesConfig, _> = serde_yaml::from_str(yaml);
        assert!(res.is_err());
    }
}
