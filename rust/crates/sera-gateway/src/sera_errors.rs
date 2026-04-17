//! `From` impls bridging sera-gateway error types into [`SeraError`].
//!
//! These are the interop seams that allow callers using the unified
//! error taxonomy to receive a `SeraError` from gateway-internal errors
//! without those callers needing to know about `AppError`.
//!
//! `From<ProcessError> for SeraError` lives in `process_manager.rs` because
//! `ProcessError` is defined in the lib crate and the orphan rule requires
//! the impl to be in the same crate as at least one of the types.

use sera_errors::{SeraError, SeraErrorCode};

use crate::error::AppError;

// ---------------------------------------------------------------------------
// AppError → SeraError
// ---------------------------------------------------------------------------

impl From<AppError> for SeraError {
    fn from(err: AppError) -> Self {
        match err {
            AppError::Db(db_err) => {
                use sera_db::DbError;
                match db_err {
                    DbError::NotFound { entity, key, value } => SeraError::new(
                        SeraErrorCode::NotFound,
                        format!("{entity} with {key}={value} not found"),
                    ),
                    DbError::Conflict(msg) => SeraError::new(SeraErrorCode::AlreadyExists, msg),
                    DbError::Integrity(msg) => SeraError::new(SeraErrorCode::InvalidInput, msg),
                    DbError::Sqlx(e) => {
                        SeraError::with_source(SeraErrorCode::Internal, "database error", e)
                    }
                }
            }
            AppError::BadRequest(msg) => SeraError::new(SeraErrorCode::InvalidInput, msg),
            AppError::TooManyRequests(msg) => SeraError::new(SeraErrorCode::RateLimited, msg),
            AppError::Auth(_) => SeraError::new(SeraErrorCode::Unauthorized, "Unauthorized"),
            AppError::Forbidden(msg) => SeraError::new(SeraErrorCode::Forbidden, msg),
            AppError::Internal(e) => {
                SeraError::new(SeraErrorCode::Internal, format!("internal error: {e}"))
            }
            AppError::Sera(e) => e,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sera_auth::AuthError;
    use sera_db::DbError;
    use sera_errors::SeraErrorCode;

    #[test]
    fn app_db_not_found_maps_to_not_found() {
        let err = AppError::Db(DbError::NotFound {
            entity: "agent_instance",
            key: "id",
            value: "inst-1".to_string(),
        });
        let sera: SeraError = err.into();
        assert_eq!(sera.code, SeraErrorCode::NotFound);
        assert!(sera.message.contains("not found"));
    }

    #[test]
    fn app_db_conflict_maps_to_already_exists() {
        let err = AppError::Db(DbError::Conflict("duplicate key".to_string()));
        let sera: SeraError = err.into();
        assert_eq!(sera.code, SeraErrorCode::AlreadyExists);
        assert_eq!(sera.message, "duplicate key");
    }

    #[test]
    fn app_db_integrity_maps_to_invalid_input() {
        let err = AppError::Db(DbError::Integrity("bad foreign key".to_string()));
        let sera: SeraError = err.into();
        assert_eq!(sera.code, SeraErrorCode::InvalidInput);
    }

    #[test]
    fn app_auth_maps_to_unauthorized() {
        let err = AppError::Auth(AuthError::Unauthorized);
        let sera: SeraError = err.into();
        assert_eq!(sera.code, SeraErrorCode::Unauthorized);
    }

    #[test]
    fn app_forbidden_maps_to_forbidden() {
        let err = AppError::Forbidden("no access".to_string());
        let sera: SeraError = err.into();
        assert_eq!(sera.code, SeraErrorCode::Forbidden);
        assert_eq!(sera.message, "no access");
    }

    #[test]
    fn app_sera_variant_is_passthrough() {
        let inner = SeraError::new(SeraErrorCode::RateLimited, "too fast");
        let err = AppError::Sera(inner);
        let sera: SeraError = err.into();
        assert_eq!(sera.code, SeraErrorCode::RateLimited);
        assert_eq!(sera.message, "too fast");
    }
}
