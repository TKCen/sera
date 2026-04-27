//! Admin workflow CRUD (sera-nrn9).
//!
//! Reads from the same workflow_store wired into the public API. Create and
//! delete return 501 — the existing store has insert/get but no delete, and
//! the admin "create from manifest" path needs scheduling integration that
//! lands with L.5.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;

use crate::admin::state::AdminAppState;

fn not_ready() -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(serde_json::json!({
            "error": "workflow_store_not_ready",
            "detail": "admin workflow CRUD is gated on L.5 wiring; route shell only in L.3"
        })),
    )
}

/// GET /admin/workflows
pub async fn list<S>(State(state): State<Arc<S>>) -> Json<serde_json::Value>
where
    S: AdminAppState,
{
    let Some(store) = state.workflow_store() else {
        return Json(serde_json::json!({"workflows": []}));
    };
    let records = store.list().await;
    let summaries: Vec<_> = records
        .into_iter()
        .map(|r| {
            serde_json::json!({
                "id": r.task.id.to_string(),
                "agent_id": r.agent_id,
                "status": r.status,
                "title": r.task.title,
            })
        })
        .collect();
    Json(serde_json::json!({"workflows": summaries}))
}

/// POST /admin/workflows
pub async fn create<S>(
    State(_state): State<Arc<S>>,
    Json(_body): Json<serde_json::Value>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<serde_json::Value>)>
where
    S: AdminAppState,
{
    Err(not_ready())
}

/// GET /admin/workflows/{id}
pub async fn show<S>(
    State(state): State<Arc<S>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)>
where
    S: AdminAppState,
{
    let Some(store) = state.workflow_store() else {
        return Err(not_ready());
    };
    let rec = store.get(&id).await.ok_or((
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({"error": "workflow_not_found"})),
    ))?;
    Ok(Json(serde_json::json!({
        "id": rec.task.id.to_string(),
        "agent_id": rec.agent_id,
        "resume_token": rec.resume_token,
        "status": rec.status,
        "resolved_at": rec.resolved_at,
        "title": rec.task.title,
        "await_type": rec.task.await_type,
    })))
}

/// DELETE /admin/workflows/{id}
pub async fn delete<S>(
    State(_state): State<Arc<S>>,
    Path(_id): Path<String>,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)>
where
    S: AdminAppState,
{
    Err(not_ready())
}
