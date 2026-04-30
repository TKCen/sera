//! Principal model — identity for any acting entity in SERA.
//!
//! MVS simplification: no groups, no external agents (per mvs-review-plan §6.5).
//! Every acting entity (human operator, agent, system) is a Principal.
//! This enables uniform audit trails and authorization checks.

use serde::{Deserialize, Serialize};

/// The kind of principal acting in the system.
/// SPEC-identity-authz: any acting entity is a first-class Principal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrincipalKind {
    /// Human operator interacting via CLI, TUI, or Web UI.
    Human,
    /// An agent instance running in a container or in-process.
    Agent,
    /// An external agent connecting via A2A or ACP protocol.
    ExternalAgent,
    /// A service identity (API integrations, webhooks, connectors).
    Service,
    /// The SERA system itself (for automated actions).
    System,
}

impl PrincipalKind {
    /// Snake-case path segment used in WIMSE/SPIFFE URI projection.
    /// Mirrors the serde `rename_all = "snake_case"` representation.
    pub fn as_path_segment(&self) -> &'static str {
        match self {
            PrincipalKind::Human => "human",
            PrincipalKind::Agent => "agent",
            PrincipalKind::ExternalAgent => "external_agent",
            PrincipalKind::Service => "service",
            PrincipalKind::System => "system",
        }
    }

    /// Default trust level for principals of this kind.
    /// External agents start unverified; everything else is first-party in Tier-1.
    pub fn default_trust_level(&self) -> TrustLevel {
        match self {
            PrincipalKind::ExternalAgent => TrustLevel::Unverified,
            _ => TrustLevel::FirstParty,
        }
    }
}

/// Trust level pinned to a principal at registration.
/// Mirrors ZeroID's three-level enum (see SPEC-identity-authz §10 open question 3
/// and the zeroid-agent-identity-scout-2026-04-30 report §2.10).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrustLevel {
    /// SERA-owned or operator-pinned identity.
    FirstParty,
    /// External harness or service that has been verified by an operator.
    VerifiedThirdParty,
    /// External or community-supplied identity with no verification.
    Unverified,
}

impl Default for TrustLevel {
    fn default() -> Self {
        Self::FirstParty
    }
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

/// Tier-1 / pet-mode default account id used in WIMSE URI projection.
pub const WIMSE_DEFAULT_ACCOUNT_ID: &str = "local";
/// Tier-1 / pet-mode default project id used in WIMSE URI projection.
pub const WIMSE_DEFAULT_PROJECT_ID: &str = "default";

/// Percent-encode a single path segment for use in a SPIFFE/WIMSE URI.
///
/// Conservative encoding: characters in the RFC 3986 `unreserved` set
/// (`A-Z a-z 0-9 - . _ ~`) pass through unchanged; every other byte is
/// percent-encoded as `%XX` with uppercase hex. This is deterministic and
/// round-trip-safe for common SERA principal ids like `discord:123` or
/// `ext:a2a:reviewer-bot`, which contain `:` (a reserved sub-delim).
fn wimse_escape_segment(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        let is_unreserved = byte.is_ascii_alphanumeric()
            || matches!(byte, b'-' | b'.' | b'_' | b'~');
        if is_unreserved {
            out.push(byte as char);
        } else {
            out.push('%');
            out.push_str(&format!("{:02X}", byte));
        }
    }
    out
}

