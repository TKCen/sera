//! File-based memory backend for SERA agents.
//! Markdown files in agent workspace. Keyword/heading search.
//! No embeddings, no git management (POST-MVS).

use std::fs;
use std::path::{Path, PathBuf};

/// A file-based memory store rooted at an agent's workspace directory.
/// All operations are scoped to `.md` files within this workspace.
pub struct FileMemory {
    workspace: PathBuf,
}

/// A file that matched a keyword search, with individual line matches.
#[derive(Debug, Clone)]
pub struct SearchResult {
    /// Relative path from the workspace root.
    pub path: String,
    /// Individual line matches within the file.
    pub matches: Vec<SearchMatch>,
}

/// A single line that matched a keyword search.
#[derive(Debug, Clone)]
pub struct SearchMatch {
    /// 1-based line number.
    pub line_number: usize,
    /// The full text of the matching line.
    pub line: String,
    /// The nearest markdown heading above (or on) this line, if any.
    pub heading: Option<String>,
}

impl FileMemory {
    /// Create a new `FileMemory` rooted at `workspace`.
    /// The directory is created if it does not exist.
    pub fn new(workspace: &Path) -> Self {
        Self {
            workspace: workspace.to_path_buf(),
        }
    }

    /// Read the contents of a markdown file at `relative_path`.
    pub fn read(&self, relative_path: &str) -> std::io::Result<String> {
        let path = self.resolve(relative_path);
        fs::read_to_string(path)
    }

    /// Write `content` to a markdown file, creating parent directories as needed.
    /// Overwrites any existing file.
    pub fn write(&self, relative_path: &str, content: &str) -> std::io::Result<()> {
        let path = self.resolve(relative_path);
        self.ensure_parent_dir(&path)?;
        fs::write(path, content)
    }

    /// Append `content` to an existing file, or create it if it does not exist.
    pub fn append(&self, relative_path: &str, content: &str) -> std::io::Result<()> {
        let path = self.resolve(relative_path);
        self.ensure_parent_dir(&path)?;
        let mut existing = fs::read_to_string(&path).unwrap_or_default();
        existing.push_str(content);
        fs::write(path, existing)
    }

    /// List all `.md` files recursively under the workspace.
    /// Returns relative paths with forward slashes.
    pub fn list(&self) -> std::io::Result<Vec<String>> {
        let mut results = Vec::new();
        self.collect_md_files(&self.workspace, &mut results)?;
        results.sort();
        Ok(results)
    }

    /// Case-insensitive keyword search across all `.md` files.
    /// Returns files with matching lines and the nearest heading context.
    pub fn search(&self, query: &str) -> std::io::Result<Vec<SearchResult>> {
        let query_lower = query.to_lowercase();
        let files = self.list()?;
        let mut results = Vec::new();

        for file_path in &files {
            let content = self.read(file_path)?;
            let matches = self.search_in_content(&content, &query_lower);
            if !matches.is_empty() {
                results.push(SearchResult {
                    path: file_path.clone(),
                    matches,
                });
            }
        }

        Ok(results)
    }

    /// Delete a file at `relative_path`.
    pub fn delete(&self, relative_path: &str) -> std::io::Result<()> {
        let path = self.resolve(relative_path);
        fs::remove_file(path)
    }

    /// Check whether a file exists at `relative_path`.
    pub fn exists(&self, relative_path: &str) -> bool {
        self.resolve(relative_path).is_file()
    }

    // -- private helpers --

    fn resolve(&self, relative_path: &str) -> PathBuf {
        self.workspace.join(relative_path)
    }

    fn ensure_parent_dir(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        Ok(())
    }

