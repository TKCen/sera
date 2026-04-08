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

/// Budget status response — matches frontend AgentBudget type.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BudgetResponse {
    pub max_llm_tokens_per_hour: Option<u64>,
    pub max_llm_tokens_per_day: Option<u64>,
    pub current_hour_tokens: u64,
    pub current_day_tokens: u64,
}

/// GET /api/budget/agents/:id/budget
pub async fn get_agent_budget(
    State(state): State<AppState>,
    Path(agent_id): Path<String>,
) -> Result<Json<BudgetResponse>, AppError> {
    let status = MeteringRepository::check_budget(state.db.inner(), &agent_id).await?;
    Ok(Json(BudgetResponse {
        max_llm_tokens_per_hour: if status.hourly_quota > 0 { Some(status.hourly_quota) } else { None },
        max_llm_tokens_per_day: if status.daily_quota > 0 { Some(status.daily_quota) } else { None },
        current_hour_tokens: status.hourly_used,
        current_day_tokens: status.daily_used,
    }))
}

/// GET /api/budget/agents/:id — 7-day usage history.
pub async fn get_agent_usage_history(
    State(state): State<AppState>,
    Path(agent_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let rows = MeteringRepository::agent_daily_usage(state.db.inner(), &agent_id).await?;
    let usage: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|r| {
            serde_json::json!({
                "date": r.date.to_string(),
                "totalTokens": r.total_tokens
            })
        })
        .collect();
    Ok(Json(serde_json::json!({
        "agentId": agent_id,
        "usage": usage
    })))
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

/// Query params for usage data.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct UsageQuery {
    pub group_by: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
}

/// GET /api/metering/usage — usage data grouped by day.
pub async fn get_usage(
    State(state): State<AppState>,
    axum::extract::Query(_params): axum::extract::Query<UsageQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let rows = MeteringRepository::global_daily_usage(state.db.inner()).await?;
    let data: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|r| {
            serde_json::json!({
                "period": r.date.to_string(),
                "totalTokens": r.total_tokens,
                "costUsd": 0
            })
        })
        .collect();
    Ok(Json(serde_json::json!({ "data": data })))
}

/// GET /api/budget — global usage totals (last 7 days by day).
pub async fn get_global_budget(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, AppError> {
    let rows = MeteringRepository::global_daily_usage(state.db.inner()).await?;
    let usage: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|r| {
            serde_json::json!({
                "date": r.date.to_string(),
                "totalTokens": r.total_tokens
            })
        })
        .collect();
    Ok(Json(serde_json::json!({"usage": usage})))
}

/// GET /api/budget/agents — per-agent token rankings.
pub async fn get_agent_rankings(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, AppError> {
    let rows = MeteringRepository::agent_rankings(state.db.inner()).await?;
    let rankings: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|r| {
            serde_json::json!({
                "agentId": r.agent_id,
                "totalTokens": r.total_tokens
            })
        })
        .collect();
    Ok(Json(serde_json::json!({"rankings": rankings})))
}

/// GET /api/metering/summary — today's totals across all agents.
pub async fn get_metering_summary(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, AppError> {
    let row = MeteringRepository::today_summary(state.db.inner()).await?;
    Ok(Json(serde_json::json!({
        "todayTotalTokens": row.total_tokens.unwrap_or(0)
    })))
}
