//! Glob file pattern matching tool.
//!
//! Native `Tool` trait implementation (bead sera-ttrm-5) with
//! [`RiskLevel::Read`] — glob is a pure directory traversal.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use async_trait::async_trait;
use sera_types::tool::{
    ExecutionTarget, FunctionParameters, ParameterSchema, RiskLevel, Tool, ToolContext, ToolError,
    ToolInput, ToolMetadata, ToolOutput, ToolSchema,
};

pub struct Glob;

#[async_trait]
impl Tool for Glob {
    fn metadata(&self) -> ToolMetadata {
        ToolMetadata {
            name: "glob".to_string(),
            description: "Find files matching a glob pattern".to_string(),
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
            "pattern".to_string(),
            ParameterSchema {
                schema_type: "string".to_string(),
                description: Some("Glob pattern (e.g., **/*.rs)".to_string()),
                enum_values: None,
                default: None,
            },
        );
        properties.insert(
            "path".to_string(),
            ParameterSchema {
                schema_type: "string".to_string(),
                description: Some(
                    "Base directory path (default: current working directory)".to_string(),
                ),
                enum_values: None,
                default: None,
            },
        );
        ToolSchema {
            parameters: FunctionParameters {
                schema_type: "object".to_string(),
                properties,
                required: vec!["pattern".to_string()],
            },
        }
    }

    async fn execute(
        &self,
        input: ToolInput,
        _ctx: ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let args = &input.arguments;
        let pattern = args["pattern"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidInput("Missing 'pattern'".to_string()))?;
        let base_path = args["path"].as_str().unwrap_or(".");

        let mut results = Vec::new();
        glob_search(base_path, pattern, &mut results)
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        if results.is_empty() {
            Ok(ToolOutput::success("No files matched the pattern".to_string()))
        } else {
            results.sort();
            Ok(ToolOutput::success(results.join("\n")))
        }
    }
}

fn glob_search(dir: &str, pattern: &str, results: &mut Vec<String>) -> anyhow::Result<()> {
    let path = Path::new(dir);

    if !path.exists() {
        return Err(anyhow::anyhow!("Path not found: {}", dir));
    }

    if path.is_file() {
        if matches_pattern(path.to_str().unwrap_or(""), pattern) {
            results.push(path.to_string_lossy().to_string());
        }
        return Ok(());
    }

    if !path.is_dir() {
        return Ok(());
    }

    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let entry_path = entry.path();

        if entry_path.is_dir() {
            let _ = glob_search(entry_path.to_str().unwrap_or(""), pattern, results);
        } else {
            let file_path = entry_path.to_string_lossy();
            if matches_pattern(&file_path, pattern) {
                results.push(file_path.to_string());
            }
        }
    }

    Ok(())
}

fn matches_pattern(file_path: &str, pattern: &str) -> bool {
    // Simple glob matching: supports * for any sequence and ** for recursive
    let normalized_pattern = pattern.replace("\\", "/");
    let normalized_path = file_path.replace("\\", "/");

    if normalized_pattern.contains("**") {
        // ** matches any number of directories
        let parts: Vec<&str> = normalized_pattern.split("**/").collect();
        if parts.len() > 1 {
            let suffix = parts[parts.len() - 1];
            return normalized_path.ends_with(suffix)
                || normalized_path.contains(&format!("/{}/", suffix));
        }
    }

    if normalized_pattern.contains('*') {
        // * matches anything except /
        let pattern_parts: Vec<&str> = normalized_pattern.split('*').collect();
        let mut idx = 0;

        for (i, part) in pattern_parts.iter().enumerate() {
            if part.is_empty() {
                continue;
            }
            if i == 0 {
                if !normalized_path.starts_with(part) {
                    return false;
                }
                idx += part.len();
            } else if i == pattern_parts.len() - 1 {
                if !normalized_path.ends_with(part) {
                    return false;
                }
            } else if let Some(pos) = normalized_path[idx..].find(part) {
                idx += pos + part.len();
            } else {
                return false;
            }
        }
        return true;
    }

    normalized_path == normalized_pattern
        || normalized_path.ends_with(&format!("/{}", normalized_pattern))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_risk_level_is_read() {
        assert_eq!(Glob.metadata().risk_level, RiskLevel::Read);
        assert_eq!(Glob.metadata().name, "glob");
    }
}
