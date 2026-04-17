//! Content search tool for finding patterns in files.
//!
//! Native `Tool` trait implementation (bead sera-ttrm-5) with
//! [`RiskLevel::Read`] — grep is pure file reading.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use async_trait::async_trait;
use sera_types::tool::{
    ExecutionTarget, FunctionParameters, ParameterSchema, RiskLevel, Tool, ToolContext, ToolError,
    ToolInput, ToolMetadata, ToolOutput, ToolSchema,
};

pub struct Grep;

#[async_trait]
impl Tool for Grep {
    fn metadata(&self) -> ToolMetadata {
        ToolMetadata {
            name: "grep".to_string(),
            description: "Search for a pattern in files and return matching lines".to_string(),
            version: "1.0.0".to_string(),
            author: None,
            risk_level: RiskLevel::Read,
            execution_target: ExecutionTarget::Local,
            tags: vec!["fs".to_string(), "search".to_string()],
        }
    }

    fn schema(&self) -> ToolSchema {
        let mut properties: HashMap<String, ParameterSchema> = HashMap::new();
        properties.insert(
            "pattern".to_string(),
            ParameterSchema {
                schema_type: "string".to_string(),
                description: Some("Pattern to search for (string or regex)".to_string()),
                enum_values: None,
                default: None,
            },
        );
        properties.insert(
            "path".to_string(),
            ParameterSchema {
                schema_type: "string".to_string(),
                description: Some(
                    "File or directory path (default: current directory)".to_string(),
                ),
                enum_values: None,
                default: None,
            },
        );
        properties.insert(
            "include".to_string(),
            ParameterSchema {
                schema_type: "string".to_string(),
                description: Some("File extension filter (e.g., .rs)".to_string()),
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
        let path = args["path"].as_str().unwrap_or(".");
        let include = args["include"].as_str();

        let mut results = Vec::new();

        search_recursive(path, pattern, include, &mut results)
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        if results.is_empty() {
            Ok(ToolOutput::success("No matches found".to_string()))
        } else {
            results.sort();
            Ok(ToolOutput::success(results.join("\n")))
        }
    }
}

fn search_recursive(
    dir: &str,
    pattern: &str,
    include: Option<&str>,
    results: &mut Vec<String>,
) -> anyhow::Result<()> {
    let path = Path::new(dir);

    if path.is_file() {
        if let Some(ext) = include {
            if !path.to_string_lossy().ends_with(ext) {
                return Ok(());
            }
            search_file(dir, pattern, results)?;
        } else {
            search_file(dir, pattern, results)?;
        }
        return Ok(());
    }

    if !path.is_dir() {
        return Err(anyhow::anyhow!("Path not found: {}", dir));
    }

    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let entry_path = entry.path();

        if entry_path.is_dir() {
            let _ = search_recursive(entry_path.to_str().unwrap_or(""), pattern, include, results);
        } else if let Some(ext) = include {
            if entry_path.to_string_lossy().ends_with(ext) {
                search_file(entry_path.to_str().unwrap_or(""), pattern, results)?;
            }
        } else {
            search_file(entry_path.to_str().unwrap_or(""), pattern, results)?;
        }
    }

    Ok(())
}

fn search_file(file_path: &str, pattern: &str, results: &mut Vec<String>) -> anyhow::Result<()> {
    match fs::read_to_string(file_path) {
        Ok(content) => {
            for (line_num, line) in content.lines().enumerate() {
                if line.contains(pattern) {
                    results.push(format!("{}:{}: {}", file_path, line_num + 1, line));
                }
            }
            Ok(())
        }
        Err(_) => Ok(()), // Skip files that can't be read
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_risk_level_is_read() {
        assert_eq!(Grep.metadata().risk_level, RiskLevel::Read);
        assert_eq!(Grep.metadata().name, "grep");
    }
}