/// Build a deterministic WIMSE/SPIFFE URI projection from principal coordinates.
///
/// Form: `spiffe://{domain}/{account}/{project}/{kind}/{id}` where `id` is
/// percent-encoded per `wimse_escape_segment`.
fn build_wimse_uri(
    domain: &str,
    account_id: &str,
    project_id: &str,
    kind: PrincipalKind,
    id: &PrincipalId,
) -> String {
    format!(
        "spiffe://{domain}/{account}/{project}/{kind}/{id}",
        domain = domain,
        account = account_id,
        project = project_id,
        kind = kind.as_path_segment(),
        id = wimse_escape_segment(&id.0),
    )
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
    /// Trust level pinned at registration; defaults to `FirstParty` for
    /// backward-compatible deserialization of records persisted before this
    /// field existed.
    #[serde(default)]
    pub trust_level: TrustLevel,
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
            trust_level: PrincipalKind::Human.default_trust_level(),
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
            trust_level: PrincipalKind::Human.default_trust_level(),
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
            trust_level: PrincipalKind::Agent.default_trust_level(),
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
            trust_level: PrincipalKind::System.default_trust_level(),
        }
    }

    /// Create a principal for an external agent (A2A/ACP protocol).
    pub fn external_agent(protocol: &str, agent_name: &str) -> Self {
        Self {
            id: PrincipalId::new(format!("ext:{protocol}:{agent_name}")),
            kind: PrincipalKind::ExternalAgent,
            name: agent_name.to_string(),
            external_id: None,
            platform: Some(protocol.to_string()),
            trust_level: PrincipalKind::ExternalAgent.default_trust_level(),
        }
    }

    /// Create a principal for a service identity.
    pub fn service(service_name: &str) -> Self {
        Self {
            id: PrincipalId::new(format!("svc:{service_name}")),
            kind: PrincipalKind::Service,
            name: service_name.to_string(),
            external_id: None,
            platform: None,
            trust_level: PrincipalKind::Service.default_trust_level(),
        }
    }

    /// A reference to this principal for embedding in events and audit entries.
    pub fn as_ref(&self) -> PrincipalRef {
        PrincipalRef {
            id: self.id.clone(),
            kind: self.kind,
        }
    }

    /// Project this principal to a deterministic WIMSE/SPIFFE URI using
    /// Tier-1 / pet-mode defaults (`account="local"`, `project="default"`).
    /// `Principal::id` remains the source of truth; the URI is a derived
    /// projection for cross-system audit and federation.
    pub fn wimse_uri(&self, domain: &str) -> String {
        build_wimse_uri(
            domain,
            WIMSE_DEFAULT_ACCOUNT_ID,
            WIMSE_DEFAULT_PROJECT_ID,
            self.kind,
            &self.id,
        )
    }

    /// Variant of `wimse_uri` allowing explicit account/project coordinates
    /// (enterprise tier; not used by Tier-1).
    pub fn wimse_uri_for(&self, domain: &str, account_id: &str, project_id: &str) -> String {
        build_wimse_uri(domain, account_id, project_id, self.kind, &self.id)
    }
}

/// Lightweight reference to a principal, embedded in events and audit records.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PrincipalRef {
    pub id: PrincipalId,
    pub kind: PrincipalKind,
}

impl PrincipalRef {
    /// Project this reference to a deterministic WIMSE/SPIFFE URI using
    /// Tier-1 / pet-mode defaults.
    pub fn wimse_uri(&self, domain: &str) -> String {
        build_wimse_uri(
            domain,
            WIMSE_DEFAULT_ACCOUNT_ID,
            WIMSE_DEFAULT_PROJECT_ID,
            self.kind,
            &self.id,
        )
    }

