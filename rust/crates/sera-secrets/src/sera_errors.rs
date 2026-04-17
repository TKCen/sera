//! `From` impl bridging [`SecretsError`] into [`SeraError`].

use sera_errors::{SeraError, SeraErrorCode};

use crate::SecretsError;

impl From<SecretsError> for SeraError {
    fn from(err: SecretsError) -> Self {
        let code = match &err {
            SecretsError::NotFound { .. } => SeraErrorCode::NotFound,
            SecretsError::Provider { .. } => SeraErrorCode::Internal,
            SecretsError::ReadOnly => SeraErrorCode::Forbidden,
            SecretsError::Io { .. } => SeraErrorCode::Internal,
        };
        SeraError::with_source(code, err.to_string(), err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn not_found_maps_to_not_found() {
        let e: SeraError = SecretsError::NotFound { key: "DB_PASS".into() }.into();
        assert_eq!(e.code, SeraErrorCode::NotFound);
        assert!(e.message.contains("DB_PASS"));
    }

    #[test]
    fn provider_error_maps_to_internal() {
        let e: SeraError = SecretsError::Provider { reason: "vault unreachable".into() }.into();
        assert_eq!(e.code, SeraErrorCode::Internal);
        assert!(e.message.contains("vault unreachable"));
    }

    #[test]
    fn read_only_maps_to_forbidden() {
        let e: SeraError = SecretsError::ReadOnly.into();
        assert_eq!(e.code, SeraErrorCode::Forbidden);
    }

    #[test]
    fn io_error_maps_to_internal() {
        let e: SeraError = SecretsError::Io { reason: "disk full".into() }.into();
        assert_eq!(e.code, SeraErrorCode::Internal);
        assert!(e.message.contains("disk full"));
    }
}
