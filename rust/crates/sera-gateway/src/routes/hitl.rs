//! HITL approval request routes — Wave D Phase 1 (sera-z6ql).
//!
//! Routes:
//!   GET  /api/hitl/requests              — list all tickets
//!   GET  /api/hitl/requests/{id}         — fetch a single ticket
//!   POST /api/hitl/requests/{id}/approve — approve (marks ticket but does
//!                                          NOT resume the suspended turn)
//!   POST /api/hitl/requests/{id}/reject  — reject
//!   POST /api/hitl/requests/{id}/escalate — advance to the next target
//!
//! Phase 1 scope: approve/reject/escalate mutate the ticket state machine
//! only. The chat_handler turn that created the ticket already returned 403
//! to the caller — resume semantics are Phase 2.
#![allow(dead_code)]

use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use sera_hitl::{ApprovalTicket, HitlError};
use sera_types::principal::Principal;

use sera_gateway::hitl_gateway::{HitlAppState, HitlResumedEvent, TicketStoreError};

// ── Request/response shapes ──────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct ListTicketsResponse {
    pub tickets: Vec<ApprovalTicket>,
    pub count: usize,
}

#[derive(Debug, Deserialize, Default)]
pub struct DecisionBody {
    /// Optional free-text reason recorded against the decision.
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct DecisionResponse {
    pub id: String,
    pub status: sera_hitl::TicketStatus,
}

// ── Auth ─────────────────────────────────────────────────────────────────────

fn check_auth(api_key: &Option<String>, headers: &HeaderMap) -> Result<(), StatusCode> {
    let expected = match api_key {
        None => return Ok(()),
        Some(k) => k,
    };
    let provided = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));
    match provided {
        Some(k) if k == expected => Ok(()),
        _ => Err(StatusCode::UNAUTHORIZED),
    }
}

// ── Error mapping ────────────────────────────────────────────────────────────

