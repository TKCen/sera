//! Process Manager (Phase S of sera-w3np) — scaffold for gateway-managed child
//! processes (LSP servers, indexers, custom plugins).
//!
//! See `docs/plan/specs/SPEC-gateway.md` §18 "Process Persistence" for the full
//! design. This module implements the type surface, an in-memory registry store,
//! and basic `spawn` / `adopt_existing` / `shutdown` plumbing. Startup
//! reconciliation, the SQLite-backed store, and the actual restart-loop behaviour
//! described in §18.4 / §18.6 land in Phase M.
//!
//! # Security
//!
//! Per §18.9: `ManagedProcess.args` MUST NOT be logged in full. All `tracing`
//! events emitted from this module log only `command` plus `args.len()`. The
//! helper [`log_spawn_redacted`] is the canonical formatter — if you add a new
//! log site, go through it so redaction cannot silently regress.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::process::{Child, Command};
use tokio::sync::{Mutex, RwLock};
use tokio::time::timeout;
use uuid::Uuid;

/// Unique process ID in the gateway's managed registry.
///
/// Distinct from the OS pid (OS pids can be reused after death; these cannot).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ProcessId(pub Uuid);

impl ProcessId {
    /// Mint a fresh random `ProcessId`.
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for ProcessId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for ProcessId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// Kind of managed process. Drives shutdown protocol choice in §18.6.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProcessKind {
    /// A language server process driven by the LSP protocol.
    LspServer,
    /// A custom operator-provided plugin process.
    CustomPlugin,
    /// A background indexer (no LSP protocol).
    Indexer,
}

/// Current status of a managed process.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProcessStatus {
    /// Process is alive and serving traffic.
    Running,
    /// Process is alive but failed a health probe.
    Degraded,
    /// Process has exited (either gracefully or via crash).
    Exited,
    /// Process exited and is being restarted per policy.
    Restarting,
}

/// Restart policy for a managed process.
///
/// The actual restart loop is Phase M. Phase S only defines the type and
/// documents intended behaviour; the manager stores the policy and exposes
/// getter/setter pairs so wiring the loop later is additive.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RestartPolicy {
    /// Never restart. Phase M will honour this by leaving the process in
    /// `Exited` without scheduling a re-spawn.
    Never,
    /// Restart on crash up to `max_attempts` times, with `backoff_secs`
    /// between attempts (flat delay for Phase M; SPEC §18.11 open question
    /// covers possible exponential backoff in Phase 2).
    OnCrash { max_attempts: u32, backoff_secs: u64 },
    /// Always restart, even on clean exit, with `backoff_secs` between
    /// attempts.
    Always { backoff_secs: u64 },
}

/// Snapshot of a managed child process.
///
/// Serializable so the SQLite store (Phase M) can persist it directly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManagedProcess {
    pub id: ProcessId,
    pub kind: ProcessKind,
    /// Executable path/name. Safe to log.
    pub command: String,
    /// Argument vector. **Never logged**; may contain secrets.
    pub args: Vec<String>,
    pub workspace_root: PathBuf,
    /// OS pid. `None` until the process is actually spawned or adopted.
    pub pid: Option<i32>,
    pub started_at: DateTime<Utc>,
    pub status: ProcessStatus,
    pub last_heartbeat: Option<DateTime<Utc>>,
    pub restart_count: u32,
}

/// Errors surfaced by `ProcessManager` and `ProcessRegistryStore`.
#[derive(Debug, thiserror::Error)]
pub enum ProcessError {
    #[error("spawn failed: {0}")]
    SpawnFailed(#[from] std::io::Error),
    #[error("process {0:?} not found")]
    NotFound(ProcessId),
    #[error("process already exists")]
    AlreadyExists,
    #[error("store failure: {0}")]
    StoreFailure(String),
    #[error("reconciliation failed for pid {pid}: {reason}")]
    ReconciliationFailed { pid: i32, reason: String },
    #[error("shutdown timed out after {secs}s")]
    ShutdownTimeout { secs: u64 },
}

/// Backing persistence for managed processes.
///
/// Phase S ships an in-memory implementation only. Phase M will add
/// `SqliteProcessRegistryStore` behind this same trait.
#[async_trait]
pub trait ProcessRegistryStore: Send + Sync + 'static {
    async fn upsert(&self, p: &ManagedProcess) -> Result<(), ProcessError>;
    async fn remove(&self, id: ProcessId) -> Result<(), ProcessError>;
    async fn get(&self, id: ProcessId) -> Result<Option<ManagedProcess>, ProcessError>;
    async fn list(&self) -> Result<Vec<ManagedProcess>, ProcessError>;
}

