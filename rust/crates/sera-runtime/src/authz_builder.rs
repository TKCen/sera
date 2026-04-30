//! sera-ve9x: shared `AuthzProviderHandle` construction for the runtime
//! binary and the gateway's embedded dispatch path.
//!
//! Lifted from `sera-runtime/src/main.rs` so both the standalone runtime
//! binary and `EmbeddedRuntimeTransport` (in `sera-gateway`) build the same
//! provider from the same `RuntimeConfig` without duplicating the env-var
//! plumbing or the role-clause parser.

use std::sync::Arc;

use sera_auth::authz::{ActionKind, AuthzProviderAdapter, RoleBasedAuthzProvider};
use sera_types::principal::PrincipalId;
use sera_types::tool::AuthzProviderHandle;

use crate::config::RuntimeConfig;

/// Build an [`AuthzProviderHandle`] from `config`.
///
/// When `config.tool_authz_roles` is set, parses a compact role definition
/// string and constructs a [`RoleBasedAuthzProvider`] wrapped in an
/// [`AuthzProviderAdapter`]. Format:
///
/// ```text
/// <role>:<kind>[,<kind>...][;<role>:...]
/// ```
///
/// Supported `<kind>` names (case-insensitive): `read`, `write`, `execute`,
/// `admin`, `tool_call`, `session_op`, `memory_access`, `config_change`,
/// `propose_change`, `approve_change`.
///
/// When `tool_authz_roles` is `None`, returns the allow-all default stub.
pub fn build_provider_from_config(config: &RuntimeConfig) -> Arc<dyn AuthzProviderHandle> {
    let Some(roles_str) = &config.tool_authz_roles else {
        return Arc::new(sera_types::tool::DefaultAuthzProviderStub);
    };

    let mut builder = RoleBasedAuthzProvider::builder();

    for role_clause in roles_str.split(';') {
        let role_clause = role_clause.trim();
        if role_clause.is_empty() {
            continue;
        }
        let Some((role, kinds_str)) = role_clause.split_once(':') else {
            tracing::warn!(
                "TOOL_AUTHZ_ROLES: skipping malformed clause (no ':'): {role_clause}"
            );
            continue;
        };
        let role = role.trim();
        let kinds: Vec<ActionKind> = kinds_str
            .split(',')
            .filter_map(|k| parse_action_kind(k.trim()))
            .collect();
        builder = builder.grant(role, kinds);
    }

    // Assign the runtime's own agent-id as a full-access principal so the
    // default single-agent deployment works without additional config.
    if !config.agent_id.is_empty() {
        builder = builder.assign(
            PrincipalId::new(format!("agent:{}", config.agent_id)),
            ["operator"],
        );
    }

    Arc::new(AuthzProviderAdapter::new(builder.build()))
}

/// Parse a single action-kind name (case-insensitive).
pub fn parse_action_kind(s: &str) -> Option<ActionKind> {
    match s.to_ascii_lowercase().as_str() {
        "read" => Some(ActionKind::Read),
        "write" => Some(ActionKind::Write),
        "execute" => Some(ActionKind::Execute),
        "admin" => Some(ActionKind::Admin),
        "tool_call" => Some(ActionKind::ToolCall),
        "session_op" => Some(ActionKind::SessionOp),
        "memory_access" => Some(ActionKind::MemoryAccess),
        "config_change" => Some(ActionKind::ConfigChange),
        "propose_change" => Some(ActionKind::ProposeChange),
        "approve_change" => Some(ActionKind::ApproveChange),
        other => {
            tracing::warn!("TOOL_AUTHZ_ROLES: unknown action kind '{other}', skipping");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg_with_roles(roles: Option<&str>) -> RuntimeConfig {
        let mut c = RuntimeConfig::from_env();
        c.tool_authz_roles = roles.map(|s| s.to_string());
        c.agent_id = "test-agent".to_string();
        c
    }

    #[test]
    fn no_roles_yields_allow_all_stub() {
        let cfg = cfg_with_roles(None);
        let _provider = build_provider_from_config(&cfg);
        // Constructed without panicking — coarse sanity check.
    }

    #[test]
    fn role_clause_parses_without_panic() {
        let cfg =
            cfg_with_roles(Some("operator:tool_call,read;admin:tool_call,read,write,admin"));
        let _provider = build_provider_from_config(&cfg);
    }

    #[test]
    fn parse_action_kind_known_variants() {
        assert!(matches!(parse_action_kind("read"), Some(ActionKind::Read)));
        assert!(matches!(
            parse_action_kind("WRITE"),
            Some(ActionKind::Write)
        ));
        assert!(matches!(
            parse_action_kind("tool_call"),
            Some(ActionKind::ToolCall)
        ));
        assert!(parse_action_kind("not_a_kind").is_none());
    }
}
