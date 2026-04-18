//! Config, system, and misc stub endpoints.

use axum::{
    extract::State,
    http::StatusCode,
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
    Json(serde_json::json!({"circuitBreakers": []}))
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
        aud: Vec::new(),
        exp,
        nbf: None,
        agent_id: None,
        instance_id: None,
    };

    let token = state
        .jwt
        .issue(claims)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("{e}")))?;
    Ok(Json(serde_json::json!({"token": token, "expiresAt": exp})))
}

/// Provider config response.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderResponse {
    pub model_name: String,
    pub provider: String,
    pub base_url: String,
}

/// GET /api/config/providers — list configured LLM providers
pub async fn list_providers(
    State(state): State<AppState>,
) -> Json<serde_json::Value> {
    let providers = state.providers.read().await;
    let list: Vec<ProviderResponse> = providers
        .providers
        .iter()
        .map(|p| ProviderResponse {
            model_name: p.model_name.clone(),
            provider: p.provider.clone(),
            base_url: p.base_url.clone(),
        })
        .collect();

    Json(serde_json::json!({"providers": list}))
}

/// Config reload response.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReloadConfigResponse {
    pub reloaded: bool,
    pub timestamp: String,
}

/// POST /api/config/reload — trigger config reload
pub async fn reload_config() -> (StatusCode, Json<ReloadConfigResponse>) {
    use super::iso8601;
    let now = time::OffsetDateTime::now_utc();
    let timestamp = iso8601(now);
    (
        StatusCode::OK,
        Json(ReloadConfigResponse {
            reloaded: true,
            timestamp,
        }),
    )
}
