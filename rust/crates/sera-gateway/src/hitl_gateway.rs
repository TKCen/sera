//! HITL gateway plumbing — Wave D Phase 1 (sera-z6ql).
//!
//! This module owns the pieces that connect the sera-hitl crate (router,
//! ticket state machine, escalation chains) to the HTTP gateway:
//!
//! - [`TicketStore`] trait + [`InMemoryTicketStore`] default implementation.
//! - Helpers for resolving an [`AgentSpec`]'s HITL configuration into
//!   concrete `sera_hitl` types.
//! - Phase 1 decision: the consultation in `chat_handler` only *blocks and
//!   tickets* when approval is required. No suspension or resume — that is
//!   Phase 2 (follow-up bead).

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use sera_hitl::{ApprovalRouting, ApprovalTicket, HitlMode};
use sera_types::config_manifest::AgentSpec;

// ── TicketStore ──────────────────────────────────────────────────────────────

/// Errors surfaced from [`TicketStore`] operations. Kept intentionally small
/// for Phase 1 — the only failure modes are "not found" and "backend error".
#[derive(Debug, thiserror::Error)]
pub enum TicketStoreError {
    #[error("ticket not found: {id}")]
    NotFound { id: String },
    #[error("ticket store backend error: {reason}")]
    Backend { reason: String },
}

/// Persistence boundary for approval tickets.
///
/// Phase 1 uses [`InMemoryTicketStore`] exclusively. A SQLite-backed store
/// (mirroring `SqliteGitSessionStore`) is a follow-up.
#[async_trait::async_trait]
pub trait TicketStore: Send + Sync {
    /// Persist a freshly minted ticket. Replaces any existing entry with the
    /// same ID — tickets are immutable after creation except through
    /// `update_*` calls on this trait.
    async fn insert(&self, ticket: ApprovalTicket) -> Result<(), TicketStoreError>;

    /// Fetch a ticket by ID. Returns [`TicketStoreError::NotFound`] when the
    /// ticket does not exist.
    async fn get(&self, id: &str) -> Result<ApprovalTicket, TicketStoreError>;

    /// List every ticket currently in the store. Callers filter client-side;
    /// pagination lives in Phase 2.
    async fn list(&self) -> Result<Vec<ApprovalTicket>, TicketStoreError>;

    /// Overwrite an existing ticket with a mutated copy (after approve,
    /// reject, or escalate). Returns [`TicketStoreError::NotFound`] if the
    /// ticket is unknown — callers must `get` first to obtain the current
    /// state.
    async fn update(&self, ticket: ApprovalTicket) -> Result<(), TicketStoreError>;
}

/// Process-local [`TicketStore`] backed by a `HashMap`. Sufficient for Phase 1
/// — tickets are ephemeral anyway (Phase 1 does not resume suspended turns).
#[derive(Default)]
pub struct InMemoryTicketStore {
    inner: RwLock<HashMap<String, ApprovalTicket>>,
}

impl InMemoryTicketStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait::async_trait]
impl TicketStore for InMemoryTicketStore {
    async fn insert(&self, ticket: ApprovalTicket) -> Result<(), TicketStoreError> {
        let mut map = self.inner.write().await;
        map.insert(ticket.id.clone(), ticket);
        Ok(())
    }

    async fn get(&self, id: &str) -> Result<ApprovalTicket, TicketStoreError> {
        let map = self.inner.read().await;
        map.get(id)
            .cloned()
            .ok_or_else(|| TicketStoreError::NotFound { id: id.to_owned() })
    }

    async fn list(&self) -> Result<Vec<ApprovalTicket>, TicketStoreError> {
        let map = self.inner.read().await;
        Ok(map.values().cloned().collect())
    }

    async fn update(&self, ticket: ApprovalTicket) -> Result<(), TicketStoreError> {
        let mut map = self.inner.write().await;
        if !map.contains_key(&ticket.id) {
            return Err(TicketStoreError::NotFound {
                id: ticket.id.clone(),
            });
        }
        map.insert(ticket.id.clone(), ticket);
        Ok(())
    }
}

// ── AgentSpec → HITL config resolution ───────────────────────────────────────

/// Resolve an [`AgentSpec`]'s opaque `enforcement_mode` string into a concrete
/// [`HitlMode`]. Defaults to [`HitlMode::Autonomous`] when absent or when
/// parsing fails — fail-open preserves the pre-wiring behaviour for agents
/// with no explicit HITL configuration.
pub fn resolve_hitl_mode(spec: &AgentSpec) -> HitlMode {
    match spec.enforcement_mode.as_deref() {
        Some(raw) => {
            let json = format!("\"{}\"", raw);
            serde_json::from_str::<HitlMode>(&json).unwrap_or(HitlMode::Autonomous)
        }
        None => HitlMode::Autonomous,
    }
}

/// Resolve an [`AgentSpec`]'s opaque `approval_policy` JSON blob into a
/// concrete [`ApprovalRouting`]. Defaults to [`ApprovalRouting::Autonomous`]
/// when absent or when deserialisation fails.
pub fn resolve_approval_routing(spec: &AgentSpec) -> ApprovalRouting {
    match spec.approval_policy.as_ref() {
        Some(value) => serde_json::from_value::<ApprovalRouting>(value.clone())
            .unwrap_or(ApprovalRouting::Autonomous),
        None => ApprovalRouting::Autonomous,
    }
}

