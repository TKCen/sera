//! Subagent spawn and inspection routes (sera-vuss).
//!
//! Routes:
//!   POST /api/sessions/{id}/subagents — spawn a subagent under a session
//!   GET  /api/sessions/{id}/subagents — list active subagents for a session
//!
//! The runtime already exposes a [`SubagentManager`] trait
//! (`sera-runtime/src/subagent.rs`) but no HTTP path lets a caller drive it.
//! This module wires that trait through axum so the gateway can spawn,
//! inspect, and (in a follow-up) cancel subagents on behalf of a parent
//! session.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
};
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, watch};

use sera_runtime::subagent::{SubagentError, SubagentHandle, SubagentManager, SubagentStatus};

// ── AppState abstraction ─────────────────────────────────────────────────────

/// Abstraction over AppState needed by the subagent routes.
pub trait SubagentsAppState: Send + Sync + 'static {
    fn api_key(&self) -> &Option<String>;
    fn subagent_manager(&self) -> Arc<dyn SubagentManager>;
}

// ── Auth helper ──────────────────────────────────────────────────────────────

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

// ── Request / response types ─────────────────────────────────────────────────

/// Request body for `POST /api/sessions/{id}/subagents`.
#[derive(Debug, Deserialize)]
pub struct SpawnSubagentRequest {
    /// Agent template identifier to spawn.
    pub agent: String,
    /// Task prompt forwarded to the subagent as input.
    pub prompt: String,
    /// Optional extra structured context forwarded verbatim.
    #[serde(default)]
    pub context: serde_json::Value,
}

/// Response body for a successful spawn.
#[derive(Debug, Serialize, Deserialize)]
pub struct SpawnSubagentResponse {
    /// Session-scoped key assigned to this subagent invocation. The caller
    /// uses this to look the subagent up in subsequent status / cancel calls.
    pub task_id: String,
    /// Spawning agent identifier (echoed from the request).
    pub agent: String,
    /// Initial lifecycle status (typically `spawning`).
    pub status: SubagentStatus,
}

/// One entry in the active-subagents list.
#[derive(Debug, Serialize, Deserialize)]
pub struct SubagentEntry {
    pub task_id: String,
    pub status: SubagentStatus,
}

/// Response body for `GET /api/sessions/{id}/subagents`.
#[derive(Debug, Serialize, Deserialize)]
pub struct ListSubagentsResponse {
    pub subagents: Vec<SubagentEntry>,
}

// ── Route handlers ───────────────────────────────────────────────────────────

fn map_spawn_err(e: SubagentError) -> StatusCode {
    match e {
        SubagentError::NotFound { .. } => StatusCode::NOT_FOUND,
        SubagentError::SpawnFailed { .. } => StatusCode::INTERNAL_SERVER_ERROR,
        SubagentError::NotActive { .. } => StatusCode::CONFLICT,
        SubagentError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

/// `POST /api/sessions/{id}/subagents` — spawn a new subagent under
/// `session_id`. The session id is forwarded to the manager as the
/// `session_key` so a future implementation can scope the child to its
/// parent.
pub async fn spawn_subagent<S: SubagentsAppState>(
    State(state): State<Arc<S>>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
    Json(body): Json<SpawnSubagentRequest>,
) -> Result<(StatusCode, Json<SpawnSubagentResponse>), StatusCode> {
    check_auth(state.api_key(), &headers)?;

    let mgr = state.subagent_manager();
    let input = serde_json::json!({
        "prompt": body.prompt,
        "context": body.context,
    });

    let handle = mgr
        .spawn(&body.agent, &session_id, input)
        .await
        .map_err(map_spawn_err)?;

    let resp = SpawnSubagentResponse {
        task_id: handle.session_key.clone(),
        agent: body.agent,
        status: handle.current_status(),
    };
    Ok((StatusCode::CREATED, Json(resp)))
}

/// `GET /api/sessions/{id}/subagents` — list active subagents for
/// `session_id`. The handler queries the manager for all currently active
/// task keys and filters to those that belong to this session (task keys
/// of the form `{session_id}/<suffix>`).
pub async fn list_subagents<S: SubagentsAppState>(
    State(state): State<Arc<S>>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
) -> Result<Json<ListSubagentsResponse>, StatusCode> {
    check_auth(state.api_key(), &headers)?;

    let mgr = state.subagent_manager();
    let all_active = mgr.list_active().await;

    let mut subagents = Vec::new();
    let prefix = format!("{session_id}/");
    for key in all_active {
        if key.starts_with(&prefix) {
            let status = mgr
                .status(&key)
                .await
                .unwrap_or(SubagentStatus::Cancelled);
            subagents.push(SubagentEntry {
                task_id: key,
                status,
            });
        }
    }

    Ok(Json(ListSubagentsResponse { subagents }))
}

// ── InMemorySubagentManager ──────────────────────────────────────────────────

/// Default in-memory [`SubagentManager`] used by the gateway when no real
/// runtime backend is wired. Each `spawn` records an entry keyed by
/// `{session_key}/{agent_id}` with an initial `Spawning` status; no real
/// child process is launched. This keeps the HTTP surface alive end-to-end
/// (`POST` → `GET` → `cancel`) while richer backends are implemented in
/// follow-up beads.
pub struct InMemorySubagentManager {
    /// Map from task_key → status sender.
    entries: Mutex<HashMap<String, watch::Sender<SubagentStatus>>>,
}

impl InMemorySubagentManager {
    pub fn new() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
        }
    }
}

