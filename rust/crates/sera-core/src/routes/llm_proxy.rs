//! LLM Proxy — OpenAI-compatible gateway for agent containers.
//!
//! POST /v1/llm/chat/completions — proxied chat completion (streaming + non-streaming)
//! GET  /v1/llm/models            — list available models

use axum::{
    body::Body,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};

use sera_db::metering::MeteringRepository;

use crate::error::AppError;
use crate::state::AppState;

/// OpenAI-compatible chat completion request.
#[derive(Debug, Deserialize)]
pub struct ChatCompletionRequest {
    pub model: Option<String>,
    pub messages: Vec<serde_json::Value>,
    #[serde(default)]
    pub temperature: Option<f64>,
    #[serde(default)]
    pub stream: Option<bool>,
    #[serde(default)]
    pub tools: Option<Vec<serde_json::Value>>,
    #[serde(default)]
    pub max_tokens: Option<u64>,
    // Pass through any extra fields
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Model list response.
#[derive(Debug, Serialize)]
pub struct ModelListResponse {
    pub object: &'static str,
    pub data: Vec<ModelEntry>,
}

#[derive(Debug, Serialize)]
pub struct ModelEntry {
    pub id: String,
    pub object: &'static str,
    pub owned_by: String,
}

/// GET /v1/llm/models
pub async fn list_models(State(state): State<AppState>) -> Json<ModelListResponse> {
    let models = match &state.config.providers {
        Some(config) => config
            .providers
            .iter()
            .map(|p| ModelEntry {
                id: p.model_name.clone(),
                object: "model",
                owned_by: p.provider.clone(),
            })
            .collect(),
        None => vec![],
    };

    Json(ModelListResponse {
        object: "list",
        data: models,
    })
}

/// POST /v1/llm/chat/completions
///
/// Budget-gated proxy to upstream LLM providers.
/// Supports both streaming (SSE) and non-streaming responses.
pub async fn chat_completions(
    State(state): State<AppState>,
    Json(body): Json<ChatCompletionRequest>,
) -> Result<Response, AppError> {
    // Resolve agent ID from request context (in full impl, from JWT)
    // For now, extract from X-Agent-Id header or default
    let agent_id = "unknown"; // TODO: extract from auth context

    // Resolve model name
    let model_name = body
        .model
        .as_deref()
        .unwrap_or(&state.config.llm.model);

    // ── 1. Budget gate ──────────────────────────────────────────────────────
    match MeteringRepository::check_budget(state.db.inner(), agent_id).await {
        Ok(budget) if !budget.allowed => {
            let period = if budget.hourly_used >= budget.hourly_quota {
                "hourly"
            } else {
                "daily"
            };
            return Ok((
                StatusCode::TOO_MANY_REQUESTS,
                Json(serde_json::json!({
                    "error": "budget_exceeded",
                    "period": period,
                    "limit": if period == "hourly" { budget.hourly_quota } else { budget.daily_quota },
                    "used": if period == "hourly" { budget.hourly_used } else { budget.daily_used },
                })),
            )
                .into_response());
        }
        Err(e) => {
            // Fail-open: if metering DB is down, allow but log
            tracing::error!("Budget check failed (allowing request): {e}");
        }
        _ => {}
    }

    // ── 2. Validate request ─────────────────────────────────────────────────
    if body.messages.is_empty() {
        return Ok((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": {"message": "`messages` array is required and must be non-empty"}
            })),
        )
            .into_response());
    }

    // ── 3. Resolve upstream provider URL ────────────────────────────────────
    let providers = state.providers.read().await;
    let provider = providers
        .providers
        .iter()
        .find(|p| p.model_name == model_name);

    let (base_url, api_key) = match provider {
        Some(p) => (p.base_url.clone(), p.api_key.clone()),
        None => (state.config.llm.base_url.clone(), state.config.llm.api_key.clone()),
    };
    drop(providers);

    let upstream_url = format!("{}/chat/completions", base_url.trim_end_matches('/'));

    // ── 4. Build upstream request body ──────────────────────────────────────
    let mut upstream_body = serde_json::json!({
        "model": model_name,
        "messages": body.messages,
    });

    if let Some(temp) = body.temperature {
        upstream_body["temperature"] = serde_json::json!(temp);
    }
    if let Some(true) = body.stream {
        upstream_body["stream"] = serde_json::json!(true);
    }
    if let Some(tools) = &body.tools {
        upstream_body["tools"] = serde_json::json!(tools);
    }
    if let Some(max) = body.max_tokens {
        upstream_body["max_tokens"] = serde_json::json!(max);
    }
    // Pass through extra fields
    for (k, v) in &body.extra {
        upstream_body[k] = v.clone();
    }

    // ── 5. Forward to upstream ──────────────────────────────────────────────
    let client = reqwest::Client::new();
    let mut req_builder = client.post(&upstream_url).json(&upstream_body);

    if !api_key.is_empty() {
        req_builder = req_builder.header("Authorization", format!("Bearer {api_key}"));
    }

    let upstream_res = req_builder.send().await.map_err(|e| {
        tracing::error!("Upstream LLM error: {e}");
        AppError::Internal(anyhow::anyhow!("Upstream LLM error: {e}"))
    })?;

    let status = upstream_res.status();

    // ── 6. Streaming path ───────────────────────────────────────────────────
    if body.stream == Some(true) {
        let stream = upstream_res.bytes_stream();
        let body = Body::from_stream(stream);
        return Ok(Response::builder()
            .status(status.as_u16())
            .header("Content-Type", "text/event-stream")
            .header("Cache-Control", "no-cache")
            .header("Connection", "keep-alive")
            .header("X-Accel-Buffering", "no")
            .body(body)
            .unwrap());
    }

    // ── 7. Non-streaming path ───────────────────────────────────────────────
    let response_bytes = upstream_res.bytes().await.map_err(|e| {
        AppError::Internal(anyhow::anyhow!("Failed to read upstream response: {e}"))
    })?;

    // Try to parse for metering (best-effort)
    if let Ok(json) = serde_json::from_slice::<serde_json::Value>(&response_bytes)
        && let Some(usage) = json.get("usage")
    {
        let prompt = usage.get("prompt_tokens").and_then(|v| v.as_i64()).unwrap_or(0);
        let completion = usage.get("completion_tokens").and_then(|v| v.as_i64()).unwrap_or(0);
        let total = usage.get("total_tokens").and_then(|v| v.as_i64()).unwrap_or(prompt + completion);

        // Record usage asynchronously (fire-and-forget)
        let pool = state.db.inner().clone();
        let agent = agent_id.to_string();
        let model = model_name.to_string();
        tokio::spawn(async move {
            let _ = MeteringRepository::record_usage(
                &pool, &agent, None, &model, prompt, completion, total, None, None, "success",
            )
            .await;
        });
    }

    Ok(Response::builder()
        .status(status.as_u16())
        .header("Content-Type", "application/json")
        .body(Body::from(response_bytes))
        .unwrap())
}
