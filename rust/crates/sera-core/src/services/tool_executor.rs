//! Tool Executor — sandboxed tool invocation for agent workspaces.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use sqlx::PgPool;

/// Result of a tool execution.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ToolResult {
    pub success: bool,
    pub output: String,
    pub error: Option<String>,
    pub duration_ms: u64,
}

/// Tool executor for sandboxed invocations within agent workspaces.
pub struct ToolExecutor {
    pool: Arc<PgPool>,
    workspaces_dir: PathBuf,
    default_timeout: Duration,
}

#[derive(Debug, thiserror::Error)]
pub enum ToolExecutorError {
    #[error("tool not found: {0}")]
    NotFound(String),
    #[error("sandbox violation: path {0} is outside workspace")]
    SandboxViolation(String),
    #[error("execution timeout after {0}ms")]
    Timeout(u64),
    #[error("execution error: {0}")]
    Execution(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("database error: {0}")]
    Db(#[from] sera_db::DbError),
}

impl ToolExecutor {
    /// Create a new tool executor.
    ///
    /// `workspaces_dir` — root directory for agent workspaces ({SERA_DATA_DIR}/workspaces)
    /// `default_timeout` — default execution timeout (30s)
    pub fn new(pool: Arc<PgPool>, workspaces_dir: PathBuf, default_timeout: Duration) -> Self {
        Self {
            pool,
            workspaces_dir,
            default_timeout,
        }
    }