impl Default for InMemorySubagentManager {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SubagentManager for InMemorySubagentManager {
    async fn spawn(
        &self,
        agent_id: &str,
        session_key: &str,
        _input: serde_json::Value,
    ) -> Result<SubagentHandle, SubagentError> {
        let task_key = format!("{session_key}/{agent_id}");
        let (tx, rx) = watch::channel(SubagentStatus::Spawning);
        let mut map = self.entries.lock().await;
        map.insert(task_key.clone(), tx);
        Ok(SubagentHandle::new(agent_id, task_key, rx))
    }

    async fn status(&self, session_key: &str) -> Result<SubagentStatus, SubagentError> {
        let map = self.entries.lock().await;
        map.get(session_key)
            .map(|tx| tx.borrow().clone())
            .ok_or_else(|| SubagentError::NotFound {
                agent_id: session_key.to_string(),
            })
    }

    async fn cancel(&self, session_key: &str) -> Result<(), SubagentError> {
        let map = self.entries.lock().await;
        match map.get(session_key) {
            Some(tx) => {
                let _ = tx.send(SubagentStatus::Cancelled);
                Ok(())
            }
            None => Err(SubagentError::NotFound {
                agent_id: session_key.to_string(),
            }),
        }
    }

    async fn list_active(&self) -> Vec<String> {
        let map = self.entries.lock().await;
        map.iter()
            .filter(|(_, tx)| {
                let s = tx.borrow();
                matches!(*s, SubagentStatus::Spawning | SubagentStatus::Running)
            })
            .map(|(k, _)| k.clone())
            .collect()
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn spawn_records_entry() {
        let mgr = InMemorySubagentManager::new();
        let handle = mgr
            .spawn("worker", "ses_abc", serde_json::json!({}))
            .await
            .unwrap();
        assert_eq!(handle.current_status(), SubagentStatus::Spawning);
        assert_eq!(handle.agent_id, "worker");
        assert!(handle.session_key.starts_with("ses_abc/"));
    }

    #[tokio::test]
    async fn list_active_returns_spawning() {
        let mgr = InMemorySubagentManager::new();
        mgr.spawn("worker", "ses_1", serde_json::json!({}))
            .await
            .unwrap();
        let active = mgr.list_active().await;
        assert_eq!(active.len(), 1);
        assert!(active[0].starts_with("ses_1/"));
    }

    #[tokio::test]
    async fn cancel_transitions_status() {
        let mgr = InMemorySubagentManager::new();
        let handle = mgr
            .spawn("worker", "ses_2", serde_json::json!({}))
            .await
            .unwrap();
        mgr.cancel(&handle.session_key).await.unwrap();
        let status = mgr.status(&handle.session_key).await.unwrap();
        assert_eq!(status, SubagentStatus::Cancelled);
    }

    #[tokio::test]
    async fn cancelled_not_in_active_list() {
        let mgr = InMemorySubagentManager::new();
        let handle = mgr
            .spawn("worker", "ses_3", serde_json::json!({}))
            .await
            .unwrap();
        mgr.cancel(&handle.session_key).await.unwrap();
        let active = mgr.list_active().await;
        assert!(active.is_empty());
    }

    #[tokio::test]
    async fn status_unknown_key_returns_not_found() {
        let mgr = InMemorySubagentManager::new();
        let err = mgr.status("nonexistent").await.unwrap_err();
        assert!(matches!(err, SubagentError::NotFound { .. }));
    }
}
