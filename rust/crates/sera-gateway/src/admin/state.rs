//! Admin app-state abstraction (sera-nrn9).
//!
//! Routes are generic over [`AdminAppState`] so the binary's `AppState` can
//! plug in production stores while tests use lightweight fakes — same pattern
//! `routes::workflow` already follows for the public API.

use std::path::PathBuf;
use std::sync::Arc;

use crate::admin::audit::AdminAuditLogger;
use crate::admin::auth::AdminAuth;
use crate::capability_enforcement::CapabilityRegistry;
use crate::kill_switch::KillSwitch;
use crate::workflow_store::WorkflowTaskStore;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;

/// Snapshot of a session for `/admin/sessions` listings. Kept narrow so impls
/// don't have to expose their full transcript surface.
#[derive(Debug, Clone, serde::Serialize)]
pub struct AdminSessionInfo {
    pub id: String,
    pub agent_id: String,
    pub session_key: String,
    pub state: String,
}

/// Capabilities the admin handlers require from the surrounding app state.
///
/// Capability negotiation is intentionally narrow: each method returns either
/// the live thing we can act on, or `None` to indicate the handler should
/// answer 501. No silent fallbacks — the handler decides what to do with the
/// `None`.
#[async_trait::async_trait]
pub trait AdminAppState: Send + Sync + 'static {
    fn auth(&self) -> Arc<AdminAuth>;
    fn audit(&self) -> Arc<AdminAuditLogger>;
    fn kill_switch(&self) -> Arc<KillSwitch>;
    fn capability_registry(&self) -> Arc<RwLock<Arc<CapabilityRegistry>>>;
    fn policies_dir(&self) -> PathBuf;

    /// Workflow store, when wired. None → routes return 501.
    fn workflow_store(&self) -> Option<Arc<dyn WorkflowTaskStore>> {
        None
    }

    /// List active sessions.
    async fn list_sessions(&self) -> Vec<AdminSessionInfo> {
        Vec::new()
    }

    /// Look up one session by id.
    async fn get_session(&self, _id: &str) -> Option<AdminSessionInfo> {
        None
    }

    /// Cancel an in-flight session by session_key, returning the cancelled
    /// token (so the caller can also drop the lane slot). Returns `false`
    /// when no token was registered for that session.
    fn cancel_session(&self, _session_key: &str) -> bool {
        false
    }

    /// Look up agent metadata for `id`. None → 404.
    fn agent_metadata(&self, _id: &str) -> Option<serde_json::Value> {
        None
    }

    /// Spawn an agent. Default impl returns None → routes return 501.
    async fn spawn_agent(&self, _spec: serde_json::Value) -> Option<serde_json::Value> {
        None
    }

    /// Kill an agent by id. Default impl returns false → routes return 501.
    async fn kill_agent(&self, _id: &str) -> bool {
        false
    }
}

/// In-flight cancellation token registry helper. The binary's `AppState`
/// already owns one of these on the `active_cancellation_tokens` field; the
/// trait method [`AdminAppState::cancel_session`] is the canonical hook
/// admin routes use, so we expose this helper here for tests that want to
/// stand up an `AdminAppState` impl quickly.
pub type CancellationRegistry =
    Arc<std::sync::Mutex<std::collections::HashMap<String, CancellationToken>>>;
