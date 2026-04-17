//! Training-exports endpoints.
//!
//! POST /api/training-exports         — create + run export inline
//! GET  /api/training-exports/:id     — fetch export record
//! GET  /api/training-exports/:id/download — stream the output file

use axum::{
    body::Body,
    extract::{Path, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;
use std::sync::Arc;
use tokio_util::io::ReaderStream;
use uuid::Uuid;

use sera_db::training_exports::TrainingExportRepo;
use sera_types::training_export::{TrainingExportRecord, TrainingExportRequest, TrainingExportStatus};

use crate::error::AppError;
use crate::services::training_export_job::{FakeDbTranscriptRepo, TrainingExportJob};
use crate::state::AppState;

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateExportResponse {
    pub id: String,
    pub status: String,
    pub download_url: String,
}

fn record_status_str(s: TrainingExportStatus) -> &'static str {
    match s {
        TrainingExportStatus::Queued => "queued",
        TrainingExportStatus::Running => "running",
        TrainingExportStatus::Complete => "complete",
        TrainingExportStatus::Failed => "failed",
    }
}

fn record_to_json(r: &TrainingExportRecord) -> serde_json::Value {
    serde_json::json!({
        "id": r.id.to_string(),
        "format": serde_json::to_value(r.format).unwrap_or(serde_json::Value::Null),
        "filter": serde_json::to_value(&r.filter).unwrap_or(serde_json::Value::Null),
        "piiRedaction": r.pii_redaction,
        "status": record_status_str(r.status),
        "totalRecords": r.total_records,
        "outputPath": r.output_path,
        "error": r.error,
        "createdAt": r.created_at.to_rfc3339(),
        "startedAt": r.started_at.map(|d| d.to_rfc3339()),
        "finishedAt": r.finished_at.map(|d| d.to_rfc3339()),
    })
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// POST /api/training-exports
///
/// Accepts a `TrainingExportRequest`, runs the job inline (Phase 2 — no
/// worker queue), and returns `{id, status, download_url}`.
pub async fn create_export(
    State(state): State<AppState>,
    Json(body): Json<TrainingExportRequest>,
) -> Result<(StatusCode, Json<CreateExportResponse>), AppError> {
    let repo = get_repo(&state);
    let id = repo
        .create(&body)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("{e}")))?;

    // Transcript repo — use the SQLite-backed stub for Phase 2.
    // Follow-up bead (sera-dca-phase3) will wire in the real PG repo.
    let transcript_repo = Arc::new(FakeDbTranscriptRepo::from_sqlite(&state));

    let output_dir = export_output_dir();
    let req_clone = body.clone();

    // Run synchronously in a blocking thread to avoid blocking the async executor.
    let repo_clone = repo.clone();
    let result = tokio::task::spawn_blocking(move || {
        TrainingExportJob::run(req_clone, repo_clone, transcript_repo, &output_dir, id)
    })
    .await
    .map_err(|e| AppError::Internal(anyhow::anyhow!("join error: {e}")))?;

    // Even if the job failed, we return the id so the client can query status.
    let status_str = match &result {
        Ok(_) => "complete",
        Err(_) => "failed",
    };

    let resp = CreateExportResponse {
        id: id.to_string(),
        status: status_str.to_string(),
        download_url: format!("/api/training-exports/{id}/download"),
    };
    Ok((StatusCode::CREATED, Json(resp)))
}

/// GET /api/training-exports/:id
pub async fn get_export(
    State(state): State<AppState>,
    Path(id_str): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let id = parse_uuid(&id_str)?;
    let repo = get_repo(&state);
    let record = repo
        .get(id)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("{e}")))?
        .ok_or_else(|| {
            AppError::Db(sera_db::DbError::NotFound {
                entity: "training_export",
                key: "id",
                value: id_str,
            })
        })?;
    Ok(Json(record_to_json(&record)))
}

