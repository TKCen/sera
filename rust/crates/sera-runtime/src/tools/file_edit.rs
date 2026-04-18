//! Locked file-edit tool: read → apply patch → write under advisory lock.
//!
//! Native `Tool` trait implementation (bead sera-ttrm-5) with
//! [`RiskLevel::Write`].

use std::collections::HashMap;

use async_trait::async_trait;
use sera_types::tool::{
    ExecutionTarget, FunctionParameters, ParameterSchema, RiskLevel, Tool, ToolContext, ToolError,
    ToolInput, ToolMetadata, ToolOutput, ToolSchema,
};

use crate::tools::file_write::locked_write;

/// Edit a file by replacing an exact string occurrence with new content.
pub struct FileEdit;

#[async_trait]
impl Tool for FileEdit {
    fn metadata(&self) -> ToolMetadata {
        ToolMetadata {
            name: "file-edit".to_string(),
            description: "Replace an exact string in a file with new content (advisory-locked)"
                .to_string(),
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
                description: Some("File path to edit".to_string()),
                enum_values: None,
                default: None,
            },
        );
        properties.insert(
            "old_string".to_string(),
            ParameterSchema {
                schema_type: "string".to_string(),
                description: Some("Exact string to replace".to_string()),
                enum_values: None,
                default: None,
            },
        );
        properties.insert(
            "new_string".to_string(),
            ParameterSchema {
                schema_type: "string".to_string(),
                description: Some("Replacement string".to_string()),
                enum_values: None,
                default: None,
            },
        );
        ToolSchema {
            parameters: FunctionParameters {
                schema_type: "object".to_string(),
                properties,
                required: vec![
                    "path".to_string(),
                    "old_string".to_string(),
                    "new_string".to_string(),
                ],
            },
        }
    }

    async fn execute(
        &self,
        input: ToolInput,
        _ctx: ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let args = &input.arguments;
        let path = args["path"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidInput("Missing 'path'".to_string()))?;
        let old_string = args["old_string"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidInput("Missing 'old_string'".to_string()))?;
        let new_string = args["new_string"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidInput("Missing 'new_string'".to_string()))?;

        let current = tokio::fs::read_to_string(path)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("Cannot read {path}: {e}")))?;

        if !current.contains(old_string) {
            return Err(ToolError::ExecutionFailed(format!(
                "old_string not found in {path}"
            )));
        }

        let patched = current.replacen(old_string, new_string, 1);
        let byte_len = patched.len();

        locked_write(std::path::Path::new(path), patched.into_bytes())
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        Ok(ToolOutput::success(format!(
            "Edited {path}: replaced string ({byte_len} bytes written)"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_risk_level_is_write() {
        assert_eq!(FileEdit.metadata().risk_level, RiskLevel::Write);
        assert_eq!(FileEdit.metadata().name, "file-edit");
    }
}