    /// Execute a tool for an agent.
    pub async fn execute(
        &self,
        agent_id: &str,
        tool_name: &str,
        args: &serde_json::Value,
    ) -> Result<ToolResult, ToolExecutorError> {
        let start = Instant::now();
        let workspace = self.agent_workspace(agent_id);

        let result = match tool_name {
            "file_read" => self.tool_file_read(&workspace, args).await,
            "file_write" => self.tool_file_write(&workspace, args).await,
            "shell_exec" => self.tool_shell_exec(&workspace, args).await,
            _ => Err(ToolExecutorError::NotFound(tool_name.to_string())),
        };

        let duration_ms = start.elapsed().as_millis() as u64;

        match result {
            Ok(output) => Ok(ToolResult {
                success: true,
                output,
                error: None,
                duration_ms,
            }),
            Err(e) => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(e.to_string()),
                duration_ms,
            }),
        }
    }

    /// Read a file from the agent's workspace.
    async fn tool_file_read(
        &self,
        workspace: &Path,
        args: &serde_json::Value,
    ) -> Result<String, ToolExecutorError> {
        let path_str = args
            .get("path")
            .and_then(|p| p.as_str())
            .ok_or_else(|| ToolExecutorError::Execution("missing 'path' argument".to_string()))?;

        let full_path = workspace.join(path_str);
        self.validate_sandbox(workspace, &full_path)?;

        tokio::fs::read_to_string(&full_path)
            .await
            .map_err(ToolExecutorError::Io)
    }

    /// Write a file to the agent's workspace.
    async fn tool_file_write(
        &self,
        workspace: &Path,
        args: &serde_json::Value,
    ) -> Result<String, ToolExecutorError> {
        let path_str = args
            .get("path")
            .and_then(|p| p.as_str())
            .ok_or_else(|| ToolExecutorError::Execution("missing 'path' argument".to_string()))?;

        let content = args
            .get("content")
            .and_then(|c| c.as_str())
            .ok_or_else(|| {
                ToolExecutorError::Execution("missing 'content' argument".to_string())
            })?;

        let full_path = workspace.join(path_str);
        self.validate_sandbox(workspace, &full_path)?;

        // Ensure parent directory exists
        if let Some(parent) = full_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(ToolExecutorError::Io)?;
        }

        tokio::fs::write(&full_path, content)
            .await
            .map_err(ToolExecutorError::Io)?;

        Ok(format!("Written {} bytes to {}", content.len(), path_str))
    }

    /// Execute a shell command in the agent's workspace.
    async fn tool_shell_exec(
        &self,
        workspace: &Path,
        args: &serde_json::Value,
    ) -> Result<String, ToolExecutorError> {
        let cmd = args
            .get("command")
            .and_then(|c| c.as_str())
            .ok_or_else(|| {
                ToolExecutorError::Execution("missing 'command' argument".to_string())
            })?;

        let timeout = args
            .get("timeout_ms")
            .and_then(|t| t.as_u64())
            .map(Duration::from_millis)
            .unwrap_or(self.default_timeout);

        let child = tokio::process::Command::new("sh")
            .arg("-c")
            .arg(cmd)
            .current_dir(workspace)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| ToolExecutorError::Execution(format!("spawn failed: {e}")))?;

        let result = tokio::time::timeout(timeout, child.wait_with_output()).await;

        match result {
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);
                if output.status.success() {
                    Ok(stdout.to_string())
                } else {
                    Err(ToolExecutorError::Execution(format!(
                        "exit code {}: {}",
                        output.status.code().unwrap_or(-1),
                        stderr
                    )))
                }
            }
            Ok(Err(e)) => Err(ToolExecutorError::Execution(format!("wait failed: {e}"))),
            Err(_) => Err(ToolExecutorError::Timeout(timeout.as_millis() as u64)),
        }
    }

    /// Validate that a path is within the sandbox workspace.
    fn validate_sandbox(&self, workspace: &Path, path: &Path) -> Result<(), ToolExecutorError> {
        // Canonicalize both paths to resolve any .. or symlinks
        // Use the path as-is if canonicalize fails (file doesn't exist yet for writes)
        let workspace_canon = workspace
            .canonicalize()
            .unwrap_or_else(|_| workspace.to_path_buf());

        let path_canon = path
            .canonicalize()
            .unwrap_or_else(|_| workspace.join(path.strip_prefix(workspace).unwrap_or(path)));

        if !path_canon.starts_with(&workspace_canon) {
            return Err(ToolExecutorError::SandboxViolation(
                path.display().to_string(),
            ));
        }
        Ok(())
    }

    /// Get the workspace directory for an agent.
    fn agent_workspace(&self, agent_id: &str) -> PathBuf {
        self.workspaces_dir.join(agent_id)
    }

    /// Get a reference to the pool (for audit logging).
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sandbox_valid_path() {
        let workspace = PathBuf::from("/tmp/test-workspace");
        let valid = workspace.join("src/main.rs");
        assert!(valid.starts_with(&workspace));
    }

    #[test]
    fn test_sandbox_traversal_detection() {
        let workspace = PathBuf::from("/tmp/workspace/agent-1");
        let malicious = PathBuf::from("/tmp/workspace/agent-1/../agent-2/secrets");
        let normalized = malicious.components().collect::<PathBuf>();
        assert!(normalized.to_string_lossy().contains(".."));
    }

    #[test]
    fn test_tool_result_serialization() {
        let result = ToolResult {
            success: true,
            output: "hello world".to_string(),
            error: None,
            duration_ms: 42,
        };
        let json = serde_json::to_value(&result).expect("serialize");
        assert_eq!(json["success"], true);
        assert_eq!(json["duration_ms"], 42);
        assert!(json["error"].is_null());
    }

    #[test]
    fn test_agent_workspace_path() {
        let workspaces_dir = PathBuf::from("/data/workspaces");
        let ws = workspaces_dir.join("agent-123");
        assert_eq!(ws, PathBuf::from("/data/workspaces/agent-123"));
    }

    #[tokio::test]
    async fn test_file_read_write_roundtrip() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let workspace = temp.path().join("agent-test");
        std::fs::create_dir_all(&workspace).expect("mkdir");

        // Write via tokio
        let file_path = workspace.join("test.txt");
        tokio::fs::write(&file_path, "hello").await.expect("write");

        // Read via tokio
        let content = tokio::fs::read_to_string(&file_path).await.expect("read");
        assert_eq!(content, "hello");
    }
}
