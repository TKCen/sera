//! OIDC authentication flow endpoints.
#![allow(dead_code, unused_imports)]

use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
    response::Redirect,
};
use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::state::AppState;

#[derive(Serialize)]
pub struct OidcConfig {
    pub enabled: bool,
    pub issuer: Option<String>,
    pub client_id: Option<String>,
    pub authorization_endpoint: Option<String>,
    pub token_endpoint: Option<String>,
    pub scopes: Vec<String>,
}

/// GET /api/auth/oidc-config — return OIDC provider configuration
pub async fn get_oidc_config(
    State(state): State<AppState>,
) -> Json<OidcConfig> {
    let config = &state.config;
    let enabled = config.oidc_issuer.is_some();

    Json(OidcConfig {
        enabled,
        issuer: config.oidc_issuer.clone(),
        client_id: config.oidc_client_id.clone(),
        authorization_endpoint: config.oidc_issuer.as_ref().map(|i| format!("{i}/authorize")),
        token_endpoint: config.oidc_issuer.as_ref().map(|i| format!("{i}/token")),
        scopes: vec!["openid".to_string(), "profile".to_string(), "email".to_string()],
    })
}

#[derive(Deserialize)]
pub struct LoginQuery {
    pub redirect_uri: Option<String>,
}

/// GET /api/auth/login — initiate OIDC login flow (redirect to provider)
pub async fn login(
    State(state): State<AppState>,
    Query(query): Query<LoginQuery>,
) -> Result<Redirect, AppError> {
    let issuer = state.config.oidc_issuer.as_ref()
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("OIDC not configured")))?;
    let client_id = state.config.oidc_client_id.as_ref()
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("OIDC client_id not configured")))?;

    let redirect_uri = query.redirect_uri
        .unwrap_or_else(|| format!("{}/api/auth/oidc/callback", state.config.external_url.clone().unwrap_or_else(|| "http://localhost:3001".to_string())));

    // Generate state parameter for CSRF protection
    let csrf_state = uuid::Uuid::new_v4().to_string();

    let auth_url = format!(
        "{issuer}/authorize?response_type=code&client_id={client_id}&redirect_uri={redirect_uri}&scope=openid+profile+email&state={csrf_state}"
    );

    Ok(Redirect::temporary(&auth_url))
}

#[derive(Deserialize)]
pub struct CallbackQuery {
    pub code: String,
    pub state: Option<String>,
}

#[derive(Serialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub token_type: String,
    pub expires_in: u64,
    pub id_token: Option<String>,
}

/// POST /api/auth/oidc/callback — handle OIDC callback, exchange code for tokens
pub async fn callback(
    State(state): State<AppState>,
    Query(query): Query<CallbackQuery>,
) -> Result<Json<TokenResponse>, AppError> {
    let issuer = state.config.oidc_issuer.as_ref()
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("OIDC not configured")))?;
    let client_id = state.config.oidc_client_id.as_ref()
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("OIDC client_id not configured")))?;
    let client_secret = state.config.oidc_client_secret.as_ref()
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("OIDC client_secret not configured")))?;

    let redirect_uri = format!("{}/api/auth/oidc/callback",
        state.config.external_url.clone().unwrap_or_else(|| "http://localhost:3001".to_string()));

    let token_url = format!("{issuer}/token");
    let client = reqwest::Client::new();

    let resp = client.post(&token_url)
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", &query.code),
            ("redirect_uri", &redirect_uri),
            ("client_id", client_id),
            ("client_secret", client_secret),
        ])
        .send()
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("Token exchange failed: {e}")))?;

    if !resp.status().is_success() {
        let error_body = resp.text().await.unwrap_or_default();
        return Err(AppError::Internal(anyhow::anyhow!("Token exchange HTTP error: {error_body}")));
    }

    let token_data: serde_json::Value = resp.json().await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("Invalid token response: {e}")))?;

    Ok(Json(TokenResponse {
        access_token: token_data["access_token"].as_str().unwrap_or_default().to_string(),
        token_type: "Bearer".to_string(),
        expires_in: token_data["expires_in"].as_u64().unwrap_or(3600),
        id_token: token_data["id_token"].as_str().map(String::from),
    }))
}

/// POST /api/auth/logout — logout endpoint
pub async fn logout() -> Json<serde_json::Value> {
    // In production this would invalidate the session/token
    Json(serde_json::json!({"status": "logged_out"}))
}