    /// Variant allowing explicit account/project coordinates.
    pub fn wimse_uri_for(&self, domain: &str, account_id: &str, project_id: &str) -> String {
        build_wimse_uri(domain, account_id, project_id, self.kind, &self.id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_admin() {
        let admin = Principal::default_admin();
        assert_eq!(admin.id.0, "admin");
        assert_eq!(admin.kind, PrincipalKind::Human);
        assert_eq!(admin.trust_level, TrustLevel::FirstParty);
    }

    #[test]
    fn discord_principal() {
        let p = Principal::from_discord("123456789", "testuser");
        assert_eq!(p.id.0, "discord:123456789");
        assert_eq!(p.kind, PrincipalKind::Human);
        assert_eq!(p.external_id.as_deref(), Some("123456789"));
        assert_eq!(p.platform.as_deref(), Some("discord"));
        assert_eq!(p.trust_level, TrustLevel::FirstParty);
    }

    #[test]
    fn agent_principal() {
        let p = Principal::for_agent("agent-1", "sera");
        assert_eq!(p.id.0, "agent:agent-1");
        assert_eq!(p.kind, PrincipalKind::Agent);
        assert_eq!(p.trust_level, TrustLevel::FirstParty);
    }

    #[test]
    fn system_principal() {
        let p = Principal::system();
        assert_eq!(p.id.0, "system");
        assert_eq!(p.kind, PrincipalKind::System);
        assert_eq!(p.trust_level, TrustLevel::FirstParty);
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
    fn principal_kind_all_variants_serde() {
        let variants = vec![
            (PrincipalKind::Human, "human"),
            (PrincipalKind::Agent, "agent"),
            (PrincipalKind::ExternalAgent, "external_agent"),
            (PrincipalKind::Service, "service"),
            (PrincipalKind::System, "system"),
        ];
        for (kind, expected) in variants {
            let json = serde_json::to_string(&kind).unwrap();
            assert_eq!(json, format!("\"{expected}\""));
            let parsed: PrincipalKind = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, kind);
        }
    }

    #[test]
    fn external_agent_principal() {
        let p = Principal::external_agent("a2a", "reviewer-bot");
        assert_eq!(p.id.0, "ext:a2a:reviewer-bot");
        assert_eq!(p.kind, PrincipalKind::ExternalAgent);
        assert_eq!(p.platform.as_deref(), Some("a2a"));
        // External agents default to Unverified, not FirstParty.
        assert_eq!(p.trust_level, TrustLevel::Unverified);
    }

    #[test]
    fn service_principal() {
        let p = Principal::service("discord-connector");
        assert_eq!(p.id.0, "svc:discord-connector");
        assert_eq!(p.kind, PrincipalKind::Service);
        assert_eq!(p.trust_level, TrustLevel::FirstParty);
    }

    #[test]
    fn principal_roundtrip() {
        let p = Principal::from_discord("999", "user");
        let json = serde_json::to_string(&p).unwrap();
        let parsed: Principal = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, p.id);
        assert_eq!(parsed.name, "user");
        assert_eq!(parsed.trust_level, TrustLevel::FirstParty);
    }

    // ── Trust-level + kind defaults ─────────────────────────────────────────

    #[test]
    fn trust_level_default_is_first_party() {
        assert_eq!(TrustLevel::default(), TrustLevel::FirstParty);
    }

    #[test]
    fn principal_kind_default_trust_level() {
        assert_eq!(PrincipalKind::Human.default_trust_level(), TrustLevel::FirstParty);
        assert_eq!(PrincipalKind::Agent.default_trust_level(), TrustLevel::FirstParty);
        assert_eq!(PrincipalKind::Service.default_trust_level(), TrustLevel::FirstParty);
        assert_eq!(PrincipalKind::System.default_trust_level(), TrustLevel::FirstParty);
        assert_eq!(
            PrincipalKind::ExternalAgent.default_trust_level(),
            TrustLevel::Unverified,
        );
    }

    #[test]
    fn trust_level_serde_snake_case() {
        let cases = [
            (TrustLevel::FirstParty, "\"first_party\""),
            (TrustLevel::VerifiedThirdParty, "\"verified_third_party\""),
            (TrustLevel::Unverified, "\"unverified\""),
        ];
        for (level, expected) in cases {
            let json = serde_json::to_string(&level).unwrap();
            assert_eq!(json, expected);
            let parsed: TrustLevel = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, level);
        }
    }

