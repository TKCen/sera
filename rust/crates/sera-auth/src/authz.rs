//! Authorization layer — pluggable PDP via the `AuthorizationProvider` trait.
//!
//! SPEC-identity-authz §5: Any acting entity is authorized (or denied) via this
//! trait. The built-in implementation is RBAC; enterprise deployments can swap
//! in an AuthZen PDP by providing a different `AuthorizationProvider`.

use std::collections::HashMap;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use sera_types::evolution::{BlastRadius, ChangeArtifactId};
use sera_types::principal::PrincipalRef;

// ---------------------------------------------------------------------------
// Action
// ---------------------------------------------------------------------------

/// The operation a principal is attempting to perform.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type", content = "value")]
pub enum Action {
    Read,
    Write,
    Execute,
    Admin,
    /// Call a named tool (e.g. `"bash"`, `"web_search"`).
    ToolCall(String),
    /// A session-level operation (e.g. `"join"`, `"terminate"`).
    SessionOp(String),
    /// Access a named memory scope.
    MemoryAccess(String),
    /// Modify a named config path.
    ConfigChange(String),
    /// Propose a change artifact within the given blast radius.
    ProposeChange(BlastRadius),
    /// Approve a specific change artifact.
    ApproveChange(ChangeArtifactId),
}

// ---------------------------------------------------------------------------
// Resource
// ---------------------------------------------------------------------------

/// The resource a principal is acting upon.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type", content = "id")]
pub enum Resource {
    Session(String),
    Agent(String),
    Tool(String),
    Memory(String),
    Config(String),
    Workflow(String),
    System,
    /// A specific change artifact (identified by content-addressed hash).
    ChangeArtifact(ChangeArtifactId),
}

// ---------------------------------------------------------------------------
// PendingApprovalHint
// ---------------------------------------------------------------------------

/// Hint returned alongside [`AuthzDecision::NeedsApproval`] for Phase 0.
///
/// Carries enough context for the HITL routing layer to find the right
/// approval queue without embedding full policy logic here.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingApprovalHint {
    /// Human-readable routing key (e.g. approval policy ID or queue name).
    pub routing_hint: String,
    /// Optional scope annotation (e.g. blast-radius label, tier name).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
}

// ---------------------------------------------------------------------------
// AuthzContext
// ---------------------------------------------------------------------------

/// Additional context supplied to the authorization check.
///
/// Carries per-request metadata that a PDP may use for risk-based or
/// context-dependent decisions.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuthzContext {
    /// Session within which the action is occurring, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// Risk score in [0.0, 1.0] — higher means riskier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub risk_score: Option<f64>,
    /// Arbitrary metadata (hook-injected values, request annotations, etc.).
    #[serde(default)]
    pub metadata: HashMap<String, serde_json::Value>,
}

impl AuthzContext {
    /// Construct a context for a named session.
    pub fn for_session(session_id: impl Into<String>) -> Self {
        Self {
            session_id: Some(session_id.into()),
            ..Default::default()
        }
    }
}

// ---------------------------------------------------------------------------
// DenyReason
// ---------------------------------------------------------------------------

/// Machine-readable reason for a `Deny` decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DenyReason {
    /// Short code suitable for API responses (e.g. `"permission_denied"`).
    pub code: String,
    /// Human-readable explanation.
    pub message: String,
}

impl DenyReason {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

// ---------------------------------------------------------------------------
// AuthzDecision
// ---------------------------------------------------------------------------

/// The outcome of an authorization check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthzDecision {
    /// The action is permitted.
    Allow,
    /// The action is denied for the given reason.
    Deny(DenyReason),
    /// The action requires human-in-the-loop approval; the string carries a
    /// routing hint (e.g. approval policy ID or queue name).
    NeedsApproval(String),
}

// ---------------------------------------------------------------------------
// AuthzError
// ---------------------------------------------------------------------------

/// Errors returned by `AuthorizationProvider::check`.
#[derive(Debug, Clone, Error)]
pub enum AuthzError {
    #[error("authorization provider unavailable: {0}")]
    ProviderUnavailable(String),
    #[error("policy evaluation error: {0}")]
    PolicyError(String),
    #[error("internal authorization error: {0}")]
    Internal(String),
}

// ---------------------------------------------------------------------------
// AuthorizationProvider trait
// ---------------------------------------------------------------------------

/// Pluggable Policy Decision Point (PDP).
///
/// Implement this trait to supply custom authorization logic. The default
/// implementation is [`DefaultAuthzProvider`], which applies built-in RBAC.
/// Enterprise deployments can replace it with an AuthZen-compliant external PDP.
#[async_trait]
pub trait AuthorizationProvider: Send + Sync {
    /// Evaluate whether `principal` may perform `action` on `resource`.
    ///
    /// Returns:
    /// - `Ok(AuthzDecision::Allow)` — proceed.
    /// - `Ok(AuthzDecision::Deny(_))` — reject immediately.
    /// - `Ok(AuthzDecision::NeedsApproval(_))` — escalate to HITL.
    /// - `Err(_)` — PDP itself failed; callers should treat this as a deny.
    async fn check(
        &self,
        principal: &PrincipalRef,
        action: &Action,
        resource: &Resource,
        context: &AuthzContext,
    ) -> Result<AuthzDecision, AuthzError>;
}

// ---------------------------------------------------------------------------
// DefaultAuthzProvider
// ---------------------------------------------------------------------------

