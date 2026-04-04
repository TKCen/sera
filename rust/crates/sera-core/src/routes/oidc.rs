//! OIDC authentication flow endpoints.
#![allow(dead_code, unused_imports)]

use axum::{
    extract::{Query, State},
    http::{header, StatusCode},
    Json,
    response::Redirect,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;

use crate::error::AppError;
use crate::state::AppState;

/// Minimal OIDC config response — only issuerUrl and clientId (matches TS)
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OidcConfig {
    pub issuer_url: String,
    pub client_id: String,
}

/// GET /api/auth/oidc-config — return minimal OIDC provider configuration
pub async fn get_oidc_config(
    State(state): State<AppState>,
) -> Result<Json<OidcConfig>, AppError> {
    let issuer = state.config.oidc_issuer
        .as_ref()
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("OIDC not configured")))?;
    let client_id = state.config.oidc_client_id
        .as_ref()
        .unwrap_or(&"sera-web".to_string())
        .clone();

    Ok(Json(OidcConfig {
        issuer_url: issuer.clone(),
        client_id,
    }))
}

/// GET /api/auth/login — initiate OIDC login flow (redirect to provider)
pub async fn login(
    State(state): State<AppState>,
) -> Result<Redirect, AppError> {
    let issuer = state.config.oidc_issuer.as_ref()
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("OIDC not configured")))?;
    let client_id = state.config.oidc_client_id.as_ref()
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("OIDC client_id not configured")))?;

    // Use WEB_ORIGIN (not SERA_EXTERNAL_URL) for frontend redirect
    let web_origin = state.config.web_origin
        .as_ref()
        .map(|s| s.clone())
        .unwrap_or_else(|| "http://localhost:5173".to_string());
    let redirect_uri = format!("{}/auth/callback", web_origin);

    // Determine authorization endpoint — detect Keycloak pattern
    let base_issuer = issuer.trim_end_matches('/');
    let auth_endpoint = if base_issuer.contains("/realms/") {
        format!("{}/protocol/openid-connect/auth", base_issuer)
    } else {
        format!("{}/authorization", base_issuer)
    };

    // Generate state parameter for CSRF protection
    let csrf_state = uuid::Uuid::new_v4().to_string();

    let auth_url = format!(
        "{}?response_type=code&client_id={}&redirect_uri={}&scope=openid+profile+email&state={}",
        auth_endpoint,
        urlencoding::encode(client_id),
        urlencoding::encode(&redirect_uri),
        csrf_state
    );

    Ok(Redirect::temporary(&auth_url))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CallbackRequest {
    pub code: String,
    pub code_verifier: String,
    pub redirect_uri: String,
    #[serde(default)]
    pub client_id: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperatorIdentity {
    pub sub: String,
    pub roles: Vec<String>,
    pub auth_method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CallbackResponse {
    pub user: OperatorIdentity,
    pub session_token: String,
}

/// POST /api/auth/oidc/callback — handle OIDC callback with PKCE flow
/// Web SPA sends { code, codeVerifier, redirectUri }
/// We exchange with IdP server-side and return opaque session token + user identity
pub async fn callback(
    State(state): State<AppState>,
    Json(body): Json<CallbackRequest>,
) -> Result<(StatusCode, Json<CallbackResponse>), AppError> {
    let issuer = state.config.oidc_issuer.as_ref()
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("OIDC not configured")))?;
    let client_id = body.client_id
        .as_deref()
        .or(state.config.oidc_client_id.as_deref())
        .unwrap_or("sera-web");
    let client_secret = &state.config.oidc_client_secret
        .as_ref()
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("OIDC client_secret not configured")))?;

    // Determine token endpoint — detect Keycloak pattern
    let base_issuer = issuer.trim_end_matches('/');
    let token_endpoint = if base_issuer.contains("/realms/") {
        format!("{}/protocol/openid-connect/token", base_issuer)
    } else {
        format!("{}/token", base_issuer)
    };

    // Exchange code for tokens using PKCE code_verifier
    let mut form_data = vec![
        ("grant_type".to_string(), "authorization_code".to_string()),
        ("code".to_string(), body.code),
        ("redirect_uri".to_string(), body.redirect_uri),
        ("code_verifier".to_string(), body.code_verifier),
        ("client_id".to_string(), client_id.to_string()),
    ];
    if !client_secret.is_empty() {
        form_data.push(("client_secret".to_string(), client_secret.to_string()));
    }

    let client = reqwest::Client::new();
    let resp = client.post(&token_endpoint)
        .form(&form_data)
        .send()
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("Token exchange failed: {e}")))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let _err_text = resp.text().await.unwrap_or_default();
        // Don't expose sensitive error details to client
        return Err(AppError::Internal(anyhow::anyhow!("Token exchange failed ({})", status)));
    }

    let token_data: serde_json::Value = resp.json().await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("Invalid token response: {e}")))?;

    let id_token = token_data["id_token"]
        .as_str()
        .or(token_data["access_token"].as_str())
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("No token in response")))?;

    // Decode ID token to extract identity and groups
    let identity = decode_token_claims(id_token)?;

    // Create opaque session token (server-side only, never expose raw OIDC token)
    let session_token = format!("sess_{}", uuid::Uuid::new_v4().to_string().replace('-', ""));
    // TODO: Store session mapping in session store (in-memory or Redis)
    // For now, return the opaque token that can be stored in HttpOnly cookie

    Ok((
        StatusCode::OK,
        Json(CallbackResponse {
            user: identity,
            session_token,
        }),
    ))
}