    /// Pre-`trust_level` payloads must still deserialize so existing
    /// persisted Principal records keep working after upgrade.
    #[test]
    fn principal_serde_backward_compatible_without_trust_level() {
        let legacy = r#"{
            "id": "admin",
            "kind": "human",
            "name": "admin"
        }"#;
        let parsed: Principal = serde_json::from_str(legacy).unwrap();
        assert_eq!(parsed.id.0, "admin");
        assert_eq!(parsed.kind, PrincipalKind::Human);
        // Missing field defaults to FirstParty per `#[serde(default)]`.
        assert_eq!(parsed.trust_level, TrustLevel::FirstParty);
    }

    #[test]
    fn principal_serde_accepts_explicit_trust_level() {
        let payload = r#"{
            "id": "ext:a2a:bot",
            "kind": "external_agent",
            "name": "bot",
            "platform": "a2a",
            "trust_level": "unverified"
        }"#;
        let parsed: Principal = serde_json::from_str(payload).unwrap();
        assert_eq!(parsed.kind, PrincipalKind::ExternalAgent);
        assert_eq!(parsed.trust_level, TrustLevel::Unverified);
    }

    // ── WIMSE URI projection ────────────────────────────────────────────────

    #[test]
    fn wimse_uri_default_admin() {
        let p = Principal::default_admin();
        assert_eq!(
            p.wimse_uri("sera.local"),
            "spiffe://sera.local/local/default/human/admin"
        );
    }

    #[test]
    fn wimse_uri_is_deterministic() {
        let p = Principal::default_admin();
        let a = p.wimse_uri("sera.local");
        let b = p.wimse_uri("sera.local");
        assert_eq!(a, b, "WIMSE URI must be deterministic for identical input");
    }

    #[test]
    fn wimse_uri_uses_kind_path_segment() {
        let cases: &[(Principal, &str)] = &[
            (Principal::default_admin(), "human"),
            (Principal::for_agent("a-1", "a"), "agent"),
            (Principal::external_agent("a2a", "bot"), "external_agent"),
            (Principal::service("svc"), "service"),
            (Principal::system(), "system"),
        ];
        for (p, expected_kind) in cases {
            let uri = p.wimse_uri("sera.local");
            let segment = format!("/{expected_kind}/");
            assert!(
                uri.contains(&segment),
                "URI {uri} should contain kind segment {segment}",
            );
        }
    }

    /// `Principal::id` carries `:` separators (`discord:123`, `ext:a2a:bar`).
    /// `:` is a reserved sub-delim in RFC 3986; we percent-encode it for a
    /// conservative, deterministic SPIFFE-URI shape. This test pins that
    /// choice — adjust intentionally if encoding policy ever changes.
    #[test]
    fn wimse_uri_percent_encodes_reserved_chars() {
        let p = Principal::from_discord("123", "user");
        assert_eq!(
            p.wimse_uri("sera.local"),
            "spiffe://sera.local/local/default/human/discord%3A123",
        );
        let ext = Principal::external_agent("a2a", "reviewer-bot");
        assert_eq!(
            ext.wimse_uri("sera.local"),
            "spiffe://sera.local/local/default/external_agent/ext%3Aa2a%3Areviewer-bot",
        );
    }

    #[test]
    fn wimse_uri_explicit_account_project() {
        let p = Principal::for_agent("a-1", "a");
        assert_eq!(
            p.wimse_uri_for("sera.example.com", "acct-7", "proj-9"),
            "spiffe://sera.example.com/acct-7/proj-9/agent/agent%3Aa-1",
        );
    }

    #[test]
    fn principal_ref_wimse_uri_matches_principal() {
        let p = Principal::for_agent("a-1", "a");
        let r = p.as_ref();
        assert_eq!(r.wimse_uri("sera.local"), p.wimse_uri("sera.local"));
    }

    #[test]
    fn wimse_escape_unreserved_passthrough() {
        // unreserved set: A-Z a-z 0-9 - . _ ~
        let s = "Abc-123._~";
        assert_eq!(wimse_escape_segment(s), s);
    }

    #[test]
    fn wimse_escape_reserved_encoded() {
        assert_eq!(wimse_escape_segment("a:b"), "a%3Ab");
        assert_eq!(wimse_escape_segment("a/b"), "a%2Fb");
        assert_eq!(wimse_escape_segment("a b"), "a%20b");
    }
}
