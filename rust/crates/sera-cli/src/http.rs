//! Thin `reqwest` client factory.
//!
//! The auth bead (`sera-j1hs`) will extend this by adding a bearer token
//! header via `ClientBuilder::default_headers`.  All CLI commands obtain their
//! HTTP client through this module so the auth extension is automatically
//! applied everywhere.

use anyhow::{Context, Result};
use reqwest::Client;

/// Build a `reqwest::Client` suitable for CLI use.
///
/// Extension seam for `sera-j1hs`: wrap this function (or add an overload
/// that accepts a token) to inject `Authorization: Bearer <token>` as a
/// default header on every request.
pub fn build_client() -> Result<Client> {
    Client::builder()
        .build()
        .context("failed to build HTTP client")
}
