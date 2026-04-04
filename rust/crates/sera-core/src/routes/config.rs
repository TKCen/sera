//! Config, system, and misc stub endpoints.

use axum::{
    extract::State,
    Json,
};
use serde::Serialize;

use crate::error::AppError;
use crate::state::AppState;

/// LLM config response.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmConfigResponse {
    pub base_url: String,
    pub model: String,
}

/// GET /api/config/llm
pub async fn get_llm_config(
    State(state): State<AppState>,
) -> Json<LlmConfigResponse> {
    Json(LlmConfigResponse {
        base_url: state.config.llm.base_url.clone(),
        model: state.config.llm.model.clone(),
    })
}

/// GET /api/federation/peers — stub
pub async fn list_federation_peers() -> Json<serde_json::Value> {
    Json(serde_json::json!({"peers": []}))
}

/// GET /api/system/circuit-breakers — stub
pub async fn get_circuit_breakers() -> Json<serde_json::Value> {
    Json(serde_json::json!({"breakers": {}}))
}

/// GET /api/rt/token — issue a Centrifugo connection token.
pub async fn get_rt_token(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, AppError> {
    use sera_auth::JwtClaims;
    let exp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        + 3600; // 1 hour

    let claims = JwtClaims {
        sub: "web-client".to_string(),
        iss: "sera".to_string(),
        exp,
        agent_id: None,
        instance_id: None,
    };

    let token = state
        .jwt
        .issue(claims)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("{e}")))?;
    Ok(Json(serde_json::json!({"token": token})))
}