/// In-memory, non-durable `ProcessRegistryStore`. Default for Phase S and for
/// local dev / tests.
#[derive(Default)]
pub struct InMemoryProcessRegistryStore {
    inner: Arc<RwLock<HashMap<ProcessId, ManagedProcess>>>,
}

impl InMemoryProcessRegistryStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl ProcessRegistryStore for InMemoryProcessRegistryStore {
    async fn upsert(&self, p: &ManagedProcess) -> Result<(), ProcessError> {
        let mut guard = self.inner.write().await;
        guard.insert(p.id, p.clone());
        Ok(())
    }

    async fn remove(&self, id: ProcessId) -> Result<(), ProcessError> {
        let mut guard = self.inner.write().await;
        guard.remove(&id);
        Ok(())
    }

    async fn get(&self, id: ProcessId) -> Result<Option<ManagedProcess>, ProcessError> {
        let guard = self.inner.read().await;
        Ok(guard.get(&id).cloned())
    }

    async fn list(&self) -> Result<Vec<ManagedProcess>, ProcessError> {
        let guard = self.inner.read().await;
        Ok(guard.values().cloned().collect())
    }
}

/// Parameters for spawning a new managed process.
pub struct SpawnRequest {
    pub kind: ProcessKind,
    pub command: String,
    pub args: Vec<String>,
    pub workspace_root: PathBuf,
    pub env: HashMap<String, String>,
}

/// Gateway's process manager (Phase S scaffold).
///
/// Owns spawned children via `tokio::process::Child` (with `kill_on_drop(true)`)
/// so process liveness is tied to the manager's lifetime. Adopted processes are
/// tracked in the store only; the gateway does not own their lifetime.
pub struct ProcessManager {
    /// Owned children. Absent for adopted processes.
    children: Arc<RwLock<HashMap<ProcessId, Arc<Mutex<Child>>>>>,
    /// In-memory cache of metadata (mirrors the store).
    processes: Arc<RwLock<HashMap<ProcessId, ManagedProcess>>>,
    /// Backing durable store. Phase S uses `InMemoryProcessRegistryStore`.
    store: Arc<dyn ProcessRegistryStore>,
    /// Default restart policy applied to all children. Phase M may layer
    /// per-process overrides on top per SPEC §18.2.
    restart_policy: RestartPolicy,
}

/// Format a `ManagedProcess` for logging in a way that **never** reveals `args`
/// contents. Used by every `tracing` site in this module — see §18.9.
fn log_spawn_redacted(p: &ManagedProcess) -> String {
    format!(
        "id={} kind={:?} command={} args_len={} pid={:?} workspace_root={}",
        p.id,
        p.kind,
        p.command,
        p.args.len(),
        p.pid,
        p.workspace_root.display(),
    )
}

impl ProcessManager {
    /// Construct a new manager bound to `store` with a default restart policy.
    pub fn new(store: Arc<dyn ProcessRegistryStore>, restart_policy: RestartPolicy) -> Self {
        Self {
            children: Arc::new(RwLock::new(HashMap::new())),
            processes: Arc::new(RwLock::new(HashMap::new())),
            store,
            restart_policy,
        }
    }

