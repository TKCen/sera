//! MicroVM sandbox provider stub.

use std::collections::HashMap;

use async_trait::async_trait;

use super::{ExecResult, SandboxConfig, SandboxError, SandboxHandle, SandboxProvider};

/// Stub MicroVM sandbox provider — all methods return `NotImplemented`.
pub struct MicroVmSandboxProvider;

#[async_trait]
impl SandboxProvider for MicroVmSandboxProvider {
    fn name(&self) -> &str {
        "microvm"
    }

    async fn create(&self, _config: &SandboxConfig) -> Result<SandboxHandle, SandboxError> {
        Err(SandboxError::NotImplemented)
    }

    async fn execute(
        &self,
        _handle: &SandboxHandle,
        _command: &str,
        _env: &HashMap<String, String>,
    ) -> Result<ExecResult, SandboxError> {
        Err(SandboxError::NotImplemented)
    }

    async fn read_file(
        &self,
        _handle: &SandboxHandle,
        _path: &str,
    ) -> Result<Vec<u8>, SandboxError> {
        Err(SandboxError::NotImplemented)
    }

    async fn write_file(
        &self,
        _handle: &SandboxHandle,
        _path: &str,
        _content: &[u8],
    ) -> Result<(), SandboxError> {
        Err(SandboxError::NotImplemented)
    }

    async fn destroy(&self, _handle: &SandboxHandle) -> Result<(), SandboxError> {
        Err(SandboxError::NotImplemented)
    }

    async fn status(&self, _handle: &SandboxHandle) -> Result<String, SandboxError> {
        Err(SandboxError::NotImplemented)
    }
}
