//! Admin HTTP control-plane (sera-nrn9, L.3).
//!
//! Separate HTTP server that runs alongside the public API. Binds to
//! `127.0.0.1:3002` by default and requires a separate `SERA_ADMIN_TOKEN`.
//! Every request is appended to a JSONL audit log with a token fingerprint
//! (never the raw token) so operators can trace who did what.
//!
//! Endpoint inventory:
//! * `/admin/workflows[/{id}]` — workflow CRUD (create/delete return 501 in L.3)
//! * `/admin/policies[/{name}]`, `/reload`, `/validate` — capability policies
//! * `/admin/sessions[/{id}]` — list/show/kill (cancellation token fired)
//! * `/admin/agents/{id}`, `/agents/spawn` — show; spawn/kill return 501
//! * `/admin/killswitch/{arm,disarm,status}` — HTTP twin of the unix admin sock

pub mod audit;
pub mod auth;
pub mod routes;
pub mod server;
pub mod state;

pub use audit::{AdminAuditEntry, AdminAuditLogger, TokenFingerprint};
pub use auth::{AdminAuth, AdminAuthError, AdminCaller, require_admin_token};
pub use server::{
    DEFAULT_ADMIN_BIND, DEFAULT_ADMIN_PORT, build_admin_router, resolve_admin_bind,
    resolve_admin_port, serve_admin,
};
pub use state::{AdminAppState, AdminSessionInfo, CancellationRegistry};
