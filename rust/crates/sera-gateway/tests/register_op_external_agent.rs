//! Integration tests for `Op::Register(RegisterOp::ExternalAgent(_))`
//! handling (sera-pcvp PR2).
//!
//! These tests exercise the same module seam the production SQ admission arm
//! will call in PR3:
//!
//! 1. Pull the `Submission`'s `session_key` and the `ExternalAgentRegistration`
//!    payload from `Op::Register(RegisterOp::ExternalAgent(_))`.
//! 2. Call [`ExternalAgentRegistry::register`].
//! 3. On `Ok(session)`: emit `EventMsg::SessionTransition { from:
//!    "registering", to: "active" }` (the test fixture asserts the EventMsg
//!    shape verbatim).
//! 4. On `Err(e)`: emit `EventMsg::Error { code: e.code(), message: e.to_string() }`.
//! 5. After registration, resolve the new principal's [`PrincipalPolicy`] via
//!    [`PolicyResolver`] and verify default-deny — the registry must NOT
//!    widen the `ExternalAgent` policy default.
//!
//! Mirrors `byoh_registration_handshake.rs` style — a module-seam integration
//! without spinning up a runtime child or `AppState`. The contract under
//! test is the registry handler logic; the production wire-up that calls
//! into it is intentionally a separate (PR3) delta.

use std::sync::Arc;

use sera_gateway::external_agent_registry::{
    DelegatorValidator, ExternalAgentRegistry, InMemoryDelegatorValidator, RegisterError,
};
use sera_gateway::policy_resolution::PolicyResolver;
use sera_types::envelope::{
    BudgetHandle, EventMsg, ExternalAgentRegistration, ExternalProtocol, Op, RegisterOp,
    Submission, W3cTraceContext,
};
use sera_types::principal::{Principal, PrincipalId, PrincipalKind, PrincipalRef};
use sera_types::sandbox::EgressEndpoint;
use uuid::Uuid;

// ── fixtures ────────────────────────────────────────────────────────────────

fn agent_ref(id: &str) -> PrincipalRef {
    PrincipalRef {
        id: PrincipalId::new(id),
        kind: PrincipalKind::Agent,
    }
}

fn submission_for(session_key: &str, op: Op) -> Submission {
    Submission {
        id: Uuid::new_v4(),
        op,
        trace: W3cTraceContext::default(),
        change_artifact: None,
        session_key: Some(session_key.to_string()),
        parent_session_key: None,
        parent_task_id: None,
    }
}

fn external_agent_op(reg: ExternalAgentRegistration) -> Op {
    Op::Register(RegisterOp::ExternalAgent(reg))
}

/// The exact seam PR3 will wire into `bin/sera.rs`. Pulled out into a helper
/// so the test surface mirrors the production dispatch arm one-for-one and a
/// future refactor of either side can't drift them apart silently.
fn handle_register_submission(
    registry: &ExternalAgentRegistry,
    submission: &Submission,
) -> EventMsg {
    let Some(session_key) = submission.session_key.as_deref() else {
        // No session_key → not a Pattern B registration. PR3 will surface a
        // protocol-level error here; the test path always supplies one.
        return EventMsg::Error {
            code: "register_op_invalid_delegator".to_string(),
            message: "missing session_key".to_string(),
        };
    };
    let Op::Register(RegisterOp::ExternalAgent(ref reg)) = submission.op else {
        unreachable!("test driver must construct an ExternalAgent register op");
    };
    match registry.register(session_key, reg) {
        Ok(_) => EventMsg::SessionTransition {
            from: "registering".to_string(),
            to: "active".to_string(),
        },
        Err(e) => EventMsg::Error {
            code: e.code().to_string(),
            message: e.to_string(),
        },
    }
}

fn registry_with_proxy(parent: PrincipalRef, proxy_enabled: bool) -> ExternalAgentRegistry {
    let validator: Arc<dyn DelegatorValidator> =
        Arc::new(InMemoryDelegatorValidator::new().with_known(parent));
    ExternalAgentRegistry::new(proxy_enabled, validator)
}

