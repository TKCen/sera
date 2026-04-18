//! Circles endpoint.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx;

use sera_db::circles::CircleRepository;

use crate::error::AppError;
use crate::state::AppState;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CircleResponse {
    pub id: String,
    pub name: String,
    pub display_name: String,
    pub description: Option<String>,
}

/// GET /api/circles
pub async fn list_circles(
    State(state): State<AppState>,
) -> Result<Json<Vec<CircleResponse>>, AppError> {
    let rows = CircleRepository::list_circles(state.db.inner()).await?;
    let circles: Vec<CircleResponse> = rows
        .into_iter()
        .map(|r| CircleResponse {
            id: r.id.to_string(),
            name: r.name,
            display_name: r.display_name,
            description: r.description,
        })
        .collect();
    Ok(Json(circles))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateCircleRequest {
    pub name: String,
    pub display_name: String,
    pub description: Option<String>,
}

/// POST /api/circles
pub async fn create_circle(
    State(state): State<AppState>,
    Json(body): Json<CreateCircleRequest>,
) -> Result<(StatusCode, Json<CircleResponse>), AppError> {
    let id = uuid::Uuid::new_v4().to_string();
    CircleRepository::create_circle(
        state.db.inner(),
        &id,
        &body.name,
        &body.display_name,
        body.description.as_deref(),
    )
    .await?;
    Ok((StatusCode::CREATED, Json(CircleResponse {
        id,
        name: body.name,
        display_name: body.display_name,
        description: body.description,
    })))
}

/// GET /api/circles/{id} — get a single circle by id.
pub async fn get_circle(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<CircleResponse>, AppError> {
    let row = CircleRepository::get_by_name(state.db.inner(), &id).await?;
    Ok(Json(CircleResponse {
        id: row.id.to_string(),
        name: row.name,
        display_name: row.display_name,
        description: row.description,
    }))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCircleRequest {
    pub display_name: Option<String>,
    pub description: Option<String>,
}

/// PATCH /api/circles/{id} — update a circle.
pub async fn update_circle(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<UpdateCircleRequest>,
) -> Result<Json<CircleResponse>, AppError> {
    // Get current circle first to merge updates
    let current = CircleRepository::get_by_name(state.db.inner(), &id).await?;

    let display_name = body.display_name.unwrap_or(current.display_name);
    let description = body.description.or(current.description);

    // Update in database
    sqlx::query(
        "UPDATE circles SET display_name = $1, description = $2, updated_at = NOW() WHERE id::text = $3 OR name = $3"
    )
    .bind(&display_name)
    .bind(&description)
    .bind(&id)
    .execute(state.db.inner())
    .await
    .map_err(|e| AppError::Db(sera_db::DbError::Sqlx(e)))?;

    let row = CircleRepository::get_by_name(state.db.inner(), &id).await?;
    Ok(Json(CircleResponse {
        id: row.id.to_string(),
        name: row.name,
        display_name: row.display_name,
        description: row.description,
    }))
}

/// DELETE /api/circles/:id
pub async fn delete_circle(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    CircleRepository::delete_circle(state.db.inner(), &id).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ── Constitution routes (sera-8d1.4) ─────────────────────────────────────────

/// Response body for GET /api/circles/{id}/constitution.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConstitutionResponse {
    /// The current constitution markdown text, or null if none.
    pub text: Option<String>,
    /// Current version number (0 if no versions recorded yet).
    pub version: i32,
}

/// Request body for PUT /api/circles/{id}/constitution.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateConstitutionRequest {
    /// New markdown constitution text. Send `null` or omit to clear.
    pub text: Option<String>,
    /// Identifier of the principal making the change (recorded in audit trail).
    pub changed_by: Option<String>,
}

/// GET /api/circles/{id}/constitution — returns current constitution + version.
pub async fn get_constitution(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ConstitutionResponse>, AppError> {
    let row = CircleRepository::get_by_name(state.db.inner(), &id).await?;
    let versions =
        CircleRepository::get_constitution_versions(state.db.inner(), &row.id.to_string())
            .await?;
    let version = versions.last().map(|v| v.version).unwrap_or(0);
    Ok(Json(ConstitutionResponse {
        text: row.constitution,
        version,
    }))
}

/// PUT /api/circles/{id}/constitution — update constitution + record audit entry.
pub async fn update_constitution(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<UpdateConstitutionRequest>,
) -> Result<Json<ConstitutionResponse>, AppError> {
    // Resolve the circle first so we have a stable UUID.
    let row = CircleRepository::get_by_name(state.db.inner(), &id).await?;
    let circle_id = row.id.to_string();

    // Compute SHA-256 of the new text (empty string hash for None/clear).
    let text_ref = body.text.as_deref().unwrap_or("");
    let hash = hex::encode(Sha256::digest(text_ref.as_bytes()));

    let changed_by = body
        .changed_by
        .as_deref()
        .unwrap_or("unknown")
        .to_string();

    // Write the updated constitution text and record the audit entry.
    CircleRepository::update_constitution(
        state.db.inner(),
        &circle_id,
        body.text.as_deref(),
    )
    .await?;

    let new_version = CircleRepository::record_constitution_update(
        state.db.inner(),
        &circle_id,
        &hash,
        &changed_by,
    )
    .await?;

    Ok(Json(ConstitutionResponse {
        text: body.text,
        version: new_version,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn circle_response_serializes() {
        let circle = CircleResponse {
            id: "123".to_string(),
            name: "engineering".to_string(),
            display_name: "Engineering Circle".to_string(),
            description: Some("Main engineering team".to_string()),
        };

        let json = serde_json::to_value(&circle).unwrap();
        assert_eq!(json["id"], "123");
        assert_eq!(json["displayName"], "Engineering Circle");
    }

    #[test]
    fn create_circle_request_deserializes() {
        let input = r#"{
            "name": "eng",
            "displayName": "Engineering",
            "description": "Team"
        }"#;

        let req: CreateCircleRequest = serde_json::from_str(input).unwrap();
        assert_eq!(req.name, "eng");
        assert_eq!(req.display_name, "Engineering");
        assert_eq!(req.description, Some("Team".to_string()));
    }

    #[test]
    fn update_circle_request_deserializes() {
        let input = r#"{
            "displayName": "New Name"
        }"#;

        let req: UpdateCircleRequest = serde_json::from_str(input).unwrap();
        assert_eq!(req.display_name, Some("New Name".to_string()));
        assert_eq!(req.description, None);
    }

    // ── Constitution route type tests (sera-8d1.4) ────────────────────────────

    #[test]
    fn constitution_response_serializes_camel_case() {
        let resp = ConstitutionResponse {
            text: Some("# Stack".to_string()),
            version: 3,
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["text"], "# Stack");
        assert_eq!(json["version"], 3);
    }

    #[test]
    fn constitution_response_null_text_serializes() {
        let resp = ConstitutionResponse {
            text: None,
            version: 0,
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert!(json["text"].is_null());
        assert_eq!(json["version"], 0);
    }

    #[test]
    fn update_constitution_request_deserializes_full() {
        let input = r#"{"text":"# Rules\n- Be safe","changedBy":"alice"}"#;
        let req: UpdateConstitutionRequest = serde_json::from_str(input).unwrap();
        assert_eq!(req.text.as_deref(), Some("# Rules\n- Be safe"));
        assert_eq!(req.changed_by.as_deref(), Some("alice"));
    }

    #[test]
    fn update_constitution_request_deserializes_clear() {
        let input = r#"{}"#;
        let req: UpdateConstitutionRequest = serde_json::from_str(input).unwrap();
        assert!(req.text.is_none());
        assert!(req.changed_by.is_none());
    }
}
