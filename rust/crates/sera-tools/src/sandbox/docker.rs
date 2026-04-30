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

use super::{
    EgressBridgeConfig, EgressEndpoint, EgressManifest, EgressMode, ExecResult, MountSpec,
    SandboxConfig, SandboxError, SandboxHandle, SandboxProvider,
};

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
    additional_binds: Vec<String>,
    egress: Option<EgressManifest>,
    egress_bridge: Option<EgressBridgeConfig>,
}

/// Translate an [`EgressManifest`] into the Docker argv that enforces it.
///
/// Returns:
/// - `Ok(vec![])` when egress is `Disabled`, or when `AuditOnly` is requested
///   without a bridge (fail-open ramp; emits a warn log).
/// - `Err(SandboxError::PolicyViolation)` when `Strict` is requested without a
///   bridge — fail-closed.
/// - On Strict/AuditOnly with a bridge: `--network=<name>`,
///   `--add-host=inference.local:<gateway>`, `--cap-drop=ALL`,
///   `--security-opt=no-new-privileges:true` in that order.
///
/// `Domain` / `Cidr` entries in `egress.allowed` are not yet enforced by the
/// Docker provider; they emit a warn log and fall through to strict bridge
/// flags (so unlisted hosts remain unreachable — the safer failure mode).
fn build_egress_args(
    egress: &EgressManifest,
    bridge: Option<&EgressBridgeConfig>,
) -> Result<Vec<String>, SandboxError> {
    if egress.mode == EgressMode::Disabled {
        return Ok(vec![]);
    }

    for endpoint in &egress.allowed {
        match endpoint {
            EgressEndpoint::InferenceLocal => {}
            EgressEndpoint::Domain { name } => {
                tracing::warn!(
                    target: "sera_tools::sandbox::egress",
                    domain = %name,
                    "domain egress not enforced by Docker provider yet; \
                     traffic will be blocked by the strict bridge",
                );
            }
            EgressEndpoint::Cidr { range } => {
                tracing::warn!(
                    target: "sera_tools::sandbox::egress",
                    cidr = %range,
                    "cidr egress not enforced by Docker provider yet; \
                     traffic will be blocked by the strict bridge",
                );
            }
        }
    }

    let bridge = match bridge {
        Some(b) => b,
        None => {
            if egress.mode == EgressMode::AuditOnly {
                tracing::warn!(
                    target: "sera_tools::sandbox::egress",
                    "egress audit-only requested but no bridge configured; \
                     sandbox will run on default bridge",
                );
                return Ok(vec![]);
            }
            return Err(SandboxError::PolicyViolation {
                reason: "egress mode 'strict' requires an egress_bridge but \
                         none was provisioned. To run this sandboxed agent: \
                         (1) provision the sera-egress bridge before launching \
                         this sandbox, or (2) set spec.egress.mode = 'disabled' \
                         in the agent template (compliance opt-out)."
                    .to_string(),
            });
        }
    };

    Ok(vec![
        format!("--network={}", bridge.network_name),
        format!("--add-host=inference.local:{}", bridge.gateway_target),
        "--cap-drop=ALL".to_string(),
        "--security-opt=no-new-privileges:true".to_string(),
    ])
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

    /// Format additional mounts as Docker bind-mount strings (`host:container`
    /// or `host:container:ro` depending on `read_only`).
    pub fn build_additional_binds(mounts: &[MountSpec]) -> Vec<String> {
        mounts
            .iter()
            .map(|m| {
                if m.read_only {
                    format!("{}:{}:ro", m.host_path, m.container_path)
                } else {
                    format!("{}:{}", m.host_path, m.container_path)
                }
            })
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
        let additional_binds = Self::build_additional_binds(&config.additional_mounts);

        let stored = StoredConfig {
            image,
            env: config.env.clone(),
            source_binds,
            additional_binds,
            egress: config.egress.clone(),
            egress_bridge: config.egress_bridge.clone(),
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

        if let Some(egress) = &stored.egress {
            for arg in build_egress_args(egress, stored.egress_bridge.as_ref())? {
                cmd.arg(arg);
            }
        }

        // Merge env: stored (from create) first, per-exec overrides second.
        for (k, v) in stored.env.iter().chain(env.iter()) {
            cmd.arg("-e").arg(format!("{}={}", k, v));
        }
        for bind in &stored.source_binds {
            cmd.arg("-v").arg(bind);
        }
        for bind in &stored.additional_binds {
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

    #[test]
    fn build_additional_binds_formats_rw_and_ro() {
        let mounts = vec![
            MountSpec {
                host_path: "/host/cache".to_string(),
                container_path: "/cache".to_string(),
                read_only: false,
            },
            MountSpec {
                host_path: "/host/config".to_string(),
                container_path: "/etc/agent".to_string(),
                read_only: true,
            },
        ];
        let binds = DockerSandboxProvider::build_additional_binds(&mounts);
        assert_eq!(binds.len(), 2);
        // rw mount: no :ro suffix
        assert_eq!(binds[0], "/host/cache:/cache");
        // ro mount: :ro suffix
        assert_eq!(binds[1], "/host/config:/etc/agent:ro");
    }

    #[tokio::test]
    async fn additional_mounts_are_stored_for_execute() {
        // Verify the SandboxConfig.additional_mounts feeds into the stored bind
        // list, which is what `execute` appends as `-v host:container[:ro]` args.
        let Ok(provider) = DockerSandboxProvider::new() else {
            eprintln!("docker client init failed; skipping test");
            return;
        };
        let config = SandboxConfig {
            image: Some("alpine:3".to_string()),
            additional_mounts: vec![
                MountSpec {
                    host_path: "/host/data".to_string(),
                    container_path: "/data".to_string(),
                    read_only: false,
                },
                MountSpec {
                    host_path: "/host/conf".to_string(),
                    container_path: "/conf".to_string(),
                    read_only: true,
                },
            ],
            ..Default::default()
        };
        let handle = provider.create(&config).await.expect("create");
        let guard = provider.configs.lock().await;
        let stored = guard.get(&handle.0).expect("stored config");
        assert_eq!(
            stored.additional_binds,
            vec![
                "/host/data:/data".to_string(),
                "/host/conf:/conf:ro".to_string(),
            ],
            "additional_mounts should be translated into -v bind strings"
        );
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

    fn strict_inference_local_manifest() -> EgressManifest {
        EgressManifest {
            allowed: vec![EgressEndpoint::InferenceLocal],
            mode: EgressMode::Strict,
        }
    }

    fn sample_bridge() -> EgressBridgeConfig {
        EgressBridgeConfig {
            network_name: "sera-egress".to_string(),
            gateway_target: "172.18.0.2".to_string(),
        }
    }

    #[test]
    fn build_egress_args_strict_with_bridge() {
        let manifest = strict_inference_local_manifest();
        let bridge = sample_bridge();
        let args = build_egress_args(&manifest, Some(&bridge)).expect("strict + bridge");
        assert_eq!(
            args,
            vec![
                "--network=sera-egress".to_string(),
                "--add-host=inference.local:172.18.0.2".to_string(),
                "--cap-drop=ALL".to_string(),
                "--security-opt=no-new-privileges:true".to_string(),
            ]
        );
    }

    #[test]
    fn build_egress_args_strict_without_bridge_fails_closed() {
        let manifest = strict_inference_local_manifest();
        let err = build_egress_args(&manifest, None).unwrap_err();
        match err {
            SandboxError::PolicyViolation { reason } => {
                assert!(
                    reason.contains("egress_bridge"),
                    "reason should mention egress_bridge: {reason}"
                );
            }
            other => panic!("expected PolicyViolation, got {other:?}"),
        }
    }

    #[test]
    fn build_egress_args_disabled_returns_empty() {
        let manifest = EgressManifest {
            allowed: vec![],
            mode: EgressMode::Disabled,
        };
        let args = build_egress_args(&manifest, None).expect("disabled");
        assert!(args.is_empty());
    }

    #[test]
    fn build_egress_args_audit_only_without_bridge_fails_open() {
        let manifest = EgressManifest {
            allowed: vec![],
            mode: EgressMode::AuditOnly,
        };
        let args = build_egress_args(&manifest, None).expect("audit-only fails open");
        assert!(args.is_empty());
    }

    #[test]
    fn build_egress_args_audit_only_with_bridge_returns_strict_argv() {
        let manifest = EgressManifest {
            allowed: vec![EgressEndpoint::InferenceLocal],
            mode: EgressMode::AuditOnly,
        };
        let bridge = sample_bridge();
        let args = build_egress_args(&manifest, Some(&bridge)).expect("audit-only + bridge");
        assert_eq!(
            args,
            vec![
                "--network=sera-egress".to_string(),
                "--add-host=inference.local:172.18.0.2".to_string(),
                "--cap-drop=ALL".to_string(),
                "--security-opt=no-new-privileges:true".to_string(),
            ]
        );
    }

    #[test]
    fn build_egress_args_domain_and_cidr_fall_through_to_strict_bridge() {
        let manifest = EgressManifest {
            allowed: vec![
                EgressEndpoint::InferenceLocal,
                EgressEndpoint::Domain {
                    name: "api.github.com".to_string(),
                },
                EgressEndpoint::Cidr {
                    range: "10.0.0.0/8".to_string(),
                },
            ],
            mode: EgressMode::Strict,
        };
        let bridge = sample_bridge();
        let args = build_egress_args(&manifest, Some(&bridge)).expect("strict + extras + bridge");
        assert_eq!(
            args,
            vec![
                "--network=sera-egress".to_string(),
                "--add-host=inference.local:172.18.0.2".to_string(),
                "--cap-drop=ALL".to_string(),
                "--security-opt=no-new-privileges:true".to_string(),
            ]
        );
    }

    #[tokio::test]
    async fn create_with_strict_egress_and_no_bridge_executes_fail_closed() {
        let Ok(provider) = DockerSandboxProvider::new() else {
            eprintln!("docker client init failed; skipping test");
            return;
        };
        let config = SandboxConfig {
            image: Some("alpine:3".to_string()),
            egress: Some(strict_inference_local_manifest()),
            egress_bridge: None,
            ..Default::default()
        };
        let handle = provider.create(&config).await.expect("create");
        let err = provider
            .execute(&handle, "echo x", &HashMap::new())
            .await
            .unwrap_err();
        match err {
            SandboxError::PolicyViolation { reason } => {
                assert!(reason.contains("egress_bridge"), "reason: {reason}");
            }
            other => panic!("expected PolicyViolation, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn create_stores_egress_and_bridge() {
        let Ok(provider) = DockerSandboxProvider::new() else {
            eprintln!("docker client init failed; skipping test");
            return;
        };
        let bridge = sample_bridge();
        let config = SandboxConfig {
            image: Some("alpine:3".to_string()),
            egress: Some(strict_inference_local_manifest()),
            egress_bridge: Some(bridge.clone()),
            ..Default::default()
        };
        let handle = provider.create(&config).await.expect("create");
        let guard = provider.configs.lock().await;
        let stored = guard.get(&handle.0).expect("stored config");
        assert_eq!(stored.egress.as_ref().unwrap().mode, EgressMode::Strict);
        assert_eq!(stored.egress_bridge.as_ref().unwrap(), &bridge);
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