fn valid_registration(parent: PrincipalRef) -> ExternalAgentRegistration {
    ExternalAgentRegistration {
        protocol: ExternalProtocol::A2a,
        external_id: "claude-code-pane-1".to_string(),
        delegated_by: parent,
        declared_egress: vec![EgressEndpoint::InferenceLocal],
        budget_handle: BudgetHandle("ext:a2a:claude-code-pane-1".to_string()),
    }
}

// ── tests ───────────────────────────────────────────────────────────────────

/// **Happy path.** A submission carrying `Op::Register(ExternalAgent(_))`
/// with a known delegator and InferenceLocal in the egress list yields a
/// `SessionTransition` event and the registry now holds the session.
#[test]
fn submit_external_agent_registers_principal() {
    let parent = agent_ref("agent:parent-1");
    let registry = registry_with_proxy(parent.clone(), true);

    let submission = submission_for("sess-1", external_agent_op(valid_registration(parent.clone())));
    let event = handle_register_submission(&registry, &submission);

    match event {
        EventMsg::SessionTransition { from, to } => {
            assert_eq!(from, "registering");
            assert_eq!(to, "active");
        }
        other => panic!("expected SessionTransition, got {other:?}"),
    }

    let stored = registry.get("sess-1").expect("session stored");
    assert_eq!(stored.principal_ref.id.0, "ext:a2a:claude-code-pane-1");
    assert_eq!(stored.principal_ref.kind, PrincipalKind::ExternalAgent);
    // Anti-laundering invariant: the delegator is recorded against the session.
    assert_eq!(stored.delegated_by, parent);
}

/// Unknown delegator → `register_op_invalid_delegator` error and no state change.
#[test]
fn submit_external_agent_with_unknown_delegator_rejected() {
    let known = agent_ref("agent:known");
    let stranger = agent_ref("agent:stranger");
    let registry = registry_with_proxy(known, true);

    let submission = submission_for(
        "sess-rej",
        external_agent_op(valid_registration(stranger)),
    );
    let event = handle_register_submission(&registry, &submission);

    match event {
        EventMsg::Error { code, .. } => {
            assert_eq!(code, "register_op_invalid_delegator");
        }
        other => panic!("expected Error event, got {other:?}"),
    }
    assert_eq!(registry.registered_count(), 0);
    assert!(registry.get("sess-rej").is_none());
}

/// After registration, the freshly minted `ExternalAgent` principal must
/// resolve to the default-deny `PrincipalPolicy`. The registry must never
/// widen this — PR3's dispatch arm relies on it for AuthZ at tool dispatch.
#[tokio::test]
async fn external_agent_principal_inherits_default_policy() {
    let parent = agent_ref("agent:parent-1");
    let registry = registry_with_proxy(parent.clone(), true);

    let submission = submission_for("sess-pol", external_agent_op(valid_registration(parent)));
    let event = handle_register_submission(&registry, &submission);
    assert!(matches!(event, EventMsg::SessionTransition { .. }));

    let stored = registry.get("sess-pol").expect("session stored");
    let principal = Principal::external_agent("a2a", "claude-code-pane-1");
    assert_eq!(stored.principal_ref, principal.as_ref());

    let resolver = PolicyResolver::in_memory();
    let policy = resolver.resolve(&principal).await;
    assert_eq!(policy.max_delegation_depth, 0, "default-deny depth");
    assert!(
        policy.allowed_tool_scopes.is_empty(),
        "default-deny must yield empty tool scopes"
    );
    assert!(
        !policy.allow_subagent_spawn,
        "default-deny must forbid subagent spawn"
    );
}

