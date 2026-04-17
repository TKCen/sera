//! Thin `reqwest` client factory.
//!
//! Two constructors:
//! - [`build_client`] — unauthenticated (used by `ping` and the login probe)
//! - [`build_client_with_token`] — sets `Authorization: Bearer <token>` as a
//!   default header on every request (used by authenticated commands)

use anyhow::{Context, Result};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
use reqwest::Client;

/// Build an unauthenticated `reqwest::Client` suitable for CLI use.
pub fn build_client() -> Result<Client> {
    Client::builder()
        .build()
        .context("failed to build HTTP client")
}

/// Build a `reqwest::Client` that injects `Authorization: Bearer <token>` on
/// every request.
pub fn build_client_with_token(token: &str) -> Result<Client> {
    let mut headers = HeaderMap::new();
    let mut value =
        HeaderValue::from_str(&format!("Bearer {token}")).context("invalid token value")?;
    value.set_sensitive(true);
    headers.insert(AUTHORIZATION, value);

    Client::builder()
        .default_headers(headers)
        .build()
        .context("failed to build authenticated HTTP client")
}