// ── AppState trait abstraction for the HITL routes ───────────────────────────

/// Abstraction over the binary's `AppState` so the HITL HTTP handlers can
/// live in the library half of the crate (following the existing pattern in
/// `routes/plugins.rs`, `routes/a2a.rs`, etc.).
pub trait HitlAppState: Send + Sync + 'static {
    fn api_key(&self) -> &Option<String>;
    fn ticket_store(&self) -> Arc<dyn TicketStore>;
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use sera_hitl::{
        ApprovalEvidence, ApprovalScope, ApprovalSpec, ApprovalUrgency,
    };
    use sera_types::principal::Principal;
    use sera_types::tool::RiskLevel;
    use std::time::Duration;

    fn sample_ticket() -> ApprovalTicket {
        let spec = ApprovalSpec {
            scope: ApprovalScope::ToolCall {
                tool_name: "shell".to_string(),
                risk_level: RiskLevel::Execute,
            },
            description: "test ticket".to_string(),
            urgency: ApprovalUrgency::Medium,
            routing: ApprovalRouting::Autonomous,
            timeout: Duration::from_secs(300),
            required_approvals: 1,
            evidence: ApprovalEvidence {
                tool_args: None,
                risk_score: None,
                principal: Principal::default_admin().as_ref(),
                session_context: None,
                additional: Default::default(),
            },
        };
        ApprovalTicket::new(spec, "session-1")
    }

    #[tokio::test]
    async fn in_memory_insert_and_get() {
        let store = InMemoryTicketStore::new();
        let ticket = sample_ticket();
        let id = ticket.id.clone();
        store.insert(ticket).await.unwrap();
        let got = store.get(&id).await.unwrap();
        assert_eq!(got.id, id);
    }

    #[tokio::test]
    async fn in_memory_get_missing_is_not_found() {
        let store = InMemoryTicketStore::new();
        let err = store.get("nope").await.unwrap_err();
        assert!(matches!(err, TicketStoreError::NotFound { .. }));
    }

    #[tokio::test]
    async fn in_memory_list_returns_all() {
        let store = InMemoryTicketStore::new();
        store.insert(sample_ticket()).await.unwrap();
        store.insert(sample_ticket()).await.unwrap();
        let all = store.list().await.unwrap();
        assert_eq!(all.len(), 2);
    }

    #[tokio::test]
    async fn in_memory_update_missing_is_not_found() {
        let store = InMemoryTicketStore::new();
        let ticket = sample_ticket();
        let err = store.update(ticket).await.unwrap_err();
        assert!(matches!(err, TicketStoreError::NotFound { .. }));
    }

    #[tokio::test]
    async fn in_memory_update_persists_new_state() {
        let store = InMemoryTicketStore::new();
        let ticket = sample_ticket();
        let id = ticket.id.clone();
        store.insert(ticket.clone()).await.unwrap();

        let mut mutated = ticket;
        mutated
            .approve(Principal::default_admin().as_ref(), Some("ok".into()))
            .unwrap();
        store.update(mutated).await.unwrap();

        let got = store.get(&id).await.unwrap();
        assert_eq!(got.status, sera_hitl::TicketStatus::Approved);
    }

    #[test]
    fn resolve_mode_defaults_to_autonomous() {
        let spec = AgentSpec {
            provider: "x".into(),
            model: None,
            persona: None,
            tools: None,
            workspace: None,
            policy_ref: None,
            enforcement_mode: None,
            approval_policy: None,
        };
        assert_eq!(resolve_hitl_mode(&spec), HitlMode::Autonomous);
    }

    #[test]
    fn resolve_mode_parses_strict() {
        let spec = AgentSpec {
            provider: "x".into(),
            model: None,
            persona: None,
            tools: None,
            workspace: None,
            policy_ref: None,
            enforcement_mode: Some("strict".into()),
            approval_policy: None,
        };
        assert_eq!(resolve_hitl_mode(&spec), HitlMode::Strict);
    }

    #[test]
    fn resolve_mode_unknown_value_falls_back_to_autonomous() {
        let spec = AgentSpec {
            provider: "x".into(),
            model: None,
            persona: None,
            tools: None,
            workspace: None,
            policy_ref: None,
            enforcement_mode: Some("bogus".into()),
            approval_policy: None,
        };
        assert_eq!(resolve_hitl_mode(&spec), HitlMode::Autonomous);
    }

    #[test]
    fn resolve_routing_defaults_to_autonomous() {
        let spec = AgentSpec {
            provider: "x".into(),
            model: None,
            persona: None,
            tools: None,
            workspace: None,
            policy_ref: None,
            enforcement_mode: None,
            approval_policy: None,
        };
        assert!(matches!(
            resolve_approval_routing(&spec),
            ApprovalRouting::Autonomous
        ));
    }

    #[test]
    fn resolve_routing_parses_static() {
        let json = serde_json::json!({
            "mode": "static",
            "targets": [{ "kind": "role", "name": "ops" }],
        });
        let spec = AgentSpec {
            provider: "x".into(),
            model: None,
            persona: None,
            tools: None,
            workspace: None,
            policy_ref: None,
            enforcement_mode: None,
            approval_policy: Some(json),
        };
        let routing = resolve_approval_routing(&spec);
        match routing {
            ApprovalRouting::Static { targets } => assert_eq!(targets.len(), 1),
            other => panic!("expected Static, got {other:?}"),
        }
    }
}
