//! Shell command execution tool.

use super::ToolExecutor;
use tokio::process::Command;

pub struct ShellExec;

#[async_trait::async_trait]
impl ToolExecutor for ShellExec {
    fn name(&self) -> &str { "shell-exec" }
    fn description(&self) -> &str { "Execute a shell command and return its output" }
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "command": { "type": "string", "description": "Shell command to execute" },
                "working_dir": { "type": "string", "description": "Working directory (optional)" },
                "timeout_ms": { "type": "integer", "description": "Timeout in milliseconds (default 30000)" }
            },
            "required": ["command"]
        })
    }

    async fn execute(&self, args: &serde_json::Value) -> anyhow::Result<String> {
        let command = args["command"].as_str().ok_or_else(|| anyhow::anyhow!("Missing 'command'"))?;
        let timeout_ms = args["timeout_ms"].as_u64().unwrap_or(30_000);

        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(command);

        if let Some(dir) = args["working_dir"].as_str() {
            cmd.current_dir(dir);
        }

        let output = tokio::time::timeout(
            std::time::Duration::from_millis(timeout_ms),
            cmd.output(),
        )
        .await
        .map_err(|_| anyhow::anyhow!("Command timed out after {timeout_ms}ms"))??;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let exit_code = output.status.code().unwrap_or(-1);

        Ok(format!("exit_code: {exit_code}\nstdout:\n{stdout}\nstderr:\n{stderr}"))
    }
}
