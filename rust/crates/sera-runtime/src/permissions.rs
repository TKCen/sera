//! Permission mode escalation for agent runtime (GH#545 — bead sera-ddz).
//!
//! At runtime, an agent can request elevated capabilities for a bounded
//! operation (e.g. escalate from `Standard` to `Elevated` tier for one tool
//! call), subject to HITL approval. If HITL denies, the operation fails with
//! a clear [`EscalationError::Denied`].
//!
//! This module defines the **state machine + API surface** for escalation.
//! [`HitlEscalationAuthority`] is the production seam; until the
//! `sera-hitl::ApprovalRouter` integration lands it is a fail-closed
//! placeholder (sera-ytgx) — see its doc comment.

use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ── Permission mode ─────────────────────────────────────────────────────────

/// Caller permission tier.
///
/// Ordering: `Standard < Elevated < Admin`. The [`Ord`] impl is total so
/// `caller_mode >= tool_mode` is the natural capability gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionMode {
    /// Default tier — read-class tools, no privileged side effects.
    Standard,
    /// Elevated tier — write-class tools and most execute-class tools.
    Elevated,
    /// Admin tier — full capability, including policy / config mutation.
    Admin,
}

impl PermissionMode {
    /// True if `self` grants at least the capability of `required`.
    pub fn satisfies(self, required: PermissionMode) -> bool {
        self >= required
    }
}

// ── Escalation scope ────────────────────────────────────────────────────────

/// The scope for which an escalation, if granted, is valid.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EscalationScope {
    /// Grant applies to a single tool call, then expires.
    SingleCall,
    /// Grant applies for the remainder of the session.
    Session,
    /// Grant applies for a bounded wall-clock duration.
    Bounded {
        /// Duration measured from the moment of approval.
        #[serde(with = "duration_millis")]
        duration: Duration,
    },
}

mod duration_millis {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::Duration;

    pub fn serialize<S: Serializer>(value: &Duration, ser: S) -> Result<S::Ok, S::Error> {
        ser.serialize_u64(value.as_millis() as u64)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(de: D) -> Result<Duration, D::Error> {
        let millis = u64::deserialize(de)?;
        Ok(Duration::from_millis(millis))
    }
}

// ── Escalation request / decision ───────────────────────────────────────────

/// A runtime request to escalate from one [`PermissionMode`] to another.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EscalationRequest {
    /// The caller's current permission mode.
    pub from: PermissionMode,
    /// The requested higher (or equal) mode.
    pub to: PermissionMode,
    /// Human-readable reason recorded on the audit trail.
    pub reason: String,
    /// Scope of the requested grant.
    pub scope: EscalationScope,
}

impl EscalationRequest {
    /// Construct a single-call request.
    pub fn single_call(
        from: PermissionMode,
        to: PermissionMode,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            from,
            to,
            reason: reason.into(),
            scope: EscalationScope::SingleCall,
        }
    }
}

/// The authority's decision on an [`EscalationRequest`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EscalationDecision {
    /// Whether the escalation was granted.
    pub granted: bool,
    /// The granted mode, if any. For a grant this is typically
    /// `Some(request.to)` but an authority MAY down-grade to a lower tier.
    pub granted_mode: Option<PermissionMode>,
    /// Wall-clock expiry, if applicable. `None` for [`EscalationScope::SingleCall`]
    /// (the caller is responsible for consuming it exactly once).
    pub expires_at: Option<DateTime<Utc>>,
    /// Reason surfaced on denial (or down-grade).
    pub denial_reason: Option<String>,
}

impl EscalationDecision {
    /// Construct an approval decision for the full requested mode.
    pub fn approve(
        to: PermissionMode,
        expires_at: Option<DateTime<Utc>>,
    ) -> Self {
        Self {
            granted: true,
            granted_mode: Some(to),
            expires_at,
            denial_reason: None,
        }
    }

    /// Construct a denial decision.
    pub fn deny(reason: impl Into<String>) -> Self {
        Self {
            granted: false,
            granted_mode: None,
            expires_at: None,
            denial_reason: Some(reason.into()),
        }
    }
}

// ── Errors ──────────────────────────────────────────────────────────────────

/// Failure modes surfaced by an [`EscalationAuthority`].
#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum EscalationError {
    /// The authority denied the request.
    #[error("escalation denied: {0}")]
    Denied(String),
    /// The request was malformed (e.g. `to < from`).
    #[error("invalid escalation request: {0}")]
    Invalid(String),
    /// The authority is currently unavailable (queue, RPC failure, …).
    #[error("escalation authority unavailable: {0}")]
    Unavailable(String),
}

