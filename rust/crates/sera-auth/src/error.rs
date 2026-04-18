//! Authentication error types.

use sera_errors::{SeraError, SeraErrorCode};
use thiserror::Error;

/// Errors that can occur during authentication and authorization.
#[derive(Debug, Error)]
pub enum AuthError {
    #[error("Invalid token")]
    InvalidToken,

    #[error("Token has expired")]
    ExpiredToken,

    #[error("Missing authorization header")]
    MissingHeader,

    #[error("Invalid authorization header format")]
    InvalidHeader,

    #[error("Unauthorized")]
    Unauthorized,

    #[error("JWT error: {0}")]
    JwtError(String),
}

impl From<AuthError> for SeraError {
    fn from(err: AuthError) -> Self {
        let code = match &err {
            AuthError::InvalidToken => SeraErrorCode::Unauthorized,
            AuthError::ExpiredToken => SeraErrorCode::Unauthorized,
            AuthError::MissingHeader => SeraErrorCode::Unauthorized,
            AuthError::InvalidHeader => SeraErrorCode::InvalidInput,
            AuthError::Unauthorized => SeraErrorCode::Unauthorized,
            AuthError::JwtError(_) => SeraErrorCode::Unauthorized,
        };
        SeraError::with_source(code, err.to_string(), err)
    }
}

impl From<jsonwebtoken::errors::Error> for AuthError {
    fn from(err: jsonwebtoken::errors::Error) -> Self {
        match err.kind() {
            jsonwebtoken::errors::ErrorKind::ExpiredSignature => AuthError::ExpiredToken,
            jsonwebtoken::errors::ErrorKind::InvalidSignature => AuthError::InvalidToken,
            _ => AuthError::JwtError(err.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sera_errors::SeraErrorCode;

    #[test]
    fn test_error_display() {
        assert_eq!(
            AuthError::InvalidToken.to_string(),
            "Invalid token"
        );
        assert_eq!(
            AuthError::ExpiredToken.to_string(),
            "Token has expired"
        );
        assert_eq!(
            AuthError::MissingHeader.to_string(),
            "Missing authorization header"
        );
        assert_eq!(
            AuthError::Unauthorized.to_string(),
            "Unauthorized"
        );
    }

    #[test]
    fn auth_error_invalid_token_maps_to_unauthorized() {
        let e: SeraError = AuthError::InvalidToken.into();
        assert_eq!(e.code, SeraErrorCode::Unauthorized);
    }

    #[test]
    fn auth_error_expired_token_maps_to_unauthorized() {
        let e: SeraError = AuthError::ExpiredToken.into();
        assert_eq!(e.code, SeraErrorCode::Unauthorized);
    }

    #[test]
    fn auth_error_missing_header_maps_to_unauthorized() {
        let e: SeraError = AuthError::MissingHeader.into();
        assert_eq!(e.code, SeraErrorCode::Unauthorized);
    }

    #[test]
    fn auth_error_invalid_header_maps_to_invalid_input() {
        let e: SeraError = AuthError::InvalidHeader.into();
        assert_eq!(e.code, SeraErrorCode::InvalidInput);
    }

    #[test]
    fn auth_error_unauthorized_maps_to_unauthorized() {
        let e: SeraError = AuthError::Unauthorized.into();
        assert_eq!(e.code, SeraErrorCode::Unauthorized);
    }

    #[test]
    fn auth_error_jwt_error_maps_to_unauthorized() {
        let e: SeraError = AuthError::JwtError("bad sig".to_string()).into();
        assert_eq!(e.code, SeraErrorCode::Unauthorized);
        assert!(e.message.contains("bad sig"));
    }
}
