//! Plugin registry trait and in-memory implementation.
//!
//! For stdio plugins, the registry also manages subprocess lifecycle:
//! spawn on register, heartbeat over stdin/stdout, SIGTERM/SIGKILL on
//! deregister, and restart-with-backoff on crash (§6.5).

use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

use crate::circuit_breaker::CircuitBreaker;
use crate::error::PluginError;
use crate::manifest::validate_stdio_command;
use crate::types::{
    PluginCapability, PluginHealth, PluginInfo, PluginRegistration, PluginTransport,
};

/// Grace period before SIGKILL is sent to a stdio plugin on shutdown.
const STDIO_SHUTDOWN_GRACE: Duration = Duration::from_secs(5);

/// Initial backoff before the first restart attempt.
const BACKOFF_INITIAL: Duration = Duration::from_secs(1);

/// Maximum backoff between restart attempts.
const BACKOFF_MAX: Duration = Duration::from_secs(60);

/// Registry of active plugins.
///
/// Implementors store, retrieve and query plugin registrations. The primary
/// implementation is [`InMemoryPluginRegistry`]; persistent backends can be
/// added later by implementing this trait.
#[async_trait]
pub trait PluginRegistry: Send + Sync {
    /// Register a new plugin. Returns [`PluginError::RegistrationFailed`] if a
    /// plugin with the same name already exists.
    async fn register(&self, registration: PluginRegistration) -> Result<(), PluginError>;

    /// Remove a plugin from the registry.
    async fn deregister(&self, name: &str) -> Result<(), PluginError>;

    /// Look up a single plugin by name.
    async fn get(&self, name: &str) -> Result<PluginInfo, PluginError>;

    /// List all registered plugins.
    async fn list(&self) -> Vec<PluginInfo>;

    /// Find all plugins that advertise a given capability.
    async fn find_by_capability(&self, cap: &PluginCapability) -> Vec<PluginInfo>;

    /// Update the health snapshot for a plugin.
    async fn update_health(&self, name: &str, health: PluginHealth) -> Result<(), PluginError>;
}

/// Live state for a running stdio subprocess plugin.
struct StdioProcess {
    child: Child,
    /// Circuit breaker tracking consecutive restart failures for this plugin.
    breaker: CircuitBreaker,
}

/// Thread-safe in-memory plugin registry backed by a `RwLock<HashMap>`.
///
/// For stdio plugins this also owns the subprocess handle and manages the
/// subprocess lifecycle (spawn, heartbeat, shutdown, restart-with-backoff).
#[derive(Default)]
pub struct InMemoryPluginRegistry {
    plugins: Arc<RwLock<HashMap<String, PluginInfo>>>,
    /// Live subprocess handles for stdio plugins, keyed by plugin name.
    stdio_procs: Arc<RwLock<HashMap<String, StdioProcess>>>,
}

// Manual Debug impl because Child is not Debug.
impl std::fmt::Debug for InMemoryPluginRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InMemoryPluginRegistry")
            .finish_non_exhaustive()
    }
}

impl Clone for InMemoryPluginRegistry {
    fn clone(&self) -> Self {
        Self {
            plugins: Arc::clone(&self.plugins),
            stdio_procs: Arc::clone(&self.stdio_procs),
        }
    }
}

