//! Admin agent control (sera-nrn9).
//!
//! `show` reads from the existing manifest set surfaced by the AppState.
//! `spawn` and `kill` are 501 in L.3 — process-level lifecycle hooks ship in a
//! follow-up bead. The route shells exist so CLI verbs (L.4) and the main
//! agent (L.5) can wire against a stable URL today.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;

use crate::admin::state::AdminAppState;

/// GET /admin/agents/{id}
pub async fn show<S>(
    State(state): State<Arc<S>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode>
where
    S: AdminAppState,
{
    state
        .agent_metadata(&id)
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}

/// POST /admin/agents/spawn
pub async fn spawn<S>(
    State(state): State<Arc<S>>,
    Json(body): Json<serde_json::Value>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<serde_json::Value>)>
where
    S: AdminAppState,
{
    match state.spawn_agent(body).await {
        Some(meta) => Ok((StatusCode::CREATED, Json(meta))),
        None => Err((
            StatusCode::NOT_IMPLEMENTED,
            Json(serde_json::json!({
                "error": "agent_spawn_not_implemented",
                "detail": "process_manager not wired into gateway boot — L.3 ships the route shell only"
            })),
        )),
    }
}

/// DELETE /admin/agents/{id}
pub async fn kill<S>(
    State(state): State<Arc<S>>,
    Path(id): Path<String>,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)>
where
    S: AdminAppState,
{
    if state.kill_agent(&id).await {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err((
            StatusCode::NOT_IMPLEMENTED,
            Json(serde_json::json!({
                "error": "agent_kill_not_implemented",
                "detail": "process_manager not wired into gateway boot — L.3 ships the route shell only"
            })),
        ))
    }
}