// ── Authority trait ─────────────────────────────────────────────────────────

/// Trait for requesting a permission escalation.
///
/// [`StubEscalationAuthority`] auto-approves every request and is intended
/// for tests + early integration only. [`HitlEscalationAuthority`] is the
/// production seam; until the `sera-hitl::ApprovalRouter` wire-up lands it
/// is fail-closed (returns [`EscalationError::Unavailable`]) rather than
/// delegating to the stub.
///
/// Chosen shape:
/// - **async** — matches `sera-hitl::ApprovalRouter::needs_approval` callers,
///   which are async all the way down.
/// - **`&self`** — authorities are expected to be shared behind `Arc`; any
///   internal mutation uses interior mutability.
#[async_trait::async_trait]
pub trait EscalationAuthority: Send + Sync {
    /// Submit an escalation request.
    async fn request_escalation(
        &self,
        req: EscalationRequest,
    ) -> Result<EscalationDecision, EscalationError>;
}

// ── Stub authority ──────────────────────────────────────────────────────────

/// Auto-approving authority for tests and early integration.
///
/// Rejects only requests that would *lower* the caller's tier
/// (`to < from`) — those are treated as invalid.
#[derive(Debug, Default, Clone, Copy)]
pub struct StubEscalationAuthority;

#[async_trait::async_trait]
impl EscalationAuthority for StubEscalationAuthority {
    async fn request_escalation(
        &self,
        req: EscalationRequest,
    ) -> Result<EscalationDecision, EscalationError> {
        if req.to < req.from {
            return Err(EscalationError::Invalid(format!(
                "requested mode {:?} is lower than current {:?}",
                req.to, req.from
            )));
        }
        let expires_at = match req.scope {
            EscalationScope::SingleCall => None,
            EscalationScope::Session => None,
            EscalationScope::Bounded { duration } => {
                Some(Utc::now() + chrono::Duration::from_std(duration).unwrap_or_default())
            }
        };
        Ok(EscalationDecision::approve(req.to, expires_at))
    }
}

// ── HITL authority (fail-closed placeholder) ───────────────────────────────

/// Fail-closed placeholder for the production HITL escalation seam.
///
/// **Status (sera-ytgx):** this authority is intentionally a fail-closed
/// placeholder. The real `sera-hitl::ApprovalRouter` integration (mapping
/// [`EscalationRequest`] to an `ApprovalSpec`, submitting it to a shared
/// `ApprovalRouter`, and awaiting its terminal state) has not landed yet,
/// so until it does, [`HitlEscalationAuthority::request_escalation`] always
/// returns [`EscalationError::Unavailable`] rather than delegating to
/// [`StubEscalationAuthority`].
///
/// Why fail-closed instead of delegating to the stub: under strict HITL the
/// stub auto-approves every request, which would silently turn a "requires
/// HITL approval" gate into "approved" — exactly the silent bypass this
/// authority must not allow in production. Returning `Unavailable` keeps
/// the gate honest; the dispatcher maps it to a permission denial.
///
/// Tests and early integration that want auto-approval should construct
/// [`StubEscalationAuthority`] directly, or a custom authority backed by
/// their test fixtures.
///
/// Follow-up: replace this with an adapter over
/// `sera-hitl::ApprovalRouter` once it exposes an async "submit and await"
/// surface. See sera-z6ql / sera-93h4 for the routing/suspend-resume
/// integration.
#[derive(Debug, Default, Clone, Copy)]
pub struct HitlEscalationAuthority;

