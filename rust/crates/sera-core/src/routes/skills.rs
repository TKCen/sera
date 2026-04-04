//! Skills endpoint.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};

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

/// Request body for creating a skill.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSkillRequest {
    pub name: String,
    pub version: String,
    pub description: String,
    pub triggers: Option<serde_json::Value>,
    pub content: String,
    pub category: Option<String>,
    pub tags: Option<serde_json::Value>,
    pub max_tokens: Option<i32>,
}

/// POST /api/skills
pub async fn create_skill(
    State(state): State<AppState>,
    Json(body): Json<CreateSkillRequest>,
) -> Result<(StatusCode, Json<SkillResponse>), AppError> {
    let triggers = body.triggers.unwrap_or(serde_json::json!([]));

    let row = SkillRepository::create_skill(
        state.db.inner(),
        &body.name,
        &body.version,
        &body.description,
        &triggers,
        &body.content,
        body.category.as_deref(),
        body.tags.as_ref(),
        body.max_tokens,
    )
    .await?;

    Ok((
        StatusCode::CREATED,
        Json(SkillResponse {
            id: row.id.to_string(),
            skill_id: row.skill_id,
            name: row.name,
            version: row.version,
            description: row.description,
            triggers: row.triggers,
            source: row.source,
            category: row.category,
            tags: row.tags,
        }),
    ))
}

/// DELETE /api/skills/:name
pub async fn delete_skill(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let deleted = SkillRepository::delete_skill(state.db.inner(), &name).await?;
    if !deleted {
        return Err(AppError::Db(sera_db::DbError::NotFound {
            entity: "skill",
            key: "name",
            value: name,
        }));
    }
    Ok(Json(serde_json::json!({"success": true})))
}
