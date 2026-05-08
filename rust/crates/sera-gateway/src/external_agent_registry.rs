//! In-memory registry of Pattern B external-agent sessions (sera-pcvp PR2).
//!
//! Each entry is keyed by the SQ-envelope `session_key` and records the typed
//! handshake the gateway pinned at registration: who delegated the session,
//! the egress allow-list it declared, the budget bucket it is bound to, and
//! the minted [`PrincipalRef`].
//!
//! ## Why this exists
//!
//! Pattern A harnesses (Agent / Connector / Plugin manifest variants of
//! [`RegisterOp`]) flow through the existing manifest path. Pattern B
//! sessions — a clawhip-launched Claude Code pane, an OMC orchestrator pane,
//! a Hermes profile, an external A2A/ACP agent — assert their own identity
//! over the SQ envelope. The gateway must:
//!
//! 1. Refuse a registration whose `delegated_by` is unknown or wrong-kind
//!    (anti-laundering invariant; SPEC-gateway §1d).
//! 2. Enforce that a session that intends to talk to the LLM proxy actually
//!    declared `EgressEndpoint::InferenceLocal` in its allow-list (mirrors
//!    `EgressManifest::allows_inference_local` for the typed handshake).
//! 3. Refuse re-binding the same `session_key` to a different principal —
//!    the BYOH registry replaces on collision because catalogs are
//!    idempotent; Pattern B sessions are *identity*, so a duplicate
//!    `session_key` is a programmer error and must surface as one.
//! 4. Mint the principal via [`Principal::external_agent_with_delegator`] so
//!    the call-site explicitly threads `delegated_by`.
//!
//! ## Integration seam (deferred to PR3)
//!
//! There is no production `Op::Register` dispatch arm in
//! `sera-gateway/src/bin/sera.rs` today — `Op::Register(_)` is a wildcard
//! audit arm in [`session_store::op_kind_label`] and a no-op in
//! [`sera_runtime::stdio`]. PR2 ships the pure handler + module-seam
//! integration tests; the production wire-up (call this registry from a
//! `Submission` arrival, emit `EventMsg::SessionTransition` on success and
//! `EventMsg::Error { code, message }` on failure) is the smallest PR3
//! delta. See `tests/register_op_external_agent.rs` for the seam shape.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};

use chrono::{DateTime, Utc};
use thiserror::Error;

use sera_types::envelope::{BudgetHandle, ExternalAgentRegistration, ExternalProtocol};
use sera_types::principal::{Principal, PrincipalKind, PrincipalRef};
use sera_types::sandbox::EgressEndpoint;

/// One Pattern B session pinned by the gateway at registration time.
///
/// Cloneable so the registry can return a snapshot without holding the lock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalAgentSession {
    /// Lightweight reference to the freshly minted ExternalAgentPrincipal —
    /// suitable for embedding in events and audit rows.
    pub principal_ref: PrincipalRef,
    /// The principal that delegated this session into existence. Validated
    /// at registration; downstream code may trust it.
    pub delegated_by: PrincipalRef,
    /// The egress allow-list the registering session declared. Recorded as
    /// pinned scope; sera-eq0m's kernel/sandbox layer is the enforcement.
    pub declared_egress: Vec<EgressEndpoint>,
    /// Budget bucket key. Today an opaque newtype wrapper over a string;
    /// sera-plcv will replace the inner shape without re-touching this field.
    pub budget_handle: BudgetHandle,
    /// Wall-clock registration timestamp. Used by audit + `bd remember`-style
    /// observability tooling; not load-bearing for authorization.
    pub registered_at: DateTime<Utc>,
}

/// Errors a Pattern B registration can surface back to the SQ caller.
///
/// `code()` returns the stable wire identifier the production dispatch arm
/// (PR3) will copy into [`EventMsg::Error::code`]. Tests pin both the wire
/// code and the human-readable message so a future seam refactor can't
/// silently downgrade the contract.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum RegisterError {
    /// The submitted `delegated_by` principal is not on the validator's
    /// allow-list, or is the wrong kind. The wire code is
    /// `register_op_invalid_delegator`.
    #[error("register_op_invalid_delegator: unknown or wrong-kind delegator '{0}'")]
    InvalidDelegator(String),

    /// The LLM proxy is enabled but the registration did not include
    /// [`EgressEndpoint::InferenceLocal`] in `declared_egress`. Wire code
    /// `register_op_egress_invalid`.
    #[error(
        "register_op_egress_invalid: declared_egress must include InferenceLocal when the LLM proxy is enabled"
    )]
    EgressInvalid,

    /// Another Pattern B session is already registered under this
    /// `session_key`. Wire code `register_op_duplicate_session`. Replace
    /// semantics are intentionally NOT supported — see module docs.
    #[error("register_op_duplicate_session: session_key '{0}' already bound")]
    DuplicateSession(String),
}