#[async_trait::async_trait]
impl EscalationAuthority for HitlEscalationAuthority {
    async fn request_escalation(
        &self,
        _req: EscalationRequest,
    ) -> Result<EscalationDecision, EscalationError> {
        Err(EscalationError::Unavailable(
            "HitlEscalationAuthority is a fail-closed placeholder until the \
             sera-hitl ApprovalRouter integration lands; no auto-approval \
             via stub is permitted in production"
                .to_string(),
        ))
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_mode_ordering() {
        assert!(PermissionMode::Standard < PermissionMode::Elevated);
        assert!(PermissionMode::Elevated < PermissionMode::Admin);
        assert!(PermissionMode::Admin > PermissionMode::Standard);
    }

    #[test]
    fn permission_mode_satisfies() {
        assert!(PermissionMode::Admin.satisfies(PermissionMode::Standard));
        assert!(PermissionMode::Elevated.satisfies(PermissionMode::Elevated));
        assert!(!PermissionMode::Standard.satisfies(PermissionMode::Elevated));
    }

    #[tokio::test]
    async fn stub_authority_auto_approves() {
        let req = EscalationRequest::single_call(
            PermissionMode::Standard,
            PermissionMode::Elevated,
            "test",
        );
        let decision = StubEscalationAuthority
            .request_escalation(req)
            .await
            .unwrap();
        assert!(decision.granted);
        assert_eq!(decision.granted_mode, Some(PermissionMode::Elevated));
        // SingleCall → no wall-clock expiry.
        assert!(decision.expires_at.is_none());
        assert!(decision.denial_reason.is_none());
    }

    #[tokio::test]
    async fn stub_authority_rejects_down_escalation() {
        let req = EscalationRequest::single_call(
            PermissionMode::Admin,
            PermissionMode::Standard,
            "test",
        );
        let err = StubEscalationAuthority
            .request_escalation(req)
            .await
            .unwrap_err();
        assert!(matches!(err, EscalationError::Invalid(_)));
    }

    #[tokio::test]
    async fn stub_authority_bounded_scope_has_expiry() {
        let req = EscalationRequest {
            from: PermissionMode::Standard,
            to: PermissionMode::Elevated,
            reason: "bounded".to_string(),
            scope: EscalationScope::Bounded {
                duration: Duration::from_secs(60),
            },
        };
        let before = Utc::now();
        let decision = StubEscalationAuthority
            .request_escalation(req)
            .await
            .unwrap();
        assert!(decision.granted);
        let expires = decision.expires_at.expect("bounded scope sets expiry");
        assert!(expires > before);
    }

    /// Explicit-deny authority — used by the dispatcher integration test below
    /// to exercise the denial path without touching real HITL.
    #[derive(Debug, Default)]
    struct AlwaysDenyAuthority;

    #[async_trait::async_trait]
    impl EscalationAuthority for AlwaysDenyAuthority {
        async fn request_escalation(
            &self,
            _req: EscalationRequest,
        ) -> Result<EscalationDecision, EscalationError> {
            Ok(EscalationDecision::deny("always-deny"))
        }
    }

    #[tokio::test]
    async fn request_grant_then_deny() {
        let grant = StubEscalationAuthority
            .request_escalation(EscalationRequest::single_call(
                PermissionMode::Standard,
                PermissionMode::Admin,
                "grant",
            ))
            .await
            .unwrap();
        assert!(grant.granted);

        let deny = AlwaysDenyAuthority
            .request_escalation(EscalationRequest::single_call(
                PermissionMode::Standard,
                PermissionMode::Admin,
                "deny",
            ))
            .await
            .unwrap();
        assert!(!deny.granted);
        assert_eq!(deny.denial_reason.as_deref(), Some("always-deny"));
    }

    /// Regression (sera-ytgx): `HitlEscalationAuthority` must not delegate
    /// to [`StubEscalationAuthority`]. Until the real `sera-hitl::ApprovalRouter`
    /// integration lands, it is a fail-closed placeholder; otherwise strict
    /// HITL would silently auto-approve through the stub in production.
    #[tokio::test]
    async fn hitl_authority_is_fail_closed_not_stub_delegate() {
        let req = EscalationRequest::single_call(
            PermissionMode::Standard,
            PermissionMode::Elevated,
            "regression",
        );

        let stub_decision = StubEscalationAuthority
            .request_escalation(req.clone())
            .await
            .expect("stub auto-approves");
        assert!(stub_decision.granted, "stub baseline must auto-approve");

        let hitl_err = HitlEscalationAuthority
            .request_escalation(req)
            .await
            .expect_err("HITL authority must not auto-approve via stub");
        assert!(
            matches!(hitl_err, EscalationError::Unavailable(_)),
            "expected Unavailable, got {hitl_err:?}"
        );
    }

    /// Regression (sera-ytgx): even with a permissive scope and a non-trivial
    /// caller mode, `HitlEscalationAuthority` must refuse rather than fall
    /// through to stub auto-approval.
    #[tokio::test]
    async fn hitl_authority_refuses_bounded_scope_request() {
        let req = EscalationRequest {
            from: PermissionMode::Standard,
            to: PermissionMode::Admin,
            reason: "regression".to_string(),
            scope: EscalationScope::Bounded {
                duration: Duration::from_secs(60),
            },
        };
        let err = HitlEscalationAuthority
            .request_escalation(req)
            .await
            .expect_err("HITL authority must refuse, not auto-approve");
        assert!(matches!(err, EscalationError::Unavailable(_)));
    }

    #[test]
    fn escalation_scope_serde_roundtrip() {
        let scope = EscalationScope::Bounded {
            duration: Duration::from_millis(30_000),
        };
        let json = serde_json::to_string(&scope).unwrap();
        let parsed: EscalationScope = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, scope);
    }
}