/// GET /api/training-exports/:id/download
///
/// Streams the output file. Returns 404 if the export is not complete or has
/// no output path.
pub async fn download_export(
    State(state): State<AppState>,
    Path(id_str): Path<String>,
) -> Result<Response, AppError> {
    let id = parse_uuid(&id_str)?;
    let repo = get_repo(&state);
    let record = repo
        .get(id)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("{e}")))?
        .ok_or_else(|| {
            AppError::Db(sera_db::DbError::NotFound {
                entity: "training_export",
                key: "id",
                value: id_str.clone(),
            })
        })?;

    if record.status != TrainingExportStatus::Complete {
        return Ok((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "export not complete"})),
        )
            .into_response());
    }

    let path = record.output_path.ok_or_else(|| {
        AppError::Internal(anyhow::anyhow!("complete export has no output_path"))
    })?;

    let file = tokio::fs::File::open(&path)
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("open file: {e}")))?;

    let filename = std::path::Path::new(&path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("export.jsonl")
        .to_string();

    let stream = ReaderStream::new(file);
    let body = Body::from_stream(stream);

    Ok((
        StatusCode::OK,
        [
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{filename}\""),
            ),
            (
                header::CONTENT_TYPE,
                "application/x-ndjson".to_string(),
            ),
        ],
        body,
    )
        .into_response())
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn parse_uuid(s: &str) -> Result<Uuid, AppError> {
    s.parse::<Uuid>()
        .map_err(|_| AppError::Internal(anyhow::anyhow!("invalid uuid: {s}")))
}

fn get_repo(state: &AppState) -> Arc<TrainingExportRepo> {
    state
        .training_export_repo
        .clone()
        .expect("training_export_repo must be set in AppState")
}

fn export_output_dir() -> std::path::PathBuf {
    let dir = std::env::var("SERA_EXPORT_DIR").unwrap_or_else(|_| "/tmp/sera-exports".to_string());
    let p = std::path::PathBuf::from(dir);
    let _ = std::fs::create_dir_all(&p);
    p
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::to_bytes,
        http::Request,
        Router,
        routing::{get, post},
    };
    use rusqlite::Connection;
    use std::sync::{Arc, Mutex};
    use tower::ServiceExt;

    use crate::services::training_export_job::TranscriptRepo;
    use sera_db::sqlite::TranscriptRow;
    use sera_types::training_export::{TrainingExportFilter, TrainingExportFormat};

    // Build a minimal AppState for route tests.
    fn test_state() -> AppState {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS training_exports (
                id TEXT PRIMARY KEY,
                format TEXT NOT NULL,
                filter_min_score REAL,
                filter_date_from TEXT,
                filter_date_to TEXT,
                filter_trigger_type TEXT,
                pii_redaction INTEGER NOT NULL DEFAULT 1,
                status TEXT NOT NULL DEFAULT 'queued',
                total_records INTEGER,
                output_path TEXT,
                error TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                started_at TEXT,
                finished_at TEXT
            );",
        )
        .expect("schema");
        let repo = Arc::new(TrainingExportRepo::new(Arc::new(Mutex::new(conn))));
        crate::state::test_app_state_with_export_repo(repo)
    }

    fn test_router(state: AppState) -> Router {
        Router::new()
            .route("/api/training-exports", post(create_export))
            .route("/api/training-exports/:id", get(get_export))
            .route("/api/training-exports/:id/download", get(download_export))
            .with_state(state)
    }

    #[tokio::test]
    async fn post_creates_export_and_returns_created() {
        let state = test_state();
        let app = test_router(state);

        let body = serde_json::json!({
            "format": "openai_jsonl",
            "filter": {},
            "pii_redaction": false
        });

        let req = Request::builder()
            .method("POST")
            .uri("/api/training-exports")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);

        let bytes = to_bytes(resp.into_body(), 4096).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(json["id"].is_string());
        assert!(json["downloadUrl"].as_str().unwrap().contains("/download"));
    }

    #[tokio::test]
    async fn get_export_returns_404_for_unknown_id() {
        let state = test_state();
        let app = test_router(state);
        let unknown_id = Uuid::new_v4();

        let req = Request::builder()
            .method("GET")
            .uri(format!("/api/training-exports/{unknown_id}"))
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }
}
