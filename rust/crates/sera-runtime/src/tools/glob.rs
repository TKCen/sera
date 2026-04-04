//! Glob file pattern matching tool.

use super::ToolExecutor;
use std::fs;
use std::path::Path;

pub struct Glob;

#[async_trait::async_trait]
impl ToolExecutor for Glob {
    fn name(&self) -> &str { "glob" }
    fn description(&self) -> &str { "Find files matching a glob pattern" }
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": { "type": "string", "description": "Glob pattern (e.g., **/*.rs)" },
                "path": { "type": "string", "description": "Base directory path (default: current working directory)" }
            },
            "required": ["pattern"]
        })
    }

    async fn execute(&self, args: &serde_json::Value) -> anyhow::Result<String> {
        let pattern = args["pattern"].as_str().ok_or_else(|| anyhow::anyhow!("Missing 'pattern'"))?;
        let base_path = args["path"].as_str().unwrap_or(".");

        let mut results = Vec::new();
        glob_search(base_path, pattern, &mut results)?;

        if results.is_empty() {
            Ok("No files matched the pattern".to_string())
        } else {
            results.sort();
            Ok(results.join("\n"))
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
            return normalized_path.ends_with(suffix) || normalized_path.contains(&format!("/{}/", suffix));
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

    normalized_path == normalized_pattern || normalized_path.ends_with(&format!("/{}", normalized_pattern))
}