impl InMemoryPluginRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Spawn a stdio plugin child process and store its handle.
    ///
    /// Validates `command[0]` is absolute (§6.2). The child's stdin and stdout
    /// are piped; stderr is inherited so plugin log output reaches the gateway's
    /// stderr without further wiring.
    async fn spawn_stdio(
        &self,
        name: &str,
        registration: &PluginRegistration,
    ) -> Result<(), PluginError> {
        let stdio_cfg = match &registration.transport {
            PluginTransport::Stdio { stdio } => stdio,
            _ => return Ok(()), // gRPC plugins have no subprocess to spawn
        };

        validate_stdio_command(&stdio_cfg.command).map_err(|e| {
            PluginError::RegistrationFailed {
                reason: e.to_string(),
            }
        })?;

        let (program, args) = stdio_cfg
            .command
            .split_first()
            .expect("validate_stdio_command ensures non-empty");

        let mut cmd = Command::new(program);
        cmd.args(args)
            .envs(&stdio_cfg.env)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit());

        let child = cmd.spawn().map_err(|e| PluginError::RegistrationFailed {
            reason: format!("failed to spawn stdio plugin '{name}': {e}"),
        })?;

        info!(plugin = %name, "stdio plugin subprocess spawned");

        let breaker = CircuitBreaker::new(name, 3, Duration::from_secs(30));
        let mut procs = self.stdio_procs.write().await;
        procs.insert(name.to_owned(), StdioProcess { child, breaker });

        Ok(())
    }

    /// Send a single-line JSON-RPC heartbeat to a stdio plugin and read the
    /// response. Updates the circuit breaker based on the outcome.
    ///
    /// The wire format is newline-framed JSON matching the proto-defined
    /// `Heartbeat` method (§2.2, §6.5).
    pub async fn stdio_heartbeat(&self, name: &str) -> Result<(), PluginError> {
        let mut procs = self.stdio_procs.write().await;
        let proc = procs
            .get_mut(name)
            .ok_or_else(|| PluginError::PluginNotFound {
                name: name.to_owned(),
            })?;

        if proc.breaker.allow().is_err() {
            return Err(PluginError::CircuitOpen {
                name: name.to_owned(),
            });
        }

        let heartbeat = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "Heartbeat",
            "params": {},
            "id": 1
        });
        let mut line = serde_json::to_string(&heartbeat).unwrap();
        line.push('\n');

        // Write to stdin
        let stdin = proc.child.stdin.as_mut().ok_or_else(|| {
            proc.breaker.record_failure();
            PluginError::HealthCheckFailed {
                name: name.to_owned(),
                reason: "stdin pipe not available".into(),
            }
        })?;

        if let Err(e) = stdin.write_all(line.as_bytes()).await {
            proc.breaker.record_failure();
            return Err(PluginError::HealthCheckFailed {
                name: name.to_owned(),
                reason: format!("write to stdin failed: {e}"),
            });
        }

        // Read one line from stdout
        let stdout = proc.child.stdout.as_mut().ok_or_else(|| {
            proc.breaker.record_failure();
            PluginError::HealthCheckFailed {
                name: name.to_owned(),
                reason: "stdout pipe not available".into(),
            }
        })?;

        let mut reader = BufReader::new(stdout);
        let mut response = String::new();
        match tokio::time::timeout(Duration::from_secs(5), reader.read_line(&mut response)).await {
            Ok(Ok(_)) => {
                proc.breaker.record_success();
                debug!(plugin = %name, "stdio heartbeat ok");
                Ok(())
            }
            Ok(Err(e)) => {
                proc.breaker.record_failure();
                Err(PluginError::HealthCheckFailed {
                    name: name.to_owned(),
                    reason: format!("read from stdout failed: {e}"),
                })
            }
            Err(_elapsed) => {
                proc.breaker.record_failure();
                Err(PluginError::HealthCheckFailed {
                    name: name.to_owned(),
                    reason: "heartbeat response timed out after 5s".into(),
                })
            }
        }
    }

    /// Gracefully shut down a stdio subprocess: SIGTERM, wait up to
    /// `STDIO_SHUTDOWN_GRACE`, then SIGKILL.
    async fn shutdown_stdio(&self, name: &str) {
        let mut procs = self.stdio_procs.write().await;
        let Some(mut proc) = procs.remove(name) else {
            return;
        };

        // SIGTERM
        #[cfg(unix)]
        {
            if let Some(id) = proc.child.id() {
                // Safety: kill(2) with SIGTERM is safe to call on a live pid.
                unsafe {
                    libc::kill(id as libc::pid_t, libc::SIGTERM);
                }
            }
        }
        #[cfg(not(unix))]
        {
            // On Windows, kill() sends TerminateProcess.
            let _ = proc.child.kill().await;
        }

        // Wait up to grace period, then SIGKILL.
        match tokio::time::timeout(STDIO_SHUTDOWN_GRACE, proc.child.wait()).await {
            Ok(Ok(status)) => {
                info!(plugin = %name, ?status, "stdio plugin exited cleanly");
            }
            Ok(Err(e)) => {
                warn!(plugin = %name, "error waiting for stdio plugin exit: {e}");
            }
            Err(_) => {
                warn!(plugin = %name, "stdio plugin did not exit within grace period, sending SIGKILL");
                let _ = proc.child.kill().await;
            }
        }
    }

    /// Restart a crashed stdio plugin with exponential backoff.
    ///
    /// The circuit breaker for the plugin is incremented on each failed
    /// restart attempt. If the breaker opens, restarting stops.
    pub async fn restart_stdio_with_backoff(&self, name: &str, registration: &PluginRegistration) {
        let mut backoff = BACKOFF_INITIAL;
        loop {
            // Check whether the breaker permits a restart attempt.
            {
                let procs = self.stdio_procs.read().await;
                if let Some(proc) = procs.get(name)
                    && proc.breaker.allow().is_err()
                {
                    warn!(plugin = %name, "circuit breaker open, stopping restart attempts");
                    return;
                }
            }

            tokio::time::sleep(backoff).await;
            backoff = (backoff * 2).min(BACKOFF_MAX);

            match self.spawn_stdio(name, registration).await {
                Ok(()) => {
                    // Record success on the breaker.
                    let procs = self.stdio_procs.read().await;
                    if let Some(proc) = procs.get(name) {
                        proc.breaker.record_success();
                    }
                    info!(plugin = %name, "stdio plugin restarted successfully");
                    return;
                }
                Err(e) => {
                    error!(plugin = %name, "stdio plugin restart failed: {e}");
                    // Record failure so the breaker can open.
                    let procs = self.stdio_procs.read().await;
                    if let Some(proc) = procs.get(name) {
                        proc.breaker.record_failure();
                    }
                }
            }
        }
    }
}

