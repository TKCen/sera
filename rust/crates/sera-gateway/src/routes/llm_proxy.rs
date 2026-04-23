//! LLM Proxy — OpenAI-compatible gateway for agent containers.
//!
//! POST /v1/llm/chat/completions — proxied chat completion (streaming + non-streaming)
//! GET  /v1/llm/models            — list available models

use axum::{
    Json,
    body::Body,
    extract::{Extension, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};

use sera_auth::ActingContext;
use sera_db::metering::MeteringRepository;

use crate::error::AppError;
use crate::state::AppState;

/// Resolve the agent ID for metering/budget attribution.
///
/// Priority:
/// 1. `ActingContext.agent_id` — set by `require_auth` when the JWT carries
///    an `agent_id` claim. This is the authoritative path for agent-runtime
///    callers.
/// 2. `ActingContext.operator_id` prefixed with `op:` — operator-scoped
///    callers (dashboard, bootstrap API key) don't represent a specific
///    agent, but we still want a distinct bucket so operator usage doesn't
///    silently accrue to `"unknown"`.
/// 3. `X-Agent-Id` header — legacy fallback for containers that don't yet
///    mint JWTs with `agent_id` claims.
/// 4. `"unknown"` — last-resort bucket matching the prior behavior.
fn resolve_agent_id(ctx: &ActingContext, headers: &axum::http::HeaderMap) -> String {
    if let Some(agent) = ctx.agent_id.as_deref() {
        return agent.to_string();
    }
    if let Some(op) = ctx.operator_id.as_deref() {
        return format!("op:{op}");
    }
    if let Some(hdr) = headers.get("x-agent-id").and_then(|v| v.to_str().ok())
        && !hdr.is_empty()
    {
        return hdr.to_string();
    }
    "unknown".to_string()
}

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
    Extension(ctx): Extension<ActingContext>,
    headers: axum::http::HeaderMap,
    Json(body): Json<ChatCompletionRequest>,
) -> Result<Response, AppError> {
    // Resolve agent ID from the auth context (populated by require_auth
    // middleware), falling back to the X-Agent-Id header for legacy callers.
    let agent_id = resolve_agent_id(&ctx, &headers);
    let agent_id = agent_id.as_str();

    // Resolve model name
    let model_name = body.model.as_deref().unwrap_or(&state.config.llm.model);

    // ── 1. Budget gate ──────────────────────────────────────────────────────
    match MeteringRepository::check_budget(state.db.require_pg_pool(), agent_id).await {
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
        None => (
            state.config.llm.base_url.clone(),
            state.config.llm.api_key.clone(),
        ),
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
        let prompt = usage
            .get("prompt_tokens")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        let completion = usage
            .get("completion_tokens")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        let total = usage
            .get("total_tokens")
            .and_then(|v| v.as_i64())
            .unwrap_or(prompt + completion);

        // Record usage asynchronously (fire-and-forget)
        let pool = state.db.require_pg_pool().clone();
        let agent = agent_id.to_string();
        let model = model_name.to_string();
        tokio::spawn(async move {
            let _ = MeteringRepository::record_usage(
                &pool,
                sera_db::metering::RecordUsageInput {
                    agent_id: &agent,
                    circle_id: None,
                    model: &model,
                    prompt_tokens: prompt,
                    completion_tokens: completion,
                    total_tokens: total,
                    cost_usd: None,
                    latency_ms: None,
                    status: "success",
                },
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderMap;
    use sera_auth::types::AuthMethod;

    fn empty_headers() -> HeaderMap {
        HeaderMap::new()
    }

    fn headers_with_agent(agent: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert("x-agent-id", agent.parse().unwrap());
        h
    }

    fn ctx_agent(agent: &str) -> ActingContext {
        ActingContext {
            operator_id: None,
            agent_id: Some(agent.to_string()),
            instance_id: None,
            api_key_id: None,
            auth_method: AuthMethod::Jwt,
        }
    }

    fn ctx_operator(op: &str) -> ActingContext {
        ActingContext {
            operator_id: Some(op.to_string()),
            agent_id: None,
            instance_id: None,
            api_key_id: None,
            auth_method: AuthMethod::Jwt,
        }
    }

    fn ctx_empty() -> ActingContext {
        ActingContext {
            operator_id: None,
            agent_id: None,
            instance_id: None,
            api_key_id: None,
            auth_method: AuthMethod::ApiKey,
        }
    }

    #[test]
    fn auth_context_agent_wins_over_header() {
        // The JWT-derived agent_id is authoritative — a spoofed X-Agent-Id
        // header must not override it, otherwise an agent could impersonate
        // another agent's budget bucket.
        let ctx = ctx_agent("agent-real");
        let headers = headers_with_agent("agent-spoof");
        assert_eq!(resolve_agent_id(&ctx, &headers), "agent-real");
    }

    #[test]
    fn operator_context_yields_prefixed_id() {
        // Operator callers don't have an agent_id but should bucket usage
        // under a distinct `op:*` key rather than falling through to legacy
        // header / "unknown".
        let ctx = ctx_operator("bootstrap");
        assert_eq!(resolve_agent_id(&ctx, &empty_headers()), "op:bootstrap");
    }

    #[test]
    fn header_fallback_when_context_has_no_identity() {
        // Legacy agent-runtime containers that haven't yet migrated to JWTs
        // still set X-Agent-Id — the handler must honour it when the
        // middleware didn't populate the context.
        let ctx = ctx_empty();
        assert_eq!(
            resolve_agent_id(&ctx, &headers_with_agent("agent-legacy")),
            "agent-legacy"
        );
    }

    #[test]
    fn unknown_when_nothing_provided() {
        let ctx = ctx_empty();
        assert_eq!(resolve_agent_id(&ctx, &empty_headers()), "unknown");
    }

    #[test]
    fn empty_header_falls_through_to_unknown() {
        let ctx = ctx_empty();
        let mut headers = HeaderMap::new();
        headers.insert("x-agent-id", "".parse().unwrap());
        assert_eq!(resolve_agent_id(&ctx, &headers), "unknown");
    }
}
