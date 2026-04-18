//! Authentication middleware layer for protected routes.

use axum::{
    extract::Request,
    http::StatusCode,
    middleware::Next,
    response::Response,
    Json,
};
use serde_json::json;
use std::sync::Arc;
use tracing::debug;

use sera_auth::JwtService;
use sera_auth::types::{ActingContext, AuthMethod};

/// Combined auth middleware: checks API key first, then JWT.
///
/// If the Bearer token matches the bootstrap API key, creates an operator context.
/// Otherwise attempts JWT verification.
pub async fn require_auth(
    request: Request,
    next: Next,
    jwt_service: Arc<JwtService>,
    api_key: Arc<String>,
) -> Result<Response, (StatusCode, Json<serde_json::Value>)> {
    let auth_header = match request.headers().get("authorization") {
        Some(h) => match h.to_str() {
            Ok(s) => s.to_string(),
            Err(_) => {
                return Err((
                    StatusCode::UNAUTHORIZED,
                    Json(json!({"error": "Unauthorized"})),
                ))
            }
        },
        None => {
            return Err((
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Unauthorized"})),
            ))
        }
    };

    let token = match auth_header.strip_prefix("Bearer ") {
        Some(t) => t,
        None => {
            return Err((
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Unauthorized"})),
            ))
        }
    };

    // Check API key first (simple string match for bootstrap dev key)
    let acting_context = if token == api_key.as_str() {
        debug!("Authenticated via API key");
        ActingContext {
            operator_id: Some("bootstrap".to_string()),
            agent_id: None,
            instance_id: None,
            api_key_id: Some("bootstrap".to_string()),
            auth_method: AuthMethod::ApiKey,
        }
    } else {
        // Try JWT verification
        match jwt_service.verify(token) {
            Ok(claims) => {
                debug!("Authenticated via JWT: sub={}", claims.sub);
                ActingContext {
                    operator_id: if claims.sub.starts_with("op-") {
                        Some(claims.sub.clone())
                    } else {
                        None
                    },
                    agent_id: claims.agent_id.clone(),
                    instance_id: claims.instance_id.clone(),
                    api_key_id: None,
                    auth_method: AuthMethod::Jwt,
                }
            }
            Err(e) => {
                debug!("Auth failed: {e}");
                return Err((
                    StatusCode::UNAUTHORIZED,
                    Json(json!({"error": "Unauthorized"})),
                ));
            }
        }
    };

    let mut request = request;
    request.extensions_mut().insert(acting_context);
    Ok(next.run(request).await)
}

#[cfg(test)]
mod tests {
    #[test]
    fn bearer_prefix_stripping() {
        let header = "Bearer my-token-123";
        let token = header.strip_prefix("Bearer ").unwrap();
        assert_eq!(token, "my-token-123");
    }
}
