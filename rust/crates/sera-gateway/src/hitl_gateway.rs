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

// ── SuspendedTurn (Phase 2, sera-93h4) ──────────────────────────────────────

/// Captures the state needed to resume a chat turn that was blocked at the
/// gateway pre-LLM HITL gate.
///
/// Phase 2 (sera-93h4) decision: the gateway pre-gate stays as the fast-fail
/// front gate (returns 403 before the LLM runs) and the suspended-turn record
/// is what clients use to resubmit the original request once the ticket has
/// been approved. Server-side auto-replay of the blocked turn is deferred to
/// a follow-up bead — at this layer, "resume" means "the ticket cleared, now
/// the client may re-POST the same message and it will pass the gate".
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SuspendedTurn {
    /// Ticket that gates this turn.
    pub ticket_id: String,
    /// Session key the original request targeted (`http:{agent}:{session_id}`).
    pub session_key: String,
    /// Session row id in the SqliteDb. Carried so a future server-side replay
    /// can rehydrate the transcript without re-deriving it from `session_key`.
    pub session_id: String,
    /// Manifest agent name the chat handler resolved.
    pub agent_name: String,
    /// Original `req.message` payload as provided by the caller.
    pub message: String,
    /// Whether the original submission opted into SSE streaming (`req.stream`).
    /// Recorded so a future replay can match the caller's expected response
    /// shape; today it is informational only.
    #[serde(default)]
    pub stream: bool,
}

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

    /// Phase 2 (sera-93h4): persist a [`SuspendedTurn`] alongside its ticket.
    ///
    /// Called from `chat_handler` when the HITL gate mints a ticket and the
    /// caller's submission must be carried forward so it can be resumed
    /// after approval. Default is a no-op so implementors that have no
    /// Phase-2 wiring (e.g. legacy tests) compile unchanged.
    async fn record_suspended_turn(
        &self,
        _turn: SuspendedTurn,
    ) -> Result<(), TicketStoreError> {
        Ok(())
    }

    /// Phase 2 (sera-93h4): fetch a previously recorded [`SuspendedTurn`].
    ///
    /// Returns [`TicketStoreError::NotFound`] when no suspended turn was
    /// recorded for `ticket_id`. Default returns `NotFound` so legacy
    /// implementors degrade gracefully.
    async fn get_suspended_turn(
        &self,
        ticket_id: &str,
    ) -> Result<SuspendedTurn, TicketStoreError> {
        Err(TicketStoreError::NotFound {
            id: ticket_id.to_owned(),
        })
    }
}

/// Process-local [`TicketStore`] backed by a `HashMap`. Phase 2 (sera-93h4)
/// adds a sibling `suspended_turns` map keyed by ticket id so the
/// `chat_handler` can park the request payload until the ticket is approved.
#[derive(Default)]
pub struct InMemoryTicketStore {
    inner: RwLock<HashMap<String, ApprovalTicket>>,
    suspended_turns: RwLock<HashMap<String, SuspendedTurn>>,
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

    async fn record_suspended_turn(
        &self,
        turn: SuspendedTurn,
    ) -> Result<(), TicketStoreError> {
        let mut map = self.suspended_turns.write().await;
        map.insert(turn.ticket_id.clone(), turn);
        Ok(())
    }

    async fn get_suspended_turn(
        &self,
        ticket_id: &str,
    ) -> Result<SuspendedTurn, TicketStoreError> {
        let map = self.suspended_turns.read().await;
        map.get(ticket_id)
            .cloned()
            .ok_or_else(|| TicketStoreError::NotFound {
                id: ticket_id.to_owned(),
            })
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

// ── HITL resume event channel (Phase 2, sera-93h4) ──────────────────────────

/// Notification emitted on the HITL resume broadcast channel after a
/// suspended ticket transitions to `Approved`.
///
/// Subscribers (typically a TUI watching `GET /api/hitl/events` or a long-
/// poll loop in a CLI) use this to know when their previously 403'd chat
/// turn may safely be resubmitted. The channel carries the original
/// `ticket_id` and the `session_key` of the blocked turn — that pair is the
/// minimal correlation the caller needs to match a resumed event to its
/// in-flight retry.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HitlResumedEvent {
    pub ticket_id: String,
    pub session_key: String,
}

/// Broadcast handle shared with subscribers. Wrapped in [`Arc`] in
/// `AppState` so the route handlers can [`tokio::sync::broadcast::Sender::send`]
/// without taking a write lock.
pub type HitlResumedSender = tokio::sync::broadcast::Sender<HitlResumedEvent>;

// ── AppState trait abstraction for the HITL routes ───────────────────────────

/// Abstraction over the binary's `AppState` so the HITL HTTP handlers can
/// live in the library half of the crate (following the existing pattern in
/// `routes/plugins.rs`, `routes/a2a.rs`, etc.).
pub trait HitlAppState: Send + Sync + 'static {
    fn api_key(&self) -> &Option<String>;
    fn ticket_store(&self) -> Arc<dyn TicketStore>;

    /// Phase 2 (sera-93h4): broadcast handle for [`HitlResumedEvent`].
    ///
    /// The default impl returns `None`, which keeps existing test states
    /// (and any future readers that do not care about resume notifications)
    /// compiling. The production `AppState` overrides this to return a
    /// process-wide sender so the approve route can fan out to live SSE
    /// subscribers.
    fn hitl_resumed_tx(&self) -> Option<HitlResumedSender> {
        None
    }
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
    async fn in_memory_suspended_turn_roundtrip() {
        let store = InMemoryTicketStore::new();
        let turn = SuspendedTurn {
            ticket_id: "t-1".to_string(),
            session_key: "http:a:s".to_string(),
            session_id: "s".to_string(),
            agent_name: "a".to_string(),
            message: "hello".to_string(),
            stream: true,
        };
        store.record_suspended_turn(turn.clone()).await.unwrap();
        let got = store.get_suspended_turn("t-1").await.unwrap();
        assert_eq!(got.ticket_id, "t-1");
        assert_eq!(got.message, "hello");
        assert!(got.stream);
    }

    #[tokio::test]
    async fn in_memory_get_suspended_turn_missing_is_not_found() {
        let store = InMemoryTicketStore::new();
        let err = store.get_suspended_turn("nope").await.unwrap_err();
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
            subagents_allowed: Vec::new(),
            features: sera_types::config_manifest::AgentFeatureSetSpec::default(),
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
            subagents_allowed: Vec::new(),
            features: sera_types::config_manifest::AgentFeatureSetSpec::default(),
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
            subagents_allowed: Vec::new(),
            features: sera_types::config_manifest::AgentFeatureSetSpec::default(),
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
            subagents_allowed: Vec::new(),
            features: sera_types::config_manifest::AgentFeatureSetSpec::default(),
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
            subagents_allowed: Vec::new(),
            features: sera_types::config_manifest::AgentFeatureSetSpec::default(),
        };
        let routing = resolve_approval_routing(&spec);
        match routing {
            ApprovalRouting::Static { targets } => assert_eq!(targets.len(), 1),
            other => panic!("expected Static, got {other:?}"),
        }
    }
}