fn map_store_err(err: TicketStoreError) -> StatusCode {
    match err {
        TicketStoreError::NotFound { .. } => StatusCode::NOT_FOUND,
        TicketStoreError::Backend { .. } => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

fn map_hitl_err(err: &HitlError) -> StatusCode {
    match err {
        HitlError::TicketNotFound { .. } => StatusCode::NOT_FOUND,
        HitlError::TicketExpired { .. } => StatusCode::GONE,
        HitlError::InvalidTransition { .. } => StatusCode::CONFLICT,
        HitlError::EscalationExhausted { .. } => StatusCode::CONFLICT,
        HitlError::InsufficientApprovals { .. } => StatusCode::CONFLICT,
    }
}

// ── Handlers ─────────────────────────────────────────────────────────────────

/// GET /api/hitl/requests — list every known ticket.
pub async fn list_tickets<S>(
    State(state): State<Arc<S>>,
    headers: HeaderMap,
) -> Result<Json<ListTicketsResponse>, StatusCode>
where
    S: HitlAppState,
{
    check_auth(state.api_key(), &headers)?;
    let tickets = state
        .ticket_store()
        .list()
        .await
        .map_err(map_store_err)?;
    let count = tickets.len();
    Ok(Json(ListTicketsResponse { tickets, count }))
}

/// GET /api/hitl/requests/{id}
pub async fn get_ticket<S>(
    State(state): State<Arc<S>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<ApprovalTicket>, StatusCode>
where
    S: HitlAppState,
{
    check_auth(state.api_key(), &headers)?;
    let ticket = state.ticket_store().get(&id).await.map_err(map_store_err)?;
    Ok(Json(ticket))
}

/// POST /api/hitl/requests/{id}/approve
///
/// Phase 2 (sera-93h4): records the approval and, if the ticket was minted
/// for a chat turn that was parked at the gateway pre-LLM gate, broadcasts a
/// [`HitlResumedEvent`] so subscribers know the original submission may be
/// resubmitted. Server-side replay of the parked turn is deferred to a
/// follow-up bead — the resume contract at this layer is "the gate is now
/// clear; client may retry the same request".
pub async fn approve_ticket<S>(
    State(state): State<Arc<S>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    body: Option<Json<DecisionBody>>,
) -> Result<Json<DecisionResponse>, StatusCode>
where
    S: HitlAppState,
{
    check_auth(state.api_key(), &headers)?;

    let store = state.ticket_store();
    let mut ticket = store.get(&id).await.map_err(map_store_err)?;
    let reason = body.and_then(|Json(b)| b.reason);

    let status = ticket
        .approve(Principal::default_admin().as_ref(), reason)
        .map_err(|e| map_hitl_err(&e))?;
    store.update(ticket).await.map_err(map_store_err)?;

    // Phase 2: notify subscribers that a parked chat turn may be resumed.
    // Best-effort — a `SendError` only fires when there are no subscribers
    // (the receiver-side has no live readers), which is a normal idle state
    // and must not turn an otherwise successful approval into a 5xx.
    if let Some(tx) = state.hitl_resumed_tx()
        && let Ok(suspended) = store.get_suspended_turn(&id).await
    {
        let _ = tx.send(HitlResumedEvent {
            ticket_id: id.clone(),
            session_key: suspended.session_key,
        });
    }

    Ok(Json(DecisionResponse { id, status }))
}

/// POST /api/hitl/requests/{id}/reject
pub async fn reject_ticket<S>(
    State(state): State<Arc<S>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    body: Option<Json<DecisionBody>>,
) -> Result<Json<DecisionResponse>, StatusCode>
where
    S: HitlAppState,
{
    check_auth(state.api_key(), &headers)?;

    let store = state.ticket_store();
    let mut ticket = store.get(&id).await.map_err(map_store_err)?;
    let reason = body.and_then(|Json(b)| b.reason);

    let status = ticket
        .reject(Principal::default_admin().as_ref(), reason)
        .map_err(|e| map_hitl_err(&e))?;
    store.update(ticket).await.map_err(map_store_err)?;

    Ok(Json(DecisionResponse { id, status }))
}

/// POST /api/hitl/requests/{id}/escalate
pub async fn escalate_ticket<S>(
    State(state): State<Arc<S>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<DecisionResponse>, StatusCode>
where
    S: HitlAppState,
{
    check_auth(state.api_key(), &headers)?;

    let store = state.ticket_store();
    let mut ticket = store.get(&id).await.map_err(map_store_err)?;
    ticket.escalate().map_err(|e| map_hitl_err(&e))?;
    let status = ticket.status;
    store.update(ticket).await.map_err(map_store_err)?;

    Ok(Json(DecisionResponse { id, status }))
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use sera_gateway::hitl_gateway::{
        HitlResumedSender, InMemoryTicketStore, SuspendedTurn, TicketStore,
    };
    use axum::{
        Router,
        body::Body,
        http::Request,
        routing::{get, post},
    };
    use sera_hitl::{
        ApprovalEvidence, ApprovalRouting, ApprovalScope, ApprovalSpec, ApprovalUrgency,
        TicketStatus,
    };
    use sera_types::tool::RiskLevel;
    use std::time::Duration;
    use tower::ServiceExt;

    struct TestState {
        api_key: Option<String>,
        tickets: Arc<InMemoryTicketStore>,
        resumed_tx: Option<HitlResumedSender>,
    }

    impl HitlAppState for TestState {
        fn api_key(&self) -> &Option<String> {
            &self.api_key
        }
        fn ticket_store(&self) -> Arc<dyn TicketStore> {
            Arc::clone(&self.tickets) as Arc<dyn TicketStore>
        }
        fn hitl_resumed_tx(&self) -> Option<HitlResumedSender> {
            self.resumed_tx.clone()
        }
    }

    fn sample_ticket() -> ApprovalTicket {
        let spec = ApprovalSpec {
            scope: ApprovalScope::ToolCall {
                tool_name: "shell".to_string(),
                risk_level: RiskLevel::Execute,
            },
            description: "test".to_string(),
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

    fn router(state: Arc<TestState>) -> Router {
        Router::new()
            .route("/api/hitl/requests", get(list_tickets::<TestState>))
            .route(
                "/api/hitl/requests/{id}",
                get(get_ticket::<TestState>),
            )
            .route(
                "/api/hitl/requests/{id}/approve",
                post(approve_ticket::<TestState>),
            )
            .route(
                "/api/hitl/requests/{id}/reject",
                post(reject_ticket::<TestState>),
            )
            .route(
                "/api/hitl/requests/{id}/escalate",
                post(escalate_ticket::<TestState>),
            )
            .with_state(state)
    }

    async fn fresh_state_with_ticket() -> (Arc<TestState>, String) {
        let tickets = Arc::new(InMemoryTicketStore::new());
        let t = sample_ticket();
        let id = t.id.clone();
        tickets.insert(t).await.unwrap();
        let state = Arc::new(TestState {
            api_key: None,
            tickets,
            resumed_tx: None,
        });
        (state, id)
    }

    #[tokio::test]
    async fn list_tickets_empty() {
        let state = Arc::new(TestState {
            api_key: None,
            tickets: Arc::new(InMemoryTicketStore::new()),
            resumed_tx: None,
        });
        let app = router(state);
        let resp = app
            .oneshot(Request::get("/api/hitl/requests").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let parsed: ListTicketsResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(parsed.count, 0);
    }

    #[tokio::test]
    async fn list_tickets_with_entry() {
        let (state, _id) = fresh_state_with_ticket().await;
        let app = router(state);
        let resp = app
            .oneshot(Request::get("/api/hitl/requests").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let parsed: ListTicketsResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(parsed.count, 1);
    }

    #[tokio::test]
    async fn get_ticket_not_found() {
        let state = Arc::new(TestState {
            api_key: None,
            tickets: Arc::new(InMemoryTicketStore::new()),
            resumed_tx: None,
        });
        let app = router(state);
        let resp = app
            .oneshot(
                Request::get("/api/hitl/requests/nope")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn get_ticket_happy_path() {
        let (state, id) = fresh_state_with_ticket().await;
        let app = router(state);
        let resp = app
            .oneshot(
                Request::get(format!("/api/hitl/requests/{id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn approve_ticket_marks_approved() {
        let (state, id) = fresh_state_with_ticket().await;
        let app = router(Arc::clone(&state));
        let resp = app
            .oneshot(
                Request::post(format!("/api/hitl/requests/{id}/approve"))
                    .header("content-type", "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let got = state.ticket_store().get(&id).await.unwrap();
        assert_eq!(got.status, TicketStatus::Approved);
    }

    #[tokio::test]
    async fn approve_ticket_without_body_is_ok() {
        // Callers can approve with no body — the DecisionBody is optional.
        let (state, id) = fresh_state_with_ticket().await;
        let app = router(Arc::clone(&state));
        let resp = app
            .oneshot(
                Request::post(format!("/api/hitl/requests/{id}/approve"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn reject_ticket_marks_rejected() {
        let (state, id) = fresh_state_with_ticket().await;
        let app = router(Arc::clone(&state));
        let resp = app
            .oneshot(
                Request::post(format!("/api/hitl/requests/{id}/reject"))
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"reason":"nope"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let got = state.ticket_store().get(&id).await.unwrap();
        assert_eq!(got.status, TicketStatus::Rejected);
    }

    #[tokio::test]
    async fn escalate_ticket_advances_target_index() {
        // Build a ticket whose routing has two targets so escalation succeeds.
        let routing = ApprovalRouting::Static {
            targets: vec![
                sera_hitl::ApprovalTarget::Agent {
                    id: "a1".to_string(),
                },
                sera_hitl::ApprovalTarget::Role {
                    name: "ops".to_string(),
                },
            ],
        };
        let spec = ApprovalSpec {
            scope: ApprovalScope::ToolCall {
                tool_name: "shell".to_string(),
                risk_level: RiskLevel::Execute,
            },
            description: "x".to_string(),
            urgency: ApprovalUrgency::Medium,
            routing,
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
        let t = ApprovalTicket::new(spec, "session-esc");
        let id = t.id.clone();

        let tickets = Arc::new(InMemoryTicketStore::new());
        tickets.insert(t).await.unwrap();
        let state = Arc::new(TestState {
            api_key: None,
            tickets,
            resumed_tx: None,
        });

        let app = router(Arc::clone(&state));
        let resp = app
            .oneshot(
                Request::post(format!("/api/hitl/requests/{id}/escalate"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let got = state.ticket_store().get(&id).await.unwrap();
        assert_eq!(got.status, TicketStatus::Escalated);
        assert_eq!(got.current_target_index, 1);
    }

    #[tokio::test]
    async fn escalate_without_room_returns_conflict() {
        let (state, id) = fresh_state_with_ticket().await;
        // Autonomous routing has zero targets — escalate must report conflict.
        let app = router(state);
        let resp = app
            .oneshot(
                Request::post(format!("/api/hitl/requests/{id}/escalate"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn approve_on_already_approved_returns_conflict() {
        let (state, id) = fresh_state_with_ticket().await;
        let app = router(Arc::clone(&state));
        // First approval succeeds.
        let r1 = app
            .clone()
            .oneshot(
                Request::post(format!("/api/hitl/requests/{id}/approve"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r1.status(), StatusCode::OK);
        // Second approval hits the terminal-state guard → 409.
        let r2 = app
            .oneshot(
                Request::post(format!("/api/hitl/requests/{id}/approve"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r2.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn auth_required_when_api_key_set() {
        let tickets = Arc::new(InMemoryTicketStore::new());
        let state = Arc::new(TestState {
            api_key: Some("secret".into()),
            tickets,
            resumed_tx: None,
        });
        let app = router(state);
        let resp = app
            .oneshot(
                Request::get("/api/hitl/requests")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    // -- Phase 2 (sera-93h4) — suspend/resume --

    /// Approving a ticket that has a recorded `SuspendedTurn` must broadcast
    /// a `HitlResumedEvent` carrying the original ticket id and session key.
    /// Subscribers exit their `recv()` with the matching payload — that is
    /// what tells a client it is now safe to retry the 403'd chat turn.
    #[tokio::test]
    async fn approve_emits_resumed_event_when_suspended_turn_recorded() {
        let tickets = Arc::new(InMemoryTicketStore::new());
        let ticket = sample_ticket();
        let id = ticket.id.clone();
        tickets.insert(ticket).await.unwrap();
        // Record a suspended turn for this ticket. The session_key must
        // round-trip to the broadcast subscriber.
        tickets
            .record_suspended_turn(SuspendedTurn {
                ticket_id: id.clone(),
                session_key: "http:agent-x:sess-1".to_string(),
                session_id: "sess-1".to_string(),
                agent_name: "agent-x".to_string(),
                message: "do the thing".to_string(),
                stream: false,
            })
            .await
            .unwrap();

        let (tx, mut rx) = tokio::sync::broadcast::channel(8);
        let state = Arc::new(TestState {
            api_key: None,
            tickets,
            resumed_tx: Some(tx),
        });

        let app = router(Arc::clone(&state));
        let resp = app
            .oneshot(
                Request::post(format!("/api/hitl/requests/{id}/approve"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let event = rx.try_recv().expect("expected a HitlResumedEvent");
        assert_eq!(event.ticket_id, id);
        assert_eq!(event.session_key, "http:agent-x:sess-1");
    }

    /// Approving a ticket with no recorded suspended turn must still succeed
    /// (Phase 1 ApprovalRouter use cases hit this path) and must not blow
    /// up when the broadcast channel has no listeners.
    #[tokio::test]
    async fn approve_without_suspended_turn_is_noop_for_resumed_channel() {
        let tickets = Arc::new(InMemoryTicketStore::new());
        let ticket = sample_ticket();
        let id = ticket.id.clone();
        tickets.insert(ticket).await.unwrap();

        let (tx, _rx) = tokio::sync::broadcast::channel(8);
        // Drop the receiver to simulate "no subscribers". The approve route
        // must still return 200.
        drop(_rx);
        let state = Arc::new(TestState {
            api_key: None,
            tickets,
            resumed_tx: Some(tx),
        });

        let app = router(state);
        let resp = app
            .oneshot(
                Request::post(format!("/api/hitl/requests/{id}/approve"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }
}
