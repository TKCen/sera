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
            } else if path.extension().and_then(|e| e.to_str()) == Some("md")
                && let Ok(rel) = path.strip_prefix(&self.workspace) {
                // Normalise to forward slashes for cross-platform consistency.
                let rel_str = rel.to_string_lossy().replace('\\', "/");
                out.push(rel_str);
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

// ── Spec-aligned Memory architecture (SPEC-memory) ──────────────────────────

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Memory tier — determines scope, durability, and access patterns.
/// SPEC-memory: three tiers with different lifecycle characteristics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryTier {
    /// Session-scoped, volatile — lost when session ends.
    ShortTerm,
    /// Agent workspace, durable — persists across sessions.
    LongTerm,
    /// Cross-agent, durable — shared knowledge within a circle.
    Shared,
}

/// Search strategy for memory queries.
/// SPEC-memory §2a: embedding-based semantic search with fallback to keyword.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchStrategy {
    /// Embedding-based semantic search.
    Semantic,
    /// Heading/text keyword search (current FileMemory default).
    Keyword,
    /// Both semantic + keyword with weighted merge.
    Hybrid,
    /// Exact string match.
    Exact(String),
}

/// A memory query with search parameters.
/// SPEC-memory: used by MemoryBackend::search().
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryQuery {
    pub text: String,
    pub strategy: SearchStrategy,
    #[serde(default = "default_top_k")]
    pub top_k: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub similarity_threshold: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tier_filter: Option<MemoryTier>,
}

fn default_top_k() -> u32 {
    10
}

/// Unique memory entry identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MemoryId(pub String);

impl MemoryId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
    pub fn generate() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }
}

impl std::fmt::Display for MemoryId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A memory entry — the unit of storage in the memory system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub id: MemoryId,
    pub content: String,
    pub tier: MemoryTier,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub heading: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

/// A result from memory search — content with relevance score.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemorySearchResult {
    pub id: MemoryId,
    pub content: String,
    /// Relevance score (0.0–1.0).
    pub score: f64,
    pub tier: MemoryTier,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

/// Recall signal — tracks memory access patterns for dreaming.
/// SPEC-memory §2b: used by the workflow engine's dreaming process
/// to decide which memories to consolidate into long-term storage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecallSignal {
    pub memory_id: MemoryId,
    pub query_text: String,
    pub query_hash: u64,
    pub score: f64,
    pub timestamp: String,
}

/// Aggregated recall tracking for a single memory entry.
/// SPEC-memory §2b: dreaming signals use these to compute promotion scores.
/// Promotion gates: minScore ≥ 0.8, minRecallCount ≥ 3, minUniqueQueries ≥ 3.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecallTracker {
    pub memory_id: MemoryId,
    pub recall_count: u32,
    pub unique_queries: HashSet<u64>,
    pub total_relevance: f64,
    pub first_seen: String,
    pub last_seen: String,
}

impl RecallTracker {
    /// Create a new tracker from the first recall signal.
    pub fn from_signal(signal: &RecallSignal) -> Self {
        let mut unique = HashSet::new();
        unique.insert(signal.query_hash);
        Self {
            memory_id: signal.memory_id.clone(),
            recall_count: 1,
            unique_queries: unique,
            total_relevance: signal.score,
            first_seen: signal.timestamp.clone(),
            last_seen: signal.timestamp.clone(),
        }
    }

    /// Record an additional recall signal.
    pub fn record(&mut self, signal: &RecallSignal) {
        self.recall_count += 1;
        self.unique_queries.insert(signal.query_hash);
        self.total_relevance += signal.score;
        self.last_seen = signal.timestamp.clone();
    }

    /// Average relevance across all recalls.
    pub fn avg_relevance(&self) -> f64 {
        if self.recall_count == 0 {
            0.0
        } else {
            self.total_relevance / self.recall_count as f64
        }
    }

