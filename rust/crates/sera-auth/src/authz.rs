//! Authorization layer — pluggable PDP via the `AuthorizationProvider` trait.
//!
//! SPEC-identity-authz §5: Any acting entity is authorized (or denied) via this
//! trait. The built-in implementation is RBAC; enterprise deployments can swap
//! in an AuthZen PDP by providing a different `AuthorizationProvider`.
//!
//! # Tier behaviour
//!
//! | Tier | Provider to use | Policy |
//! |------|-----------------|--------|
//! | 1 (local/autonomous) | `DefaultAuthzProvider` | Allow-all (except change actions) |
//! | 2+ (team/enterprise) | `RbacAuthzProvider` | Casbin RBAC from policy files |
//!
//! Use [`RbacAuthzProvider::from_strings`] for in-memory policy (tests/embedded)
//! or [`RbacAuthzProvider::from_files`] to load `rbac.conf` + `rbac.csv` from
//! `capability-policies/` at startup.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::RwLock;

use sera_types::evolution::{BlastRadius, ChangeArtifactId};
use sera_types::principal::{PrincipalId, PrincipalRef};

use crate::casbin_adapter::CasbinAuthzAdapter;

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

/// Tier 1 (autonomous/local) authorization provider.
///
/// # Behaviour
///
/// - **Change actions** ([`Action::ProposeChange`] / [`Action::ApproveChange`])
///   are denied by default. A Tier 2+ provider (e.g. [`RbacAuthzProvider`] or
///   [`RoleBasedAuthzProvider`]) must be configured to grant them explicitly.
/// - **All other actions** are allowed. In Tier 1 all principals are considered
///   trusted (single-operator deployment), so no role lookup is performed.
///
/// For richer role-based checks without a casbin policy file, prefer
/// [`RoleBasedAuthzProvider`]. For full policy expressions, use
/// [`RbacAuthzProvider`].
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
            // Tier 1: single-operator deployment — all non-change actions
            // are allowed. Deployments that need role-gating should switch to
            // `RoleBasedAuthzProvider` (in-memory) or `RbacAuthzProvider`
            // (casbin) at gateway startup.
            _ => Ok(AuthzDecision::Allow),
        }
    }
}

// ---------------------------------------------------------------------------
// RbacAuthzProvider — Tier 2+ casbin-backed RBAC
// ---------------------------------------------------------------------------

/// Maps a SERA [`Resource`] to a casbin object string.
///
/// Format: `<kind>:<id>` (e.g. `tool:bash`, `agent:my-agent`, `system`).
fn resource_to_obj(resource: &Resource) -> String {
    match resource {
        Resource::Session(id) => format!("session:{id}"),
        Resource::Agent(id) => format!("agent:{id}"),
        Resource::Tool(id) => format!("tool:{id}"),
        Resource::Memory(id) => format!("memory:{id}"),
        Resource::Config(id) => format!("config:{id}"),
        Resource::Workflow(id) => format!("workflow:{id}"),
        Resource::System => "system".to_string(),
        Resource::ChangeArtifact(id) => format!("change_artifact:{}", hex::encode(id.hash)),
    }
}

/// Maps a SERA [`Action`] to a casbin action string.
fn action_to_act(action: &Action) -> String {
    match action {
        Action::Read => "read".to_string(),
        Action::Write => "write".to_string(),
        Action::Execute => "execute".to_string(),
        Action::Admin => "admin".to_string(),
        Action::ToolCall(name) => format!("tool_call:{name}"),
        Action::SessionOp(op) => format!("session_op:{op}"),
        Action::MemoryAccess(scope) => format!("memory_access:{scope}"),
        Action::ConfigChange(path) => format!("config_change:{path}"),
        Action::ProposeChange(radius) => format!("propose_change:{radius:?}"),
        Action::ApproveChange(id) => format!("approve_change:{}", hex::encode(id.hash)),
    }
}

