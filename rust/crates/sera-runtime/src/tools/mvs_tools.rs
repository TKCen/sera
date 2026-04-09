//! MVS (Minimum Viable SERA) built-in tools.
//!
//! Seven tools that every agent gets by default:
//! - memory_read, memory_write, memory_search
//! - file_read, file_write
//! - shell
//! - session_reset

use std::path::{Path, PathBuf};

use serde_json::Value;
use tokio::process::Command;

/// Registry of MVS built-in tools scoped to an agent workspace.
pub struct MvsToolRegistry {
    workspace: PathBuf,
}

impl MvsToolRegistry {
    pub fn new(workspace: &Path) -> Self {
        Self {
            workspace: workspace.to_path_buf(),
        }
    }

    /// Return the memory subdirectory within the workspace.
    fn memory_dir(&self) -> PathBuf {
        self.workspace.join("memory")
    }

    /// Validate that a resolved path stays within `base`. Returns the
    /// canonicalized path on success or an error message on escape attempt.
    fn safe_resolve(base: &Path, user_path: &str) -> Result<PathBuf, String> {
        // Ensure the base directory exists so we can canonicalize it.
        std::fs::create_dir_all(base)
            .map_err(|e| format!("Failed to create base directory: {e}"))?;

        let joined = base.join(user_path);

        // Try to canonicalize as much of the path as exists.
        let resolved = canonicalize_existing_prefix(&joined);

        // The resolved path must start with the canonicalized base directory.
        let canon_base = base
            .canonicalize()
            .map_err(|e| format!("Failed to canonicalize base: {e}"))?;

        if resolved.starts_with(&canon_base) {
            Ok(resolved)
        } else {
            Err(format!(
                "Path escapes workspace: {user_path} resolves to {}",
                resolved.display()
            ))
        }
    }

