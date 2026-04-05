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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn db_error_not_found_display() {
        let err = DbError::NotFound {
            entity: "Agent",
            key: "id",
            value: "agent-123".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("not found"));
        assert!(msg.contains("Agent"));
        assert!(msg.contains("agent-123"));
    }

    #[test]
    fn db_error_conflict_display() {
        let err = DbError::Conflict("Duplicate key value".to_string());
        let msg = err.to_string();
        assert!(msg.contains("conflict"));
        assert!(msg.contains("Duplicate"));
    }

    #[test]
    fn db_error_integrity_display() {
        let err = DbError::Integrity("Foreign key constraint violated".to_string());
        let msg = err.to_string();
        assert!(msg.contains("integrity error"));
        assert!(msg.contains("Foreign key"));
    }

    #[test]
    fn db_error_variants_are_unique() {
        let not_found = DbError::NotFound {
            entity: "Agent",
            key: "id",
            value: "123".to_string(),
        };
        let conflict = DbError::Conflict("test".to_string());
        let integrity = DbError::Integrity("test".to_string());

        assert!(not_found.to_string().contains("not found"));
        assert!(conflict.to_string().contains("conflict"));
        assert!(integrity.to_string().contains("integrity"));
    }
}