    /// Recursively collect `.md` files, returning paths relative to the workspace.
    fn collect_md_files(&self, dir: &Path, out: &mut Vec<String>) -> std::io::Result<()> {
        if !dir.is_dir() {
            return Ok(());
        }
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                self.collect_md_files(&path, out)?;
            } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
                if let Ok(rel) = path.strip_prefix(&self.workspace) {
                    // Normalise to forward slashes for cross-platform consistency.
                    let rel_str = rel.to_string_lossy().replace('\\', "/");
                    out.push(rel_str);
                }
            }
        }
        Ok(())
    }

    /// Search file content for a case-insensitive keyword, tracking headings.
    fn search_in_content(&self, content: &str, query_lower: &str) -> Vec<SearchMatch> {
        let mut matches = Vec::new();
        let mut current_heading: Option<String> = None;

        for (idx, line) in content.lines().enumerate() {
            let trimmed = line.trim_start();
            if trimmed.starts_with('#') {
                // Extract heading text (strip leading '#' characters and whitespace).
                let heading_text = trimmed.trim_start_matches('#').trim();
                if !heading_text.is_empty() {
                    current_heading = Some(heading_text.to_string());
                }
            }

            if line.to_lowercase().contains(query_lower) {
                matches.push(SearchMatch {
                    line_number: idx + 1,
                    line: line.to_string(),
                    heading: current_heading.clone(),
                });
            }
        }

        matches
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup() -> (TempDir, FileMemory) {
        let dir = TempDir::new().expect("create temp dir");
        let mem = FileMemory::new(dir.path());
        (dir, mem)
    }

    #[test]
    fn write_and_read() {
        let (_dir, mem) = setup();
        mem.write("notes.md", "hello world").unwrap();
        let content = mem.read("notes.md").unwrap();
        assert_eq!(content, "hello world");
    }

    #[test]
    fn write_overwrites() {
        let (_dir, mem) = setup();
        mem.write("a.md", "first").unwrap();
        mem.write("a.md", "second").unwrap();
        assert_eq!(mem.read("a.md").unwrap(), "second");
    }

    #[test]
    fn append_creates_if_missing() {
        let (_dir, mem) = setup();
        mem.append("new.md", "line1\n").unwrap();
        assert_eq!(mem.read("new.md").unwrap(), "line1\n");
    }

    #[test]
    fn append_adds_to_existing() {
        let (_dir, mem) = setup();
        mem.write("log.md", "a\n").unwrap();
        mem.append("log.md", "b\n").unwrap();
        assert_eq!(mem.read("log.md").unwrap(), "a\nb\n");
    }

    #[test]
    fn read_nonexistent_returns_error() {
        let (_dir, mem) = setup();
        let result = mem.read("no-such-file.md");
        assert!(result.is_err());
    }

    #[test]
    fn list_empty_workspace() {
        let (_dir, mem) = setup();
        let files = mem.list().unwrap();
        assert!(files.is_empty());
    }

    #[test]
    fn list_flat_files() {
        let (_dir, mem) = setup();
        mem.write("b.md", "").unwrap();
        mem.write("a.md", "").unwrap();
        let files = mem.list().unwrap();
        assert_eq!(files, vec!["a.md", "b.md"]);
    }

    #[test]
    fn list_nested_directories() {
        let (_dir, mem) = setup();
        mem.write("top.md", "").unwrap();
        mem.write("sub/deep.md", "").unwrap();
        mem.write("sub/another.md", "").unwrap();
        let files = mem.list().unwrap();
        assert_eq!(
            files,
            vec!["sub/another.md", "sub/deep.md", "top.md"]
        );
    }

    #[test]
    fn list_ignores_non_md_files() {
        let (_dir, mem) = setup();
        mem.write("notes.md", "").unwrap();
        // Write a .txt file directly to the workspace.
        fs::write(mem.workspace.join("data.txt"), "text").unwrap();
        let files = mem.list().unwrap();
        assert_eq!(files, vec!["notes.md"]);
    }

    #[test]
    fn search_case_insensitive() {
        let (_dir, mem) = setup();
        mem.write("doc.md", "Hello World\nhello again\nno match here")
            .unwrap();
        let results = mem.search("HELLO").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].matches.len(), 2);
        assert_eq!(results[0].matches[0].line_number, 1);
        assert_eq!(results[0].matches[0].line, "Hello World");
        assert_eq!(results[0].matches[1].line_number, 2);
    }

    #[test]
    fn search_reports_nearest_heading() {
        let (_dir, mem) = setup();
        let content = "\
# Introduction

Some intro text with keyword.

## Details

More keyword content here.

### Sub-section

No match in this section.

## Conclusion

Final keyword mention.";
        mem.write("doc.md", content).unwrap();
        let results = mem.search("keyword").unwrap();
        assert_eq!(results.len(), 1);
        let matches = &results[0].matches;
        assert_eq!(matches.len(), 3);

        // First match is under "Introduction"
        assert_eq!(matches[0].heading.as_deref(), Some("Introduction"));
        // Second match is under "Details"
        assert_eq!(matches[1].heading.as_deref(), Some("Details"));
        // Third match is under "Conclusion"
        assert_eq!(matches[2].heading.as_deref(), Some("Conclusion"));
    }

    #[test]
    fn search_no_heading_returns_none() {
        let (_dir, mem) = setup();
        mem.write("plain.md", "just some text with target word")
            .unwrap();
        let results = mem.search("target").unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].matches[0].heading.is_none());
    }

    #[test]
    fn search_across_multiple_files() {
        let (_dir, mem) = setup();
        mem.write("a.md", "apple pie").unwrap();
        mem.write("b.md", "banana split").unwrap();
        mem.write("c.md", "cherry apple tart").unwrap();
        let results = mem.search("apple").unwrap();
        assert_eq!(results.len(), 2);
        // Results should be sorted by file path since list() sorts.
        assert_eq!(results[0].path, "a.md");
        assert_eq!(results[1].path, "c.md");
    }

    #[test]
    fn search_no_matches() {
        let (_dir, mem) = setup();
        mem.write("doc.md", "nothing relevant here").unwrap();
        let results = mem.search("missing").unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn search_heading_on_same_line_as_keyword() {
        let (_dir, mem) = setup();
        mem.write("doc.md", "# Important keyword heading\n\nBody text.")
            .unwrap();
        let results = mem.search("keyword").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(
            results[0].matches[0].heading.as_deref(),
            Some("Important keyword heading")
        );
    }

    #[test]
    fn delete_existing_file() {
        let (_dir, mem) = setup();
        mem.write("gone.md", "bye").unwrap();
        assert!(mem.exists("gone.md"));
        mem.delete("gone.md").unwrap();
        assert!(!mem.exists("gone.md"));
    }

    #[test]
    fn delete_nonexistent_returns_error() {
        let (_dir, mem) = setup();
        let result = mem.delete("nope.md");
        assert!(result.is_err());
    }

    #[test]
    fn exists_true_and_false() {
        let (_dir, mem) = setup();
        assert!(!mem.exists("x.md"));
        mem.write("x.md", "").unwrap();
        assert!(mem.exists("x.md"));
    }

    #[test]
    fn nested_directory_write_and_read() {
        let (_dir, mem) = setup();
        mem.write("a/b/c/deep.md", "deep content").unwrap();
        assert_eq!(mem.read("a/b/c/deep.md").unwrap(), "deep content");
        assert!(mem.exists("a/b/c/deep.md"));
    }

    #[test]
    fn search_in_nested_files() {
        let (_dir, mem) = setup();
        mem.write("top.md", "no match").unwrap();
        mem.write("sub/note.md", "# Title\n\nfind me here").unwrap();
        let results = mem.search("find me").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].path, "sub/note.md");
        assert_eq!(results[0].matches[0].heading.as_deref(), Some("Title"));
    }
}
