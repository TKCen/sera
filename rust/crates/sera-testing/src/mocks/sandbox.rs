use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use tokio::sync::Mutex;
use async_trait::async_trait;
use sera_tools::sandbox::{
    DockerSandboxPolicy, ExecResult, SandboxConfig, SandboxError, SandboxHandle, SandboxPolicy,
    SandboxProvider,
};

struct MockSandbox {
    policy: Option<SandboxPolicy>,
    files: HashMap<String, Vec<u8>>,
    status: String,
}

/// Returns `true` if the first token of `command` looks like an egress tool
/// (curl, wget, or anything containing "http").
fn is_egress_command(command: &str) -> bool {
    let first = command.split_whitespace().next().unwrap_or("");
    matches!(first, "curl" | "wget") || command.contains("http")
}

/// Returns `true` if the first token of `command` is a common subprocess spawner.
fn is_subprocess_command(command: &str) -> bool {
    let first = command.split_whitespace().next().unwrap_or("");
    matches!(first, "bash" | "sh" | "python" | "python3")
}

/// Check a Docker policy against a command, returning a `PolicyViolation` if denied.
fn check_docker_policy(
    policy: &DockerSandboxPolicy,
    command: &str,
) -> Result<(), SandboxError> {
    if policy.network.default_deny && is_egress_command(command) {
        return Err(SandboxError::PolicyViolation {
            reason: "egress-denied".to_string(),
        });
    }
    if policy.deny_subprocess && is_subprocess_command(command) {
        return Err(SandboxError::PolicyViolation {
            reason: "subprocess-denied".to_string(),
        });
    }
    Ok(())
}

/// In-memory mock sandbox provider for testing.
pub struct MockSandboxProvider {
    sandboxes: Arc<Mutex<HashMap<String, MockSandbox>>>,
    next_id: Arc<std::sync::atomic::AtomicU64>,
}

impl MockSandboxProvider {
    pub fn new() -> Self {
        Self {
            sandboxes: Arc::new(Mutex::new(HashMap::new())),
            next_id: Arc::new(std::sync::atomic::AtomicU64::new(1)),
        }
    }
}

impl Default for MockSandboxProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SandboxProvider for MockSandboxProvider {
    fn name(&self) -> &str {
        "mock"
    }