    /// Get OpenAI function-calling tool definitions for all 7 MVS tools.
    pub fn definitions(&self) -> Vec<Value> {
        vec![
            tool_def(
                "memory_read",
                "Read a specific memory file by path",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Relative path within the memory directory" }
                    },
                    "required": ["path"]
                }),
            ),
            tool_def(
                "memory_write",
                "Write or append to a memory file",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Relative path within the memory directory" },
                        "content": { "type": "string", "description": "Content to write" },
                        "mode": { "type": "string", "enum": ["write", "append"], "description": "Write mode (default: write)" }
                    },
                    "required": ["path", "content"]
                }),
            ),
            tool_def(
                "memory_search",
                "Keyword/heading search across memory files",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "Search query (case-insensitive substring match)" }
                    },
                    "required": ["query"]
                }),
            ),
            tool_def(
                "file_read",
                "Read a file from the agent workspace",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Relative path within the workspace" }
                    },
                    "required": ["path"]
                }),
            ),
            tool_def(
                "file_write",
                "Write a file to the agent workspace",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Relative path within the workspace" },
                        "content": { "type": "string", "description": "Content to write" }
                    },
                    "required": ["path", "content"]
                }),
            ),
            tool_def(
                "shell",
                "Execute a shell command in the agent workspace",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "command": { "type": "string", "description": "Shell command to execute" },
                        "timeout_ms": { "type": "integer", "description": "Timeout in milliseconds (default: 30000)" }
                    },
                    "required": ["command"]
                }),
            ),
            tool_def(
                "session_reset",
                "Archive current session and start fresh",
                serde_json::json!({
                    "type": "object",
                    "properties": {},
                    "required": []
                }),
            ),
        ]
    }

    /// Execute a tool by name with JSON arguments. Returns result string.
    pub async fn execute(&self, name: &str, args: &Value) -> Result<String, String> {
        match name {
            "memory_read" => self.memory_read(args),
            "memory_write" => self.memory_write(args),
            "memory_search" => self.memory_search(args),
            "file_read" => self.file_read(args),
            "file_write" => self.file_write(args),
            "shell" => self.shell(args).await,
            "session_reset" => self.session_reset(args),
            _ => Err(format!("Unknown MVS tool: {name}")),
        }
    }

    fn memory_read(&self, args: &Value) -> Result<String, String> {
        let path = require_str(args, "path")?;
        let resolved = Self::safe_resolve(&self.memory_dir(), path)?;
        std::fs::read_to_string(&resolved)
            .map_err(|e| format!("Failed to read memory file: {e}"))
    }

    fn memory_write(&self, args: &Value) -> Result<String, String> {
        let path = require_str(args, "path")?;
        let content = require_str(args, "content")?;
        let mode = args
            .get("mode")
            .and_then(|v| v.as_str())
            .unwrap_or("write");

        let resolved = Self::safe_resolve(&self.memory_dir(), path)?;

        // Ensure parent directory exists
        if let Some(parent) = resolved.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create directories: {e}"))?;
        }

        match mode {
            "append" => {
                use std::io::Write;
                let mut f = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&resolved)
                    .map_err(|e| format!("Failed to open for append: {e}"))?;
                f.write_all(content.as_bytes())
                    .map_err(|e| format!("Failed to append: {e}"))?;
                Ok(format!("Appended {} bytes to {path}", content.len()))
            }
            "write" | _ => {
                std::fs::write(&resolved, content)
                    .map_err(|e| format!("Failed to write memory file: {e}"))?;
                Ok(format!("Written {} bytes to {path}", content.len()))
            }
        }
    }

    fn memory_search(&self, args: &Value) -> Result<String, String> {
        let query = require_str(args, "query")?;
        let query_lower = query.to_lowercase();
        let mem_dir = self.memory_dir();

        if !mem_dir.exists() {
            return Ok("No memory directory found.".to_string());
        }

        let mut results = Vec::new();
        search_dir_recursive(&mem_dir, &mem_dir, &query_lower, &mut results)
            .map_err(|e| format!("Search error: {e}"))?;

        if results.is_empty() {
            Ok(format!("No matches found for \"{query}\"."))
        } else {
            Ok(results.join("\n---\n"))
        }
    }

    fn file_read(&self, args: &Value) -> Result<String, String> {
        let path = require_str(args, "path")?;
        let resolved = Self::safe_resolve(&self.workspace, path)?;
        std::fs::read_to_string(&resolved)
            .map_err(|e| format!("Failed to read file: {e}"))
    }

    fn file_write(&self, args: &Value) -> Result<String, String> {
        let path = require_str(args, "path")?;
        let content = require_str(args, "content")?;
        let resolved = Self::safe_resolve(&self.workspace, path)?;

        if let Some(parent) = resolved.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create directories: {e}"))?;
        }

        std::fs::write(&resolved, content)
            .map_err(|e| format!("Failed to write file: {e}"))?;
        Ok(format!("Written {} bytes to {path}", content.len()))
    }

    async fn shell(&self, args: &Value) -> Result<String, String> {
        let command = require_str(args, "command")?;
        let timeout_ms = args
            .get("timeout_ms")
            .and_then(|v| v.as_u64())
            .unwrap_or(30_000);

        let output = tokio::time::timeout(
            std::time::Duration::from_millis(timeout_ms),
            Command::new("sh")
                .arg("-c")
                .arg(command)
                .current_dir(&self.workspace)
                .output(),
        )
        .await
        .map_err(|_| format!("Command timed out after {timeout_ms}ms"))?
        .map_err(|e| format!("Failed to execute command: {e}"))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let exit_code = output.status.code().unwrap_or(-1);

        Ok(format!(
            "exit_code: {exit_code}\nstdout:\n{stdout}\nstderr:\n{stderr}"
        ))
    }

    fn session_reset(&self, _args: &Value) -> Result<String, String> {
        // Actual session state change happens at the gateway level.
        // The runtime returns a signal message that the gateway interprets.
        Ok("SESSION_RESET_REQUESTED: The gateway will archive the current session and start fresh.".to_string())
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn require_str<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args.get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| format!("Missing required argument: {key}"))
}

fn tool_def(name: &str, description: &str, parameters: Value) -> Value {
    serde_json::json!({
        "type": "function",
        "function": {
            "name": name,
            "description": description,
            "parameters": parameters,
        }
    })
}