    /// Check if this memory meets dreaming promotion gates.
    /// SPEC-memory: minScore ≥ 0.8, minRecallCount ≥ 3, minUniqueQueries ≥ 3.
    pub fn meets_promotion_gates(&self) -> bool {
        self.avg_relevance() >= 0.8 && self.recall_count >= 3 && self.unique_queries.len() >= 3
    }
}

/// Compaction scope — what to compact and how.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactionScope {
    pub agent_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tier: Option<MemoryTier>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_age_days: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_entries: Option<u32>,
}

/// Result of a compaction operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactionResult {
    pub entries_before: u32,
    pub entries_after: u32,
    pub entries_removed: u32,
    pub entries_merged: u32,
}

/// Index status for a memory backend.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum IndexStatus {
    NotConfigured,
    Building,
    Ready {
        entry_count: u64,
        last_updated: String,
    },
    Stale {
        entry_count: u64,
        last_updated: String,
    },
}

/// Statistics about a memory backend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryStats {
    pub total_entries: u64,
    pub entries_by_tier: HashMap<MemoryTier, u64>,
    pub total_size_bytes: u64,
    pub index_status: IndexStatus,
}

// ── MemoryError ──────────────────────────────────────────────────────────────

/// Errors from memory backend operations.
#[derive(Debug, thiserror::Error)]
pub enum MemoryError {
    #[error("memory not found: {id}")]
    NotFound { id: String },
    #[error("memory I/O error: {reason}")]
    IoError { reason: String },
    #[error("search failed: {reason}")]
    SearchFailed { reason: String },
    #[error("compaction failed: {reason}")]
    CompactionFailed { reason: String },
    #[error("memory backend unavailable: {reason}")]
    Unavailable { reason: String },
}

impl From<std::io::Error> for MemoryError {
    fn from(e: std::io::Error) -> Self {
        MemoryError::IoError { reason: e.to_string() }
    }
}

// ── MemoryContext ────────────────────────────────────────────────────────────

/// Scoping context passed to write/search operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryContext {
    pub agent_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub principal_id: Option<String>,
}

// ── MemoryBackend trait ──────────────────────────────────────────────────────

/// Async trait for memory storage backends.
/// SPEC-memory: all backend implementations must be Send + Sync.
#[async_trait::async_trait]
pub trait MemoryBackend: Send + Sync {
    async fn write(&self, entry: MemoryEntry, ctx: &MemoryContext) -> Result<MemoryId, MemoryError>;
    async fn search(&self, query: &MemoryQuery, ctx: &MemoryContext) -> Result<Vec<MemorySearchResult>, MemoryError>;
    async fn get(&self, id: &MemoryId) -> Result<MemoryEntry, MemoryError>;
    async fn delete(&self, id: &MemoryId) -> Result<(), MemoryError>;
    async fn compact(&self, scope: &CompactionScope) -> Result<CompactionResult, MemoryError>;
    async fn stats(&self) -> MemoryStats;
}

// ── FileMemoryBackend ────────────────────────────────────────────────────────

/// Async wrapper around the sync `FileMemory` file-based backend.
pub struct FileMemoryBackend {
    inner: FileMemory,
}

impl FileMemoryBackend {
    pub fn new(workspace: &std::path::Path) -> Self {
        Self {
            inner: FileMemory::new(workspace),
        }
    }
}

#[async_trait::async_trait]
impl MemoryBackend for FileMemoryBackend {
    async fn write(&self, entry: MemoryEntry, _ctx: &MemoryContext) -> Result<MemoryId, MemoryError> {
        let id = MemoryId::generate();
        let path = format!("{}.md", id.0);
        self.inner.write(&path, &entry.content)?;
        Ok(id)
    }

    async fn search(&self, query: &MemoryQuery, _ctx: &MemoryContext) -> Result<Vec<MemorySearchResult>, MemoryError> {
        let results = self.inner.search(&query.text)?;
        let out = results
            .into_iter()
            .flat_map(|sr| {
                let path = sr.path.clone();
                sr.matches.into_iter().map(move |m| MemorySearchResult {
                    id: MemoryId::new(path.trim_end_matches(".md")),
                    content: m.line,
                    score: 1.0,
                    tier: MemoryTier::LongTerm,
                    source: Some(path.clone()),
                })
            })
            .take(query.top_k as usize)
            .collect();
        Ok(out)
    }