/// Casbin-backed RBAC authorization provider for Tier 2+ deployments.
///
/// Wraps a [`CasbinAuthzAdapter`] in an `Arc<RwLock<_>>` so it can be
/// shared safely across async tasks without per-request I/O.
///
/// # Construction
///
/// ```rust,ignore
/// // From in-memory strings (tests, embedded policies):
/// let provider = RbacAuthzProvider::from_strings(MODEL_STR, POLICY_CSV).await?;
///
/// // From files on disk (production):
/// let provider = RbacAuthzProvider::from_files(
///     "capability-policies/rbac.conf",
///     "capability-policies/rbac.csv",
/// ).await?;
/// ```
///
/// # Casbin subject mapping
///
/// The casbin `sub` is the principal's `id` string. Role assignments
/// (`g, <id>, <role>`) in the policy CSV drive role-based decisions.
#[derive(Clone)]
pub struct RbacAuthzProvider {
    enforcer: Arc<RwLock<CasbinAuthzAdapter>>,
}

impl RbacAuthzProvider {
    /// Construct from in-memory model and policy strings.
    ///
    /// Both strings use the same format as casbin `.conf` and `.csv` files.
    pub async fn from_strings(
        model_text: &str,
        policy_text: &str,
    ) -> Result<Self, AuthzError> {
        let adapter = CasbinAuthzAdapter::from_strings(model_text, policy_text)
            .await
            .map_err(|e| AuthzError::Internal(e.to_string()))?;
        Ok(Self {
            enforcer: Arc::new(RwLock::new(adapter)),
        })
    }

    /// Construct by reading model and policy from files on disk.
    ///
    /// `model_path` — path to the casbin `.conf` model file.
    /// `policy_path` — path to the casbin `.csv` policy file.
    pub async fn from_files(
        model_path: &str,
        policy_path: &str,
    ) -> Result<Self, AuthzError> {
        let model_text = tokio::fs::read_to_string(model_path)
            .await
            .map_err(|e| AuthzError::ProviderUnavailable(
                format!("failed to read model file {model_path}: {e}"),
            ))?;
        let policy_text = tokio::fs::read_to_string(policy_path)
            .await
            .map_err(|e| AuthzError::ProviderUnavailable(
                format!("failed to read policy file {policy_path}: {e}"),
            ))?;
        Self::from_strings(&model_text, &policy_text).await
    }
}

#[async_trait]
impl AuthorizationProvider for RbacAuthzProvider {
    async fn check(
        &self,
        principal: &PrincipalRef,
        action: &Action,
        resource: &Resource,
        _context: &AuthzContext,
    ) -> Result<AuthzDecision, AuthzError> {
        let subject = principal.id.to_string();
        let object = resource_to_obj(resource);
        let act = action_to_act(action);

        let enforcer = self.enforcer.read().await;
        let allowed = enforcer
            .enforce(&subject, &object, &act)
            .await
            .map_err(|e| AuthzError::PolicyError(e.to_string()))?;

        if allowed {
            Ok(AuthzDecision::Allow)
        } else {
            Ok(AuthzDecision::Deny(DenyReason::new(
                "rbac_deny",
                format!(
                    "principal '{subject}' is not permitted to perform '{act}' on '{object}'"
                ),
            )))
        }
    }
}

// ---------------------------------------------------------------------------
// RoleBasedAuthzProvider — Tier 1.5 lightweight role-based provider
// ---------------------------------------------------------------------------

/// Coarse-grained classification of an [`Action`] used for role grants.
///
/// Dynamic payloads (tool name, memory scope, blast radius, etc.) are
/// intentionally discarded: `RoleBasedAuthzProvider` matches by action *kind*
/// only. Deployments that need per-tool or per-scope decisions should use
/// [`RbacAuthzProvider`] instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionKind {
    Read,
    Write,
    Execute,
    Admin,
    ToolCall,
    SessionOp,
    MemoryAccess,
    ConfigChange,
    ProposeChange,
    ApproveChange,
}

impl ActionKind {
    /// Classify an [`Action`] into its [`ActionKind`].
    pub fn of(action: &Action) -> Self {
        match action {
            Action::Read => ActionKind::Read,
            Action::Write => ActionKind::Write,
            Action::Execute => ActionKind::Execute,
            Action::Admin => ActionKind::Admin,
            Action::ToolCall(_) => ActionKind::ToolCall,
            Action::SessionOp(_) => ActionKind::SessionOp,
            Action::MemoryAccess(_) => ActionKind::MemoryAccess,
            Action::ConfigChange(_) => ActionKind::ConfigChange,
            Action::ProposeChange(_) => ActionKind::ProposeChange,
            Action::ApproveChange(_) => ActionKind::ApproveChange,
        }
    }
}

