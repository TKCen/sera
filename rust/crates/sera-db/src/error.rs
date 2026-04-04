//! Database error types.

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("database error: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("not found: {entity} with {key}={value}")]
    NotFound {
        entity: &'static str,
        key: &'static str,
        value: String,
    },
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("integrity error: {0}")]
    Integrity(String),
}
