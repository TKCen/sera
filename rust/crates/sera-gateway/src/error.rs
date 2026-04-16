//! Application error types with axum IntoResponse.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

use sera_auth::AuthError;
use sera_db::DbError;
use sera_errors::SeraError;

/// Application-level error that converts to HTTP responses.
#[allow(dead_code)]
#[derive(Debug)]
pub enum AppError {
    /// Database errors.
    Db(DbError),
    /// Authentication errors.
    Auth(AuthError),
    /// Forbidden action.
    Forbidden(String),
    /// Generic internal errors.
    Internal(anyhow::Error),
    /// Sera-classified errors from the shared error taxonomy.
    Sera(SeraError),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            AppError::Db(DbError::NotFound { entity, key, value }) => (
                StatusCode::NOT_FOUND,
                format!("{entity} with {key}={value} not found"),
            ),
            AppError::Db(DbError::Conflict(msg)) => (StatusCode::CONFLICT, msg.clone()),
            AppError::Db(DbError::Integrity(msg)) => {
                (StatusCode::UNPROCESSABLE_ENTITY, msg.clone())
            }
            AppError::Db(DbError::Sqlx(e)) => {
                tracing::error!("Database error: {e}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal server error".to_string(),
                )
            }
            AppError::Auth(_) => (StatusCode::UNAUTHORIZED, "Unauthorized".to_string()),
            AppError::Forbidden(msg) => (StatusCode::FORBIDDEN, msg.clone()),
            AppError::Internal(e) => {
                tracing::error!("Internal error: {e}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal server error".to_string(),
                )
            }
            AppError::Sera(e) => {
                let status = StatusCode::from_u16(e.code.http_status())
                    .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
                (status, e.message.clone())
            }
        };

        (status, Json(json!({"error": message}))).into_response()
    }
}

impl From<DbError> for AppError {
    fn from(err: DbError) -> Self {
        AppError::Db(err)
    }
}

impl From<AuthError> for AppError {
    fn from(err: AuthError) -> Self {
        AppError::Auth(err)
    }
}

impl From<anyhow::Error> for AppError {
    fn from(err: anyhow::Error) -> Self {
        AppError::Internal(err)
    }
}

impl From<SeraError> for AppError {
    fn from(err: SeraError) -> Self {
        AppError::Sera(err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use axum::http::StatusCode;

    #[tokio::test]
    async fn not_found_returns_404() {
        let err = AppError::Db(DbError::NotFound {
            entity: "agent_instance",
            key: "id",
            value: "inst-123".to_string(),
        });
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        let body = to_bytes(response.into_body(), 1024).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json["error"].as_str().unwrap().contains("not found"));
    }

    #[tokio::test]
    async fn auth_error_returns_401() {
        let err = AppError::Auth(AuthError::Unauthorized);
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        let body = to_bytes(response.into_body(), 1024).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"], "Unauthorized");
    }

    #[tokio::test]
    async fn internal_error_returns_500() {
        let err = AppError::Internal(anyhow::anyhow!("something broke"));
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);

        let body = to_bytes(response.into_body(), 1024).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"], "Internal server error");
    }

    #[tokio::test]
    async fn conflict_returns_409() {
        let err = AppError::Db(DbError::Conflict("duplicate key".to_string()));
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn sera_error_maps_to_http_status() {
        let err = AppError::Sera(SeraError::not_found("agent not found"));
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        let body = to_bytes(response.into_body(), 1024).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"], "agent not found");
    }
}