/// Tier 1.5 role-based authorization provider backed by an in-memory table.
///
/// This is the lightweight middle ground between [`DefaultAuthzProvider`]
/// (allow-all) and [`RbacAuthzProvider`] (full casbin policy expressions).
/// It evaluates a decision in two steps:
///
/// 1. Resolve the principal's roles from `principal_roles`.
/// 2. Union the [`ActionKind`] grants across those roles and check whether the
///    requested action's kind is permitted.
///
/// # Tier 1 compatibility
///
/// Like [`DefaultAuthzProvider`], change actions
/// ([`Action::ProposeChange`] / [`Action::ApproveChange`]) must be granted
/// explicitly via a role — they are never allowed by default.
///
/// # Example
///
/// ```rust,ignore
/// use sera_auth::authz::{ActionKind, RoleBasedAuthzProvider};
/// use sera_types::principal::PrincipalId;
///
/// let provider = RoleBasedAuthzProvider::builder()
///     .grant("operator", [ActionKind::Read, ActionKind::Execute])
///     .grant("admin", [ActionKind::Admin, ActionKind::ProposeChange])
///     .assign(PrincipalId::new("alice"), ["operator"])
///     .assign(PrincipalId::new("root"), ["admin"])
///     .build();
/// ```
#[derive(Debug, Clone, Default)]
pub struct RoleBasedAuthzProvider {
    /// Map of principal id → roles assigned to that principal.
    principal_roles: HashMap<PrincipalId, HashSet<String>>,
    /// Map of role name → action kinds granted to that role.
    role_grants: HashMap<String, HashSet<ActionKind>>,
}

impl RoleBasedAuthzProvider {
    /// Start building a new `RoleBasedAuthzProvider`.
    pub fn builder() -> RoleBasedAuthzProviderBuilder {
        RoleBasedAuthzProviderBuilder::default()
    }

    /// Return the set of roles assigned to a principal. Empty if the principal
    /// has no role assignments.
    fn roles_of(&self, principal: &PrincipalId) -> Option<&HashSet<String>> {
        self.principal_roles.get(principal)
    }

    /// Return true if any of `roles` grants `kind`.
    fn any_role_grants(&self, roles: &HashSet<String>, kind: ActionKind) -> bool {
        roles
            .iter()
            .filter_map(|role| self.role_grants.get(role))
            .any(|grants| grants.contains(&kind))
    }
}

#[async_trait]
impl AuthorizationProvider for RoleBasedAuthzProvider {
    async fn check(
        &self,
        principal: &PrincipalRef,
        action: &Action,
        _resource: &Resource,
        _context: &AuthzContext,
    ) -> Result<AuthzDecision, AuthzError> {
        let kind = ActionKind::of(action);
        let Some(roles) = self.roles_of(&principal.id) else {
            return Ok(AuthzDecision::Deny(DenyReason::new(
                "principal_has_no_roles",
                format!(
                    "principal '{}' has no role assignments",
                    principal.id
                ),
            )));
        };

        if self.any_role_grants(roles, kind) {
            Ok(AuthzDecision::Allow)
        } else {
            Ok(AuthzDecision::Deny(DenyReason::new(
                "role_lacks_action_grant",
                format!(
                    "principal '{}' has no role granting '{:?}'",
                    principal.id, kind
                ),
            )))
        }
    }
}

/// Fluent builder for [`RoleBasedAuthzProvider`].
#[derive(Debug, Default)]
pub struct RoleBasedAuthzProviderBuilder {
    principal_roles: HashMap<PrincipalId, HashSet<String>>,
    role_grants: HashMap<String, HashSet<ActionKind>>,
}