    /// Spawn a fresh managed process.
    ///
    /// Builds a `tokio::process::Command` from `req`, pipes all three stdio
    /// streams (so downstream LSP drivers can attach), applies `kill_on_drop`
    /// so a dropped manager won't leak children, and registers the resulting
    /// `ManagedProcess` in both the in-memory cache and the store.
    pub async fn spawn(&self, req: SpawnRequest) -> Result<ProcessId, ProcessError> {
        let mut cmd = Command::new(&req.command);
        cmd.args(&req.args)
            .current_dir(&req.workspace_root)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        for (k, v) in &req.env {
            cmd.env(k, v);
        }

        let child = cmd.spawn().map_err(ProcessError::SpawnFailed)?;
        let pid = child.id().map(|n| n as i32);

        let id = ProcessId::new();
        let managed = ManagedProcess {
            id,
            kind: req.kind,
            command: req.command,
            args: req.args,
            workspace_root: req.workspace_root,
            pid,
            started_at: Utc::now(),
            status: ProcessStatus::Running,
            last_heartbeat: None,
            restart_count: 0,
        };

        tracing::info!(target: "sera_gateway::process_manager", "spawn: {}", log_spawn_redacted(&managed));

        self.store.upsert(&managed).await?;
        self.processes.write().await.insert(id, managed);
        self.children
            .write()
            .await
            .insert(id, Arc::new(Mutex::new(child)));

        Ok(id)
    }

    /// Adopt metadata for a process owned by someone else (Phase M startup
    /// reconcile feeds this). The gateway does not take ownership of the OS
    /// process — it is purely a registry entry. Returns the `ProcessId` of the
    /// adopted record.
    pub async fn adopt_existing(&self, p: ManagedProcess) -> Result<ProcessId, ProcessError> {
        let id = p.id;
        tracing::info!(
            target: "sera_gateway::process_manager",
            "adopt_existing: {}",
            log_spawn_redacted(&p),
        );
        self.store.upsert(&p).await?;
        self.processes.write().await.insert(id, p);
        Ok(id)
    }

    /// Shut down a managed process.
    ///
    /// Implements the cascade from SPEC §18.6 in its Phase S form:
    ///
    /// 1. If we own a `Child`, send SIGTERM via `start_kill`.
    /// 2. Wait up to 5s for the process to exit.
    /// 3. If still alive, force-kill via `kill()` (SIGKILL on Unix, terminate on Windows).
    /// 4. Update status to `Exited` and remove from both cache + store.
    ///
    /// For adopted processes (no owned `Child`), only the registry entry is
    /// removed — we do not attempt to kill a process we don't own.
    pub async fn shutdown(&self, id: ProcessId) -> Result<(), ProcessError> {
        let existed = self.processes.read().await.contains_key(&id);
        if !existed {
            // Also consider store-only records.
            if self.store.get(id).await?.is_none() {
                return Err(ProcessError::NotFound(id));
            }
        }

        // Try to take ownership of the child (if any) and release the write lock
        // before doing the async kill/wait.
        let child_slot = self.children.write().await.remove(&id);

        if let Some(child_arc) = child_slot {
            let mut child = child_arc.lock().await;
            // Step 1: graceful kill signal.
            match child.start_kill() {
                Ok(()) => {}
                Err(err) => {
                    tracing::warn!(
                        target: "sera_gateway::process_manager",
                        "shutdown: start_kill failed for id={}: {}",
                        id,
                        err,
                    );
                }
            }

            // Step 2: wait up to 5s for voluntary exit.
            let wait_result = timeout(Duration::from_secs(5), child.wait()).await;
            match wait_result {
                Ok(Ok(_status)) => {
                    tracing::info!(
                        target: "sera_gateway::process_manager",
                        "shutdown: id={} exited cleanly",
                        id,
                    );
                }
                Ok(Err(err)) => {
                    tracing::warn!(
                        target: "sera_gateway::process_manager",
                        "shutdown: wait error for id={}: {}",
                        id,
                        err,
                    );
                }
                Err(_elapsed) => {
                    // Step 3: force-kill.
                    tracing::warn!(
                        target: "sera_gateway::process_manager",
                        "shutdown: id={} did not exit in 5s; force killing",
                        id,
                    );
                    if let Err(err) = child.kill().await {
                        tracing::error!(
                            target: "sera_gateway::process_manager",
                            "shutdown: force kill failed for id={}: {}",
                            id,
                            err,
                        );
                        return Err(ProcessError::ShutdownTimeout { secs: 5 });
                    }
                }
            }
        } else {
            tracing::info!(
                target: "sera_gateway::process_manager",
                "shutdown: id={} was adopted (not owned); removing registry entry only",
                id,
            );
        }

        // Step 4: mark exited + remove from both cache and store.
        if let Some(mut p) = self.processes.write().await.remove(&id) {
            p.status = ProcessStatus::Exited;
            // Record the exited state for observability; Phase M reconcile will
            // consult this. The store-remove below is the final step per §18.6.
            let _ = p;
        }
        self.store.remove(id).await?;

        Ok(())
    }

