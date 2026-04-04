//! Axum middleware for authentication.

use axum::{
    extract::Request,
    http::StatusCode,
    middleware::Next,
    response::Response,
    Json,
};
use serde_json::json;
use std::sync::Arc;
use tracing::{debug, warn};

use crate::jwt::JwtService;
use crate::types::ActingContext;

/// Extract the Bearer token from the Authorization header.
fn extract_bearer_token(auth_header: &str) -> Result<&str, ()> {
    auth_header.strip_prefix("Bearer ").ok_or(())
}

/// Authentication middleware that extracts JWT tokens and inserts ActingContext into extensions.
///
/// # Behavior
///
/// - Extracts Bearer token from Authorization header
/// - Verifies JWT token using the provided JWT service
/// - Inserts ActingContext into request extensions on success
/// - Returns 401 JSON response on authentication failure
pub async fn auth_middleware(
    mut request: Request,
    next: Next,
    jwt_service: Arc<JwtService>,
) -> Result<Response, (StatusCode, Json<serde_json::Value>)> {
    // Get the authorization header
    let auth_header = match request.headers().get("authorization") {
        Some(header) => match header.to_str() {
            Ok(s) => s,
            Err(_) => {
                warn!("Invalid authorization header encoding");
                return Err((
                    StatusCode::UNAUTHORIZED,
                    Json(json!({"error": "Unauthorized"})),
                ));
            }
        },
        None => {
            debug!("Missing authorization header");
            return Err((
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Unauthorized"})),
            ));
        }
    };

    // Extract the Bearer token
    let token = match extract_bearer_token(auth_header) {
        Ok(t) => t,
        Err(_) => {
            warn!("Invalid authorization header format");
            return Err((
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Unauthorized"})),
            ));
        }
    };

    // Verify the JWT token
    let claims = match jwt_service.verify(token) {
        Ok(c) => c,
        Err(e) => {
            debug!("JWT verification failed: {}", e);
            return Err((
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Unauthorized"})),
            ));
        }
    };

    // Create acting context from claims
    let acting_context = ActingContext {
        operator_id: if claims.sub.starts_with("op-") {
            Some(claims.sub.clone())
        } else {
            None
        },
        agent_id: claims.agent_id.clone(),
        instance_id: claims.instance_id.clone(),
        api_key_id: None,
        auth_method: crate::types::AuthMethod::Jwt,
    };

    // Insert the context into request extensions
    request.extensions_mut().insert(acting_context);

    Ok(next.run(request).await)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_bearer_token_valid() {
        let result = extract_bearer_token("Bearer my-token-123");
        assert_eq!(result, Ok("my-token-123"));
    }

    #[test]
    fn test_extract_bearer_token_invalid_prefix() {
        let result = extract_bearer_token("Basic my-token-123");
        assert!(result.is_err());
    }

    #[test]
    fn test_extract_bearer_token_no_prefix() {
        let result = extract_bearer_token("my-token-123");
        assert!(result.is_err());
    }

    #[test]
    fn test_extract_bearer_token_empty() {
        let result = extract_bearer_token("Bearer ");
        assert_eq!(result, Ok(""));
    }
}
