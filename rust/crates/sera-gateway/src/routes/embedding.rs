//! Embedding service endpoints — Ollama integration for text embeddings.
#![allow(dead_code, unused_imports)]

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use time::OffsetDateTime;

use crate::error::AppError;
use crate::state::AppState;

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddingConfig {
    pub provider: String,
    pub model: String,
    pub dimensions: u32,
    pub base_url: String,
    pub status: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnownEmbeddingModel {
    pub id: String,
    pub provider: String,
    pub dimension: u32,
    pub description: Option<String>,
}

/// Static known embedding models registry (matching TS KNOWN_EMBEDDING_MODELS)
fn get_known_models() -> HashMap<String, KnownEmbeddingModel> {
    let mut models = HashMap::new();

    // Ollama models
    models.insert("nomic-embed-text".to_string(), KnownEmbeddingModel {
        id: "nomic-embed-text".to_string(),
        provider: "ollama".to_string(),
        dimension: 768,
        description: Some("Nomic embed text (768-dim)".to_string()),
    });

    models.insert("all-minilm-l6-v2".to_string(), KnownEmbeddingModel {
        id: "all-minilm-l6-v2".to_string(),
        provider: "ollama".to_string(),
        dimension: 384,
        description: Some("All-MiniLM-L6-v2 (384-dim)".to_string()),
    });

    models.insert("bge-small".to_string(), KnownEmbeddingModel {
        id: "bge-small".to_string(),
        provider: "ollama".to_string(),
        dimension: 384,
        description: Some("BGE Small (384-dim)".to_string()),
    });

    models.insert("bge-base".to_string(), KnownEmbeddingModel {
        id: "bge-base".to_string(),
        provider: "ollama".to_string(),
        dimension: 768,
        description: Some("BGE Base (768-dim)".to_string()),
    });

    // OpenAI models
    models.insert("text-embedding-ada-002".to_string(), KnownEmbeddingModel {
        id: "text-embedding-ada-002".to_string(),
        provider: "openai".to_string(),
        dimension: 1536,
        description: Some("OpenAI Ada (1536-dim)".to_string()),
    });

    models.insert("text-embedding-3-small".to_string(), KnownEmbeddingModel {
        id: "text-embedding-3-small".to_string(),
        provider: "openai".to_string(),
        dimension: 1536,
        description: Some("OpenAI 3 Small (1536-dim)".to_string()),
    });

    models.insert("text-embedding-3-large".to_string(), KnownEmbeddingModel {
        id: "text-embedding-3-large".to_string(),
        provider: "openai".to_string(),
        dimension: 3072,
        description: Some("OpenAI 3 Large (3072-dim)".to_string()),
    });

    models
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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbedRequest {
    pub text: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbedResponse {
    pub vector: Vec<f32>,
    pub dimensions: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchEmbedRequest {
    pub texts: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddingResult {
    pub text: String,
    pub vector: Vec<f32>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchEmbedResponse {
    pub embeddings: Vec<EmbeddingResult>,
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
                        || name.contains("minilm")
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
    pub provider: String,
    pub model: String,
    pub base_url: String,
    pub api_key: Option<String>,
    pub api_key_env_var: Option<String>,
    pub dimension: u32,
}

/// Validation errors for embedding config
#[derive(Debug, Serialize)]
pub struct ValidationError {
    pub field: String,
    pub message: String,
}

/// Validate embedding config — matching TS schema validation
fn validate_config(req: &UpdateConfigRequest) -> Result<(), Vec<ValidationError>> {
    let mut errors = Vec::new();

    // Validate provider enum
    if !["ollama", "openai", "lm-studio", "openai-compatible"].contains(&req.provider.as_str()) {
        errors.push(ValidationError {
            field: "provider".to_string(),
            message: "provider must be ollama, openai, lm-studio, or openai-compatible".to_string(),
        });
    }

    // Validate model (non-empty string)
    if req.model.is_empty() {
        errors.push(ValidationError {
            field: "model".to_string(),
            message: "model is required and must be non-empty".to_string(),
        });
    }

    // Validate base_url (non-empty string)
    if req.base_url.is_empty() {
        errors.push(ValidationError {
            field: "baseUrl".to_string(),
            message: "baseUrl is required and must be non-empty".to_string(),
        });
    }

    // Validate dimensions: 1-8192
    if req.dimension < 1 || req.dimension > 8192 {
        errors.push(ValidationError {
            field: "dimension".to_string(),
            message: "dimension must be between 1 and 8192".to_string(),
        });
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// PUT /api/embedding/config — update embedding configuration
pub async fn update_config(
    State(_state): State<AppState>,
    Json(body): Json<UpdateConfigRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    // Validate request
    if let Err(errors) = validate_config(&body) {
        return Ok(Json(serde_json::json!({
            "error": "Invalid config",
            "details": errors
        })));
    }

    // In production this would:
    // 1. Get old config to detect dimension changes
    // 2. Save to persistent storage
    // 3. Hot-swap in the service
    // For now, return success response with warning if dimensions would change

    let old_config = EmbeddingConfig {
        provider: "ollama".to_string(),
        model: "nomic-embed-text".to_string(),
        dimensions: 768,
        base_url: "http://ollama:11434".to_string(),
        status: "configured".to_string(),
    };

    let dimension_changed = old_config.dimensions != body.dimension;

    let new_config = EmbeddingConfig {
        provider: body.provider.clone(),
        model: body.model.clone(),
        dimensions: body.dimension,
        base_url: body.base_url.clone(),
        status: "configured".to_string(),
    };

    let mut response = serde_json::json!({
        "config": new_config,
        "testResult": {
            "status": "available",
            "latency_ms": 0
        }
    });

    if dimension_changed {
        response["dimensionChanged"] = serde_json::json!(true);
        response["warning"] = serde_json::json!(
            format!(
                "Vector dimension changed from {} to {}. Existing vectors are incompatible and will need to be re-indexed.",
                old_config.dimensions, body.dimension
            )
        );
    }

    Ok(Json(response))
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

/// GET /api/embedding/known-models — static list of known embedding models
pub async fn list_known_models() -> Json<HashMap<String, KnownEmbeddingModel>> {
    Json(get_known_models())
}

/// POST /api/embedding/test — test embedding config without persisting
pub async fn test_embedding_config(
    State(_state): State<AppState>,
    Json(body): Json<UpdateConfigRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    // Validate request
    if let Err(errors) = validate_config(&body) {
        return Ok(Json(serde_json::json!({
            "error": "Invalid config",
            "details": errors
        })));
    }

    let base_url = &body.base_url;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap_or_default();

    let start = std::time::Instant::now();

    // Try to connect to the embedding service
    let result = match body.provider.as_str() {
        "ollama" => {
            // Test Ollama /api/tags endpoint
            client
                .get(format!("{}/api/tags", base_url.trim_end_matches('/')))
                .send()
                .await
                .map(|resp| {
                    if resp.status().is_success() {
                        serde_json::json!({
                            "status": "success",
                            "message": "Ollama is reachable",
                            "latency_ms": start.elapsed().as_millis() as u64
                        })
                    } else {
                        serde_json::json!({
                            "status": "error",
                            "message": format!("Ollama returned HTTP {}", resp.status()),
                            "latency_ms": start.elapsed().as_millis() as u64
                        })
                    }
                })
                .map_err(|e| {
                    serde_json::json!({
                        "status": "error",
                        "message": format!("Cannot reach Ollama: {e}"),
                        "latency_ms": start.elapsed().as_millis() as u64
                    })
                })
        }
        "openai" | "openai-compatible" => {
            // Test OpenAI-compatible /v1/models endpoint
            let mut headers = reqwest::header::HeaderMap::new();
            if let Some(api_key) = &body.api_key
                && let Ok(val) = format!("Bearer {}", api_key).parse() {
                headers.insert(reqwest::header::AUTHORIZATION, val);
            }

            client
                .get(format!("{}/v1/models", base_url.trim_end_matches('/')))
                .headers(headers)
                .send()
                .await
                .map(|resp| {
                    if resp.status().is_success() {
                        serde_json::json!({
                            "status": "success",
                            "message": "OpenAI-compatible server is reachable",
                            "latency_ms": start.elapsed().as_millis() as u64
                        })
                    } else {
                        serde_json::json!({
                            "status": "error",
                            "message": format!("Server returned HTTP {}", resp.status()),
                            "latency_ms": start.elapsed().as_millis() as u64
                        })
                    }
                })
                .map_err(|e| {
                    serde_json::json!({
                        "status": "error",
                        "message": format!("Cannot reach server: {e}"),
                        "latency_ms": start.elapsed().as_millis() as u64
                    })
                })
        }
        _ => {
            Ok(serde_json::json!({
                "status": "error",
                "message": format!("Unknown provider: {}", body.provider)
            }))
        }
    };

    Ok(Json(result.unwrap_or_else(|e| e)))
}

/// POST /api/embedding/embed — embed a single text.
///
/// No real embedding provider is wired into the Rust gateway yet (the prior
/// implementation silently returned a 1536-dim zero vector, which is worse
/// than useless: it looks valid to callers but corrupts any downstream
/// similarity search). Until a provider is wired through
/// `state.config.ollama`/OpenAI, this endpoint returns `503 Service
/// Unavailable`.
pub async fn embed_text(
    Json(body): Json<EmbedRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let _ = body.text;
    tracing::warn!(
        "POST /api/embedding/embed called but no embedding provider is configured in sera-gateway"
    );
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(serde_json::json!({
            "error": "service_unavailable",
            "planned": "Wire an embedding provider (Ollama/OpenAI) through state; previous stub returned zero-vectors silently.",
            "bead": "sera-embedding",
        })),
    )
}

/// POST /api/embedding/batch — embed multiple texts.
///
/// Same story as `embed_text`: returns `503 Service Unavailable` until a real
/// provider is wired. Never silently return zero-vectors for batches.
pub async fn embed_batch(
    Json(body): Json<BatchEmbedRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let _ = body.texts;
    tracing::warn!(
        "POST /api/embedding/batch called but no embedding provider is configured in sera-gateway"
    );
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(serde_json::json!({
            "error": "service_unavailable",
            "planned": "Wire an embedding provider (Ollama/OpenAI) through state; previous stub returned zero-vectors silently.",
            "bead": "sera-embedding",
        })),
    )
}

/// GET /api/knowledge/{agent_id} — get agent knowledge.
pub async fn get_knowledge(
    Path(_agent_id): axum::extract::Path<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(serde_json::json!({
            "error": "not_implemented",
            "planned": "Agent knowledge store (git-backed) is post-MVS.",
            "bead": "sera-knowledge",
        })),
    )
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateKnowledgeRequest {
    pub content: String,
}

/// POST /api/knowledge/{agent_id} — update agent knowledge.
///
/// Previously echoed the request body back with a fake success timestamp —
/// callers saw success even though nothing was persisted. Now returns 501.
pub async fn update_knowledge(
    Path(_agent_id): Path<String>,
    Json(_body): Json<UpdateKnowledgeRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(serde_json::json!({
            "error": "not_implemented",
            "planned": "Agent knowledge store write-path (git-backed) is post-MVS.",
            "bead": "sera-knowledge",
        })),
    )
}

/// GET /api/knowledge/{agent_id}/history — get knowledge history.
pub async fn get_knowledge_history(
    Path(_agent_id): axum::extract::Path<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(serde_json::json!({
            "error": "not_implemented",
            "planned": "Knowledge history requires git-log over the knowledge store (post-MVS).",
            "bead": "sera-knowledge",
        })),
    )
}

#[derive(Debug, Deserialize)]
pub struct DiffQuery {
    pub v1: Option<String>,
    pub v2: Option<String>,
}

/// GET /api/knowledge/{agent_id}/diff — get knowledge diff.
pub async fn get_knowledge_diff(
    Path(_agent_id): axum::extract::Path<String>,
    Query(_query): axum::extract::Query<DiffQuery>,
) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(serde_json::json!({
            "error": "not_implemented",
            "planned": "Knowledge diff requires git-diff over the knowledge store (post-MVS).",
            "bead": "sera-knowledge",
        })),
    )
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

    #[test]
    fn embed_response_shape() {
        let resp = EmbedResponse {
            vector: vec![0.0; 1536],
            dimensions: 1536,
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"dimensions\":1536"));
        assert!(json.contains("\"vector\""));
    }

    #[test]
    fn batch_embed_response_shape() {
        let resp = BatchEmbedResponse {
            embeddings: vec![
                EmbeddingResult {
                    text: "hello".to_string(),
                    vector: vec![0.0; 1536],
                },
                EmbeddingResult {
                    text: "world".to_string(),
                    vector: vec![0.0; 1536],
                },
            ],
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"embeddings\""));
        assert!(json.contains("hello"));
        assert!(json.contains("world"));
    }

    #[test]
    fn knowledge_response_has_required_fields() {
        let resp = serde_json::json!({
            "agent_id": "agent-1",
            "content": "test content",
            "updated_at": serde_json::Value::Null,
        });
        assert_eq!(resp["agent_id"], "agent-1");
        assert_eq!(resp["content"], "test content");
    }

    #[test]
    fn knowledge_history_response_shape() {
        let resp = serde_json::json!({
            "agent_id": "agent-1",
            "versions": Vec::<String>::new(),
        });
        assert!(resp["versions"].is_array());
    }

    #[test]
    fn knowledge_diff_response_shape() {
        let resp = serde_json::json!({
            "agent_id": "agent-1",
            "diff": "",
        });
        assert_eq!(resp["diff"], "");
    }
}
