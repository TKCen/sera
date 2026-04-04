//! Provider list endpoint.

use axum::{extract::State, Json};
use serde::Serialize;
use serde_json::Value;

use crate::error::AppError;
use crate::state::AppState;

/// Provider response entry (camelCase for API compatibility).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderResponse {
    pub model_name: String,
    pub api: String,
    pub provider: String,
    pub base_url: String,
    pub description: Option<String>,
    pub context_window: Option<u64>,
    pub max_tokens: Option<u64>,
    pub reasoning: bool,
}

/// GET /api/providers/list
pub async fn list_providers(
    State(state): State<AppState>,
) -> Result<Json<Value>, AppError> {
    let providers = match &state.config.providers {
        Some(config) => config
            .providers
            .iter()
            .map(|p| ProviderResponse {
                model_name: p.model_name.clone(),
                api: p.api.clone(),
                provider: p.provider.clone(),
                base_url: p.base_url.clone(),
                description: p.description.clone(),
                context_window: p.context_window,
                max_tokens: p.max_tokens,
                reasoning: p.reasoning,
            })
            .collect::<Vec<_>>(),
        None => vec![],
    };

    Ok(Json(serde_json::to_value(providers).unwrap_or_default()))
}
