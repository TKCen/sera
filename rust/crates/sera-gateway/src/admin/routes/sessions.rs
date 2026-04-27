//! Admin session inspect/kill (sera-nrn9).

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;

use crate::admin::state::{AdminAppState, AdminSessionInfo};

/// GET /admin/sessions
pub async fn list<S>(State(state): State<Arc<S>>) -> Json<Vec<AdminSessionInfo>>
where
    S: AdminAppState,
{
    Json(state.list_sessions().await)
}

/// GET /admin/sessions/{id}
pub async fn show<S>(
    State(state): State<Arc<S>>,
    Path(id): Path<String>,
) -> Result<Json<AdminSessionInfo>, StatusCode>
where
    S: AdminAppState,
{
    state
        .get_session(&id)
        .await
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}

/// DELETE /admin/sessions/{id} — fires the registered cancellation token, if
/// any. Returns 204 even when no token was registered (idempotent).
pub async fn kill<S>(
    State(state): State<Arc<S>>,
    Path(id): Path<String>,
) -> StatusCode
where
    S: AdminAppState,
{
    let cancelled = state.cancel_session(&id);
    tracing::info!(
        session_id = %id,
        cancelled,
        "admin DELETE /admin/sessions/{{id}}"
    );
    StatusCode::NO_CONTENT
}