/// Canonicalize as much of the path as exists, then append the non-existing
/// suffix. This allows safe_resolve to work for paths that don't exist yet
/// (e.g. when creating a new file).
fn canonicalize_existing_prefix(path: &Path) -> PathBuf {
    // Walk up until we find an existing ancestor, canonicalize it,
    // then re-append the remaining components.
    let mut existing = path.to_path_buf();
    let mut suffix_parts: Vec<std::ffi::OsString> = Vec::new();

    loop {
        if existing.exists() {
            match existing.canonicalize() {
                Ok(canon) => {
                    let mut result = canon;
                    for part in suffix_parts.iter().rev() {
                        result.push(part);
                    }
                    return result;
                }
                Err(_) => return path.to_path_buf(),
            }
        }

        match existing.file_name() {
            Some(name) => {
                suffix_parts.push(name.to_os_string());
                existing.pop();
            }
            None => return path.to_path_buf(),
        }
    }
}

/// Recursively search files in `dir` for lines matching `query_lower`.
fn search_dir_recursive(
    base: &Path,
    dir: &Path,
    query_lower: &str,
    results: &mut Vec<String>,
) -> Result<(), std::io::Error> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let ft = entry.file_type()?;
        if ft.is_dir() {
            search_dir_recursive(base, &entry.path(), query_lower, results)?;
        } else if ft.is_file() {
            if let Ok(content) = std::fs::read_to_string(entry.path()) {
                let matching_lines: Vec<&str> = content
                    .lines()
                    .filter(|line| line.to_lowercase().contains(query_lower))
                    .collect();
                if !matching_lines.is_empty() {
                    let rel = entry
                        .path()
                        .strip_prefix(base)
                        .unwrap_or(&entry.path())
                        .display()
                        .to_string();
                    results.push(format!(
                        "File: {rel}\n{}",
                        matching_lines.join("\n")
                    ));
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::TempDir;

    fn make_registry() -> (TempDir, MvsToolRegistry) {
        let tmp = TempDir::new().unwrap();
        let reg = MvsToolRegistry::new(tmp.path());
        (tmp, reg)
    }

    #[test]
    fn definitions_returns_seven_tools() {
        let (_tmp, reg) = make_registry();
        let defs = reg.definitions();
        assert_eq!(defs.len(), 7);

        let names: Vec<&str> = defs
            .iter()
            .map(|d| d["function"]["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"memory_read"));
        assert!(names.contains(&"memory_write"));
        assert!(names.contains(&"memory_search"));
        assert!(names.contains(&"file_read"));
        assert!(names.contains(&"file_write"));
        assert!(names.contains(&"shell"));
        assert!(names.contains(&"session_reset"));
    }

    #[tokio::test]
    async fn file_write_and_read() {
        let (_tmp, reg) = make_registry();

        let write_result = reg
            .execute(
                "file_write",
                &json!({"path": "hello.txt", "content": "Hello, world!"}),
            )
            .await;
        assert!(write_result.is_ok());
        assert!(write_result.unwrap().contains("13 bytes"));

        let read_result = reg
            .execute("file_read", &json!({"path": "hello.txt"}))
            .await;
        assert!(read_result.is_ok());
        assert_eq!(read_result.unwrap(), "Hello, world!");
    }

    #[tokio::test]
    async fn file_write_creates_subdirectories() {
        let (_tmp, reg) = make_registry();

        let result = reg
            .execute(
                "file_write",
                &json!({"path": "sub/dir/test.txt", "content": "nested"}),
            )
            .await;
        assert!(result.is_ok());

        let read = reg
            .execute("file_read", &json!({"path": "sub/dir/test.txt"}))
            .await;
        assert_eq!(read.unwrap(), "nested");
    }

    #[tokio::test]
    async fn file_read_missing_file() {
        let (_tmp, reg) = make_registry();
        let result = reg
            .execute("file_read", &json!({"path": "nonexistent.txt"}))
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Failed to read"));
    }

    #[tokio::test]
    async fn memory_write_and_read() {
        let (_tmp, reg) = make_registry();

        let w = reg
            .execute(
                "memory_write",
                &json!({"path": "notes.md", "content": "# Notes\nHello"}),
            )
            .await;
        assert!(w.is_ok());

        let r = reg
            .execute("memory_read", &json!({"path": "notes.md"}))
            .await;
        assert_eq!(r.unwrap(), "# Notes\nHello");
    }

    #[tokio::test]
    async fn memory_write_append_mode() {
        let (_tmp, reg) = make_registry();

        reg.execute(
            "memory_write",
            &json!({"path": "log.txt", "content": "line1\n"}),
        )
        .await
        .unwrap();

        reg.execute(
            "memory_write",
            &json!({"path": "log.txt", "content": "line2\n", "mode": "append"}),
        )
        .await
        .unwrap();

        let content = reg
            .execute("memory_read", &json!({"path": "log.txt"}))
            .await
            .unwrap();
        assert_eq!(content, "line1\nline2\n");
    }

    #[tokio::test]
    async fn memory_search_finds_matches() {
        let (_tmp, reg) = make_registry();

        reg.execute(
            "memory_write",
            &json!({"path": "a.md", "content": "# Project Alpha\nSome details"}),
        )
        .await
        .unwrap();
        reg.execute(
            "memory_write",
            &json!({"path": "b.md", "content": "# Project Beta\nOther info"}),
        )
        .await
        .unwrap();

        let result = reg
            .execute("memory_search", &json!({"query": "alpha"}))
            .await
            .unwrap();
        assert!(result.contains("a.md"));
        assert!(result.contains("Project Alpha"));
        assert!(!result.contains("b.md"));
    }

    #[tokio::test]
    async fn memory_search_no_matches() {
        let (_tmp, reg) = make_registry();

        reg.execute(
            "memory_write",
            &json!({"path": "x.md", "content": "nothing relevant"}),
        )
        .await
        .unwrap();

        let result = reg
            .execute("memory_search", &json!({"query": "zzzzz"}))
            .await
            .unwrap();
        assert!(result.contains("No matches"));
    }

    #[tokio::test]
    async fn memory_search_empty_dir() {
        let (_tmp, reg) = make_registry();
        let result = reg
            .execute("memory_search", &json!({"query": "anything"}))
            .await
            .unwrap();
        assert!(result.contains("No memory directory"));
    }

    #[tokio::test]
    async fn path_escape_rejected() {
        let (_tmp, reg) = make_registry();

        let result = reg
            .execute("file_read", &json!({"path": "../../../etc/passwd"}))
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("escapes workspace"));
    }

    #[tokio::test]
    async fn memory_path_escape_rejected() {
        let (_tmp, reg) = make_registry();

        let result = reg
            .execute("memory_read", &json!({"path": "../../etc/shadow"}))
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("escapes workspace"));
    }

    #[tokio::test]
    async fn shell_executes_command() {
        let (_tmp, reg) = make_registry();

        let result = reg
            .execute("shell", &json!({"command": "echo hello"}))
            .await
            .unwrap();
        assert!(result.contains("exit_code: 0"));
        assert!(result.contains("hello"));
    }

    #[tokio::test]
    async fn shell_returns_nonzero_exit() {
        let (_tmp, reg) = make_registry();

        let result = reg
            .execute("shell", &json!({"command": "exit 42"}))
            .await
            .unwrap();
        assert!(result.contains("exit_code: 42"));
    }

    #[tokio::test]
    async fn shell_respects_workspace_dir() {
        let (tmp, reg) = make_registry();

        // Create a file in the workspace
        std::fs::write(tmp.path().join("marker.txt"), "found").unwrap();

        let result = reg
            .execute("shell", &json!({"command": "cat marker.txt"}))
            .await
            .unwrap();
        assert!(result.contains("found"));
    }

    #[tokio::test]
    async fn session_reset_returns_signal() {
        let (_tmp, reg) = make_registry();

        let result = reg
            .execute("session_reset", &json!({}))
            .await
            .unwrap();
        assert!(result.contains("SESSION_RESET_REQUESTED"));
    }

    #[tokio::test]
    async fn unknown_tool_returns_error() {
        let (_tmp, reg) = make_registry();

        let result = reg
            .execute("nonexistent_tool", &json!({}))
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Unknown MVS tool"));
    }

    #[tokio::test]
    async fn missing_required_args() {
        let (_tmp, reg) = make_registry();

        // file_read without path
        let result = reg.execute("file_read", &json!({})).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Missing required argument"));

        // file_write without content
        let result = reg
            .execute("file_write", &json!({"path": "x.txt"}))
            .await;
        assert!(result.is_err());

        // shell without command
        let result = reg.execute("shell", &json!({})).await;
        assert!(result.is_err());
    }
}
