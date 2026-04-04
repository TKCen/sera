//! Embedding service endpoints — Ollama integration for text embeddings.
#![allow(dead_code, unused_imports)]

use axum::{extract::State, http::StatusCode, Json};
use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::state::AppState;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddingConfig {
    pub provider: String,
    pub model: String,
    pub dimensions: u32,
    pub base_url: String,
    pub status: String,
}

/// GET /api/embedding/config — return embedding configuration
pub async fn get_config(State(state): State<AppState>) -> Json<EmbeddingConfig> {
    let base_url = state.config.ollama.url.clone();
    Json(EmbeddingConfig {
        provider: "ollama".to_string(),
        model: "nomic-embed-text".to_string(),
        dimensions: 768,
        base_url,
        status: "configured".to_string(),
    })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddingStatus {
    pub status: String,
    pub message: String,
    pub latency_ms: Option<u64>,
}

/// GET /api/embedding/status — check Ollama connectivity
pub async fn get_status(State(state): State<AppState>) -> Json<EmbeddingStatus> {
    let base_url = state.config.ollama.url.clone();
    let client = reqwest::Client::new();

    let start = std::time::Instant::now();
    match client
        .get(format!("{base_url}/api/tags"))
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => Json(EmbeddingStatus {
            status: "available".to_string(),
            message: "Ollama is reachable".to_string(),
            latency_ms: Some(start.elapsed().as_millis() as u64),
        }),
        Ok(resp) => Json(EmbeddingStatus {
            status: "degraded".to_string(),
            message: format!("Ollama returned HTTP {}", resp.status()),
            latency_ms: Some(start.elapsed().as_millis() as u64),
        }),
        Err(e) => Json(EmbeddingStatus {
            status: "unavailable".to_string(),
            message: format!("Cannot reach Ollama: {e}"),
            latency_ms: None,
        }),
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddingModel {
    pub name: String,
    pub size: Option<String>,
}

/// GET /api/embedding/models — list available embedding models from Ollama
pub async fn list_models(
    State(state): State<AppState>,
) -> Result<Json<Vec<EmbeddingModel>>, AppError> {
    let base_url = state.config.ollama.url.clone();
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{base_url}/api/tags"))
        .send()
        .await
        .map_err(|e| {
            AppError::Internal(anyhow::anyhow!("Ollama unreachable: {e}"))
        })?;

    let body: serde_json::Value = resp.json().await.map_err(|e| {
        AppError::Internal(anyhow::anyhow!("Invalid Ollama response: {e}"))
    })?;

    let models = body
        .get("models")
        .and_then(|m| m.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|m| {
                    let name = m.get("name")?.as_str()?.to_string();
                    // Filter to embedding-capable models
                    if name.contains("embed")
                        || name.contains("nomic")
                        || name.contains("bge")
                    {
                        Some(EmbeddingModel {
                            name,
                            size: m
                                .get("size")
                                .and_then(|s| s.as_u64())
                                .map(|s| format!("{:.0}MB", s as f64 / 1_000_000.0)),
                        })
                    } else {
                        None
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(Json(models))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateConfigRequest {
    pub model: Option<String>,
    pub base_url: Option<String>,
    pub dimensions: Option<u32>,
}

/// PUT /api/embedding/config — update embedding configuration
pub async fn update_config(
    State(_state): State<AppState>,
    Json(_body): Json<UpdateConfigRequest>,
) -> Result<Json<EmbeddingConfig>, AppError> {
    // Config updates would be persisted — for now return the current config
    // In production this would update a config store
    Ok(Json(EmbeddingConfig {
        provider: "ollama".to_string(),
        model: "nomic-embed-text".to_string(),
        dimensions: 768,
        base_url: "http://ollama:11434".to_string(),
        status: "configured".to_string(),
    }))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestEmbeddingRequest {
    pub text: String,
    pub model: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestEmbeddingResponse {
    pub embedding: Vec<f32>,
    pub dimensions: usize,
    pub model: String,
    pub latency_ms: u64,
}

/// POST /api/embedding/test — test embedding generation
pub async fn test_embedding(
    State(state): State<AppState>,
    Json(body): Json<TestEmbeddingRequest>,
) -> Result<Json<TestEmbeddingResponse>, AppError> {
    let base_url = state.config.ollama.url.clone();
    let model = body.model.unwrap_or_else(|| "nomic-embed-text".to_string());
    let client = reqwest::Client::new();

    let start = std::time::Instant::now();
    let resp = client
        .post(format!("{base_url}/api/embed"))
        .json(&serde_json::json!({
            "model": model,
            "input": body.text,
        }))
        .send()
        .await
        .map_err(|e| {
            AppError::Internal(anyhow::anyhow!("Ollama embed request failed: {e}"))
        })?;

    let result: serde_json::Value = resp.json().await.map_err(|e| {
        AppError::Internal(anyhow::anyhow!("Invalid embedding response: {e}"))
    })?;

    let embeddings = result
        .get("embeddings")
        .and_then(|e| e.as_array())
        .and_then(|arr| arr.first())
        .and_then(|e| e.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_f64().map(|f| f as f32))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let dimensions = embeddings.len();

    Ok(Json(TestEmbeddingResponse {
        embedding: embeddings,
        dimensions,
        model,
        latency_ms: start.elapsed().as_millis() as u64,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedding_config_has_required_fields() {
        let config = EmbeddingConfig {
            provider: "ollama".to_string(),
            model: "nomic-embed-text".to_string(),
            dimensions: 768,
            base_url: "http://ollama:11434".to_string(),
            status: "configured".to_string(),
        };
        assert_eq!(config.provider, "ollama");
        assert_eq!(config.dimensions, 768);
    }

    #[test]
    fn embedding_model_serializes() {
        let model = EmbeddingModel {
            name: "nomic-embed-text".to_string(),
            size: Some("274MB".to_string()),
        };
        let json = serde_json::to_string(&model).unwrap();
        assert!(json.contains("nomic-embed-text"));
    }
}
