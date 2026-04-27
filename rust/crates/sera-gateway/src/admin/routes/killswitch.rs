//! Killswitch HTTP control-plane (sera-nrn9). Backed by the same
//! [`KillSwitch`](crate::kill_switch::KillSwitch) as the unix admin socket;
//! both surfaces stay live so existing tooling continues to work.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;

use crate::admin::state::AdminAppState;

/// POST /admin/killswitch/arm
pub async fn arm<S>(State(state): State<Arc<S>>) -> StatusCode
where
    S: AdminAppState,
{
    state.kill_switch().arm();
    tracing::warn!(
        event = "KILL_SWITCH_ACTIVATED",
        "Admin kill switch armed via HTTP /admin/killswitch/arm"
    );
    StatusCode::NO_CONTENT
}

/// POST /admin/killswitch/disarm
pub async fn disarm<S>(State(state): State<Arc<S>>) -> StatusCode
where
    S: AdminAppState,
{
    state.kill_switch().disarm();
    tracing::info!("Admin kill switch disarmed via HTTP /admin/killswitch/disarm");
    StatusCode::NO_CONTENT
}

/// GET /admin/killswitch/status
pub async fn status<S>(State(state): State<Arc<S>>) -> Json<serde_json::Value>
where
    S: AdminAppState,
{
    Json(serde_json::json!({
        "armed": state.kill_switch().is_armed(),
    }))
}