/// `delegated_by` is recorded on the session record so the audit trail can
/// trace any later action by this principal back to the parent that
/// delegated it.
#[test]
fn delegator_is_recorded_for_audit() {
    let parent = agent_ref("agent:audit-parent");
    let registry = registry_with_proxy(parent.clone(), true);

    let submission = submission_for(
        "sess-audit",
        external_agent_op(valid_registration(parent.clone())),
    );
    let event = handle_register_submission(&registry, &submission);
    assert!(matches!(event, EventMsg::SessionTransition { .. }));

    let stored = registry.get("sess-audit").unwrap();
    assert_eq!(stored.delegated_by.id.0, "agent:audit-parent");
    assert_eq!(stored.delegated_by.kind, PrincipalKind::Agent);
}

/// Egress allow-list missing `InferenceLocal` is rejected when the LLM proxy
/// is enabled — `register_op_egress_invalid`.
#[test]
fn declared_egress_must_include_inference_local_when_proxy_enabled() {
    let parent = agent_ref("agent:parent-1");
    let registry = registry_with_proxy(parent.clone(), true);

    let mut reg = valid_registration(parent);
    reg.declared_egress = vec![EgressEndpoint::Domain {
        name: "api.github.com".to_string(),
    }];

    let submission = submission_for("sess-egress", external_agent_op(reg));
    let event = handle_register_submission(&registry, &submission);

    match event {
        EventMsg::Error { code, .. } => assert_eq!(code, "register_op_egress_invalid"),
        other => panic!("expected egress error, got {other:?}"),
    }
    assert_eq!(registry.registered_count(), 0);
}

/// Egress invariant is *only* the LLM proxy's invariant — when the proxy is
/// disabled, missing `InferenceLocal` is acceptable.
#[test]
fn declared_egress_relaxed_when_proxy_disabled() {
    let parent = agent_ref("agent:parent-1");
    let registry = registry_with_proxy(parent.clone(), false);

    let mut reg = valid_registration(parent);
    reg.declared_egress = vec![];

    let submission = submission_for("sess-noproxy", external_agent_op(reg));
    let event = handle_register_submission(&registry, &submission);
    assert!(matches!(event, EventMsg::SessionTransition { .. }));
    assert_eq!(registry.registered_count(), 1);
}

/// Re-registering the same `session_key` is rejected with
/// `register_op_duplicate_session`. The first session keeps the slot — the
/// duplicate did NOT replace it. (Pattern B sessions are *identity*, not
/// idempotent catalogs; replace semantics would silently swap principal
/// under a live session.)
#[test]
fn duplicate_session_key_rejected_via_event_error() {
    let parent = agent_ref("agent:parent");
    let registry = registry_with_proxy(parent.clone(), true);

    let first = valid_registration(parent.clone());
    let mut second = valid_registration(parent);
    second.external_id = "different-pane".to_string();

    let event_a = handle_register_submission(
        &registry,
        &submission_for("sess-dup", external_agent_op(first)),
    );
    assert!(matches!(event_a, EventMsg::SessionTransition { .. }));

    let event_b = handle_register_submission(
        &registry,
        &submission_for("sess-dup", external_agent_op(second)),
    );
    match event_b {
        EventMsg::Error { code, .. } => assert_eq!(code, "register_op_duplicate_session"),
        other => panic!("expected duplicate error, got {other:?}"),
    }

    let stored = registry.get("sess-dup").unwrap();
    assert_eq!(
        stored.principal_ref.id.0, "ext:a2a:claude-code-pane-1",
        "first registration must keep the slot"
    );
}

/// `RegisterError::code()` is the contract surface PR3's dispatch arm copies
/// into `EventMsg::Error::code`. Pin the wire strings so a future refactor
/// cannot silently downgrade them.
#[test]
fn register_error_codes_match_wire_contract() {
    assert_eq!(
        RegisterError::InvalidDelegator("agent:x".into()).code(),
        "register_op_invalid_delegator"
    );
    assert_eq!(
        RegisterError::EgressInvalid.code(),
        "register_op_egress_invalid"
    );
    assert_eq!(
        RegisterError::DuplicateSession("k".into()).code(),
        "register_op_duplicate_session"
    );
}
