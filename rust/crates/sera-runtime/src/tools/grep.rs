//! Content search tool for finding patterns in files.

use super::ToolExecutor;
use std::fs;
use std::path::Path;

pub struct Grep;

#[async_trait::async_trait]
impl ToolExecutor for Grep {
    fn name(&self) -> &str { "grep" }
    fn description(&self) -> &str { "Search for a pattern in files and return matching lines" }
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": { "type": "string", "description": "Pattern to search for (string or regex)" },
                "path": { "type": "string", "description": "File or directory path (default: current directory)" },
                "include": { "type": "string", "description": "File extension filter (e.g., .rs)" }
            },
            "required": ["pattern"]
        })
    }

    async fn execute(&self, args: &serde_json::Value) -> anyhow::Result<String> {
        let pattern = args["pattern"].as_str().ok_or_else(|| anyhow::anyhow!("Missing 'pattern'"))?;
        let path = args["path"].as_str().unwrap_or(".");
        let include = args["include"].as_str();

        let mut results = Vec::new();

        search_recursive(path, pattern, include, &mut results)?;

        if results.is_empty() {
            Ok("No matches found".to_string())
        } else {
            results.sort();
            Ok(results.join("\n"))
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