impl RegisterError {
    /// Stable wire-level error code consumed by `EventMsg::Error::code` in
    /// the PR3 dispatch arm.
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidDelegator(_) => "register_op_invalid_delegator",
            Self::EgressInvalid => "register_op_egress_invalid",
            Self::DuplicateSession(_) => "register_op_duplicate_session",
        }
    }
}

/// Resolves whether a [`PrincipalRef`] is permitted to be a `delegated_by`
/// for a Pattern B registration.
///
/// Today the gateway has no production principal store — the closest seams
/// are the API-key middleware (which operates on tokens, not principals) and
/// `PrincipalPolicyStore` (which serves policy, not existence). PR3 will
/// back this trait with whatever store sera-db lands; PR2 ships an in-memory
/// allow-list so the handler logic is testable without a database fixture.
pub trait DelegatorValidator: Send + Sync {
    /// Returns `true` iff the principal is a valid Pattern B delegator.
    /// Implementations MUST also enforce the kind invariant: only
    /// [`PrincipalKind::Agent`] and [`PrincipalKind::Human`] may delegate
    /// (see SPEC-gateway §1d / scout report §2.5 step 2).
    fn is_valid(&self, delegator: &PrincipalRef) -> bool;
}

/// In-memory delegator allow-list for tests and bootstrap fixtures.
///
/// Production gateways will swap this for a sera-db-backed implementation in
/// PR3 without re-shaping callers.
#[derive(Debug, Default, Clone)]
pub struct InMemoryDelegatorValidator {
    known: HashSet<PrincipalRef>,
}

impl InMemoryDelegatorValidator {
    pub fn new() -> Self {
        Self::default()
    }

    /// Builder-style add for fixture setup.
    pub fn with_known(mut self, delegator: PrincipalRef) -> Self {
        self.known.insert(delegator);
        self
    }

    pub fn add(&mut self, delegator: PrincipalRef) {
        self.known.insert(delegator);
    }
}

impl DelegatorValidator for InMemoryDelegatorValidator {
    fn is_valid(&self, delegator: &PrincipalRef) -> bool {
        // Wrong-kind rejection is structural — only Agent/Human may delegate
        // a Pattern B session. The allow-list check is the existence gate.
        matches!(
            delegator.kind,
            PrincipalKind::Agent | PrincipalKind::Human
        ) && self.known.contains(delegator)
    }
}

/// Registry of currently-active Pattern B sessions, keyed by the SQ envelope
/// `session_key`.
///
/// Owns the `inference_proxy_enabled` flag so the egress invariant is
/// enforced at registration rather than threaded through every caller.
pub struct ExternalAgentRegistry {
    sessions: RwLock<HashMap<String, ExternalAgentSession>>,
    inference_proxy_enabled: bool,
    delegator_validator: Arc<dyn DelegatorValidator>,
}

impl ExternalAgentRegistry {
    /// Build a registry. `inference_proxy_enabled` mirrors the gateway's
    /// `--inference-proxy` flag (today: `inference_proxy_eq0m_acceptance`
    /// fixtures). When `true`, registrations missing
    /// [`EgressEndpoint::InferenceLocal`] are rejected.
    pub fn new(
        inference_proxy_enabled: bool,
        delegator_validator: Arc<dyn DelegatorValidator>,
    ) -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
            inference_proxy_enabled,
            delegator_validator,
        }
    }

    /// Register a Pattern B session. The `session_key` is the SQ envelope's
    /// `Submission::session_key` — the registry does not invent one.
    ///
    /// On success, returns a clone of the stored [`ExternalAgentSession`] so
    /// the caller can immediately reference the minted principal without
    /// re-locking. On failure, the registry state is unchanged.
    pub fn register(
        &self,
        session_key: &str,
        registration: &ExternalAgentRegistration,
    ) -> Result<ExternalAgentSession, RegisterError> {
        if !self
            .delegator_validator
            .is_valid(&registration.delegated_by)
        {
            return Err(RegisterError::InvalidDelegator(
                registration.delegated_by.id.0.clone(),
            ));
        }

        if self.inference_proxy_enabled
            && !registration
                .declared_egress
                .iter()
                .any(|e| matches!(e, EgressEndpoint::InferenceLocal))
        {
            return Err(RegisterError::EgressInvalid);
        }

        let mut guard = self.sessions.write().expect("sessions lock poisoned");
        if guard.contains_key(session_key) {
            return Err(RegisterError::DuplicateSession(session_key.to_string()));
        }

        let principal = Principal::external_agent_with_delegator(
            protocol_label(&registration.protocol).as_str(),
            &registration.external_id,
            &registration.delegated_by,
        );

        let session = ExternalAgentSession {
            principal_ref: principal.as_ref(),
            delegated_by: registration.delegated_by.clone(),
            declared_egress: registration.declared_egress.clone(),
            budget_handle: registration.budget_handle.clone(),
            registered_at: Utc::now(),
        };

        guard.insert(session_key.to_string(), session.clone());
        Ok(session)
    }

    /// Look up a stored session by its SQ envelope `session_key`. Returns
    /// `None` if no Pattern B session has registered under that key.
    pub fn get(&self, session_key: &str) -> Option<ExternalAgentSession> {
        self.sessions
            .read()
            .expect("sessions lock poisoned")
            .get(session_key)
            .cloned()
    }

    /// Number of currently-registered Pattern B sessions. Test introspection.
    pub fn registered_count(&self) -> usize {
        self.sessions
            .read()
            .expect("sessions lock poisoned")
            .len()
    }
}

