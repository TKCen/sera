//! Principal model — identity for any acting entity in SERA.
//!
//! MVS simplification: no groups, no external agents (per mvs-review-plan §6.5).
//! Every acting entity (human operator, agent, system) is a Principal.
//! This enables uniform audit trails and authorization checks.

use serde::{Deserialize, Serialize};

/// The kind of principal acting in the system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrincipalKind {
    /// Human operator interacting via CLI, TUI, or Web UI.
    Human,
    /// An agent instance running in a container or in-process.
    Agent,
    /// The SERA system itself (for automated actions).
    System,
}

/// A unique identifier for a principal.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PrincipalId(pub String);

impl PrincipalId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
}

impl std::fmt::Display for PrincipalId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A principal — any acting entity in SERA.
///
/// MVS scope: simplified model without groups or external agent identity.
/// All principals have full access in autonomous mode (Tier 1).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Principal {
    pub id: PrincipalId,
    pub kind: PrincipalKind,
    /// Display name for the principal.
    pub name: String,
    /// External identity mapping (e.g., Discord user ID → principal).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_id: Option<String>,
    /// Platform source of the external identity (e.g., "discord").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub platform: Option<String>,
}

impl Principal {
    /// Create the default admin principal for autonomous mode (Tier 1).
    /// Auto-created on first gateway start per MVS §6.5.
    pub fn default_admin() -> Self {
        Self {
            id: PrincipalId::new("admin"),
            kind: PrincipalKind::Human,
            name: "admin".to_string(),
            external_id: None,
            platform: None,
        }
    }

    /// Create a principal from a Discord user, auto-mapping by Discord user ID.
    pub fn from_discord(discord_user_id: &str, username: &str) -> Self {
        Self {
            id: PrincipalId::new(format!("discord:{discord_user_id}")),
            kind: PrincipalKind::Human,
            name: username.to_string(),
            external_id: Some(discord_user_id.to_string()),
            platform: Some("discord".to_string()),
        }
    }

    /// Create a principal for an agent instance.
    pub fn for_agent(agent_id: &str, agent_name: &str) -> Self {
        Self {
            id: PrincipalId::new(format!("agent:{agent_id}")),
            kind: PrincipalKind::Agent,
            name: agent_name.to_string(),
            external_id: None,
            platform: None,
        }
    }

    /// The system principal for automated actions (cron, lifecycle, etc.).
    pub fn system() -> Self {
        Self {
            id: PrincipalId::new("system"),
            kind: PrincipalKind::System,
            name: "system".to_string(),
            external_id: None,
            platform: None,
        }
    }

    /// A reference to this principal for embedding in events and audit entries.
    pub fn as_ref(&self) -> PrincipalRef {
        PrincipalRef {
            id: self.id.clone(),
            kind: self.kind,
        }
    }
}

/// Lightweight reference to a principal, embedded in events and audit records.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrincipalRef {
    pub id: PrincipalId,
    pub kind: PrincipalKind,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_admin() {
        let admin = Principal::default_admin();
        assert_eq!(admin.id.0, "admin");
        assert_eq!(admin.kind, PrincipalKind::Human);
    }

    #[test]
    fn discord_principal() {
        let p = Principal::from_discord("123456789", "testuser");
        assert_eq!(p.id.0, "discord:123456789");
        assert_eq!(p.kind, PrincipalKind::Human);
        assert_eq!(p.external_id.as_deref(), Some("123456789"));
        assert_eq!(p.platform.as_deref(), Some("discord"));
    }

    #[test]
    fn agent_principal() {
        let p = Principal::for_agent("agent-1", "sera");
        assert_eq!(p.id.0, "agent:agent-1");
        assert_eq!(p.kind, PrincipalKind::Agent);
    }

    #[test]
    fn system_principal() {
        let p = Principal::system();
        assert_eq!(p.id.0, "system");
        assert_eq!(p.kind, PrincipalKind::System);
    }

    #[test]
    fn principal_ref() {
        let p = Principal::default_admin();
        let r = p.as_ref();
        assert_eq!(r.id, p.id);
        assert_eq!(r.kind, p.kind);
    }

    #[test]
    fn principal_kind_serde() {
        let json = serde_json::to_string(&PrincipalKind::Human).unwrap();
        assert_eq!(json, "\"human\"");

        let parsed: PrincipalKind = serde_json::from_str("\"agent\"").unwrap();
        assert_eq!(parsed, PrincipalKind::Agent);
    }

    #[test]
    fn principal_roundtrip() {
        let p = Principal::from_discord("999", "user");
        let json = serde_json::to_string(&p).unwrap();
        let parsed: Principal = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, p.id);
        assert_eq!(parsed.name, "user");
    }
}