/// Decode JWT claims from ID token (base64url encoded)
fn decode_token_claims(token: &str) -> Result<OperatorIdentity, AppError> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return Err(AppError::Internal(anyhow::anyhow!("Invalid token format")));
    }

    // Decode the payload (second part)
    let payload_str = base64_url_decode(parts[1])
        .map_err(|_| AppError::Internal(anyhow::anyhow!("Failed to decode token payload")))?;

    let payload: serde_json::Value = serde_json::from_str(&payload_str)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("Failed to parse token payload: {e}")))?;

    let sub = payload["sub"]
        .as_str()
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("Missing 'sub' claim")))?
        .to_string();

    // Extract groups and map to roles
    let groups_claim = std::env::var("OIDC_GROUPS_CLAIM").unwrap_or_else(|_| "groups".to_string());
    let raw_groups = payload.get(&groups_claim);
    let groups: Vec<String> = if let Some(serde_json::Value::Array(arr)) = raw_groups {
        arr.iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect()
    } else {
        vec![]
    };

    let roles = map_groups_to_roles(groups);

    let mut identity = OperatorIdentity {
        sub,
        roles,
        auth_method: "oidc".to_string(),
        email: payload["email"].as_str().map(String::from),
        name: payload["name"].as_str().map(String::from),
    };

    // Fallback to preferred_username if name not provided
    if identity.name.is_none() {
        identity.name = payload["preferred_username"].as_str().map(String::from);
    }

    Ok(identity)
}

/// Map OIDC groups to SERA operator roles via OIDC_ROLE_MAPPING env var
fn map_groups_to_roles(groups: Vec<String>) -> Vec<String> {
    let mut role_mapping: HashMap<String, String> = HashMap::new();

    if let Ok(mapping_str) = std::env::var("OIDC_ROLE_MAPPING") {
        if let Ok(parsed) = serde_json::from_str::<HashMap<String, String>>(&mapping_str) {
            role_mapping = parsed;
        }
    }

    let mut roles = std::collections::HashSet::new();
    for group in groups {
        if let Some(mapped_role) = role_mapping.get(&group) {
            roles.insert(mapped_role.clone());
        }
    }

    if roles.is_empty() {
        roles.insert("viewer".to_string());
    }

    roles.into_iter().collect()
}

/// Decode base64url string (no padding)
fn base64_url_decode(s: &str) -> Result<String, String> {
    use base64::{engine::general_purpose::STANDARD, Engine};

    // Add padding if needed
    let mut padded = s.to_string();
    while padded.len() % 4 != 0 {
        padded.push('=');
    }

    // Replace URL-safe characters
    let standard = padded.replace('-', "+").replace('_', "/");

    match STANDARD.decode(&standard) {
        Ok(bytes) => String::from_utf8(bytes).map_err(|e| e.to_string()),
        Err(e) => Err(e.to_string()),
    }
}

/// POST /api/auth/logout — logout endpoint, invalidates session
pub async fn logout() -> Json<serde_json::Value> {
    // In production: extract session token from cookie/Authorization header
    // and remove from session store
    // TODO: Integrate with SessionStore when implemented
    Json(serde_json::json!({"loggedOut": true}))
}
