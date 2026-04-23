//! OIDC authentication flow endpoints.
#![allow(dead_code, unused_imports)]

use axum::{
    Json,
    extract::{Query, State},
    http::{StatusCode, header},
    response::Redirect,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;

use crate::error::AppError;
use crate::state::AppState;

/// Process-local session store keyed by opaque session token.
///
/// Intentionally in-memory for the MVS cut — the gateway runs as a single
/// process, and session tokens do not need to survive restarts. Swapping this
/// out for a distributed backend (Redis, or `sera-session` when it grows a
/// persistent store) only requires replacing the three access points below
/// (`insert_session`, `lookup_session`, `remove_session`).
static SESSION_STORE: std::sync::LazyLock<std::sync::RwLock<HashMap<String, OperatorIdentity>>> =
    std::sync::LazyLock::new(|| std::sync::RwLock::new(HashMap::new()));

/// Insert a session mapping. Silently drops if the RwLock is poisoned — a
/// poisoned lock here means a concurrent writer panicked, which is already a
/// logged bug; dropping the insert is preferable to panicking the callback.
fn insert_session(token: String, identity: OperatorIdentity) {
    if let Ok(mut store) = SESSION_STORE.write() {
        store.insert(token, identity);
    }
}

/// Look up the operator identity behind an opaque session token. Returns
/// `None` if the token is unknown or the store lock is poisoned.
pub fn lookup_session(token: &str) -> Option<OperatorIdentity> {
    SESSION_STORE
        .read()
        .ok()
        .and_then(|s| s.get(token).cloned())
}

/// Remove a session mapping (logout). No-op on unknown tokens or poisoned
/// locks.
fn remove_session(token: &str) {
    if let Ok(mut store) = SESSION_STORE.write() {
        store.remove(token);
    }
}

/// Minimal OIDC config response — only issuerUrl and clientId (matches TS)
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OidcConfig {
    pub issuer_url: String,
    pub client_id: String,
}

/// GET /api/auth/oidc-config — return minimal OIDC provider configuration
pub async fn get_oidc_config(State(state): State<AppState>) -> Result<Json<OidcConfig>, AppError> {
    let issuer = state
        .config
        .oidc_issuer
        .as_ref()
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("OIDC not configured")))?;
    let client_id = state
        .config
        .oidc_client_id
        .as_ref()
        .unwrap_or(&"sera-web".to_string())
        .clone();

    Ok(Json(OidcConfig {
        issuer_url: issuer.clone(),
        client_id,
    }))
}

/// GET /api/auth/login — initiate OIDC login flow (redirect to provider)
pub async fn login(State(state): State<AppState>) -> Result<Redirect, AppError> {
    let issuer = state
        .config
        .oidc_issuer
        .as_ref()
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("OIDC not configured")))?;
    let client_id = state
        .config
        .oidc_client_id
        .as_ref()
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("OIDC client_id not configured")))?;

    // Use WEB_ORIGIN (not SERA_EXTERNAL_URL) for frontend redirect
    let web_origin = state
        .config
        .web_origin
        .clone()
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

#[derive(Clone, Serialize)]
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
    let issuer = state
        .config
        .oidc_issuer
        .as_ref()
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("OIDC not configured")))?;
    let client_id = body
        .client_id
        .as_deref()
        .or(state.config.oidc_client_id.as_deref())
        .unwrap_or("sera-web");
    let client_secret =
        &state.config.oidc_client_secret.as_ref().ok_or_else(|| {
            AppError::Internal(anyhow::anyhow!("OIDC client_secret not configured"))
        })?;

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
    let resp = client
        .post(&token_endpoint)
        .form(&form_data)
        .send()
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("Token exchange failed: {e}")))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let _err_text = resp.text().await.unwrap_or_default();
        // Don't expose sensitive error details to client
        return Err(AppError::Internal(anyhow::anyhow!(
            "Token exchange failed ({})",
            status
        )));
    }

    let token_data: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("Invalid token response: {e}")))?;

    let id_token = token_data["id_token"]
        .as_str()
        .or(token_data["access_token"].as_str())
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("No token in response")))?;

    // Decode ID token to extract identity and groups
    let identity = decode_token_claims(id_token)?;

    // Create opaque session token (server-side only, never expose raw OIDC token)
    let session_token = format!("sess_{}", uuid::Uuid::new_v4().to_string().replace('-', ""));
    insert_session(session_token.clone(), identity.clone());

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

    if let Ok(mapping_str) = std::env::var("OIDC_ROLE_MAPPING")
        && let Ok(parsed) = serde_json::from_str::<HashMap<String, String>>(&mapping_str)
    {
        role_mapping = parsed;
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
    use base64::{Engine, engine::general_purpose::STANDARD};

    // Add padding if needed
    let mut padded = s.to_string();
    while !padded.len().is_multiple_of(4) {
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
pub async fn logout(headers: axum::http::HeaderMap) -> Json<serde_json::Value> {
    // Extract session token from Authorization header and remove from session store
    if let Some(auth) = headers.get("authorization").and_then(|v| v.to_str().ok()) {
        let token = auth.strip_prefix("Bearer ").unwrap_or(auth);
        remove_session(token);
    }
    Json(serde_json::json!({"loggedOut": true}))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_identity(sub: &str) -> OperatorIdentity {
        OperatorIdentity {
            sub: sub.to_string(),
            roles: vec!["viewer".to_string()],
            auth_method: "oidc".to_string(),
            email: Some(format!("{sub}@example.com")),
            name: Some(format!("User {sub}")),
        }
    }

    #[test]
    fn session_mapping_roundtrip_insert_lookup_remove() {
        // Use uuid-keyed tokens so this test never collides with other tests
        // sharing the process-global SESSION_STORE.
        let token = format!("sess_test_{}", uuid::Uuid::new_v4().simple());
        let identity = fake_identity("user-42");

        // Insert — should be visible to lookup
        insert_session(token.clone(), identity.clone());
        let got = lookup_session(&token).expect("session must be present after insert");
        assert_eq!(got.sub, identity.sub);
        assert_eq!(got.email, identity.email);
        assert_eq!(got.name, identity.name);

        // Remove — subsequent lookup must return None
        remove_session(&token);
        assert!(
            lookup_session(&token).is_none(),
            "session must be gone after remove"
        );
    }

    #[test]
    fn lookup_unknown_session_returns_none() {
        let token = format!("sess_nonexistent_{}", uuid::Uuid::new_v4().simple());
        assert!(lookup_session(&token).is_none());
    }

    #[test]
    fn remove_unknown_session_is_noop() {
        // Must not panic or corrupt the store.
        let token = format!("sess_nonexistent_{}", uuid::Uuid::new_v4().simple());
        remove_session(&token);
        assert!(lookup_session(&token).is_none());
    }

    #[test]
    fn multiple_sessions_are_independent() {
        let token_a = format!("sess_a_{}", uuid::Uuid::new_v4().simple());
        let token_b = format!("sess_b_{}", uuid::Uuid::new_v4().simple());
        insert_session(token_a.clone(), fake_identity("alice"));
        insert_session(token_b.clone(), fake_identity("bob"));

        assert_eq!(lookup_session(&token_a).unwrap().sub, "alice");
        assert_eq!(lookup_session(&token_b).unwrap().sub, "bob");

        remove_session(&token_a);
        assert!(lookup_session(&token_a).is_none());
        // token_b must remain unaffected
        assert_eq!(lookup_session(&token_b).unwrap().sub, "bob");

        remove_session(&token_b);
    }
}
