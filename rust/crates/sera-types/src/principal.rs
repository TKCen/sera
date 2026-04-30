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
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrustLevel {
    /// SERA-owned or operator-pinned identity.
    #[default]
    FirstParty,
    /// External harness or service that has been verified by an operator.
    VerifiedThirdParty,
    /// External or community-supplied identity with no verification.
    Unverified,
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

/// Inject a single path segment into a SPIFFE/WIMSE URI using a
/// collision-free, SPIFFE-safe byte-hex encoding.
///
/// Each UTF-8 byte of the input is emitted as `_HH` (a literal underscore
/// followed by two uppercase hex digits). Empty input is encoded as the
/// sentinel `_empty`. The output set is `{_, 0-9, A-F}` — entirely within
/// the SPIFFE-ID path-component allow-list (`SPIFFE-ID.md`: ASCII letters,
/// digits, `.`, `-`, `_`; no percent-encoding; no empty / `.` / `..`
/// segments).
///
/// Injectivity. Every non-empty input encodes to a string that is exactly
/// `3 * len` bytes long whose every triplet is `_HH` with `H ∈ [0-9A-F]`.
/// `_empty` is 6 bytes whose `_` is followed by `em` — `m` is not a hex
/// digit, so `_empty` cannot be the encoded form of any non-empty input.
/// Therefore the function is injective over all `&str` inputs, including
/// `""`, `"_empty"` (which encodes to `_5F_65_6D_70_74_79`), and any
/// inputs differing only in disallowed bytes (`a:b` → `_61_3A_62`,
/// `a/b` → `_61_2F_62`).
///
/// The transform is intentionally non-invertible from a *meaning*
/// perspective — `Principal::id` remains the source of truth. The WIMSE
/// URI is a derived projection for cross-system audit and federation;
/// the byte-level injectivity above is what guarantees no two distinct
/// principals collapse to the same SPIFFE ID.
fn wimse_escape_segment(input: &str) -> String {
    if input.is_empty() {
        return "_empty".to_string();
    }
    let mut out = String::with_capacity(input.len() * 3);
    for byte in input.bytes() {
        out.push('_');
        out.push_str(&format!("{byte:02X}"));
    }
    out
}

/// Sanitize the trust-domain component of a SPIFFE/WIMSE URI.
///
/// SPIFFE-ID restricts the trust domain to lowercase ASCII letters, digits,
/// dot (`.`), dash (`-`), and underscore (`_`); ports, path characters,
/// uppercase letters, and empty values are forbidden. Operators set the
/// domain via `SERA_WIMSE_DOMAIN` (see `sera-config::AuthConfig`), so this
/// helper guarantees the projection emits a SPIFFE-conformant URI even
/// when the configured value is invalid (for example `example.com:443`,
/// `EXAMPLE.COM`, `bad/path`, or empty). Uppercase ASCII is lowercased,
/// any other disallowed byte is replaced with `_`, and an empty result
/// becomes a single `_`.
fn wimse_sanitize_domain(input: &str) -> String {
    let mut out = String::with_capacity(input.len().max(1));
    for byte in input.bytes() {
        let lower = byte.to_ascii_lowercase();
        let is_safe = matches!(lower, b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_');
        out.push(if is_safe { lower as char } else { '_' });
    }
    if out.is_empty() {
        "_".to_string()
    } else {
        out
    }
}

/// Build a deterministic WIMSE/SPIFFE URI projection from principal coordinates.
///
/// Form: `spiffe://{domain}/{account}/{project}/{kind}/{id}`. The trust
/// domain is sanitized via `wimse_sanitize_domain`; every path segment
/// except `kind` (which is a fixed snake_case keyword) is sanitized via
/// `wimse_escape_segment`. The resulting URI always satisfies the
/// SPIFFE-ID character allow-list, even when callers pass invalid
/// `domain`, `account_id`, or `project_id` values.
fn build_wimse_uri(
    domain: &str,
    account_id: &str,
    project_id: &str,
    kind: PrincipalKind,
    id: &PrincipalId,
) -> String {
    format!(
        "spiffe://{domain}/{account}/{project}/{kind}/{id}",
        domain = wimse_sanitize_domain(domain),
        account = wimse_escape_segment(account_id),
        project = wimse_escape_segment(project_id),
        kind = kind.as_path_segment(),
        id = wimse_escape_segment(&id.0),
    )
}

/// A principal — any acting entity in SERA.
///
/// MVS scope: simplified model without groups or external agent identity.
/// All principals have full access in autonomous mode (Tier 1).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(from = "PrincipalDeser")]
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
    /// Trust level pinned at registration. When the field is absent in the
    /// serialized form (legacy records written before this field existed),
    /// deserialization falls back to `PrincipalKind::default_trust_level`
    /// — `Unverified` for `ExternalAgent`, `FirstParty` otherwise — so that
    /// pre-existing external-agent records do not silently upgrade.
    pub trust_level: TrustLevel,
}

/// Mirror struct used for backward-compatible `Principal` deserialization.
/// The only behavioral difference is that `trust_level` is optional and,
/// when missing, is filled from the kind-aware default.
#[derive(Deserialize)]
struct PrincipalDeser {
    id: PrincipalId,
    kind: PrincipalKind,
    name: String,
    #[serde(default)]
    external_id: Option<String>,
    #[serde(default)]
    platform: Option<String>,
    #[serde(default)]
    trust_level: Option<TrustLevel>,
}

impl From<PrincipalDeser> for Principal {
    fn from(d: PrincipalDeser) -> Self {
        let trust_level = d
            .trust_level
            .unwrap_or_else(|| d.kind.default_trust_level());
        Self {
            id: d.id,
            kind: d.kind,
            name: d.name,
            external_id: d.external_id,
            platform: d.platform,
            trust_level,
        }
    }
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
        // `local` → `_6C_6F_63_61_6C`, `default` → `_64_65_66_61_75_6C_74`,
        // `admin` → `_61_64_6D_69_6E`. Kind segment stays as the snake_case
        // enum keyword (fixed, not caller-controlled).
        assert_eq!(
            p.wimse_uri("sera.local"),
            "spiffe://sera.local/_6C_6F_63_61_6C/_64_65_66_61_75_6C_74/human/_61_64_6D_69_6E"
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
    /// SPIFFE-ID forbids `:` in path components, so the segment encoding
    /// emits each byte as `_HH`. This test pins the exact encoded form —
    /// adjust intentionally if the encoding policy ever changes.
    #[test]
    fn wimse_uri_encodes_id_as_byte_hex() {
        let p = Principal::from_discord("123", "user");
        // `discord:123` → `_64_69_73_63_6F_72_64_3A_31_32_33`
        assert_eq!(
            p.wimse_uri("sera.local"),
            "spiffe://sera.local/_6C_6F_63_61_6C/_64_65_66_61_75_6C_74/human/_64_69_73_63_6F_72_64_3A_31_32_33",
        );
        // `ext:a2a:reviewer-bot` →
        // `_65_78_74_3A_61_32_61_3A_72_65_76_69_65_77_65_72_2D_62_6F_74`
        let ext = Principal::external_agent("a2a", "reviewer-bot");
        assert_eq!(
            ext.wimse_uri("sera.local"),
            "spiffe://sera.local/_6C_6F_63_61_6C/_64_65_66_61_75_6C_74/external_agent/_65_78_74_3A_61_32_61_3A_72_65_76_69_65_77_65_72_2D_62_6F_74",
        );
    }

    #[test]
    fn wimse_uri_explicit_account_project() {
        let p = Principal::for_agent("a-1", "a");
        // `acct-7` → `_61_63_63_74_2D_37`,
        // `proj-9` → `_70_72_6F_6A_2D_39`,
        // `agent:a-1` → `_61_67_65_6E_74_3A_61_2D_31`.
        assert_eq!(
            p.wimse_uri_for("sera.example.com", "acct-7", "proj-9"),
            "spiffe://sera.example.com/_61_63_63_74_2D_37/_70_72_6F_6A_2D_39/agent/_61_67_65_6E_74_3A_61_2D_31",
        );
    }

    #[test]
    fn principal_ref_wimse_uri_matches_principal() {
        let p = Principal::for_agent("a-1", "a");
        let r = p.as_ref();
        assert_eq!(r.wimse_uri("sera.local"), p.wimse_uri("sera.local"));
    }

    /// Every byte is encoded as `_HH` — even alphanumerics — so the
    /// encoding is uniform and trivially injective.
    #[test]
    fn wimse_escape_segment_encodes_alphanumerics_as_byte_hex() {
        // 'a' = 0x61, 'b' = 0x62, 'c' = 0x63, '1' = 0x31
        assert_eq!(wimse_escape_segment("abc1"), "_61_62_63_31");
    }

    #[test]
    fn wimse_escape_segment_disallowed_bytes_are_distinct() {
        // P1: distinct disallowed bytes must encode to distinct outputs so
        // `a:b` and `a/b` cannot collide.
        assert_eq!(wimse_escape_segment("a:b"), "_61_3A_62");
        assert_eq!(wimse_escape_segment("a/b"), "_61_2F_62");
        assert_ne!(
            wimse_escape_segment("a:b"),
            wimse_escape_segment("a/b"),
            "previously-lossy substitution would collide here",
        );
        assert_eq!(wimse_escape_segment("a b"), "_61_20_62");
        assert_eq!(wimse_escape_segment("a~b"), "_61_7E_62");
    }

    /// `discord:123` is a representative principal id. The encoding must
    /// be deterministic, contain no characters that downstream SPIFFE
    /// parsers reject (`%`, `/`, `:`, whitespace), and stay inside the
    /// SPIFFE-ID character allow-list.
    #[test]
    fn wimse_escape_segment_discord_is_deterministic_and_spiffe_safe() {
        let a = wimse_escape_segment("discord:123");
        let b = wimse_escape_segment("discord:123");
        assert_eq!(a, b, "encoding must be deterministic");
        assert_eq!(a, "_64_69_73_63_6F_72_64_3A_31_32_33");
        assert!(!a.contains('%'), "must not emit percent-encoding");
        assert!(!a.contains('/'), "must not emit `/`");
        assert!(!a.contains(':'), "must not emit `:`");
        assert!(
            !a.chars().any(|c| c.is_whitespace()),
            "must not emit whitespace",
        );
        // SPIFFE-ID allow-list: ASCII letters, digits, `.`, `-`, `_`.
        assert!(
            a.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_')),
            "encoded form must stay inside SPIFFE allow-list",
        );
    }

    /// SPIFFE-ID forbids empty path components. Empty input maps to the
    /// `_empty` sentinel, which is collision-free with every non-empty
    /// encoded form (every non-empty encoding has length divisible by 3
    /// with each triplet `_HH` where `H ∈ [0-9A-F]`; `m` in `_empty` is
    /// not a hex digit, so no non-empty input encodes to `_empty`).
    #[test]
    fn wimse_escape_empty_uses_sentinel_and_does_not_collide() {
        assert_eq!(wimse_escape_segment(""), "_empty");
        // The string `_empty` as an *input* is itself encoded byte-by-byte
        // and must not collide with the empty-sentinel output.
        let encoded_literal = wimse_escape_segment("_empty");
        assert_eq!(encoded_literal, "_5F_65_6D_70_74_79");
        assert_ne!(encoded_literal, "_empty");
    }

    /// SPIFFE-ID forbids the dot-segments `.` and `..` as path components.
    /// Under `_HH` encoding, neither input projects to a dot-segment.
    #[test]
    fn wimse_escape_dot_segments_are_not_dot_segments_after_encoding() {
        let dot = wimse_escape_segment(".");
        let dotdot = wimse_escape_segment("..");
        assert_eq!(dot, "_2E");
        assert_eq!(dotdot, "_2E_2E");
        assert_ne!(dot, ".");
        assert_ne!(dotdot, "..");
    }

    /// `Principal::id` of `.` or `..` (constructable via `PrincipalId::new`
    /// or deserialization) must still project to a SPIFFE-conformant URI
    /// — the encoded id segment is never a dot-segment.
    #[test]
    fn wimse_uri_principal_id_dot_segments_are_safe() {
        let p = Principal {
            id: PrincipalId::new("."),
            kind: PrincipalKind::Agent,
            name: "dot".to_string(),
            external_id: None,
            platform: None,
            trust_level: TrustLevel::FirstParty,
        };
        assert_eq!(
            p.wimse_uri("sera.local"),
            "spiffe://sera.local/_6C_6F_63_61_6C/_64_65_66_61_75_6C_74/agent/_2E",
        );

        let pp = Principal {
            id: PrincipalId::new(".."),
            kind: PrincipalKind::Agent,
            name: "dotdot".to_string(),
            external_id: None,
            platform: None,
            trust_level: TrustLevel::FirstParty,
        };
        assert_eq!(
            pp.wimse_uri("sera.local"),
            "spiffe://sera.local/_6C_6F_63_61_6C/_64_65_66_61_75_6C_74/agent/_2E_2E",
        );
    }

    /// Account, project, and id segments all use the same encoding —
    /// no caller-controlled segment is treated specially.
    #[test]
    fn wimse_uri_account_project_id_use_same_encoding() {
        let p = Principal::for_agent("x", "x");
        let uri = p.wimse_uri_for("sera.local", "x", "x");
        // All three caller-controlled segments encode `x` (0x78) → `_78`,
        // and `agent:x` → `_61_67_65_6E_74_3A_78`.
        assert_eq!(
            uri,
            "spiffe://sera.local/_78/_78/agent/_61_67_65_6E_74_3A_78",
        );
    }

    // ── Trust-domain sanitization ───────────────────────────────────────────

    #[test]
    fn wimse_sanitize_domain_passthrough_for_valid_domain() {
        assert_eq!(wimse_sanitize_domain("sera.local"), "sera.local");
        assert_eq!(wimse_sanitize_domain("sera.example.com"), "sera.example.com");
    }

    #[test]
    fn wimse_sanitize_domain_lowercases_uppercase() {
        assert_eq!(wimse_sanitize_domain("EXAMPLE.COM"), "example.com");
        assert_eq!(wimse_sanitize_domain("Sera.Local"), "sera.local");
    }

    /// `:` (port suffix), `/` (path injection), and whitespace are not
    /// permitted in a SPIFFE trust domain — all collapse to `_`.
    #[test]
    fn wimse_sanitize_domain_replaces_port_and_path_chars() {
        assert_eq!(wimse_sanitize_domain("example.com:443"), "example.com_443");
        assert_eq!(wimse_sanitize_domain("bad/path"), "bad_path");
        assert_eq!(wimse_sanitize_domain("with space"), "with_space");
    }

    #[test]
    fn wimse_sanitize_domain_empty_substituted() {
        assert_eq!(wimse_sanitize_domain(""), "_");
    }

    /// End-to-end: an invalid `SERA_WIMSE_DOMAIN` value reaching
    /// `Principal::wimse_uri` must still produce a SPIFFE-conformant URI.
    /// Domain sanitization is intentionally separate from segment encoding
    /// (the trust domain has its own SPIFFE rules — lowercase only,
    /// limited symbol set), so disallowed bytes in the *domain* still
    /// collapse to `_` while *segments* use injective `_HH` encoding.
    #[test]
    fn wimse_uri_sanitizes_invalid_domain() {
        let p = Principal::default_admin();
        // Encoded id portion = `/_6C_6F_63_61_6C/_64_65_66_61_75_6C_74/human/_61_64_6D_69_6E`.
        let suffix = "/_6C_6F_63_61_6C/_64_65_66_61_75_6C_74/human/_61_64_6D_69_6E";
        assert_eq!(
            p.wimse_uri("EXAMPLE.COM:443"),
            format!("spiffe://example.com_443{suffix}"),
        );
        assert_eq!(p.wimse_uri(""), format!("spiffe://_{suffix}"));
        assert_eq!(
            p.wimse_uri("bad/path"),
            format!("spiffe://bad_path{suffix}"),
        );
    }

    /// `account_id` and `project_id` are caller-supplied and use the same
    /// injective `_HH` encoding as `id` — they are no longer allowed to be
    /// interpolated raw, regardless of their content.
    #[test]
    fn wimse_uri_sanitizes_account_and_project_segments() {
        let p = Principal::for_agent("a-1", "a");
        // `acct:1` → `_61_63_63_74_3A_31`, `proj/2` → `_70_72_6F_6A_2F_32`,
        // `agent:a-1` → `_61_67_65_6E_74_3A_61_2D_31`.
        assert_eq!(
            p.wimse_uri_for("sera.local", "acct:1", "proj/2"),
            "spiffe://sera.local/_61_63_63_74_3A_31/_70_72_6F_6A_2F_32/agent/_61_67_65_6E_74_3A_61_2D_31",
        );
        // `with space` → `_77_69_74_68_20_73_70_61_63_65`, empty → `_empty`.
        assert_eq!(
            p.wimse_uri_for("sera.local", "with space", ""),
            "spiffe://sera.local/_77_69_74_68_20_73_70_61_63_65/_empty/agent/_61_67_65_6E_74_3A_61_2D_31",
        );
        // `.` → `_2E`, `..` → `_2E_2E` — never bare dot-segments.
        assert_eq!(
            p.wimse_uri_for("sera.local", ".", ".."),
            "spiffe://sera.local/_2E/_2E_2E/agent/_61_67_65_6E_74_3A_61_2D_31",
        );
    }

    /// SPIFFE-ID forbids percent-encoded characters in path components,
    /// so the projection must never emit `%XX` regardless of input.
    #[test]
    fn wimse_uri_never_emits_percent_encoding() {
        let p = Principal::external_agent("a2a", "weird name!");
        let uri = p.wimse_uri("sera.local");
        assert!(!uri.contains('%'), "URI should not contain `%`: {uri}");
    }

    /// Legacy serialized records written before `trust_level` existed must
    /// deserialize using the kind-aware default — `ExternalAgent` →
    /// `Unverified` rather than the global `FirstParty`. This prevents a
    /// silent trust upgrade for pre-existing external-agent rows.
    #[test]
    fn principal_serde_legacy_external_agent_defaults_to_unverified() {
        let legacy = r#"{
            "id": "ext:a2a:bot",
            "kind": "external_agent",
            "name": "bot",
            "platform": "a2a"
        }"#;
        let parsed: Principal = serde_json::from_str(legacy).unwrap();
        assert_eq!(parsed.kind, PrincipalKind::ExternalAgent);
        assert_eq!(parsed.trust_level, TrustLevel::Unverified);
    }

    /// An explicit `trust_level` field always wins over the kind default,
    /// even when it conflicts (e.g. an external agent marked first-party).
    #[test]
    fn principal_serde_explicit_trust_level_overrides_kind_default() {
        let payload = r#"{
            "id": "ext:a2a:bot",
            "kind": "external_agent",
            "name": "bot",
            "trust_level": "first_party"
        }"#;
        let parsed: Principal = serde_json::from_str(payload).unwrap();
        assert_eq!(parsed.kind, PrincipalKind::ExternalAgent);
        assert_eq!(parsed.trust_level, TrustLevel::FirstParty);
    }
}
