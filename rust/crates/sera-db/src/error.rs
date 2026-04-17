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

    #[test]
    fn db_error_not_found_contains_key_and_value() {
        let err = DbError::NotFound {
            entity: "session",
            key: "name",
            value: "my-session".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("session"));
        assert!(msg.contains("name"));
        assert!(msg.contains("my-session"));
    }

    #[test]
    fn db_error_conflict_contains_message() {
        let msg = "unique constraint on (name)";
        let err = DbError::Conflict(msg.to_string());
        assert!(err.to_string().contains(msg));
    }

    #[test]
    fn db_error_integrity_contains_message() {
        let msg = "foreign key violation: agent_id";
        let err = DbError::Integrity(msg.to_string());
        assert!(err.to_string().contains(msg));
    }

    #[test]
    fn db_error_implements_std_error() {
        // Verify DbError satisfies std::error::Error (used by ? operator and error chains).
        fn takes_error(_: &dyn std::error::Error) {}
        let err = DbError::Conflict("test".to_string());
        takes_error(&err);
    }

    #[test]
    fn db_error_conflict_is_debug_printable() {
        let err = DbError::Conflict("dup".to_string());
        let debug_str = format!("{err:?}");
        assert!(debug_str.contains("Conflict"));
    }

    #[test]
    fn db_error_not_found_is_debug_printable() {
        let err = DbError::NotFound {
            entity: "X",
            key: "k",
            value: "v".to_string(),
        };
        let debug_str = format!("{err:?}");
        assert!(debug_str.contains("NotFound"));
    }
}