    async fn create(&self, config: &SandboxConfig) -> Result<SandboxHandle, SandboxError> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let handle_str = format!("mock-sandbox-{id}");
        let mut sandboxes = self.sandboxes.lock().await;
        sandboxes.insert(
            handle_str.clone(),
            MockSandbox {
                policy: config.sandbox_policy.clone(),
                files: HashMap::new(),
                status: "running".to_string(),
            },
        );
        Ok(SandboxHandle(handle_str))
    }

    async fn execute(
        &self,
        handle: &SandboxHandle,
        command: &str,
        _env: &HashMap<String, String>,
    ) -> Result<ExecResult, SandboxError> {
        let sandboxes = self.sandboxes.lock().await;
        let sandbox = sandboxes.get(&handle.0).ok_or(SandboxError::NotFound)?;
        if let Some(SandboxPolicy::Docker(ref docker_policy)) = sandbox.policy {
            check_docker_policy(docker_policy, command)?;
        }
        Ok(ExecResult {
            exit_code: 0,
            stdout: format!("mock: {command}"),
            stderr: String::new(),
        })
    }

    async fn read_file(
        &self,
        handle: &SandboxHandle,
        path: &str,
    ) -> Result<Vec<u8>, SandboxError> {
        let sandboxes = self.sandboxes.lock().await;
        let sandbox = sandboxes.get(&handle.0).ok_or(SandboxError::NotFound)?;
        sandbox
            .files
            .get(path)
            .cloned()
            .ok_or(SandboxError::NotFound)
    }

    async fn write_file(
        &self,
        handle: &SandboxHandle,
        path: &str,
        content: &[u8],
    ) -> Result<(), SandboxError> {
        let mut sandboxes = self.sandboxes.lock().await;
        let sandbox = sandboxes.get_mut(&handle.0).ok_or(SandboxError::NotFound)?;
        sandbox.files.insert(path.to_string(), content.to_vec());
        Ok(())
    }

    async fn destroy(&self, handle: &SandboxHandle) -> Result<(), SandboxError> {
        let mut sandboxes = self.sandboxes.lock().await;
        sandboxes.remove(&handle.0).ok_or(SandboxError::NotFound)?;
        Ok(())
    }

    async fn status(&self, handle: &SandboxHandle) -> Result<String, SandboxError> {
        let sandboxes = self.sandboxes.lock().await;
        let sandbox = sandboxes.get(&handle.0).ok_or(SandboxError::NotFound)?;
        Ok(sandbox.status.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn mock_sandbox_create_destroy() {
        let provider = MockSandboxProvider::new();
        let handle = provider.create(&SandboxConfig::default()).await.unwrap();
        let status = provider.status(&handle).await.unwrap();
        assert_eq!(status, "running");
        provider.destroy(&handle).await.unwrap();
        // After destroy, status should return NotFound
        let err = provider.status(&handle).await.unwrap_err();
        assert!(matches!(err, SandboxError::NotFound));
    }

    #[tokio::test]
    async fn mock_sandbox_exec_returns_command() {
        let provider = MockSandboxProvider::new();
        let handle = provider.create(&SandboxConfig::default()).await.unwrap();
        let result = provider
            .execute(&handle, "echo hello", &HashMap::new())
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "mock: echo hello");
        assert!(result.stderr.is_empty());
    }

    #[tokio::test]
    async fn mock_sandbox_file_roundtrip() {
        let provider = MockSandboxProvider::new();
        let handle = provider.create(&SandboxConfig::default()).await.unwrap();
        let content = b"hello, world!";
        provider
            .write_file(&handle, "/tmp/test.txt", content)
            .await
            .unwrap();
        let read_back = provider.read_file(&handle, "/tmp/test.txt").await.unwrap();
        assert_eq!(read_back, content);
    }

    #[tokio::test]
    async fn mock_sandbox_not_found() {
        let provider = MockSandboxProvider::new();
        let bogus = SandboxHandle("does-not-exist".to_string());

        let err = provider
            .execute(&bogus, "ls", &HashMap::new())
            .await
            .unwrap_err();
        assert!(matches!(err, SandboxError::NotFound));

        let err = provider.read_file(&bogus, "/any").await.unwrap_err();
        assert!(matches!(err, SandboxError::NotFound));

        let err = provider
            .write_file(&bogus, "/any", b"data")
            .await
            .unwrap_err();
        assert!(matches!(err, SandboxError::NotFound));

        let err = provider.destroy(&bogus).await.unwrap_err();
        assert!(matches!(err, SandboxError::NotFound));

        let err = provider.status(&bogus).await.unwrap_err();
        assert!(matches!(err, SandboxError::NotFound));
    }

    #[tokio::test]
    async fn mock_sandbox_provider_name() {
        let provider = MockSandboxProvider::new();
        assert_eq!(provider.name(), "mock");
    }

    #[tokio::test]
    async fn mock_sandbox_default_is_empty() {
        let provider = MockSandboxProvider::default();
        let bogus = SandboxHandle("x".to_string());
        let err = provider.status(&bogus).await.unwrap_err();
        assert!(matches!(err, SandboxError::NotFound));
    }

    #[tokio::test]
    async fn mock_sandbox_multiple_independent_sandboxes() {
        let provider = MockSandboxProvider::new();
        let h1 = provider.create(&SandboxConfig::default()).await.unwrap();
        let h2 = provider.create(&SandboxConfig::default()).await.unwrap();

        // Handles are distinct
        assert_ne!(h1, h2);

        // Files written to h1 are not visible via h2
        provider
            .write_file(&h1, "/tmp/a.txt", b"from-h1")
            .await
            .unwrap();
        let err = provider.read_file(&h2, "/tmp/a.txt").await.unwrap_err();
        assert!(matches!(err, SandboxError::NotFound));

        // Destroying h1 does not affect h2
        provider.destroy(&h1).await.unwrap();
        let status = provider.status(&h2).await.unwrap();
        assert_eq!(status, "running");
    }

    #[tokio::test]
    async fn mock_sandbox_execute_after_destroy_fails() {
        let provider = MockSandboxProvider::new();
        let handle = provider.create(&SandboxConfig::default()).await.unwrap();
        provider.destroy(&handle).await.unwrap();

        let err = provider
            .execute(&handle, "ls", &HashMap::new())
            .await
            .unwrap_err();
        assert!(matches!(err, SandboxError::NotFound));
    }

    #[tokio::test]
    async fn mock_sandbox_file_overwrite() {
        let provider = MockSandboxProvider::new();
        let handle = provider.create(&SandboxConfig::default()).await.unwrap();

        provider
            .write_file(&handle, "/tmp/f.txt", b"v1")
            .await
            .unwrap();
        provider
            .write_file(&handle, "/tmp/f.txt", b"v2-updated")
            .await
            .unwrap();

        let content = provider.read_file(&handle, "/tmp/f.txt").await.unwrap();
        assert_eq!(content, b"v2-updated");
    }

    #[tokio::test]
    async fn mock_sandbox_read_missing_file_in_live_sandbox() {
        let provider = MockSandboxProvider::new();
        let handle = provider.create(&SandboxConfig::default()).await.unwrap();

        let err = provider
            .read_file(&handle, "/tmp/no-such-file.txt")
            .await
            .unwrap_err();
        assert!(matches!(err, SandboxError::NotFound));
    }

    #[tokio::test]
    async fn mock_sandbox_clone_shares_state() {
        let p1 = MockSandboxProvider::new();
        let p2 = MockSandboxProvider {
            sandboxes: p1.sandboxes.clone(),
            next_id: p1.next_id.clone(),
        };

        let handle = p1.create(&SandboxConfig::default()).await.unwrap();
        // p2 should see the sandbox created via p1
        let status = p2.status(&handle).await.unwrap();
        assert_eq!(status, "running");

        // Destroying via p2 is visible from p1
        p2.destroy(&handle).await.unwrap();
        let err = p1.status(&handle).await.unwrap_err();
        assert!(matches!(err, SandboxError::NotFound));
    }

    #[tokio::test]
    async fn mock_sandbox_destroy_is_idempotent_on_missing() {
        let provider = MockSandboxProvider::new();
        let handle = provider.create(&SandboxConfig::default()).await.unwrap();
        provider.destroy(&handle).await.unwrap();

        // Second destroy returns NotFound — caller must handle this
        let err = provider.destroy(&handle).await.unwrap_err();
        assert!(matches!(err, SandboxError::NotFound));
    }

    // --- SandboxPolicy enforcement tests ---

    fn tier1_policy() -> SandboxPolicy {
        use sera_tools::sandbox::{
            DockerSandboxPolicy, FileSystemSandboxPolicy, NetworkSandboxPolicy,
        };
        SandboxPolicy::Docker(DockerSandboxPolicy {
            filesystem: FileSystemSandboxPolicy {
                read_paths: vec![],
                write_paths: vec![],
                include_workdir: false,
            },
            network: NetworkSandboxPolicy {
                rules: vec![],
                default_deny: true,
            },
            deny_subprocess: false,
        })
    }

    fn tier3_policy() -> SandboxPolicy {
        use sera_tools::sandbox::{
            DockerSandboxPolicy, FileSystemSandboxPolicy, NetworkSandboxPolicy,
        };
        SandboxPolicy::Docker(DockerSandboxPolicy {
            filesystem: FileSystemSandboxPolicy {
                read_paths: vec![],
                write_paths: vec![],
                include_workdir: false,
            },
            network: NetworkSandboxPolicy {
                rules: vec![],
                default_deny: false,
            },
            deny_subprocess: false,
        })
    }

    #[tokio::test]
    async fn mock_tier_1_denies_egress() {
        let provider = MockSandboxProvider::new();
        let config = SandboxConfig {
            sandbox_policy: Some(tier1_policy()),
            ..Default::default()
        };
        let handle = provider.create(&config).await.unwrap();
        let err = provider
            .execute(&handle, "curl https://example.com", &HashMap::new())
            .await
            .unwrap_err();
        assert!(
            matches!(err, SandboxError::PolicyViolation { ref reason } if reason == "egress-denied")
        );
    }

    #[tokio::test]
    async fn mock_tier_1_allows_non_egress() {
        let provider = MockSandboxProvider::new();
        let config = SandboxConfig {
            sandbox_policy: Some(tier1_policy()),
            ..Default::default()
        };
        let handle = provider.create(&config).await.unwrap();
        let result = provider
            .execute(&handle, "echo hello", &HashMap::new())
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn mock_tier_3_allows_egress() {
        let provider = MockSandboxProvider::new();
        let config = SandboxConfig {
            sandbox_policy: Some(tier3_policy()),
            ..Default::default()
        };
        let handle = provider.create(&config).await.unwrap();
        let result = provider
            .execute(&handle, "curl https://example.com", &HashMap::new())
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn mock_tier_1_denies_subprocess() {
        use sera_tools::sandbox::{
            DockerSandboxPolicy, FileSystemSandboxPolicy, NetworkSandboxPolicy,
        };
        let provider = MockSandboxProvider::new();
        let config = SandboxConfig {
            sandbox_policy: Some(SandboxPolicy::Docker(DockerSandboxPolicy {
                filesystem: FileSystemSandboxPolicy {
                    read_paths: vec![],
                    write_paths: vec![],
                    include_workdir: false,
                },
                network: NetworkSandboxPolicy {
                    rules: vec![],
                    default_deny: true,
                },
                deny_subprocess: true,
            })),
            ..Default::default()
        };
        let handle = provider.create(&config).await.unwrap();
        let err = provider
            .execute(&handle, "bash -c ls", &HashMap::new())
            .await
            .unwrap_err();
        assert!(
            matches!(err, SandboxError::PolicyViolation { ref reason } if reason == "subprocess-denied")
        );
    }
}
