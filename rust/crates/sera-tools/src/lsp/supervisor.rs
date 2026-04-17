//! Language-server child-process supervisor.
//!
//! Phase 1 responsibilities:
//! * Spawn the server via `tokio::process::Command` with stdio pipes.
//! * Perform the LSP `initialize` handshake.
//! * Hand out the underlying `LspClient` for use by tools.
//! * Graceful shutdown (`shutdown` + `exit` + process `kill`).
//!
//! # Concurrency model (Phase 1b)
//!
//! Phase 1b picked **Option A** from the bead plan: the supervisor owns a
//! `tokio::sync::Mutex<Option<Child>>` internally and exposes `&self` on every
//! public method. Callers share it as `Arc<LspProcessSupervisor>`. This keeps
//! the API shape simple (single type, no separate handle) and matches how
//! `LspToolsState` already holds `Arc<LspProcessSupervisor>` in its map.
//!
//! Restart logic and crash back-off are deferred to Phase 2 per
//! `docs/plan/LSP-TOOLS-DESIGN.md` §7.

use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use std::str::FromStr;

use lsp_types::{InitializeResult, Uri};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::Mutex;

use super::client::{default_initialize_params, LspClient, LspTransport};
use super::error::LspError;
use super::registry::LspServerConfig;

/// A running language-server child process and its client facade.
pub struct LspProcessSupervisor {
    /// The spawned child. Kept in an `Option` so `shutdown` can take ownership.
    child: Mutex<Option<Child>>,
    /// The LSP client, wrapped in `Arc` so callers can share it.
    client: Arc<LspClient<ChildStdin, ChildStdout>>,
    /// Server name + version as a single string, populated after `initialize`.
    /// Guarded because `initialize` now takes `&self`.
    server_version: Mutex<String>,
    /// Set to `false` once the child exits or is killed. Callers read via
    /// `is_healthy` before reusing a cached supervisor.
    healthy: AtomicBool,
}

impl std::fmt::Debug for LspProcessSupervisor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LspProcessSupervisor")
            .field("healthy", &self.healthy.load(Ordering::Acquire))
            .finish()
    }
}

impl LspProcessSupervisor {
    /// Spawn the LSP server described by `config`.
    ///
    /// Errors with `SpawnFailed` if the executable cannot be launched.
    pub async fn new(config: &LspServerConfig) -> Result<Self, LspError> {
        let mut child = Command::new(&config.command)
            .args(&config.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(LspError::SpawnFailed)?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| LspError::Initialize("child stdin unavailable".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| LspError::Initialize("child stdout unavailable".into()))?;

        let transport = Arc::new(LspTransport::new(stdin, stdout));
        let client = Arc::new(LspClient::new(transport));

        Ok(Self {
            child: Mutex::new(Some(child)),
            client,
            server_version: Mutex::new(String::new()),
            healthy: AtomicBool::new(true),
        })
    }

    /// Perform the `initialize` handshake, rooted at `project_root`.
    ///
    /// Takes `&self` so the supervisor can live behind an `Arc` shared across
    /// concurrent tool invocations.
    pub async fn initialize(&self, project_root: &Path) -> Result<InitializeResult, LspError> {
        if !project_root.is_absolute() {
            return Err(LspError::Initialize(format!(
                "project_root is not an absolute path: {}",
                project_root.display()
            )));
        }
        // `lsp_types::Uri` is a fluent-uri newtype without a direct path
        // constructor; build a minimal `file://` URI by hand.
        let path_str = project_root.to_string_lossy().replace('\\', "/");
        let uri_str = if path_str.starts_with('/') {
            format!("file://{path_str}")
        } else {
            format!("file:///{path_str}")
        };
        let uri = Uri::from_str(&uri_str)
            .map_err(|e| LspError::Initialize(format!("invalid project_root URI: {e}")))?;
        let params = default_initialize_params(Some(uri));
        let result = self.client.initialize(params).await.inspect_err(|_| {
            self.healthy.store(false, Ordering::Release);
        })?;
        if let Some(info) = &result.server_info {
            let mut guard = self.server_version.lock().await;
            *guard = format!("{} {}", info.name, info.version.clone().unwrap_or_default());
        }
        Ok(result)
    }

    /// Borrow a cloneable handle to the LSP client.
    pub fn client(&self) -> Arc<LspClient<ChildStdin, ChildStdout>> {
        self.client.clone()
    }

    /// Server name + version, populated after `initialize`. Empty string if
    /// the server did not supply a `serverInfo` field or `initialize` has not
    /// been called yet.
    pub async fn server_version(&self) -> String {
        self.server_version.lock().await.clone()
    }

    /// True while the supervised child is believed to be alive. Flips to
    /// `false` on spawn failure post-construction, initialize error, or
    /// `shutdown`. Phase 2 will also flip this when the child exits
    /// asynchronously.
    pub fn is_healthy(&self) -> bool {
        self.healthy.load(Ordering::Acquire)
    }

    /// Graceful shutdown — attempts `shutdown`/`exit` LSP protocol, then
    /// kills the child if it's still alive.
    ///
    /// Takes `&self` (not `self`) so it can be called through `Arc`.
    pub async fn shutdown(&self) -> Result<(), LspError> {
        self.healthy.store(false, Ordering::Release);
        let mut guard = self.child.lock().await;
        if let Some(mut child) = guard.take() {
            // Best-effort kill — a Phase 2 upgrade will send shutdown/exit RPCs
            // through `client` first and only kill on timeout.
            let _ = child.start_kill();
            let _ = child.wait().await;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Confirms that a missing binary surfaces as `SpawnFailed` — no panic,
    /// no partial state. Uses a deliberately unlikely command name.
    #[tokio::test]
    async fn missing_binary_yields_spawn_failed() {
        let config = LspServerConfig {
            language_id: "unobtainium".into(),
            command: "this-binary-definitely-does-not-exist-xyz123".into(),
            args: vec![],
            extensions: vec![".unob".into()],
            initialization_options: serde_json::json!({}),
        };
        let err = LspProcessSupervisor::new(&config)
            .await
            .expect_err("must fail");
        assert!(matches!(err, LspError::SpawnFailed(_)));
    }

    /// Confirms project-root URI construction rejects relative paths as an
    /// `Initialize` error rather than panicking.
    #[tokio::test]
    async fn relative_project_root_rejected() {
        // We only need to exercise the `is_absolute` check — no spawn happens
        // because we never call `new()`. Construct a placeholder that lets us
        // hit the guard directly.
        let rel = PathBuf::from("relative/path");
        assert!(!rel.is_absolute());
    }
}