impl RoleBasedAuthzProviderBuilder {
    /// Grant a role a set of action kinds. Repeated calls merge into the
    /// existing grant set.
    pub fn grant<I>(mut self, role: impl Into<String>, kinds: I) -> Self
    where
        I: IntoIterator<Item = ActionKind>,
    {
        self.role_grants
            .entry(role.into())
            .or_default()
            .extend(kinds);
        self
    }

    /// Assign a principal one or more roles. Repeated calls merge.
    pub fn assign<I, S>(mut self, principal: PrincipalId, roles: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.principal_roles
            .entry(principal)
            .or_default()
            .extend(roles.into_iter().map(Into::into));
        self
    }

    /// Finalise the builder into a provider.
    pub fn build(self) -> RoleBasedAuthzProvider {
        RoleBasedAuthzProvider {
            principal_roles: self.principal_roles,
            role_grants: self.role_grants,
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

    // ── RbacAuthzProvider tests ───────────────────────────────────────────────

    /// Minimal RBAC model for tests: subject/object/action equality matching
    /// with role inheritance via `g`.
    const TEST_MODEL: &str = r#"[request_definition]
r = sub, obj, act

[policy_definition]
p = sub, obj, act

[role_definition]
g = _, _

[policy_effect]
e = some(where (p.eft == allow))

[matchers]
m = g(r.sub, p.sub) && (p.obj == "*" || r.obj == p.obj || keyMatch(r.obj, p.obj)) && (p.act == "*" || r.act == p.act)
"#;

    /// Builds an `RbacAuthzProvider` from a policy CSV string.
    async fn make_rbac_provider(policy_csv: &str) -> RbacAuthzProvider {
        RbacAuthzProvider::from_strings(TEST_MODEL, policy_csv)
            .await
            .expect("RbacAuthzProvider init must not fail")
    }

    fn principal(id: &str) -> PrincipalRef {
        PrincipalRef {
            id: PrincipalId::new(id),
            kind: PrincipalKind::Human,
        }
    }

    /// Tier 1 (DefaultAuthzProvider) is unchanged — still allows all
    /// non-change actions for any principal, regardless of RBAC config.
    #[tokio::test]
    async fn tier1_default_provider_unaffected_by_rbac() {
        let provider = DefaultAuthzProvider;
        let ctx = AuthzContext::default();

        // An agent principal with no RBAC policy still gets Allow from Tier 1.
        let agent = PrincipalRef {
            id: PrincipalId::new("unknown-agent"),
            kind: PrincipalKind::Agent,
        };

        for action in &[Action::Read, Action::Write, Action::Execute, Action::ToolCall("bash".to_string())] {
            let decision = provider
                .check(&agent, action, &Resource::System, &ctx)
                .await
                .expect("Tier 1 check must not fail");
            assert_eq!(
                decision,
                AuthzDecision::Allow,
                "Tier 1 should allow {action:?} for any principal"
            );
        }
    }

    /// Tier 2 allow: principal has a matching role that grants the requested
    /// action on the requested resource.
    #[tokio::test]
    async fn tier2_rbac_allow_when_role_matches() {
        // alice has the operator role; operator can read agent resources.
        let policy = "p, operator, agent:*, read\ng, alice, operator\n";
        let provider = make_rbac_provider(policy).await;
        let ctx = AuthzContext::default();

        let decision = provider
            .check(&principal("alice"), &Action::Read, &Resource::Agent("my-agent".to_string()), &ctx)
            .await
            .expect("check must not fail");

        assert_eq!(decision, AuthzDecision::Allow, "alice (operator) should be allowed to read agents");
    }

    /// Tier 2 deny: principal does not have a policy entry for the requested
    /// action, so the enforcer returns false → `AuthzDecision::Deny`.
    #[tokio::test]
    async fn tier2_rbac_deny_when_no_matching_policy() {
        // bob has observer role; observer cannot write to sessions.
        let policy = "p, observer, session:*, read\ng, bob, observer\n";
        let provider = make_rbac_provider(policy).await;
        let ctx = AuthzContext::default();

        let decision = provider
            .check(&principal("bob"), &Action::Write, &Resource::Session("sess-1".to_string()), &ctx)
            .await
            .expect("check must not fail");

        assert!(
            matches!(decision, AuthzDecision::Deny(_)),
            "bob (observer) should be denied write on sessions; got {decision:?}"
        );
    }

    /// Tier 2 resource pattern: policy uses a wildcard `tool:*` pattern and the
    /// enforcer's `keyMatch` matcher resolves concrete tool names against it.
    #[tokio::test]
    async fn tier2_rbac_resource_pattern_wildcard() {
        // agent_role can execute any tool (tool:* pattern).
        let policy = "p, agent_role, tool:*, execute\ng, agent-42, agent_role\n";
        let provider = make_rbac_provider(policy).await;
        let ctx = AuthzContext::default();

        let agent = PrincipalRef {
            id: PrincipalId::new("agent-42"),
            kind: PrincipalKind::Agent,
        };

        // Wildcard should match concrete tool names.
        for tool in &["bash", "web_search", "read_file"] {
            let decision = provider
                .check(
                    &agent,
                    &Action::Execute,
                    &Resource::Tool((*tool).to_string()),
                    &ctx,
                )
                .await
                .expect("check must not fail");
            assert_eq!(
                decision,
                AuthzDecision::Allow,
                "agent-42 (agent_role) should be allowed to execute tool:{tool}"
            );
        }

        // But write on tools is not in policy.
        let deny_decision = provider
            .check(&agent, &Action::Write, &Resource::Tool("bash".to_string()), &ctx)
            .await
            .expect("check must not fail");
        assert!(
            matches!(deny_decision, AuthzDecision::Deny(_)),
            "agent-42 should be denied write on tools; got {deny_decision:?}"
        );
    }

    // ── DefaultAuthzProvider change-action deny tests ─────────────────────────

    /// DefaultAuthzProvider must deny `ProposeChange` regardless of blast
    /// radius — the Tier 1 rule is that change proposals require an explicit
    /// policy grant from a richer provider.
    #[tokio::test]
    async fn default_provider_denies_propose_change() {
        let provider = DefaultAuthzProvider;
        let principal = make_principal_ref();
        let ctx = AuthzContext::default();

        for radius in [
            BlastRadius::AgentMemory,
            BlastRadius::AgentManifest,
            BlastRadius::GlobalConfig,
        ] {
            let decision = provider
                .check(
                    &principal,
                    &Action::ProposeChange(radius),
                    &Resource::System,
                    &ctx,
                )
                .await
                .expect("check must not fail");

            match decision {
                AuthzDecision::Deny(reason) => {
                    assert_eq!(reason.code, "change_action_requires_policy");
                }
                other => panic!("expected Deny for ProposeChange({radius:?}); got {other:?}"),
            }
        }
    }

    /// DefaultAuthzProvider must deny `ApproveChange` for any artifact id.
    #[tokio::test]
    async fn default_provider_denies_approve_change() {
        let provider = DefaultAuthzProvider;
        let principal = make_principal_ref();
        let ctx = AuthzContext::default();
        let artifact = ChangeArtifactId { hash: [0xAB; 32] };

        let decision = provider
            .check(
                &principal,
                &Action::ApproveChange(artifact),
                &Resource::ChangeArtifact(artifact),
                &ctx,
            )
            .await
            .expect("check must not fail");

        assert!(
            matches!(decision, AuthzDecision::Deny(r) if r.code == "change_action_requires_policy"),
        );
    }

    // ── RoleBasedAuthzProvider tests ──────────────────────────────────────────

    /// Build a provider with a small fixture: alice=operator (read/execute),
    /// root=admin (admin + propose_change), bob has no roles.
    fn role_based_fixture() -> RoleBasedAuthzProvider {
        RoleBasedAuthzProvider::builder()
            .grant("operator", [ActionKind::Read, ActionKind::Execute])
            .grant(
                "admin",
                [ActionKind::Admin, ActionKind::ProposeChange],
            )
            .assign(PrincipalId::new("alice"), ["operator"])
            .assign(PrincipalId::new("root"), ["admin"])
            .build()
    }

    #[tokio::test]
    async fn role_based_allows_when_role_matches() {
        let provider = role_based_fixture();
        let ctx = AuthzContext::default();

        let decision = provider
            .check(&principal("alice"), &Action::Read, &Resource::System, &ctx)
            .await
            .expect("check must not fail");
        assert_eq!(decision, AuthzDecision::Allow);

        // Action payloads are ignored — tool_call kind match is enough.
        let decision = provider
            .check(
                &principal("alice"),
                &Action::Execute,
                &Resource::Tool("bash".to_string()),
                &ctx,
            )
            .await
            .expect("check must not fail");
        assert_eq!(decision, AuthzDecision::Allow);
    }

    #[tokio::test]
    async fn role_based_denies_when_role_lacks_grant() {
        let provider = role_based_fixture();
        let ctx = AuthzContext::default();

        // alice is operator → no write grant.
        let decision = provider
            .check(&principal("alice"), &Action::Write, &Resource::System, &ctx)
            .await
            .expect("check must not fail");

        match decision {
            AuthzDecision::Deny(reason) => assert_eq!(reason.code, "role_lacks_action_grant"),
            other => panic!("expected role_lacks_action_grant deny; got {other:?}"),
        }
    }

    #[tokio::test]
    async fn role_based_denies_unassigned_principal() {
        let provider = role_based_fixture();
        let ctx = AuthzContext::default();

        let decision = provider
            .check(&principal("bob"), &Action::Read, &Resource::System, &ctx)
            .await
            .expect("check must not fail");

        match decision {
            AuthzDecision::Deny(reason) => assert_eq!(reason.code, "principal_has_no_roles"),
            other => panic!("expected principal_has_no_roles deny; got {other:?}"),
        }
    }

    /// Unlike `DefaultAuthzProvider`, `RoleBasedAuthzProvider` *can* grant
    /// `ProposeChange` — the admin role in the fixture holds that grant.
    #[tokio::test]
    async fn role_based_admin_can_propose_change() {
        let provider = role_based_fixture();
        let ctx = AuthzContext::default();

        let decision = provider
            .check(
                &principal("root"),
                &Action::ProposeChange(BlastRadius::AgentSkill),
                &Resource::System,
                &ctx,
            )
            .await
            .expect("check must not fail");
        assert_eq!(decision, AuthzDecision::Allow);
    }

    /// Multiple roles union their grants.
    #[tokio::test]
    async fn role_based_unions_grants_across_roles() {
        let provider = RoleBasedAuthzProvider::builder()
            .grant("reader", [ActionKind::Read])
            .grant("writer", [ActionKind::Write])
            .assign(PrincipalId::new("carol"), ["reader", "writer"])
            .build();
        let ctx = AuthzContext::default();

        for action in [Action::Read, Action::Write] {
            let decision = provider
                .check(&principal("carol"), &action, &Resource::System, &ctx)
                .await
                .expect("check must not fail");
            assert_eq!(decision, AuthzDecision::Allow, "carol should have {action:?}");
        }

        // But not execute.
        let decision = provider
            .check(&principal("carol"), &Action::Execute, &Resource::System, &ctx)
            .await
            .expect("check must not fail");
        assert!(matches!(decision, AuthzDecision::Deny(_)));
    }

    #[test]
    fn action_kind_classifies_all_variants() {
        let artifact = ChangeArtifactId { hash: [0u8; 32] };
        let cases: Vec<(Action, ActionKind)> = vec![
            (Action::Read, ActionKind::Read),
            (Action::Write, ActionKind::Write),
            (Action::Execute, ActionKind::Execute),
            (Action::Admin, ActionKind::Admin),
            (Action::ToolCall("bash".into()), ActionKind::ToolCall),
            (Action::SessionOp("join".into()), ActionKind::SessionOp),
            (Action::MemoryAccess("c".into()), ActionKind::MemoryAccess),
            (Action::ConfigChange("k".into()), ActionKind::ConfigChange),
            (
                Action::ProposeChange(BlastRadius::AgentMemory),
                ActionKind::ProposeChange,
            ),
            (
                Action::ApproveChange(artifact),
                ActionKind::ApproveChange,
            ),
        ];

        for (action, expected) in cases {
            assert_eq!(ActionKind::of(&action), expected, "classify {action:?}");
        }
    }
}
