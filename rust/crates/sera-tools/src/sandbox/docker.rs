//! Docker sandbox provider — Phase 0 stub backed by bollard.
//!
//! Full ContainerManager migration happens in P0-5/P0-6.

#![cfg(feature = "docker")]

use std::collections::HashMap;

use async_trait::async_trait;
use bollard::Docker;

use sera_types::sandbox::SourceMount;

use super::{ExecResult, SandboxConfig, SandboxError, SandboxHandle, SandboxProvider};

/// Docker-backed sandbox provider.
pub struct DockerSandboxProvider {
    #[allow(dead_code)] // used in P0-5/P0-6 when full ContainerManager migration lands
    inner: Docker,
}

impl DockerSandboxProvider {
    /// Create a new provider connecting via local defaults (socket/named pipe).
    pub fn new() -> Result<Self, SandboxError> {
        let docker =
            Docker::connect_with_local_defaults().map_err(|e| SandboxError::CreateFailed {
                reason: e.to_string(),
            })?;
        Ok(Self { inner: docker })
    }

    /// Create from an existing bollard `Docker` client.
    pub fn from_client(docker: Docker) -> Self {
        Self { inner: docker }
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
        let image = config.image.clone().unwrap_or_else(|| "alpine".to_string());
        // Phase 0 stub — return a handle with the image name as placeholder ID.
        // Source bind-mounts that would be applied in the real implementation:
        // Self::build_source_binds(&config.sources) → ["host:container:ro", ...]
        let _source_binds = Self::build_source_binds(&config.sources);
        let id = format!("docker-stub-{}", image);
        Ok(SandboxHandle(id))
    }

    async fn execute(
        &self,
        handle: &SandboxHandle,
        command: &str,
        _env: &HashMap<String, String>,
    ) -> Result<ExecResult, SandboxError> {
        // Phase 0 stub
        Ok(ExecResult {
            exit_code: 0,
            stdout: format!("stub exec in {}: {}", handle.0, command),
            stderr: String::new(),
        })
    }

    async fn read_file(
        &self,
        _handle: &SandboxHandle,
        _path: &str,
    ) -> Result<Vec<u8>, SandboxError> {
        // Phase 0 stub
        Ok(Vec::new())
    }

    async fn write_file(
        &self,
        _handle: &SandboxHandle,
        _path: &str,
        _content: &[u8],
    ) -> Result<(), SandboxError> {
        // Phase 0 stub
        Ok(())
    }

    async fn destroy(&self, _handle: &SandboxHandle) -> Result<(), SandboxError> {
        // Phase 0 stub
        Ok(())
    }

    async fn status(&self, handle: &SandboxHandle) -> Result<String, SandboxError> {
        // Phase 0 stub
        Ok(format!("running:{}", handle.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
