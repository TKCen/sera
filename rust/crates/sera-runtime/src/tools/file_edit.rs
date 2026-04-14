//! Locked file-edit tool: read → apply patch → write under advisory lock.

use super::ToolExecutor;
use crate::tools::file_write::locked_write;

/// Edit a file by replacing an exact string occurrence with new content.
pub struct FileEdit;

#[async_trait::async_trait]
impl ToolExecutor for FileEdit {
    fn name(&self) -> &str {
        "file-edit"
    }

    fn description(&self) -> &str {
        "Replace an exact string in a file with new content (advisory-locked)"
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path":       { "type": "string", "description": "File path to edit" },
                "old_string": { "type": "string", "description": "Exact string to replace" },
                "new_string": { "type": "string", "description": "Replacement string" }
            },
            "required": ["path", "old_string", "new_string"]
        })
    }

    async fn execute(&self, args: &serde_json::Value) -> anyhow::Result<String> {
        let path = args["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'path'"))?;
        let old_string = args["old_string"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'old_string'"))?;
        let new_string = args["new_string"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'new_string'"))?;

        let current = tokio::fs::read_to_string(path)
            .await
            .map_err(|e| anyhow::anyhow!("Cannot read {path}: {e}"))?;

        if !current.contains(old_string) {
            return Err(anyhow::anyhow!(
                "old_string not found in {path}"
            ));
        }

        let patched = current.replacen(old_string, new_string, 1);
        let byte_len = patched.len();

        locked_write(std::path::Path::new(path), patched.into_bytes()).await?;

        Ok(format!("Edited {path}: replaced string ({byte_len} bytes written)"))
    }
}