    async fn get(&self, id: &MemoryId) -> Result<MemoryEntry, MemoryError> {
        let path = format!("{}.md", id.0);
        let content = self.inner.read(&path).map_err(|_| MemoryError::NotFound { id: id.0.clone() })?;
        Ok(MemoryEntry {
            id: id.clone(),
            content,
            tier: MemoryTier::LongTerm,
            heading: None,
            tags: vec![],
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: None,
        })
    }

    async fn delete(&self, id: &MemoryId) -> Result<(), MemoryError> {
        let path = format!("{}.md", id.0);
        self.inner.delete(&path)?;
        Ok(())
    }

    async fn compact(&self, _scope: &CompactionScope) -> Result<CompactionResult, MemoryError> {
        // Compaction not implemented for file backend; return a no-op result.
        let count = self.inner.list().map(|v| v.len() as u32).unwrap_or(0);
        Ok(CompactionResult {
            entries_before: count,
            entries_after: count,
            entries_removed: 0,
            entries_merged: 0,
        })
    }

    async fn stats(&self) -> MemoryStats {
        let files = self.inner.list().unwrap_or_default();
        let count = files.len() as u64;
        let mut by_tier = HashMap::new();
        by_tier.insert(MemoryTier::LongTerm, count);
        MemoryStats {
            total_entries: count,
            entries_by_tier: by_tier,
            total_size_bytes: 0,
            index_status: IndexStatus::NotConfigured,
        }
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

    // ── SPEC-memory aligned type tests ───────────────────────────────────

    #[test]
    fn memory_tier_serde() {
        let variants = vec![
            (MemoryTier::ShortTerm, "short_term"),
            (MemoryTier::LongTerm, "long_term"),
            (MemoryTier::Shared, "shared"),
        ];
        for (tier, expected) in variants {
            let json = serde_json::to_string(&tier).unwrap();
            assert_eq!(json, format!("\"{expected}\""));
            let parsed: MemoryTier = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, tier);
        }
    }

    #[test]
    fn search_strategy_serde() {
        let json = serde_json::to_string(&SearchStrategy::Semantic).unwrap();
        assert_eq!(json, "\"semantic\"");

        let json = serde_json::to_string(&SearchStrategy::Keyword).unwrap();
        assert_eq!(json, "\"keyword\"");

        let json = serde_json::to_string(&SearchStrategy::Hybrid).unwrap();
        assert_eq!(json, "\"hybrid\"");
    }

