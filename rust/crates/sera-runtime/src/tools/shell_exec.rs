//! Shell command execution tool.
//!
//! Native `Tool` trait implementation (bead sera-ttrm-5) with
//! [`RiskLevel::Execute`] — this tool runs arbitrary shell commands.

use std::collections::HashMap;

use async_trait::async_trait;
use sera_types::tool::{
    ExecutionTarget, FunctionParameters, ParameterSchema, RiskLevel, Tool, ToolContext, ToolError,
    ToolInput, ToolMetadata, ToolOutput, ToolSchema,
};
use tokio::process::Command;

pub struct ShellExec;

#[async_trait]
impl Tool for ShellExec {
    fn metadata(&self) -> ToolMetadata {
        ToolMetadata {
            name: "shell-exec".to_string(),
            description: "Execute a shell command and return its output".to_string(),
            version: "1.0.0".to_string(),
            author: None,
            risk_level: RiskLevel::Execute,
            execution_target: ExecutionTarget::Local,
            tags: vec!["shell".to_string()],
        }
    }

    fn schema(&self) -> ToolSchema {
        let mut properties: HashMap<String, ParameterSchema> = HashMap::new();
        properties.insert(
            "command".to_string(),
            ParameterSchema {
                schema_type: "string".to_string(),
                description: Some("Shell command to execute".to_string()),
                enum_values: None,
                default: None,
            },
        );
        properties.insert(
            "working_dir".to_string(),
            ParameterSchema {
                schema_type: "string".to_string(),
                description: Some("Working directory (optional)".to_string()),
                enum_values: None,
                default: None,
            },
        );
        properties.insert(
            "timeout_ms".to_string(),
            ParameterSchema {
                schema_type: "integer".to_string(),
                description: Some("Timeout in milliseconds (default 30000)".to_string()),
                enum_values: None,
                default: None,
            },
        );
        ToolSchema {
            parameters: FunctionParameters {
                schema_type: "object".to_string(),
                properties,
                required: vec!["command".to_string()],
            },
        }
    }

    async fn execute(
        &self,
        input: ToolInput,
        _ctx: ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let args = &input.arguments;
        let command = args["command"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidInput("Missing 'command'".to_string()))?;
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
        .map_err(|_| ToolError::Timeout)?
        .map_err(|e| ToolError::ExecutionFailed(format!("spawn failed: {e}")))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let exit_code = output.status.code().unwrap_or(-1);

        Ok(ToolOutput::success(format!(
            "exit_code: {exit_code}\nstdout:\n{stdout}\nstderr:\n{stderr}"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_risk_level_is_execute() {
        assert_eq!(ShellExec.metadata().risk_level, RiskLevel::Execute);
        assert_eq!(ShellExec.metadata().name, "shell-exec");
    }
}
