//! Authentication error types.

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
}
