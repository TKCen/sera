//! Admin token auth for the admin HTTP server (sera-nrn9).
//!
//! Reads `Authorization: Bearer <token>` and compares it against the configured
//! admin token in constant time. The bootstrap API key MUST NOT pass admin
//! auth — the admin token is a separate secret. Missing/invalid → 401.

use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use rand::RngCore;
use subtle::ConstantTimeEq;

use crate::admin::audit::TokenFingerprint;

/// Per-request token fingerprint stashed in request extensions so audit log
/// writers can reach it without re-reading the header.
#[derive(Debug, Clone)]
pub struct AdminCaller(pub TokenFingerprint);

/// Per-process admin token configuration.
#[derive(Debug, Clone)]
pub struct AdminAuth {
    expected: Arc<String>,
}

impl AdminAuth {
    pub fn new(token: impl Into<String>) -> Self {
        Self {
            expected: Arc::new(token.into()),
        }
    }

    /// Resolve the admin token from `SERA_ADMIN_TOKEN`. When unset and
    /// `local` is true, generate a random 32-byte hex token and print it once
    /// to stderr. When unset and `local` is false, returns an error so the
    /// caller can fail to start.
    pub fn resolve(local: bool) -> Result<Self, AdminAuthError> {
        if let Ok(t) = std::env::var("SERA_ADMIN_TOKEN")
            && !t.trim().is_empty()
        {
            return Ok(Self::new(t));
        }
        if !local {
            return Err(AdminAuthError::TokenMissing);
        }
        let mut buf = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut buf);
        let token = hex::encode(buf);
        eprintln!(
            "sera admin token (local mode, regenerated each boot):\n  SERA_ADMIN_TOKEN={token}"
        );
        Ok(Self::new(token))
    }

    /// Compare `provided` to the configured token in constant time. Equal
    /// length is enforced — the comparator early-returns on length mismatch
    /// because subtle's `ct_eq` requires equal-length slices.
    pub fn matches(&self, provided: &str) -> bool {
        let expected = self.expected.as_bytes();
        let provided = provided.as_bytes();
        if expected.len() != provided.len() {
            return false;
        }
        bool::from(expected.ct_eq(provided))
    }

    /// Token, useful for tests that need to construct request headers.
    pub fn token(&self) -> &str {
        &self.expected
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AdminAuthError {
    #[error(
        "SERA_ADMIN_TOKEN is not set; admin server cannot start in non-local mode without it"
    )]
    TokenMissing,
}

/// Axum middleware: require a valid `Authorization: Bearer <admin-token>` on
/// every admin route. On success injects an [`AdminCaller`] into request
/// extensions so downstream audit logging can extract the fingerprint.
pub async fn require_admin_token(
    State(auth): State<Arc<AdminAuth>>,
    mut request: Request,
    next: Next,
) -> Response {
    let token: Option<String> = request
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
        .map(|s| s.to_string());

    let token = match token {
        Some(t) => t,
        None => return StatusCode::UNAUTHORIZED.into_response(),
    };

    if !auth.matches(&token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    request
        .extensions_mut()
        .insert(AdminCaller(TokenFingerprint::of(&token)));
    next.run(request).await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serialises all tests that read or write `SERA_ADMIN_TOKEN` so they
    /// don't race each other when `cargo test` runs them in parallel.
    fn admin_token_lock() -> std::sync::MutexGuard<'static, ()> {
        use std::sync::{Mutex, OnceLock};
        static ENV_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();
        ENV_MUTEX.get_or_init(|| Mutex::new(())).lock().unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn matches_is_true_for_equal_tokens() {
        let auth = AdminAuth::new("token-abc");
        assert!(auth.matches("token-abc"));
    }

    #[test]
    fn matches_is_false_for_different_tokens() {
        let auth = AdminAuth::new("token-abc");
        assert!(!auth.matches("token-xyz"));
    }

    #[test]
    fn matches_is_false_for_different_lengths() {
        let auth = AdminAuth::new("short");
        assert!(!auth.matches("a-much-longer-token"));
    }

    #[test]
    fn bootstrap_dev_key_does_not_match_admin_token() {
        let auth = AdminAuth::new("admin-only-token");
        assert!(!auth.matches("sera_bootstrap_dev_123"));
    }

    #[test]
    fn resolve_errors_when_unset_and_not_local() {
        let _lock = admin_token_lock();
        let saved = std::env::var("SERA_ADMIN_TOKEN").ok();
        // SAFETY: callers hold admin_token_lock so no concurrent reader can
        // observe a torn value.
        unsafe { std::env::remove_var("SERA_ADMIN_TOKEN") };
        let result = AdminAuth::resolve(false);
        if let Some(v) = saved {
            unsafe { std::env::set_var("SERA_ADMIN_TOKEN", v) };
        }
        assert!(matches!(result, Err(AdminAuthError::TokenMissing)));
    }

    #[test]
    fn resolve_uses_env_var_when_set() {
        let _lock = admin_token_lock();
        let saved = std::env::var("SERA_ADMIN_TOKEN").ok();
        // SAFETY: callers hold admin_token_lock so no concurrent reader can
        // observe a torn value.
        unsafe { std::env::set_var("SERA_ADMIN_TOKEN", "explicit-token") };
        let auth = AdminAuth::resolve(false).unwrap();
        match saved {
            Some(v) => unsafe { std::env::set_var("SERA_ADMIN_TOKEN", v) },
            None => unsafe { std::env::remove_var("SERA_ADMIN_TOKEN") },
        }
        assert!(auth.matches("explicit-token"));
    }
}
