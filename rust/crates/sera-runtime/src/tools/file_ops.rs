//! File operation tools: file-read, file-write, file-list.

use super::ToolExecutor;
use crate::tools::file_write::locked_write;

/// Read file contents.
pub struct FileRead;

#[async_trait::async_trait]
impl ToolExecutor for FileRead {
    fn name(&self) -> &str { "file-read" }
    fn description(&self) -> &str { "Read the contents of a file" }
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "File path to read" }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, args: &serde_json::Value) -> anyhow::Result<String> {
        let path = args["path"].as_str().ok_or_else(|| anyhow::anyhow!("Missing 'path'"))?;
        tokio::fs::read_to_string(path).await.map_err(|e| anyhow::anyhow!("Error reading {path}: {e}"))
    }
}

/// Write content to a file.
pub struct FileWrite;

#[async_trait::async_trait]
impl ToolExecutor for FileWrite {
    fn name(&self) -> &str { "file-write" }
    fn description(&self) -> &str { "Write content to a file" }
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "File path to write" },
                "content": { "type": "string", "description": "Content to write" }
            },
            "required": ["path", "content"]
        })
    }

    async fn execute(&self, args: &serde_json::Value) -> anyhow::Result<String> {
        let path = args["path"].as_str().ok_or_else(|| anyhow::anyhow!("Missing 'path'"))?;
        let content = args["content"].as_str().ok_or_else(|| anyhow::anyhow!("Missing 'content'"))?;

        // Ensure parent directory exists
        if let Some(parent) = std::path::Path::new(path).parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let byte_len = content.len();
        locked_write(std::path::Path::new(path), content.as_bytes().to_vec()).await?;
        Ok(format!("Written {byte_len} bytes to {path}"))
    }
}

/// List directory contents.
pub struct FileList;

#[async_trait::async_trait]
impl ToolExecutor for FileList {
    fn name(&self) -> &str { "file-list" }
    fn description(&self) -> &str { "List files in a directory" }
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Directory path to list" }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, args: &serde_json::Value) -> anyhow::Result<String> {
        let path = args["path"].as_str().ok_or_else(|| anyhow::anyhow!("Missing 'path'"))?;
        let mut entries = tokio::fs::read_dir(path).await?;
        let mut result = Vec::new();

        while let Some(entry) = entries.next_entry().await? {
            let file_type = entry.file_type().await?;
            let prefix = if file_type.is_dir() { "d " } else { "f " };
            result.push(format!("{prefix}{}", entry.file_name().to_string_lossy()));
        }

        Ok(result.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::ToolExecutor;

    #[tokio::test]
    async fn file_read_nonexistent_path_returns_err() {
        let tool = FileRead;
        let args = serde_json::json!({ "path": "/nonexistent/path/that/does/not/exist.txt" });
        let result = tool.execute(&args).await;
        assert!(result.is_err(), "expected Err for nonexistent file, got Ok");
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("Error reading"), "unexpected error message: {msg}");
    }
}