#[async_trait]
impl PluginRegistry for InMemoryPluginRegistry {
    async fn register(&self, registration: PluginRegistration) -> Result<(), PluginError> {
        // For stdio plugins, spawn the subprocess before inserting into the map.
        if matches!(registration.transport, PluginTransport::Stdio { .. }) {
            self.spawn_stdio(&registration.name, &registration).await?;
        }

        let mut guard = self.plugins.write().await;
        if guard.contains_key(&registration.name) {
            warn!(plugin = %registration.name, "plugin already registered");
            return Err(PluginError::RegistrationFailed {
                reason: format!("plugin '{}' is already registered", registration.name),
            });
        }
        let name = registration.name.clone();
        guard.insert(name.clone(), PluginInfo::new(registration));
        info!(plugin = %name, "plugin registered");
        Ok(())
    }

    async fn deregister(&self, name: &str) -> Result<(), PluginError> {
        // Shut down the stdio subprocess (if any) before removing from the map.
        self.shutdown_stdio(name).await;

        let mut guard = self.plugins.write().await;
        if guard.remove(name).is_none() {
            return Err(PluginError::PluginNotFound { name: name.into() });
        }
        info!(plugin = %name, "plugin deregistered");
        Ok(())
    }

    async fn get(&self, name: &str) -> Result<PluginInfo, PluginError> {
        let guard = self.plugins.read().await;
        guard
            .get(name)
            .cloned()
            .ok_or_else(|| PluginError::PluginNotFound { name: name.into() })
    }

    async fn list(&self) -> Vec<PluginInfo> {
        let guard = self.plugins.read().await;
        guard.values().cloned().collect()
    }

    async fn find_by_capability(&self, cap: &PluginCapability) -> Vec<PluginInfo> {
        let guard = self.plugins.read().await;
        guard
            .values()
            .filter(|info| info.registration.capabilities.contains(cap))
            .cloned()
            .collect()
    }