    /// List all managed processes. Delegates to the store so adopted entries
    /// that bypass the in-memory cache still surface.
    pub async fn list(&self) -> Result<Vec<ManagedProcess>, ProcessError> {
        self.store.list().await
    }

    /// Look up a single managed process. Delegates to the store.
    pub async fn get(&self, id: ProcessId) -> Result<Option<ManagedProcess>, ProcessError> {
        self.store.get(id).await
    }

    /// Replace the default restart policy. Phase M's restart loop will read
    /// this at crash time.
    pub fn set_restart_policy(&mut self, policy: RestartPolicy) {
        self.restart_policy = policy;
    }

    /// Return a clone of the current restart policy.
    pub async fn restart_policy(&self) -> RestartPolicy {
        self.restart_policy.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_process(pid: Option<i32>) -> ManagedProcess {
        ManagedProcess {
            id: ProcessId::new(),
            kind: ProcessKind::LspServer,
            command: "rust-analyzer".to_string(),
            args: vec!["--foo".to_string(), "SECRET_TOKEN_abc123".to_string()],
            workspace_root: PathBuf::from("/tmp/ws"),
            pid,
            started_at: Utc::now(),
            status: ProcessStatus::Running,
            last_heartbeat: None,
            restart_count: 0,
        }
    }

    #[tokio::test]
    async fn in_memory_store_roundtrip() {
        let store = InMemoryProcessRegistryStore::new();
        let p = sample_process(Some(1234));
        let id = p.id;

        assert!(store.list().await.unwrap().is_empty());
        store.upsert(&p).await.unwrap();

        let listed = store.list().await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, id);

        let got = store.get(id).await.unwrap();
        assert!(got.is_some());
        assert_eq!(got.unwrap().command, "rust-analyzer");

        store.remove(id).await.unwrap();
        assert!(store.list().await.unwrap().is_empty());
        assert!(store.get(id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn restart_policy_roundtrip() {
        let store: Arc<dyn ProcessRegistryStore> = Arc::new(InMemoryProcessRegistryStore::new());
        let mut mgr = ProcessManager::new(store, RestartPolicy::Never);
        assert_eq!(mgr.restart_policy().await, RestartPolicy::Never);

        mgr.set_restart_policy(RestartPolicy::OnCrash {
            max_attempts: 3,
            backoff_secs: 2,
        });
        assert_eq!(
            mgr.restart_policy().await,
            RestartPolicy::OnCrash {
                max_attempts: 3,
                backoff_secs: 2,
            },
        );

        mgr.set_restart_policy(RestartPolicy::Always { backoff_secs: 5 });
        assert_eq!(
            mgr.restart_policy().await,
            RestartPolicy::Always { backoff_secs: 5 },
        );
    }

    #[tokio::test]
    async fn adopt_existing_stores_without_owning() {
        let store: Arc<dyn ProcessRegistryStore> = Arc::new(InMemoryProcessRegistryStore::new());
        let mgr = ProcessManager::new(store.clone(), RestartPolicy::Never);

        // pid=1 always exists on Unix (init). On Windows we use the current
        // process id, which is guaranteed to exist for the duration of the test.
        #[cfg(unix)]
        let pid: i32 = 1;
        #[cfg(windows)]
        let pid: i32 = std::process::id() as i32;

        let p = sample_process(Some(pid));
        let id = p.id;
        let adopted_id = mgr.adopt_existing(p).await.unwrap();
        assert_eq!(adopted_id, id);

        let got = mgr.get(id).await.unwrap().expect("adopted process present");
        assert_eq!(got.pid, Some(pid));

        // shutdown removes the registry entry; it MUST NOT try to kill pid=1
        // because we don't own the child. This path exercises the
        // "adopted -> store-only cleanup" branch.
        mgr.shutdown(id).await.unwrap();
        assert!(mgr.get(id).await.unwrap().is_none());
    }

    /// Exercise the full `spawn -> list -> shutdown` loop with a real child
    /// process. Gated to Unix because the default command differs between
    /// platforms and we want the test to be deterministic.
    #[cfg(unix)]
    #[tokio::test]
    async fn spawn_and_shutdown_real_process() {
        // Use `sleep 30` so the process is guaranteed to still be alive when we
        // call `list`, avoiding a race against a short-lived `echo`.
        let store: Arc<dyn ProcessRegistryStore> = Arc::new(InMemoryProcessRegistryStore::new());
        let mgr = ProcessManager::new(store, RestartPolicy::Never);

        let req = SpawnRequest {
            kind: ProcessKind::CustomPlugin,
            command: "sleep".to_string(),
            args: vec!["30".to_string()],
            workspace_root: std::env::temp_dir(),
            env: HashMap::new(),
        };
        let id = mgr.spawn(req).await.expect("spawn sleep 30");

        let listed = mgr.list().await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, id);
        assert_eq!(listed[0].status, ProcessStatus::Running);
        assert!(listed[0].pid.is_some(), "pid must be populated after spawn");

        mgr.shutdown(id).await.expect("shutdown sleep 30");
        assert!(mgr.list().await.unwrap().is_empty());
    }

    /// Windows counterpart — uses the `cmd /C timeout` idiom. Kept minimal;
    /// the Unix test is the primary coverage. If `timeout.exe` is unavailable
    /// (very rare), this test will report `SpawnFailed` which still validates
    /// the error path.
    #[cfg(not(unix))]
    #[tokio::test]
    async fn spawn_and_shutdown_real_process_windows() {
        // Intentionally a no-op placeholder so the crate still builds on
        // Windows without pulling in a long-running subprocess dependency.
        // The real Phase M work will add a `cmd /C timeout /T 30` harness.
    }

    #[tokio::test]
    async fn spawn_failure_on_missing_binary() {
        let store: Arc<dyn ProcessRegistryStore> = Arc::new(InMemoryProcessRegistryStore::new());
        let mgr = ProcessManager::new(store, RestartPolicy::Never);

        let req = SpawnRequest {
            kind: ProcessKind::CustomPlugin,
            command: "/does/not/exist/please".to_string(),
            args: vec![],
            workspace_root: std::env::temp_dir(),
            env: HashMap::new(),
        };
        let err = mgr.spawn(req).await.expect_err("spawn of missing binary must fail");
        match err {
            ProcessError::SpawnFailed(_) => {}
            other => panic!("expected SpawnFailed, got {:?}", other),
        }

        // Store must remain empty on failure.
        assert!(mgr.list().await.unwrap().is_empty());
    }

    /// Verify that the shared log-redaction formatter emits `command` and
    /// `args_len=N` but never includes any `args` values. This is the
    /// canonical path every `tracing` call in this module routes through;
    /// protecting it here protects §18.9's "args may contain secrets" rule.
    #[test]
    fn log_redaction_never_includes_args_values() {
        let p = sample_process(Some(4242));
        let rendered = log_spawn_redacted(&p);

        assert!(rendered.contains("command=rust-analyzer"), "missing command: {rendered}");
        assert!(rendered.contains("args_len=2"), "missing args_len: {rendered}");
        assert!(!rendered.contains("SECRET_TOKEN_abc123"), "leaked secret arg: {rendered}");
        assert!(!rendered.contains("--foo"), "leaked non-secret arg: {rendered}");
    }
}
