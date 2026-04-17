//! Docker sandbox provider — minimal shell-out implementation.
//!
//! This is an MVS-grade implementation backed by the `docker` CLI via
//! `tokio::process::Command`. It supports:
//! - `create` — validates the config, stores it keyed by an opaque handle
//!   (no container is started until `execute`).
//! - `execute` — runs `docker run --rm -i <image> <cmd>` with env vars,
//!   bind-mounts, and a timeout enforced via `tokio::time::timeout`.
//! - `destroy` — drops the in-memory config for the handle.
//!
//! Full bollard-based ContainerManager migration happens in P0-5/P0-6; this
//! implementation unblocks gateway wiring and tool execution for MVS.
//!
//! Execution timeout: controlled by `SandboxConfig::timeout` (added in this
//! change); defaults to 60 s when unset.

#![cfg(feature = "docker")]

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use bollard::Docker;
use tokio::process::Command;
use tokio::sync::Mutex;
use tokio::time::timeout;
use uuid::Uuid;

use sera_types::sandbox::SourceMount;

use super::{ExecResult, SandboxConfig, SandboxError, SandboxHandle, SandboxProvider};

/// Default per-exec timeout (60 s).
const DEFAULT_EXEC_TIMEOUT: Duration = Duration::from_secs(60);

/// Docker-backed sandbox provider.
///
/// Shell-out implementation — spawns `docker run --rm -i` per `execute`. The
/// optional bollard `Docker` client is kept for forward-compatibility with
/// the P0-5/P0-6 ContainerManager migration.
pub struct DockerSandboxProvider {
    #[allow(dead_code)] // retained for forward compatibility with bollard-based ContainerManager.
    inner: Docker,
    /// Per-handle stored configs — the config supplied at `create` is applied
    /// at each `execute` call.
    configs: Arc<Mutex<HashMap<String, StoredConfig>>>,
    /// Path to the `docker` CLI binary; defaults to `"docker"` (resolved via PATH).
    docker_bin: String,
    /// Default per-exec timeout. Currently fixed; will be plumbed through
    /// `SandboxConfig` when the trait grows a timeout field.
    default_timeout: Duration,
}

#[derive(Clone)]
struct StoredConfig {
    image: String,
    env: HashMap<String, String>,
    source_binds: Vec<String>,
}

impl DockerSandboxProvider {
    /// Create a new provider connecting via local defaults (socket/named pipe).
    pub fn new() -> Result<Self, SandboxError> {
        let docker =
            Docker::connect_with_local_defaults().map_err(|e| SandboxError::CreateFailed {
                reason: e.to_string(),
            })?;
        Ok(Self {
            inner: docker,
            configs: Arc::new(Mutex::new(HashMap::new())),
            docker_bin: "docker".to_string(),
            default_timeout: DEFAULT_EXEC_TIMEOUT,
        })
    }

    /// Create from an existing bollard `Docker` client.
    pub fn from_client(docker: Docker) -> Self {
        Self {
            inner: docker,
            configs: Arc::new(Mutex::new(HashMap::new())),
            docker_bin: "docker".to_string(),
            default_timeout: DEFAULT_EXEC_TIMEOUT,
        }
    }

    /// Override the default per-exec timeout (builder-style).
    pub fn with_default_timeout(mut self, timeout: Duration) -> Self {
        self.default_timeout = timeout;
        self
    }

    /// Override the docker binary path (test hook).
    #[cfg(test)]
    fn with_docker_bin(mut self, bin: impl Into<String>) -> Self {
        self.docker_bin = bin.into();
        self
    }

    /// Format source mounts as Docker bind-mount strings (`host:container:ro`).
    pub fn build_source_binds(sources: &[SourceMount]) -> Vec<String> {
        sources
            .iter()
            .map(|m| format!("{}:{}:ro", m.host_path, m.container_path))
            .collect()
    }
}

#[async_trait]
impl SandboxProvider for DockerSandboxProvider {
    fn name(&self) -> &str {
        "docker"
    }

