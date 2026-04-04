//! Schedules endpoint.

use axum::{extract::State, Json};
use serde::Serialize;

use sera_db::schedules::ScheduleRepository;

use crate::error::AppError;
use crate::state::AppState;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleResponse {
    pub id: String,
    pub agent_name: Option<String>,
    pub name: String,
    pub cron: Option<String>,
    pub expression: Option<String>,
    pub r#type: Option<String>,
    pub source: String,
    pub status: Option<String>,
    pub last_run_at: Option<String>,
    pub last_run_status: Option<String>,
    pub next_run_at: Option<String>,
    pub category: Option<String>,
    pub description: Option<String>,
}

/// GET /api/schedules
pub async fn list_schedules(
    State(state): State<AppState>,
) -> Result<Json<Vec<ScheduleResponse>>, AppError> {
    let rows = ScheduleRepository::list_schedules(state.db.inner()).await?;
    let schedules: Vec<ScheduleResponse> = rows
        .into_iter()
        .map(|r| ScheduleResponse {
            id: r.id.to_string(),
            agent_name: r.agent_name,
            name: r.name,
            cron: r.cron,
            expression: r.expression,
            r#type: r.r#type,
            source: r.source,
            status: r.status,
            last_run_at: r.last_run_at.map(|t| t.to_string()),
            last_run_status: r.last_run_status,
            next_run_at: r.next_run_at.map(|t| t.to_string()),
            category: r.category,
            description: r.description,
        })
        .collect();
    Ok(Json(schedules))
}