/// Built-in RBAC authorization provider.
///
/// # Current behaviour
///
/// TODO: Implement role-based access control using the Principal's group
/// memberships and the role table from SPEC-identity-authz §5.3.
/// For now this is a placeholder that always returns `Allow`, which is correct
/// for Tier 1 (autonomous/local) deployments where all principals are trusted.
#[derive(Debug, Clone, Default)]
pub struct DefaultAuthzProvider;

#[async_trait]
impl AuthorizationProvider for DefaultAuthzProvider {
    async fn check(
        &self,
        _principal: &PrincipalRef,
        action: &Action,
        _resource: &Resource,
        _context: &AuthzContext,
    ) -> Result<AuthzDecision, AuthzError> {
        match action {
            // Phase 0: change-proposal and approval actions require explicit
            // policy grant. Deny by default until a CasbinAuthzAdapter or
            // enterprise policy provider is wired in.
            Action::ProposeChange(_) | Action::ApproveChange(_) => {
                Ok(AuthzDecision::Deny(DenyReason::new(
                    "change_action_requires_policy",
                    "ProposeChange and ApproveChange require explicit policy grant",
                )))
            }
            // TODO(sera-32zt): Replace remaining variants with role-based
            // checks once PrincipalGroup and role tables are available.
            // Tier 1 default: allow everything else.
            _ => Ok(AuthzDecision::Allow),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use sera_types::principal::{PrincipalId, PrincipalKind};

    fn make_principal_ref() -> PrincipalRef {
        PrincipalRef {
            id: PrincipalId::new("admin"),
            kind: PrincipalKind::Human,
        }
    }

    #[tokio::test]
    async fn default_provider_always_allows() {
        let provider = DefaultAuthzProvider;
        let principal = make_principal_ref();
        let ctx = AuthzContext::default();

        let decision = provider
            .check(&principal, &Action::Read, &Resource::System, &ctx)
            .await
            .expect("check must not fail");

        assert_eq!(decision, AuthzDecision::Allow);
    }

    #[tokio::test]
    async fn default_provider_allows_all_action_variants() {
        let provider = DefaultAuthzProvider;
        let principal = make_principal_ref();
        let ctx = AuthzContext::default();

        let actions = vec![
            Action::Read,
            Action::Write,
            Action::Execute,
            Action::Admin,
            Action::ToolCall("bash".to_string()),
            Action::SessionOp("join".to_string()),
            Action::MemoryAccess("circle-1".to_string()),
            Action::ConfigChange("agent.model".to_string()),
        ];

        for action in &actions {
            let decision = provider
                .check(&principal, action, &Resource::System, &ctx)
                .await
                .expect("check must not fail");
            assert_eq!(decision, AuthzDecision::Allow, "action {action:?} should be allowed");
        }
    }

    #[test]
    fn action_serde_roundtrip() {
        let actions = vec![
            Action::Read,
            Action::Write,
            Action::Execute,
            Action::Admin,
            Action::ToolCall("web_search".to_string()),
            Action::SessionOp("terminate".to_string()),
            Action::MemoryAccess("shared".to_string()),
            Action::ConfigChange("llm.model".to_string()),
        ];

        for action in &actions {
            let json = serde_json::to_string(action).expect("serialize");
            let parsed: Action = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(&parsed, action, "roundtrip failed for {action:?}");
        }
    }

    #[test]
    fn resource_serde_roundtrip() {
        let resources = vec![
            Resource::Session("sess-1".to_string()),
            Resource::Agent("agent-abc".to_string()),
            Resource::Tool("bash".to_string()),
            Resource::Memory("circle-x".to_string()),
            Resource::Config("agent.policy".to_string()),
            Resource::Workflow("wf-deploy".to_string()),
            Resource::System,
        ];

        for resource in &resources {
            let json = serde_json::to_string(resource).expect("serialize");
            let parsed: Resource = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(&parsed, resource, "roundtrip failed for {resource:?}");
        }
    }

    #[test]
    fn authz_context_with_metadata() {
        let mut ctx = AuthzContext::for_session("sess-42");
        ctx.risk_score = Some(0.3);
        ctx.metadata.insert(
            "hook_result".to_string(),
            serde_json::Value::Bool(true),
        );

        assert_eq!(ctx.session_id.as_deref(), Some("sess-42"));
        assert_eq!(ctx.risk_score, Some(0.3));
        assert_eq!(ctx.metadata["hook_result"], serde_json::Value::Bool(true));

        // Serde roundtrip
        let json = serde_json::to_string(&ctx).expect("serialize");
        let parsed: AuthzContext = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed.session_id, ctx.session_id);
        assert_eq!(parsed.risk_score, ctx.risk_score);
        assert_eq!(parsed.metadata["hook_result"], serde_json::Value::Bool(true));
    }

    #[test]
    fn deny_reason_fields() {
        let reason = DenyReason::new("permission_denied", "Agent lacks the required role");
        assert_eq!(reason.code, "permission_denied");
        assert!(!reason.message.is_empty());

        let json = serde_json::to_string(&reason).expect("serialize");
        let parsed: DenyReason = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed, reason);
    }

    #[test]
    fn authz_decision_serde() {
        let allow = AuthzDecision::Allow;
        let deny = AuthzDecision::Deny(DenyReason::new("forbidden", "not allowed"));
        let needs = AuthzDecision::NeedsApproval("hitl-queue-1".to_string());

        for decision in &[allow, deny, needs] {
            let json = serde_json::to_string(decision).expect("serialize");
            let parsed: AuthzDecision = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(&parsed, decision);
        }
    }
}
