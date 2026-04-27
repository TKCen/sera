//! Admin HTTP server bring-up (sera-nrn9).
//!
//! Builds a router that:
//! 1. Logs every request to the JSONL audit file (success or failure).
//! 2. Requires a valid admin token via `Authorization: Bearer …`.
//! 3. Dispatches to one of the typed handler modules under `routes/`.
//!
//! The server binds separately from the public API (default `127.0.0.1:3002`)
//! so localhost-only deployments and bind-mounted reverse proxies can keep
//! the surfaces apart.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;

use axum::Router;
use axum::extract::{Request, State};
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::{get, post};

use crate::admin::audit::{AdminAuditEntry, AdminAuditLogger, TokenFingerprint, now_iso};
use crate::admin::auth::{AdminCaller, require_admin_token};
use crate::admin::routes::{agents, killswitch, policies, sessions, workflows};
use crate::admin::state::AdminAppState;

/// Default admin HTTP port. Picked above the public API (3001) and below the
/// `--local` user-facing port (42540).
pub const DEFAULT_ADMIN_PORT: u16 = 3002;

/// Default bind address — localhost only.
pub const DEFAULT_ADMIN_BIND: &str = "127.0.0.1";

/// Build the admin router, layered with auth + audit middleware.
pub fn build_admin_router<S>(state: Arc<S>) -> Router
where
    S: AdminAppState,
{
    let auth = state.auth();
    let audit = state.audit();

    Router::new()
        // ── workflows ──────────────────────────────────────────────────────
        .route(
            "/admin/workflows",
            get(workflows::list::<S>).post(workflows::create::<S>),
        )
        .route(
            "/admin/workflows/{id}",
            get(workflows::show::<S>).delete(workflows::delete::<S>),
        )
        // ── policies ───────────────────────────────────────────────────────
        .route("/admin/policies", get(policies::list::<S>))
        .route("/admin/policies/reload", post(policies::reload::<S>))
        .route("/admin/policies/validate", post(policies::validate::<S>))
        .route("/admin/policies/{name}", get(policies::show::<S>))
        // ── sessions ───────────────────────────────────────────────────────
        .route("/admin/sessions", get(sessions::list::<S>))
        .route(
            "/admin/sessions/{id}",
            get(sessions::show::<S>).delete(sessions::kill::<S>),
        )
        // ── agents ─────────────────────────────────────────────────────────
        .route("/admin/agents/spawn", post(agents::spawn::<S>))
        .route(
            "/admin/agents/{id}",
            get(agents::show::<S>).delete(agents::kill::<S>),
        )
        // ── killswitch ─────────────────────────────────────────────────────
        .route("/admin/killswitch/arm", post(killswitch::arm::<S>))
        .route("/admin/killswitch/disarm", post(killswitch::disarm::<S>))
        .route("/admin/killswitch/status", get(killswitch::status::<S>))
        // Layers wrap inside-out: the LAST `.layer(...)` runs FIRST on a
        // request. We want audit to observe every request — including 401s —
        // so audit must be the outermost layer, applied last.
        .layer(middleware::from_fn_with_state(
            Arc::clone(&auth),
            require_admin_token,
        ))
        .layer(middleware::from_fn_with_state(audit, audit_layer))
        .with_state(state)
}

/// Bind the admin listener and serve until `shutdown` resolves.
///
/// Returns the bound [`SocketAddr`] via the `bound` channel so callers can
/// observe the actual port (useful for `:0` in tests).
pub async fn serve_admin<S>(
    state: Arc<S>,
    addr: SocketAddr,
    bound: tokio::sync::oneshot::Sender<SocketAddr>,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> std::io::Result<()>
where
    S: AdminAppState,
{
    let app = build_admin_router(state);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let local = listener.local_addr()?;
    tracing::info!(%local, "admin HTTP server listening");
    let _ = bound.send(local);
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown)
        .await
}

/// Middleware wrapper: capture method/path/status/duration and write one row
/// to the audit log. Runs *after* auth so the [`AdminCaller`] extension is
/// already populated; if the request was rejected by auth (401) it falls back
/// to a fingerprint of the empty string. We intentionally do not log the
/// request body — admin requests can carry secrets (e.g. a workflow manifest
/// with embedded API keys), and surface area for accidental leaks must be
/// small.
async fn audit_layer(
    State(audit): State<Arc<AdminAuditLogger>>,
    request: Request,
    next: Next,
) -> Response {
    let method = request.method().clone();
    let path = request.uri().path().to_string();
    let started = Instant::now();
    let response = next.run(request).await;
    let status = response.status().as_u16();
    let caller = response
        .extensions()
        .get::<AdminCaller>()
        .map(|c| c.0.clone())
        .unwrap_or_else(|| TokenFingerprint::of(""));

    audit
        .append(AdminAuditEntry {
            ts: now_iso(),
            caller: caller.as_str(),
            method: method.as_str(),
            path: &path,
            status,
            args: serde_json::Value::Null,
            ms: started.elapsed().as_millis() as u64,
        })
        .await;
    response
}

/// Resolve the admin port from `SERA_ADMIN_PORT` (default [`DEFAULT_ADMIN_PORT`]).
pub fn resolve_admin_port() -> u16 {
    std::env::var("SERA_ADMIN_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_ADMIN_PORT)
}

/// Resolve the admin bind address from `SERA_ADMIN_BIND` (default
/// [`DEFAULT_ADMIN_BIND`]).
pub fn resolve_admin_bind() -> String {
    std::env::var("SERA_ADMIN_BIND")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_ADMIN_BIND.to_string())
}

