//! File operation tools: file-read, file-write, file-list.
//!
//! Native `Tool` trait implementations (bead sera-ttrm-5). Replaced the
//! legacy `ToolExecutor`-via-adapter wiring with direct `Tool` impls and
//! per-tool `RiskLevel` assignments:
//! - `FileRead` / `FileList`: [`RiskLevel::Read`]
//! - `FileWrite`: [`RiskLevel::Write`]

use std::collections::HashMap;

use async_trait::async_trait;
use sera_types::tool::{
    ExecutionTarget, FunctionParameters, ParameterSchema, RiskLevel, Tool, ToolContext, ToolError,
    ToolInput, ToolMetadata, ToolOutput, ToolSchema,
};

use crate::tools::file_write::locked_write;

// ── FileRead ────────────────────────────────────────────────────────────────

/// Read file contents.
pub struct FileRead;

#[async_trait]
impl Tool for FileRead {
    fn metadata(&self) -> ToolMetadata {
        ToolMetadata {
            name: "file-read".to_string(),
            description: "Read the contents of a file".to_string(),
            version: "1.0.0".to_string(),
            author: None,
            risk_level: RiskLevel::Read,
            execution_target: ExecutionTarget::Local,
            tags: vec!["fs".to_string()],
        }
    }

    fn schema(&self) -> ToolSchema {
        let mut properties: HashMap<String, ParameterSchema> = HashMap::new();
        properties.insert(
            "path".to_string(),
            ParameterSchema {
                schema_type: "string".to_string(),
                description: Some("File path to read".to_string()),
                enum_values: None,
                default: None,
            },
        );
        ToolSchema {
            parameters: FunctionParameters {
                schema_type: "object".to_string(),
                properties,
                required: vec!["path".to_string()],
            },
        }
    }

    async fn execute(
        &self,
        input: ToolInput,
        _ctx: ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let path = input.arguments["path"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidInput("Missing 'path'".to_string()))?;
        match tokio::fs::read_to_string(path).await {
            Ok(content) => Ok(ToolOutput::success(content)),
            Err(e) => Err(ToolError::ExecutionFailed(format!(
                "Error reading {path}: {e}"
            ))),
        }
    }
}

// ── FileWrite ───────────────────────────────────────────────────────────────

/// Write content to a file.
pub struct FileWrite;

#[async_trait]
impl Tool for FileWrite {
    fn metadata(&self) -> ToolMetadata {
        ToolMetadata {
            name: "file-write".to_string(),
            description: "Write content to a file".to_string(),
            version: "1.0.0".to_string(),
            author: None,
            risk_level: RiskLevel::Write,
            execution_target: ExecutionTarget::Local,
            tags: vec!["fs".to_string()],
        }
    }

    fn schema(&self) -> ToolSchema {
        let mut properties: HashMap<String, ParameterSchema> = HashMap::new();
        properties.insert(
            "path".to_string(),
            ParameterSchema {
                schema_type: "string".to_string(),
                description: Some("File path to write".to_string()),
                enum_values: None,
                default: None,
            },
        );
        properties.insert(
            "content".to_string(),
            ParameterSchema {
                schema_type: "string".to_string(),
                description: Some("Content to write".to_string()),
                enum_values: None,
                default: None,
            },
        );
        ToolSchema {
            parameters: FunctionParameters {
                schema_type: "object".to_string(),
                properties,
                required: vec!["path".to_string(), "content".to_string()],
            },
        }
    }

    async fn execute(
        &self,
        input: ToolInput,
        _ctx: ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let path = input.arguments["path"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidInput("Missing 'path'".to_string()))?;
        let content = input.arguments["content"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidInput("Missing 'content'".to_string()))?;

        // Ensure parent directory exists
        if let Some(parent) = std::path::Path::new(path).parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| ToolError::ExecutionFailed(format!("create_dir_all failed: {e}")))?;
        }

        let byte_len = content.len();
        locked_write(std::path::Path::new(path), content.as_bytes().to_vec())
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        Ok(ToolOutput::success(format!(
            "Written {byte_len} bytes to {path}"
        )))
    }
}

// ── FileList ────────────────────────────────────────────────────────────────

/// List directory contents.
pub struct FileList;

#[async_trait]
impl Tool for FileList {
    fn metadata(&self) -> ToolMetadata {
        ToolMetadata {
            name: "file-list".to_string(),
            description: "List files in a directory".to_string(),
            version: "1.0.0".to_string(),
            author: None,
            risk_level: RiskLevel::Read,
            execution_target: ExecutionTarget::Local,
            tags: vec!["fs".to_string()],
        }
    }

    fn schema(&self) -> ToolSchema {
        let mut properties: HashMap<String, ParameterSchema> = HashMap::new();
        properties.insert(
            "path".to_string(),
            ParameterSchema {
                schema_type: "string".to_string(),
                description: Some("Directory path to list".to_string()),
                enum_values: None,
                default: None,
            },
        );
        ToolSchema {
            parameters: FunctionParameters {
                schema_type: "object".to_string(),
                properties,
                required: vec!["path".to_string()],
            },
        }
    }

    async fn execute(
        &self,
        input: ToolInput,
        _ctx: ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let path = input.arguments["path"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidInput("Missing 'path'".to_string()))?;
        let mut entries = tokio::fs::read_dir(path)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("read_dir failed: {e}")))?;
        let mut result = Vec::new();

        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("next_entry failed: {e}")))?
        {
            let file_type = entry
                .file_type()
                .await
                .map_err(|e| ToolError::ExecutionFailed(format!("file_type failed: {e}")))?;
            let prefix = if file_type.is_dir() { "d " } else { "f " };
            result.push(format!("{prefix}{}", entry.file_name().to_string_lossy()));
        }

        Ok(ToolOutput::success(result.join("\n")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk_input(args: serde_json::Value) -> ToolInput {
        ToolInput {
            name: "file-read".to_string(),
            arguments: args,
            call_id: "test".to_string(),
        }
    }

    #[tokio::test]
    async fn file_read_nonexistent_path_returns_err() {
        let tool = FileRead;
        let result = tool
            .execute(
                mk_input(serde_json::json!({
                    "path": "/nonexistent/path/that/does/not/exist.txt"
                })),
                ToolContext::default(),
            )
            .await;
        assert!(result.is_err(), "expected Err for nonexistent file, got Ok");
        match result.unwrap_err() {
            ToolError::ExecutionFailed(msg) => {
                assert!(msg.contains("Error reading"), "unexpected error message: {msg}")
            }
            other => panic!("expected ExecutionFailed, got {other:?}"),
        }
    }

    #[test]
    fn metadata_risk_levels() {
        assert_eq!(FileRead.metadata().risk_level, RiskLevel::Read);
        assert_eq!(FileWrite.metadata().risk_level, RiskLevel::Write);
        assert_eq!(FileList.metadata().risk_level, RiskLevel::Read);
    }
}