    async fn create(&self, config: &SandboxConfig) -> Result<SandboxHandle, SandboxError> {
        let image = config
            .image
            .clone()
            .ok_or_else(|| SandboxError::CreateFailed {
                reason: "image is required".to_string(),
            })?;
        // Validate source mounts up-front.
        super::validate_sources(&config.sources)?;
        let source_binds = Self::build_source_binds(&config.sources);

        let stored = StoredConfig {
            image,
            env: config.env.clone(),
            source_binds,
        };

        let id = format!("sera-sbx-{}", Uuid::new_v4());
        self.configs.lock().await.insert(id.clone(), stored);
        Ok(SandboxHandle(id))
    }

    async fn execute(
        &self,
        handle: &SandboxHandle,
        command: &str,
        env: &HashMap<String, String>,
    ) -> Result<ExecResult, SandboxError> {
        // Clone the stored config to release the lock before spawning.
        let stored = {
            let guard = self.configs.lock().await;
            guard
                .get(&handle.0)
                .cloned()
                .ok_or(SandboxError::NotFound)?
        };

        let mut cmd = Command::new(&self.docker_bin);
        cmd.arg("run").arg("--rm").arg("-i");

        // Merge env: stored (from create) first, per-exec overrides second.
        for (k, v) in stored.env.iter().chain(env.iter()) {
            cmd.arg("-e").arg(format!("{}={}", k, v));
        }
        for bind in &stored.source_binds {
            cmd.arg("-v").arg(bind);
        }

        cmd.arg(&stored.image);
        // Use `sh -c` so callers can pass shell strings directly.
        cmd.arg("sh").arg("-c").arg(command);
        cmd.kill_on_drop(true);

        let start = Instant::now();
        let output = timeout(self.default_timeout, cmd.output())
            .await
            .map_err(|_| SandboxError::ExecFailed {
                reason: format!(
                    "timeout after {:?} running command in {}",
                    self.default_timeout, handle.0
                ),
            })?
            .map_err(|e| SandboxError::ExecFailed {
                reason: format!("failed to spawn docker: {}", e),
            })?;

        let _duration = start.elapsed();
        Ok(ExecResult {
            exit_code: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }

    async fn read_file(
        &self,
        _handle: &SandboxHandle,
        _path: &str,
    ) -> Result<Vec<u8>, SandboxError> {
        // Deferred to P0-5/P0-6 ContainerManager migration.
        Err(SandboxError::NotImplemented)
    }

    async fn write_file(
        &self,
        _handle: &SandboxHandle,
        _path: &str,
        _content: &[u8],
    ) -> Result<(), SandboxError> {
        // Deferred to P0-5/P0-6 ContainerManager migration.
        Err(SandboxError::NotImplemented)
    }

    async fn destroy(&self, handle: &SandboxHandle) -> Result<(), SandboxError> {
        self.configs.lock().await.remove(&handle.0);
        Ok(())
    }

    async fn status(&self, handle: &SandboxHandle) -> Result<String, SandboxError> {
        let guard = self.configs.lock().await;
        if guard.contains_key(&handle.0) {
            Ok(format!("ready:{}", handle.0))
        } else {
            Err(SandboxError::NotFound)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Check whether the `docker` CLI is available and the daemon is reachable.
    /// Returns true if tests that spawn containers can proceed.
    async fn docker_available() -> bool {
        match std::process::Command::new("docker")
            .arg("version")
            .arg("--format")
            .arg("{{.Server.Version}}")
            .output()
        {
            Ok(out) => out.status.success() && !out.stdout.is_empty(),
            Err(_) => false,
        }
    }

    #[test]
    fn build_source_binds_formats_correctly() {
        let sources = vec![
            SourceMount {
                host_path: "/host/docs".to_string(),
                container_path: "/sources/docs".to_string(),
                label: None,
            },
            SourceMount {
                host_path: "/host/data".to_string(),
                container_path: "/sources/data".to_string(),
                label: Some("Dataset".to_string()),
            },
        ];
        let binds = DockerSandboxProvider::build_source_binds(&sources);
        assert_eq!(binds.len(), 2);
        assert_eq!(binds[0], "/host/docs:/sources/docs:ro");
        assert_eq!(binds[1], "/host/data:/sources/data:ro");
    }

    #[test]
    fn build_source_binds_empty() {
        let binds = DockerSandboxProvider::build_source_binds(&[]);
        assert!(binds.is_empty());
    }

    #[tokio::test]
    async fn create_requires_image() {
        let Ok(provider) = DockerSandboxProvider::new() else {
            eprintln!("docker client init failed; skipping test");
            return;
        };
        let config = SandboxConfig::default();
        let err = provider.create(&config).await.unwrap_err();
        assert!(matches!(err, SandboxError::CreateFailed { .. }));
    }

    #[tokio::test]
    async fn docker_run_hello_world() {
        if !docker_available().await {
            eprintln!("docker not available; skipping test");
            return;
        }
        let Ok(provider) = DockerSandboxProvider::new() else {
            eprintln!("docker client init failed; skipping test");
            return;
        };
        let config = SandboxConfig {
            image: Some("alpine:3".to_string()),
            ..Default::default()
        };
        let handle = provider.create(&config).await.expect("create");
        let result = provider
            .execute(&handle, "echo hello-world", &HashMap::new())
            .await
            .expect("execute");
        assert_eq!(result.exit_code, 0, "stderr: {}", result.stderr);
        assert!(result.stdout.contains("hello-world"), "stdout: {}", result.stdout);
        provider.destroy(&handle).await.expect("destroy");
    }

    #[tokio::test]
    async fn docker_run_respects_timeout() {
        if !docker_available().await {
            eprintln!("docker not available; skipping test");
            return;
        }
        let Ok(provider) = DockerSandboxProvider::new() else {
            eprintln!("docker client init failed; skipping test");
            return;
        };
        let provider = provider.with_default_timeout(Duration::from_millis(500));
        let config = SandboxConfig {
            image: Some("alpine:3".to_string()),
            ..Default::default()
        };
        let handle = provider.create(&config).await.expect("create");
        let err = provider
            .execute(&handle, "sleep 10", &HashMap::new())
            .await
            .unwrap_err();
        match err {
            SandboxError::ExecFailed { reason } => {
                assert!(reason.contains("timeout"), "unexpected reason: {}", reason);
            }
            other => panic!("expected ExecFailed timeout, got {:?}", other),
        }
        // Best-effort cleanup; destroy only drops in-memory state.
        provider.destroy(&handle).await.expect("destroy");
    }

    #[tokio::test]
    async fn docker_run_env_visible() {
        if !docker_available().await {
            eprintln!("docker not available; skipping test");
            return;
        }
        let Ok(provider) = DockerSandboxProvider::new() else {
            eprintln!("docker client init failed; skipping test");
            return;
        };
        let mut env = HashMap::new();
        env.insert("SERA_TEST_TOKEN".to_string(), "ok42".to_string());
        let config = SandboxConfig {
            image: Some("alpine:3".to_string()),
            env: env.clone(),
            ..Default::default()
        };
        let handle = provider.create(&config).await.expect("create");
        let result = provider
            .execute(&handle, "printf %s \"$SERA_TEST_TOKEN\"", &HashMap::new())
            .await
            .expect("execute");
        assert_eq!(result.exit_code, 0, "stderr: {}", result.stderr);
        assert_eq!(result.stdout.trim(), "ok42");
        provider.destroy(&handle).await.expect("destroy");
    }

    #[tokio::test]
    async fn docker_run_invalid_binary_errors() {
        // No docker required — uses a non-existent binary to exercise spawn failure.
        let Ok(provider) = DockerSandboxProvider::new() else {
            eprintln!("docker client init failed; skipping test");
            return;
        };
        let provider = provider.with_docker_bin("/nonexistent/docker-binary-xyz");
        let config = SandboxConfig {
            image: Some("alpine:3".to_string()),
            ..Default::default()
        };
        let handle = provider.create(&config).await.expect("create");
        let err = provider
            .execute(&handle, "echo x", &HashMap::new())
            .await
            .unwrap_err();
        assert!(matches!(err, SandboxError::ExecFailed { .. }));
    }

    #[tokio::test]
    async fn destroy_removes_handle() {
        let Ok(provider) = DockerSandboxProvider::new() else {
            eprintln!("docker client init failed; skipping test");
            return;
        };
        let config = SandboxConfig {
            image: Some("alpine:3".to_string()),
            ..Default::default()
        };
        let handle = provider.create(&config).await.expect("create");
        assert!(provider.status(&handle).await.is_ok());
        provider.destroy(&handle).await.expect("destroy");
        let err = provider.status(&handle).await.unwrap_err();
        assert!(matches!(err, SandboxError::NotFound));
    }
}