/// Snake-case protocol label used in `Principal::id = "ext:{protocol}:{external_id}"`.
fn protocol_label(protocol: &ExternalProtocol) -> String {
    match protocol {
        ExternalProtocol::A2a => "a2a".to_string(),
        ExternalProtocol::AcpCompat => "acp_compat".to_string(),
        ExternalProtocol::Custom(s) => s.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sera_types::principal::PrincipalId;

    fn agent_ref(id: &str) -> PrincipalRef {
        PrincipalRef {
            id: PrincipalId::new(id),
            kind: PrincipalKind::Agent,
        }
    }

    fn human_ref(id: &str) -> PrincipalRef {
        PrincipalRef {
            id: PrincipalId::new(id),
            kind: PrincipalKind::Human,
        }
    }

    fn registration(
        protocol: ExternalProtocol,
        external_id: &str,
        delegated_by: PrincipalRef,
    ) -> ExternalAgentRegistration {
        ExternalAgentRegistration {
            protocol,
            external_id: external_id.to_string(),
            delegated_by,
            declared_egress: vec![EgressEndpoint::InferenceLocal],
            budget_handle: BudgetHandle(format!("ext:bucket:{external_id}")),
        }
    }

    fn registry_with_known(known: PrincipalRef) -> ExternalAgentRegistry {
        let validator = Arc::new(InMemoryDelegatorValidator::new().with_known(known));
        ExternalAgentRegistry::new(true, validator)
    }

    #[test]
    fn register_then_lookup_returns_session() {
        let parent = agent_ref("agent:parent-1");
        let registry = registry_with_known(parent.clone());
        let reg = registration(ExternalProtocol::A2a, "claude-pane-1", parent.clone());

        let session = registry
            .register("sess-001", &reg)
            .expect("registration should succeed");

        assert_eq!(session.principal_ref.id.0, "ext:a2a:claude-pane-1");
        assert_eq!(session.principal_ref.kind, PrincipalKind::ExternalAgent);
        assert_eq!(session.delegated_by, parent);
        assert_eq!(session.declared_egress, vec![EgressEndpoint::InferenceLocal]);
        assert_eq!(session.budget_handle.0, "ext:bucket:claude-pane-1");

        let stored = registry.get("sess-001").expect("session stored");
        assert_eq!(stored, session);
        assert_eq!(registry.registered_count(), 1);
    }

    #[test]
    fn unknown_delegator_rejected() {
        let known = agent_ref("agent:known");
        let stranger = agent_ref("agent:stranger");
        let registry = registry_with_known(known);

        let err = registry
            .register("sess-002", &registration(ExternalProtocol::A2a, "x", stranger))
            .unwrap_err();

        assert_eq!(err.code(), "register_op_invalid_delegator");
        assert!(matches!(err, RegisterError::InvalidDelegator(ref id) if id == "agent:stranger"));
        assert_eq!(registry.registered_count(), 0);
    }

    #[test]
    fn wrong_kind_delegator_rejected_even_when_id_in_allowlist() {
        // Allow-list contains an *Agent*-kind principal, but the submitter
        // claims an *ExternalAgent*-kind delegator with the same id. That is
        // a kind-confusion attempt and must be rejected.
        let agent = agent_ref("agent:p");
        let validator = Arc::new(InMemoryDelegatorValidator::new().with_known(agent.clone()));
        let registry = ExternalAgentRegistry::new(true, validator);

        let confused = PrincipalRef {
            id: agent.id.clone(),
            kind: PrincipalKind::ExternalAgent,
        };
        let err = registry
            .register(
                "sess-003",
                &registration(ExternalProtocol::A2a, "x", confused),
            )
            .unwrap_err();

        assert_eq!(err.code(), "register_op_invalid_delegator");
        assert_eq!(registry.registered_count(), 0);
    }

    #[test]
    fn human_delegator_accepted() {
        let operator = human_ref("admin");
        let registry = registry_with_known(operator.clone());

        let session = registry
            .register(
                "sess-004",
                &registration(ExternalProtocol::AcpCompat, "ops-pane", operator.clone()),
            )
            .expect("human delegator must be valid");

        assert_eq!(session.principal_ref.id.0, "ext:acp_compat:ops-pane");
        assert_eq!(session.delegated_by, operator);
    }

    #[test]
    fn duplicate_session_key_rejected() {
        let parent = agent_ref("agent:parent");
        let registry = registry_with_known(parent.clone());

        registry
            .register("sess-dup", &registration(ExternalProtocol::A2a, "first", parent.clone()))
            .expect("first registration should succeed");

        let err = registry
            .register(
                "sess-dup",
                &registration(ExternalProtocol::A2a, "second", parent),
            )
            .unwrap_err();

        assert_eq!(err.code(), "register_op_duplicate_session");
        assert!(matches!(err, RegisterError::DuplicateSession(ref k) if k == "sess-dup"));
        // The first registration is still the one that holds the slot — the
        // duplicate did NOT replace it.
        let stored = registry.get("sess-dup").unwrap();
        assert_eq!(stored.principal_ref.id.0, "ext:a2a:first");
        assert_eq!(registry.registered_count(), 1);
    }

    #[test]
    fn same_external_id_distinct_protocol_ok() {
        // Two sessions can share an `external_id` as long as their protocol
        // (or session_key) differs — the principal id includes the protocol,
        // and the registry is keyed by session_key.
        let parent = agent_ref("agent:parent");
        let registry = registry_with_known(parent.clone());

        let a = registry
            .register(
                "sess-a2a",
                &registration(ExternalProtocol::A2a, "bot", parent.clone()),
            )
            .unwrap();
        let b = registry
            .register(
                "sess-acp",
                &registration(ExternalProtocol::AcpCompat, "bot", parent),
            )
            .unwrap();

        assert_eq!(a.principal_ref.id.0, "ext:a2a:bot");
        assert_eq!(b.principal_ref.id.0, "ext:acp_compat:bot");
        assert_ne!(a.principal_ref, b.principal_ref);
        assert_eq!(registry.registered_count(), 2);
    }

    #[test]
    fn missing_inference_local_rejected_when_proxy_enabled() {
        let parent = agent_ref("agent:parent");
        let validator = Arc::new(InMemoryDelegatorValidator::new().with_known(parent.clone()));
        let registry = ExternalAgentRegistry::new(true, validator);

        let mut reg = registration(ExternalProtocol::A2a, "x", parent);
        reg.declared_egress = vec![EgressEndpoint::Domain {
            name: "api.github.com".to_string(),
        }];

        let err = registry.register("sess-egress", &reg).unwrap_err();
        assert_eq!(err.code(), "register_op_egress_invalid");
        assert_eq!(registry.registered_count(), 0);
    }

    #[test]
    fn missing_inference_local_accepted_when_proxy_disabled() {
        // Off-proxy deployments do not require InferenceLocal — the egress
        // invariant is the LLM proxy's invariant, not a global one.
        let parent = agent_ref("agent:parent");
        let validator = Arc::new(InMemoryDelegatorValidator::new().with_known(parent.clone()));
        let registry = ExternalAgentRegistry::new(false, validator);

        let mut reg = registration(ExternalProtocol::A2a, "x", parent);
        reg.declared_egress = vec![]; // empty allow-list

        let session = registry
            .register("sess-noproxy", &reg)
            .expect("proxy-disabled registry must accept any egress");
        assert!(session.declared_egress.is_empty());
    }

    #[test]
    fn custom_protocol_label_threaded_into_principal_id() {
        let parent = agent_ref("agent:parent");
        let registry = registry_with_known(parent.clone());

        let session = registry
            .register(
                "sess-custom",
                &registration(
                    ExternalProtocol::Custom("hermes".to_string()),
                    "profile-7",
                    parent,
                ),
            )
            .unwrap();

        assert_eq!(session.principal_ref.id.0, "ext:hermes:profile-7");
    }

    #[test]
    fn register_error_codes_are_stable_wire_strings() {
        // PR3's dispatch arm copies these into EventMsg::Error::code; pin
        // them so a refactor cannot silently break the wire contract.
        assert_eq!(
            RegisterError::InvalidDelegator("x".into()).code(),
            "register_op_invalid_delegator"
        );
        assert_eq!(RegisterError::EgressInvalid.code(), "register_op_egress_invalid");
        assert_eq!(
            RegisterError::DuplicateSession("k".into()).code(),
            "register_op_duplicate_session"
        );
    }
}
