//! MVS (Minimum Viable SERA) built-in tools.
//!
//! Eight tools that every agent gets by default:
//! - memory_read, memory_write, memory_search, memory_synthesize
//! - file_read, file_write
//! - shell
//! - session_reset

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use regex::Regex;
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

    /// Get OpenAI function-calling tool definitions for all 8 MVS tools.
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
                "memory_synthesize",
                "Read multiple memory blocks matching a query and compile a summary",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "Search query to find relevant blocks" },
                        "max_blocks": { "type": "integer", "description": "Maximum blocks to include (default: 5)" }
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
            "memory_synthesize" => self.memory_synthesize(args),
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

        let bytes_written = match mode {
            "append" => {
                use std::io::Write;
                let mut f = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&resolved)
                    .map_err(|e| format!("Failed to open for append: {e}"))?;
                f.write_all(content.as_bytes())
                    .map_err(|e| format!("Failed to append: {e}"))?;
                content.len()
            }
            _ => {
                std::fs::write(&resolved, content)
                    .map_err(|e| format!("Failed to write memory file: {e}"))?;
                content.len()
            }
        };

        // Update cross-reference index
        let crossrefs = extract_crossrefs(content);
        update_crossrefs(&self.memory_dir(), path, &crossrefs)
            .unwrap_or_else(|e| tracing::warn!("Failed to update crossrefs: {e}"));

        // Update memory index
        update_memory_index(&self.memory_dir())
            .unwrap_or_else(|e| tracing::warn!("Failed to update memory index: {e}"));

        let verb = if mode == "append" { "Appended" } else { "Written" };
        Ok(format!("{verb} {bytes_written} bytes to {path}"))
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

        // Load crossrefs map to annotate results with incoming references
        let crossrefs_path = mem_dir.join("_crossrefs.json");
        let incoming_refs: HashMap<String, Vec<String>> = if crossrefs_path.exists() {
            // Build reverse map: target -> list of files that reference it
            if let Ok(raw) = std::fs::read_to_string(&crossrefs_path) {
                if let Ok(forward_map) = serde_json::from_str::<HashMap<String, Vec<String>>>(&raw) {
                    let mut reverse: HashMap<String, Vec<String>> = HashMap::new();
                    for (source_file, refs) in &forward_map {
                        for target in refs {
                            reverse
                                .entry(target.clone())
                                .or_default()
                                .push(source_file.clone());
                        }
                    }
                    reverse
                } else {
                    HashMap::new()
                }
            } else {
                HashMap::new()
            }
        } else {
            HashMap::new()
        };

        // Annotate results with incoming crossrefs
        let annotated: Vec<String> = results
            .into_iter()
            .map(|result| {
                // Extract filename from "File: <name>\n..." format
                if let Some(name_end) = result.find('\n') {
                    let file_line = &result[..name_end]; // "File: notes.md"
                    if let Some(fname) = file_line.strip_prefix("File: ") {
                        // Strip the .md extension to match crossref targets
                        let stem = fname.trim_end_matches(".md");
                        if let Some(refs) = incoming_refs.get(stem) {
                            if !refs.is_empty() {
                                return format!("{result}\nReferenced by: {}", refs.join(", "));
                            }
                        }
                    }
                }
                result
            })
            .collect();

        if annotated.is_empty() {
            Ok(format!("No matches found for \"{query}\"."))
        } else {
            Ok(annotated.join("\n---\n"))
        }
    }

    fn memory_synthesize(&self, args: &Value) -> Result<String, String> {
        let query = require_str(args, "query")?;
        let max_blocks = args
            .get("max_blocks")
            .and_then(|v| v.as_u64())
            .unwrap_or(5) as usize;

        let mem_dir = self.memory_dir();

        if !mem_dir.exists() {
            return Ok(format!("No matches found for \"{query}\". Memory directory does not exist."));
        }

        let query_lower = query.to_lowercase();
        let mut search_results = Vec::new();
        search_dir_recursive(&mem_dir, &mem_dir, &query_lower, &mut search_results)
            .map_err(|e| format!("Search error: {e}"))?;

        if search_results.is_empty() {
            return Ok(format!("No matches found for \"{query}\"."));
        }

        // Collect matching file paths (up to max_blocks)
        let mut file_contents: Vec<(String, String)> = Vec::new();
        for result in &search_results {
            if file_contents.len() >= max_blocks {
                break;
            }
            // Extract filename from "File: <name>\n..." format
            if let Some(name_end) = result.find('\n') {
                let file_line = &result[..name_end];
                if let Some(fname) = file_line.strip_prefix("File: ") {
                    let file_path = mem_dir.join(fname);
                    if let Ok(content) = std::fs::read_to_string(&file_path) {
                        file_contents.push((fname.to_string(), content));
                    }
                }
            }
        }

        if file_contents.is_empty() {
            return Ok(format!("No matches found for \"{query}\"."));
        }

        let count = file_contents.len();
        let mut output = format!("# Synthesis: \"{query}\"\n\nSources: {count} block{} found\n", if count == 1 { "" } else { "s" });

        for (fname, content) in &file_contents {
            output.push_str(&format!("\n## From {fname}\n{content}\n"));
        }

        output.push_str(&format!("\n---\nCompiled from {count} memory block{}.", if count == 1 { "" } else { "s" }));

        Ok(output)
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

/// Extract `[[wiki-link]]` style references from content.
/// Returns the inner text of each link found.
fn extract_crossrefs(content: &str) -> Vec<String> {
    let re = Regex::new(r"\[\[([^\[\]]+)\]\]").expect("valid crossref regex");
    re.captures_iter(content)
        .map(|cap| cap[1].to_string())
        .collect()
}

/// Update the `_crossrefs.json` file in `memory_dir` with the crossrefs for
/// `file_path` (relative path within memory dir).
fn update_crossrefs(memory_dir: &Path, file_path: &str, crossrefs: &[String]) -> Result<(), String> {
    let crossrefs_path = memory_dir.join("_crossrefs.json");

    let mut map: HashMap<String, Vec<String>> = if crossrefs_path.exists() {
        std::fs::read_to_string(&crossrefs_path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    } else {
        HashMap::new()
    };

    if crossrefs.is_empty() {
        map.remove(file_path);
    } else {
        map.insert(file_path.to_string(), crossrefs.to_vec());
    }

    let json = serde_json::to_string_pretty(&map)
        .map_err(|e| format!("Failed to serialize crossrefs: {e}"))?;
    std::fs::write(&crossrefs_path, json)
        .map_err(|e| format!("Failed to write _crossrefs.json: {e}"))?;

    Ok(())
}

/// Rebuild `_index.md` in `memory_dir` listing all `.md` files with metadata.
fn update_memory_index(memory_dir: &Path) -> Result<(), String> {
    let index_path = memory_dir.join("_index.md");

    // Collect all .md files excluding _index.md itself
    let mut entries: Vec<(String, String, u64, String)> = Vec::new(); // (filename, title, size, modified)

    let read_dir = std::fs::read_dir(memory_dir)
        .map_err(|e| format!("Failed to read memory dir: {e}"))?;

    for entry in read_dir {
        let entry = entry.map_err(|e| format!("Dir entry error: {e}"))?;
        let path = entry.path();

        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }

        let fname = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();

        if fname == "_index.md" {
            continue;
        }

        let content = std::fs::read_to_string(&path).unwrap_or_default();

        // Extract first # heading or use filename
        let title = content
            .lines()
            .find(|line| line.starts_with("# "))
            .map(|line| line.trim_start_matches("# ").to_string())
            .unwrap_or_else(|| fname.clone());

        let metadata = std::fs::metadata(&path).ok();
        let size = metadata.as_ref().map(|m| m.len()).unwrap_or(0);

        // Format modified date
        let modified = metadata
            .and_then(|m| m.modified().ok())
            .map(|t| {
                let secs = t
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                // Simple date formatting: YYYY-MM-DD from unix timestamp
                let days = secs / 86400;
                // Days since epoch to date (approximate, good enough for an index)
                let year = 1970 + days / 365;
                let day_of_year = days % 365;
                let month = (day_of_year / 30) + 1;
                let day = (day_of_year % 30) + 1;
                format!("{year:04}-{month:02}-{day:02}")
            })
            .unwrap_or_else(|| "unknown".to_string());

        let size_str = if size < 1024 {
            format!("{size}B")
        } else {
            format!("{:.1}KB", size as f64 / 1024.0)
        };

        entries.push((fname, title, size, size_str + &format!(" | {modified}")));
    }

    entries.sort_by(|a, b| a.0.cmp(&b.0));

    let mut index = String::from("# Memory Index\n\n| File | Title | Size | Modified |\n|------|-------|------|----------|\n");
    for (fname, title, _size, size_mod) in &entries {
        let parts: Vec<&str> = size_mod.splitn(2, " | ").collect();
        let size_str = parts.first().copied().unwrap_or("");
        let mod_str = parts.get(1).copied().unwrap_or("");
        index.push_str(&format!("| {fname} | {title} | {size_str} | {mod_str} |\n"));
    }

    std::fs::write(&index_path, index)
        .map_err(|e| format!("Failed to write _index.md: {e}"))?;

    Ok(())
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
/// Skips metadata files `_crossrefs.json` and `_index.md`.
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
            let fname = entry
                .file_name()
                .to_string_lossy()
                .to_string();
            // Skip metadata files
            if fname == "_crossrefs.json" || fname == "_index.md" {
                continue;
            }
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
    fn definitions_returns_eight_tools() {
        let (_tmp, reg) = make_registry();
        let defs = reg.definitions();
        assert_eq!(defs.len(), 8);

        let names: Vec<&str> = defs
            .iter()
            .map(|d| d["function"]["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"memory_read"));
        assert!(names.contains(&"memory_write"));
        assert!(names.contains(&"memory_search"));
        assert!(names.contains(&"memory_synthesize"));
        assert!(names.contains(&"file_read"));
        assert!(names.contains(&"file_write"));
        assert!(names.contains(&"shell"));
        assert!(names.contains(&"session_reset"));
    }

    #[test]
    fn extract_crossrefs_finds_links() {
        let content = "Some text [[link-one]] and [[link-two]] here. Also [[another]].";
        let refs = extract_crossrefs(content);
        assert_eq!(refs.len(), 3);
        assert!(refs.contains(&"link-one".to_string()));
        assert!(refs.contains(&"link-two".to_string()));
        assert!(refs.contains(&"another".to_string()));
    }

    #[test]
    fn extract_crossrefs_empty_when_no_links() {
        let content = "No links here, just plain text.";
        let refs = extract_crossrefs(content);
        assert!(refs.is_empty());
    }

    #[tokio::test]
    async fn memory_write_creates_crossrefs_json() {
        let (tmp, reg) = make_registry();

        reg.execute(
            "memory_write",
            &json!({"path": "notes.md", "content": "See [[project]] and [[tasks]]"}),
        )
        .await
        .unwrap();

        let crossrefs_path = tmp.path().join("memory").join("_crossrefs.json");
        assert!(crossrefs_path.exists());

        let raw = std::fs::read_to_string(&crossrefs_path).unwrap();
        let map: HashMap<String, Vec<String>> = serde_json::from_str(&raw).unwrap();
        assert!(map.contains_key("notes.md"));
        let refs = &map["notes.md"];
        assert!(refs.contains(&"project".to_string()));
        assert!(refs.contains(&"tasks".to_string()));
    }

    #[tokio::test]
    async fn memory_write_no_crossrefs_no_entry() {
        let (tmp, reg) = make_registry();

        reg.execute(
            "memory_write",
            &json!({"path": "plain.md", "content": "No wiki links here"}),
        )
        .await
        .unwrap();

        let crossrefs_path = tmp.path().join("memory").join("_crossrefs.json");
        if crossrefs_path.exists() {
            let raw = std::fs::read_to_string(&crossrefs_path).unwrap();
            let map: HashMap<String, Vec<String>> = serde_json::from_str(&raw).unwrap();
            // File with no crossrefs should not have an entry
            assert!(!map.contains_key("plain.md"));
        }
        // If file doesn't exist at all, that's also fine
    }

    #[tokio::test]
    async fn memory_write_creates_index_md() {
        let (tmp, reg) = make_registry();

        reg.execute(
            "memory_write",
            &json!({"path": "notes.md", "content": "# Project Notes\nSome content"}),
        )
        .await
        .unwrap();

        let index_path = tmp.path().join("memory").join("_index.md");
        assert!(index_path.exists());

        let index = std::fs::read_to_string(&index_path).unwrap();
        assert!(index.contains("# Memory Index"));
        assert!(index.contains("notes.md"));
        assert!(index.contains("Project Notes"));
    }

    #[tokio::test]
    async fn memory_write_index_tracks_multiple_files() {
        let (tmp, reg) = make_registry();

        reg.execute(
            "memory_write",
            &json!({"path": "alpha.md", "content": "# Alpha\nContent A"}),
        )
        .await
        .unwrap();

        reg.execute(
            "memory_write",
            &json!({"path": "beta.md", "content": "# Beta\nContent B"}),
        )
        .await
        .unwrap();

        let index_path = tmp.path().join("memory").join("_index.md");
        let index = std::fs::read_to_string(&index_path).unwrap();
        assert!(index.contains("alpha.md"));
        assert!(index.contains("Alpha"));
        assert!(index.contains("beta.md"));
        assert!(index.contains("Beta"));
    }

    #[tokio::test]
    async fn memory_write_index_excludes_index_itself() {
        let (tmp, reg) = make_registry();

        reg.execute(
            "memory_write",
            &json!({"path": "notes.md", "content": "# Notes\nHello"}),
        )
        .await
        .unwrap();

        let index_path = tmp.path().join("memory").join("_index.md");
        let index = std::fs::read_to_string(&index_path).unwrap();
        // _index.md should not list itself
        let occurrences = index.matches("_index.md").count();
        assert_eq!(occurrences, 0, "_index.md should not list itself");
    }

    #[tokio::test]
    async fn memory_search_excludes_metadata_files() {
        let (_tmp, reg) = make_registry();

        // Write a file that references something
        reg.execute(
            "memory_write",
            &json!({"path": "notes.md", "content": "# Notes\nSee [[crossrefs]] for details"}),
        )
        .await
        .unwrap();

        // Search for "crossrefs" — should not return _crossrefs.json or _index.md as results
        let result = reg
            .execute("memory_search", &json!({"query": "crossrefs"}))
            .await
            .unwrap();

        assert!(!result.contains("_crossrefs.json"));
        assert!(!result.contains("_index.md"));
    }

    #[tokio::test]
    async fn memory_synthesize_compiles_matching_blocks() {
        let (_tmp, reg) = make_registry();

        reg.execute(
            "memory_write",
            &json!({"path": "rust.md", "content": "# Rust Notes\nRust is great for systems programming"}),
        )
        .await
        .unwrap();

        reg.execute(
            "memory_write",
            &json!({"path": "python.md", "content": "# Python Notes\nPython is great for scripting"}),
        )
        .await
        .unwrap();

        let result = reg
            .execute("memory_synthesize", &json!({"query": "rust"}))
            .await
            .unwrap();

        assert!(result.contains("# Synthesis:"));
        assert!(result.contains("rust.md"));
        assert!(result.contains("Rust is great"));
        assert!(result.contains("Compiled from"));
    }

    #[tokio::test]
    async fn memory_synthesize_respects_max_blocks() {
        let (_tmp, reg) = make_registry();

        for i in 0..5 {
            reg.execute(
                "memory_write",
                &json!({"path": format!("file{i}.md"), "content": format!("# File {i}\ncommon keyword here")}),
            )
            .await
            .unwrap();
        }

        let result = reg
            .execute("memory_synthesize", &json!({"query": "common keyword", "max_blocks": 2}))
            .await
            .unwrap();

        // Should contain exactly "2 blocks found" in summary
        assert!(result.contains("2 block"));
        assert!(result.contains("Compiled from 2"));
    }

    #[tokio::test]
    async fn memory_synthesize_no_matches() {
        let (_tmp, reg) = make_registry();

        reg.execute(
            "memory_write",
            &json!({"path": "notes.md", "content": "# Notes\nSome content"}),
        )
        .await
        .unwrap();

        let result = reg
            .execute("memory_synthesize", &json!({"query": "zzzzzzunlikely"}))
            .await
            .unwrap();

        assert!(result.contains("No matches found"));
    }

    #[tokio::test]
    async fn memory_synthesize_empty_memory() {
        let (_tmp, reg) = make_registry();

        let result = reg
            .execute("memory_synthesize", &json!({"query": "anything"}))
            .await
            .unwrap();

        assert!(result.contains("No matches found"));
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