    async fn update_health(&self, name: &str, health: PluginHealth) -> Result<(), PluginError> {
        let mut guard = self.plugins.write().await;
        match guard.get_mut(name) {
            Some(info) => {
                debug!(plugin = %name, healthy = %health.healthy, "health updated");
                info.health = health;
                Ok(())
            }
            None => Err(PluginError::PluginNotFound { name: name.into() }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{GrpcTransportConfig, PluginCapability, PluginTransport, PluginVersion};
    use std::time::Duration;

    fn make_grpc_registration(name: &str, caps: Vec<PluginCapability>) -> PluginRegistration {
        PluginRegistration {
            name: name.into(),
            version: PluginVersion::new(1, 0, 0),
            capabilities: caps,
            transport: PluginTransport::Grpc {
                grpc: GrpcTransportConfig {
                    endpoint: "localhost:9000".into(),
                    tls: None,
                },
            },
            health_check_interval: Duration::from_secs(30),
        }
    }

    #[tokio::test]
    async fn register_and_get() {
        let registry = InMemoryPluginRegistry::new();
        let reg = make_grpc_registration("my-plugin", vec![PluginCapability::ToolExecutor]);
        registry.register(reg).await.unwrap();
        let info = registry.get("my-plugin").await.unwrap();
        assert_eq!(info.registration.name, "my-plugin");
    }

    #[tokio::test]
    async fn duplicate_registration_fails() {
        let registry = InMemoryPluginRegistry::new();
        let reg = make_grpc_registration("dup", vec![]);
        registry.register(reg.clone()).await.unwrap();
        let err = registry.register(reg).await.unwrap_err();
        assert!(matches!(err, PluginError::RegistrationFailed { .. }));
    }

    #[tokio::test]
    async fn deregister_removes_plugin() {
        let registry = InMemoryPluginRegistry::new();
        registry
            .register(make_grpc_registration("to-remove", vec![]))
            .await
            .unwrap();
        registry.deregister("to-remove").await.unwrap();
        let err = registry.get("to-remove").await.unwrap_err();
        assert!(matches!(err, PluginError::PluginNotFound { .. }));
    }

    #[tokio::test]
    async fn deregister_missing_plugin_fails() {
        let registry = InMemoryPluginRegistry::new();
        let err = registry.deregister("ghost").await.unwrap_err();
        assert!(matches!(err, PluginError::PluginNotFound { .. }));
    }

    #[tokio::test]
    async fn list_returns_all() {
        let registry = InMemoryPluginRegistry::new();
        registry
            .register(make_grpc_registration("a", vec![]))
            .await
            .unwrap();
        registry
            .register(make_grpc_registration("b", vec![]))
            .await
            .unwrap();
        let all = registry.list().await;
        assert_eq!(all.len(), 2);
    }

    #[tokio::test]
    async fn find_by_capability_filters_correctly() {
        let registry = InMemoryPluginRegistry::new();
        registry
            .register(make_grpc_registration(
                "tool-plugin",
                vec![PluginCapability::ToolExecutor],
            ))
            .await
            .unwrap();
        registry
            .register(make_grpc_registration(
                "memory-plugin",
                vec![PluginCapability::MemoryBackend],
            ))
            .await
            .unwrap();

        let tools = registry
            .find_by_capability(&PluginCapability::ToolExecutor)
            .await;
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].registration.name, "tool-plugin");
    }

    #[tokio::test]
    async fn update_health_reflects_in_get() {
        let registry = InMemoryPluginRegistry::new();
        registry
            .register(make_grpc_registration("hp", vec![]))
            .await
            .unwrap();

        let health = PluginHealth::ok(15);
        registry.update_health("hp", health).await.unwrap();

        let info = registry.get("hp").await.unwrap();
        assert!(info.health.healthy);
        assert_eq!(info.health.latency_ms, Some(15));
    }

    #[tokio::test]
    async fn update_health_missing_plugin_fails() {
        let registry = InMemoryPluginRegistry::new();
        let err = registry
            .update_health("ghost", PluginHealth::initial())
            .await
            .unwrap_err();
        assert!(matches!(err, PluginError::PluginNotFound { .. }));
    }
}