    #[test]
    fn search_strategy_exact_serde() {
        let strategy = SearchStrategy::Exact("hello world".to_string());
        let json = serde_json::to_string(&strategy).unwrap();
        let parsed: SearchStrategy = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, SearchStrategy::Exact("hello world".to_string()));
    }

    #[test]
    fn memory_query_defaults() {
        let json = r#"{"text":"test","strategy":"keyword"}"#;
        let query: MemoryQuery = serde_json::from_str(json).unwrap();
        assert_eq!(query.text, "test");
        assert_eq!(query.top_k, 10); // default
        assert!(query.similarity_threshold.is_none());
        assert!(query.tier_filter.is_none());
    }

    #[test]
    fn memory_query_roundtrip() {
        let query = MemoryQuery {
            text: "agent architecture".to_string(),
            strategy: SearchStrategy::Hybrid,
            top_k: 5,
            similarity_threshold: Some(0.7),
            tier_filter: Some(MemoryTier::LongTerm),
        };
        let json = serde_json::to_string(&query).unwrap();
        let parsed: MemoryQuery = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.text, "agent architecture");
        assert_eq!(parsed.top_k, 5);
        assert_eq!(parsed.tier_filter, Some(MemoryTier::LongTerm));
    }

    #[test]
    fn memory_id_unique() {
        let a = MemoryId::generate();
        let b = MemoryId::generate();
        assert_ne!(a, b);
    }

    #[test]
    fn memory_entry_roundtrip() {
        let entry = MemoryEntry {
            id: MemoryId::new("mem-1"),
            content: "SERA uses trait-based architecture".to_string(),
            tier: MemoryTier::LongTerm,
            heading: Some("Architecture".to_string()),
            tags: vec!["design".to_string()],
            created_at: "2026-04-09T10:00:00Z".to_string(),
            updated_at: None,
        };
        let json = serde_json::to_string(&entry).unwrap();
        let parsed: MemoryEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, MemoryId::new("mem-1"));
        assert_eq!(parsed.tier, MemoryTier::LongTerm);
        assert_eq!(parsed.tags.len(), 1);
    }

    #[test]
    fn memory_search_result_roundtrip() {
        let result = MemorySearchResult {
            id: MemoryId::new("mem-1"),
            content: "relevant content".to_string(),
            score: 0.95,
            tier: MemoryTier::LongTerm,
            source: Some("notes/arch.md".to_string()),
        };
        let json = serde_json::to_string(&result).unwrap();
        let parsed: MemorySearchResult = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.score, 0.95);
        assert_eq!(parsed.source.as_deref(), Some("notes/arch.md"));
    }

    #[test]
    fn recall_tracker_from_signal() {
        let signal = RecallSignal {
            memory_id: MemoryId::new("mem-1"),
            query_text: "architecture".to_string(),
            query_hash: 12345,
            score: 0.9,
            timestamp: "2026-04-09T10:00:00Z".to_string(),
        };
        let tracker = RecallTracker::from_signal(&signal);
        assert_eq!(tracker.recall_count, 1);
        assert_eq!(tracker.unique_queries.len(), 1);
        assert!((tracker.avg_relevance() - 0.9).abs() < f64::EPSILON);
    }

    #[test]
    fn recall_tracker_record_multiple() {
        let signal1 = RecallSignal {
            memory_id: MemoryId::new("mem-1"),
            query_text: "architecture".to_string(),
            query_hash: 111,
            score: 0.9,
            timestamp: "2026-04-09T10:00:00Z".to_string(),
        };
        let signal2 = RecallSignal {
            memory_id: MemoryId::new("mem-1"),
            query_text: "design patterns".to_string(),
            query_hash: 222,
            score: 0.85,
            timestamp: "2026-04-09T11:00:00Z".to_string(),
        };
        let signal3 = RecallSignal {
            memory_id: MemoryId::new("mem-1"),
            query_text: "trait system".to_string(),
            query_hash: 333,
            score: 0.8,
            timestamp: "2026-04-09T12:00:00Z".to_string(),
        };

        let mut tracker = RecallTracker::from_signal(&signal1);
        tracker.record(&signal2);
        tracker.record(&signal3);

        assert_eq!(tracker.recall_count, 3);
        assert_eq!(tracker.unique_queries.len(), 3);
        assert!((tracker.avg_relevance() - 0.85).abs() < f64::EPSILON);
        assert_eq!(tracker.last_seen, "2026-04-09T12:00:00Z");
    }

    #[test]
    fn recall_tracker_promotion_gates_met() {
        let signal = RecallSignal {
            memory_id: MemoryId::new("mem-1"),
            query_text: "q1".to_string(),
            query_hash: 1,
            score: 0.9,
            timestamp: "t1".to_string(),
        };
        let mut tracker = RecallTracker::from_signal(&signal);
        tracker.record(&RecallSignal {
            query_hash: 2,
            score: 0.85,
            ..signal.clone()
        });
        tracker.record(&RecallSignal {
            query_hash: 3,
            score: 0.8,
            ..signal.clone()
        });

        assert!(tracker.meets_promotion_gates());
    }

    #[test]
    fn recall_tracker_promotion_gates_not_met_low_score() {
        let signal = RecallSignal {
            memory_id: MemoryId::new("mem-1"),
            query_text: "q".to_string(),
            query_hash: 1,
            score: 0.5, // too low
            timestamp: "t".to_string(),
        };
        let mut tracker = RecallTracker::from_signal(&signal);
        tracker.record(&RecallSignal {
            query_hash: 2,
            score: 0.5,
            ..signal.clone()
        });
        tracker.record(&RecallSignal {
            query_hash: 3,
            score: 0.5,
            ..signal.clone()
        });

        assert!(!tracker.meets_promotion_gates()); // avg 0.5 < 0.8
    }

    #[test]
    fn recall_tracker_promotion_gates_not_met_few_queries() {
        let signal = RecallSignal {
            memory_id: MemoryId::new("mem-1"),
            query_text: "q".to_string(),
            query_hash: 1,
            score: 0.9,
            timestamp: "t".to_string(),
        };
        let mut tracker = RecallTracker::from_signal(&signal);
        // Same query hash repeated — only 1 unique query
        tracker.record(&RecallSignal {
            query_hash: 1,
            score: 0.9,
            ..signal.clone()
        });
        tracker.record(&RecallSignal {
            query_hash: 1,
            score: 0.9,
            ..signal.clone()
        });

        assert!(!tracker.meets_promotion_gates()); // 3 recalls but only 1 unique query
    }

    #[test]
    fn compaction_scope_serde() {
        let scope = CompactionScope {
            agent_id: "agent-1".to_string(),
            tier: Some(MemoryTier::ShortTerm),
            max_age_days: Some(30),
            max_entries: Some(100),
        };
        let json = serde_json::to_string(&scope).unwrap();
        let parsed: CompactionScope = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.tier, Some(MemoryTier::ShortTerm));
        assert_eq!(parsed.max_entries, Some(100));
    }

    #[test]
    fn compaction_result_serde() {
        let result = CompactionResult {
            entries_before: 100,
            entries_after: 30,
            entries_merged: 50,
            entries_removed: 20,
        };
        let json = serde_json::to_string(&result).unwrap();
        let parsed: CompactionResult = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.entries_merged, 50);
    }

    #[test]
    fn memory_stats_serde() {
        let stats = MemoryStats {
            total_entries: 42,
            entries_by_tier: {
                let mut m = HashMap::new();
                m.insert(MemoryTier::LongTerm, 30u64);
                m.insert(MemoryTier::ShortTerm, 12u64);
                m
            },
            total_size_bytes: 1024 * 1024,
            index_status: IndexStatus::NotConfigured,
        };
        let json = serde_json::to_string(&stats).unwrap();
        let parsed: MemoryStats = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.total_entries, 42);
    }

    // ── MemoryError tests ────────────────────────────────────────────────────

    #[test]
    fn memory_error_display() {
        let e = MemoryError::NotFound { id: "mem-1".to_string() };
        assert_eq!(e.to_string(), "memory not found: mem-1");

        let e = MemoryError::IoError { reason: "disk full".to_string() };
        assert_eq!(e.to_string(), "memory I/O error: disk full");

        let e = MemoryError::SearchFailed { reason: "index offline".to_string() };
        assert_eq!(e.to_string(), "search failed: index offline");

        let e = MemoryError::CompactionFailed { reason: "timeout".to_string() };
        assert_eq!(e.to_string(), "compaction failed: timeout");

        let e = MemoryError::Unavailable { reason: "backend down".to_string() };
        assert_eq!(e.to_string(), "memory backend unavailable: backend down");
    }

    #[test]
    fn memory_error_from_io() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "no such file");
        let e = MemoryError::from(io_err);
        assert!(matches!(e, MemoryError::IoError { .. }));
        assert!(e.to_string().contains("memory I/O error"));
    }

    // ── MemoryContext tests ──────────────────────────────────────────────────

    #[test]
    fn memory_context_construction() {
        let ctx = MemoryContext {
            agent_id: "agent-1".to_string(),
            session_id: Some("sess-abc".to_string()),
            principal_id: None,
        };
        assert_eq!(ctx.agent_id, "agent-1");
        assert_eq!(ctx.session_id.as_deref(), Some("sess-abc"));
        assert!(ctx.principal_id.is_none());
    }

    // ── CompactionResult arithmetic ─────────────────────────────────────────

    #[test]
    fn compaction_result_arithmetic() {
        let r = CompactionResult {
            entries_before: 100,
            entries_after: 30,
            entries_removed: 20,
            entries_merged: 50,
        };
        assert_eq!(r.entries_before - r.entries_after, r.entries_removed + r.entries_merged);
    }

    // ── IndexStatus serde roundtrip ─────────────────────────────────────────

    #[test]
    fn index_status_serde_roundtrip() {
        let variants: Vec<IndexStatus> = vec![
            IndexStatus::NotConfigured,
            IndexStatus::Building,
            IndexStatus::Ready {
                entry_count: 42,
                last_updated: "2026-04-09T10:00:00Z".to_string(),
            },
            IndexStatus::Stale {
                entry_count: 10,
                last_updated: "2026-04-08T10:00:00Z".to_string(),
            },
        ];
        for v in variants {
            let json = serde_json::to_string(&v).unwrap();
            let _parsed: IndexStatus = serde_json::from_str(&json).unwrap();
        }
    }

    // ── FileMemoryBackend async tests ────────────────────────────────────────

    fn make_entry(id: &str, content: &str, tier: MemoryTier) -> MemoryEntry {
        MemoryEntry {
            id: MemoryId::new(id),
            content: content.to_string(),
            tier,
            heading: None,
            tags: vec![],
            created_at: "2026-04-09T10:00:00Z".to_string(),
            updated_at: None,
        }
    }

    fn make_ctx(agent_id: &str) -> MemoryContext {
        MemoryContext {
            agent_id: agent_id.to_string(),
            session_id: None,
            principal_id: None,
        }
    }

    #[tokio::test]
    async fn file_backend_write_get_roundtrip() {
        let dir = TempDir::new().unwrap();
        let backend = FileMemoryBackend::new(dir.path());
        let ctx = make_ctx("agent-1");
        let entry = make_entry("mem-1", "hello world", MemoryTier::LongTerm);
        let id = backend.write(entry.clone(), &ctx).await.unwrap();
        let fetched = backend.get(&id).await.unwrap();
        assert_eq!(fetched.content, "hello world");
        assert_eq!(fetched.tier, MemoryTier::LongTerm);
    }

    #[tokio::test]
    async fn file_backend_write_search_finds_it() {
        let dir = TempDir::new().unwrap();
        let backend = FileMemoryBackend::new(dir.path());
        let ctx = make_ctx("agent-1");
        let entry = make_entry("mem-2", "rust async trait architecture", MemoryTier::LongTerm);
        backend.write(entry, &ctx).await.unwrap();
        let query = MemoryQuery {
            text: "async trait".to_string(),
            strategy: SearchStrategy::Keyword,
            top_k: 10,
            similarity_threshold: None,
            tier_filter: None,
        };
        let results = backend.search(&query, &ctx).await.unwrap();
        assert!(!results.is_empty());
        assert!(results[0].content.contains("async trait"));
    }

    #[tokio::test]
    async fn file_backend_delete_removes_entry() {
        let dir = TempDir::new().unwrap();
        let backend = FileMemoryBackend::new(dir.path());
        let ctx = make_ctx("agent-1");
        let entry = make_entry("mem-3", "to be deleted", MemoryTier::ShortTerm);
        let id = backend.write(entry, &ctx).await.unwrap();
        backend.delete(&id).await.unwrap();
        let result = backend.get(&id).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn file_backend_stats_counts_entries() {
        let dir = TempDir::new().unwrap();
        let backend = FileMemoryBackend::new(dir.path());
        let ctx = make_ctx("agent-1");
        backend.write(make_entry("m1", "one", MemoryTier::LongTerm), &ctx).await.unwrap();
        backend.write(make_entry("m2", "two", MemoryTier::LongTerm), &ctx).await.unwrap();
        backend.write(make_entry("m3", "three", MemoryTier::ShortTerm), &ctx).await.unwrap();
        let stats = backend.stats().await;
        assert_eq!(stats.total_entries, 3);
    }
}
