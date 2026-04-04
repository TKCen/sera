//! Skills endpoint.

use axum::{extract::State, Json};
use serde::Serialize;

use sera_db::skills::SkillRepository;

use crate::error::AppError;
use crate::state::AppState;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillResponse {
    pub id: String,
    pub skill_id: Option<String>,
    pub name: String,
    pub version: String,
    pub description: String,
    pub triggers: serde_json::Value,
    pub source: String,
    pub category: Option<String>,
    pub tags: Option<serde_json::Value>,
}

/// GET /api/skills
pub async fn list_skills(
    State(state): State<AppState>,
) -> Result<Json<Vec<SkillResponse>>, AppError> {
    let rows = SkillRepository::list_skills(state.db.inner()).await?;
    let skills: Vec<SkillResponse> = rows
        .into_iter()
        .map(|r| SkillResponse {
            id: r.id.to_string(),
            skill_id: r.skill_id,
            name: r.name,
            version: r.version,
            description: r.description,
            triggers: r.triggers,
            source: r.source,
            category: r.category,
            tags: r.tags,
        })
        .collect();
    Ok(Json(skills))
}
