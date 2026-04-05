//! Provider list endpoint.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use sera_config::providers::ProviderEntry;

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

    Ok(Json(serde_json::json!({ "providers": providers })))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddProviderRequest {
    pub model_name: String,
    pub api: Option<String>,
    pub provider: Option<String>,
    pub base_url: Option<String>,
    pub description: Option<String>,
}

/// POST /api/providers
pub async fn add_provider(
    State(state): State<AppState>,
    Json(body): Json<AddProviderRequest>,
) -> Result<(StatusCode, Json<Value>), AppError> {
    let entry = ProviderEntry {
        model_name: body.model_name.clone(),
        api: body.api.unwrap_or_else(|| "openai-completions".to_string()),
        provider: body.provider.unwrap_or_default(),
        base_url: body.base_url.unwrap_or_default(),
        api_key: String::new(),
        description: body.description,
        context_window: None,
        max_tokens: None,
        reasoning: false,
        context_strategy: None,
        context_high_water_mark: None,
        dynamic_provider_id: None,
    };

    {
        let mut providers = state.providers.write().await;
        providers.add_provider(entry).map_err(|e| AppError::Db(
            sera_db::DbError::Conflict(e),
        ))?;
        if let Some(path) = &state.providers_path {
            providers.save_to_file(path).map_err(|e| {
                AppError::Internal(anyhow::anyhow!("{e}"))
            })?;
        }
    }

    Ok((StatusCode::CREATED, Json(serde_json::json!({
        "modelName": body.model_name,
        "result": "added"
    }))))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProviderRequest {
    pub context_window: Option<u64>,
    pub max_tokens: Option<u64>,
    pub reasoning: Option<bool>,
    pub description: Option<String>,
    pub context_strategy: Option<String>,
}

/// PATCH /api/providers/:modelName
pub async fn update_provider(
    State(state): State<AppState>,
    Path(model_name): Path<String>,
    Json(body): Json<UpdateProviderRequest>,
) -> Result<Json<Value>, AppError> {
    {
        let mut providers = state.providers.write().await;
        providers
            .update_provider(
                &model_name,
                body.context_window,
                body.max_tokens,
                body.reasoning,
                body.description,
                body.context_strategy,
            )
            .map_err(|e| {
                AppError::Db(sera_db::DbError::NotFound {
                    entity: "provider",
                    key: "modelName",
                    value: e,
                })
            })?;
        if let Some(path) = &state.providers_path {
            providers.save_to_file(path).map_err(|e| {
                AppError::Internal(anyhow::anyhow!("{e}"))
            })?;
        }
    }

    Ok(Json(serde_json::json!({"success": true, "modelName": model_name})))
}

/// DELETE /api/providers/:modelName
pub async fn delete_provider(
    State(state): State<AppState>,
    Path(model_name): Path<String>,
) -> Result<StatusCode, AppError> {
    {
        let mut providers = state.providers.write().await;
        providers.remove_provider(&model_name).map_err(|e| {
            AppError::Db(sera_db::DbError::NotFound {
                entity: "provider",
                key: "modelName",
                value: e,
            })
        })?;
        if let Some(path) = &state.providers_path {
            providers.save_to_file(path).map_err(|e| {
                AppError::Internal(anyhow::anyhow!("{e}"))
            })?;
        }
    }

    Ok(StatusCode::NO_CONTENT)
}
