//! Metering and budget endpoints.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};

use sera_db::metering::MeteringRepository;

use crate::error::AppError;
use crate::state::AppState;

/// Budget status response (camelCase for API compatibility).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BudgetResponse {
    pub allowed: bool,
    pub hourly_used: u64,
    pub hourly_quota: u64,
    pub daily_used: u64,
    pub daily_quota: u64,
}

/// GET /api/budget/agents/:id
pub async fn get_agent_budget(
    State(state): State<AppState>,
    Path(agent_id): Path<String>,
) -> Result<Json<BudgetResponse>, AppError> {
    let status = MeteringRepository::check_budget(state.db.inner(), &agent_id).await?;
    Ok(Json(BudgetResponse {
        allowed: status.allowed,
        hourly_used: status.hourly_used,
        hourly_quota: status.hourly_quota,
        daily_used: status.daily_used,
        daily_quota: status.daily_quota,
    }))
}

/// Request body for updating budget quotas.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateBudgetRequest {
    pub max_llm_tokens_per_hour: Option<i64>,
    pub max_llm_tokens_per_day: Option<i64>,
}

/// PATCH /api/budget/agents/:id/budget
pub async fn update_agent_budget(
    State(state): State<AppState>,
    Path(agent_id): Path<String>,
    Json(body): Json<UpdateBudgetRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    // Resolve: null or 0 → 0 (unlimited), absent → None (keep existing)
    let hourly = body.max_llm_tokens_per_hour.map(|v| if v == 0 { 0 } else { v });
    let daily = body.max_llm_tokens_per_day.map(|v| if v == 0 { 0 } else { v });

    if hourly.is_none() && daily.is_none() {
        return Ok(Json(serde_json::json!({"success": true})));
    }

    MeteringRepository::upsert_quota(state.db.inner(), &agent_id, hourly, daily).await?;
    Ok(Json(serde_json::json!({"success": true})))
}

/// POST /api/budget/agents/:id/budget/reset
pub async fn reset_agent_budget(
    State(state): State<AppState>,
    Path(agent_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let deleted = MeteringRepository::reset_usage(state.db.inner(), &agent_id).await?;
    Ok(Json(serde_json::json!({
        "success": true,
        "deletedRows": deleted
    })))
}

/// Request body for recording token usage.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordUsageRequest {
    pub agent_id: String,
    pub circle_id: Option<String>,
    pub model: String,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub total_tokens: i64,
    pub cost_usd: Option<f64>,
    pub latency_ms: Option<i64>,
    pub status: Option<String>,
}

/// POST /api/metering/usage
pub async fn record_usage(
    State(state): State<AppState>,
    Json(body): Json<RecordUsageRequest>,
) -> Result<StatusCode, AppError> {
    MeteringRepository::record_usage(
        state.db.inner(),
        &body.agent_id,
        body.circle_id.as_deref(),
        &body.model,
        body.prompt_tokens,
        body.completion_tokens,
        body.total_tokens,
        body.cost_usd,
        body.latency_ms,
        body.status.as_deref().unwrap_or("success"),
    )
    .await?;
    Ok(StatusCode::CREATED)
}
