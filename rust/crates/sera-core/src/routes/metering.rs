//! Metering and budget endpoints.

use axum::{
    extract::{Path, State},
    Json,
};
use serde::Serialize;

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
