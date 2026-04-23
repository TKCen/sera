//! API route modules.

/// Format a `time::OffsetDateTime` as an ISO 8601 / RFC 3339 string
/// that JavaScript's `new Date()` can parse.
pub fn iso8601(dt: time::OffsetDateTime) -> String {
    dt.format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| dt.to_string())
}

/// Optional variant for `Option<time::OffsetDateTime>`.
pub fn iso8601_opt(dt: Option<time::OffsetDateTime>) -> Option<String> {
    dt.map(iso8601)
}

pub mod a2a;
pub mod agents;
pub mod agui;
pub mod audit;
pub mod auth;
pub mod channels;
pub mod chat;
pub mod circles;
pub mod config;
pub mod delegation;
pub mod embedding;
pub mod evolve;
pub mod health;
pub mod heartbeat;
pub mod intercom;
pub mod knowledge;
pub mod llm_proxy;
pub mod lsp;
pub mod mcp;
pub mod memory;
pub mod metering;
pub mod oidc;
pub mod openai_compat;
pub mod operator_requests;
pub mod permission_requests;
pub mod pipelines;
pub mod plugins;
pub mod providers;
pub mod registry;
pub mod sandbox;
pub mod schedules;
pub mod secrets;
pub mod service_identities;
pub mod sessions;
pub mod skills;
pub mod stubs;
pub mod tasks;
pub mod webhooks;
