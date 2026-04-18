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

/// Working-memory tier — determines eviction/compaction strategy.
/// SPEC-memory §2.0 Four-Tier Memory ABC (BeeAIvalidated).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkingMemoryTier {
    /// Tier 1: No limit — keeps full history. Use for short interactive sessions.
    Unconstrained,
    /// Tier 2: Evicts oldest when token budget exceeded.
    TokenBounded,
    /// Tier 3: Fixed message-count sliding window.
    SlidingWindow,
    /// Tier 4: LLM-driven compaction when the budget is hit.
    Summarizing,
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

// ── §2b Recall Signal Tracking — dreaming support ───────────────────────────

/// Aggregate recall statistics for a single memory entry.
/// Computed from a `RecallStore` for use in dreaming promotion scoring.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecallStats {
    pub memory_id: MemoryId,
    /// Total number of times this entry has been recalled.
    pub recall_count: u32,
    /// Number of distinct queries that recalled this entry.
    pub unique_queries: u32,
    /// Most recent recall timestamp, or `None` if never recalled.
    pub last_recalled: Option<chrono::DateTime<chrono::Utc>>,
    /// Mean relevance score across all recalls.
    pub average_score: f64,
}

/// Dreaming promotion score for a memory entry.
/// Weights from SPEC-memory §2b / OpenClaw Dreaming Guide.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DreamingScore {
    pub memory_id: MemoryId,
    /// Relevance signal — weight 0.30
    pub relevance: f64,
    /// Recall frequency signal — weight 0.24
    pub frequency: f64,
    /// Query diversity signal — weight 0.15
    pub query_diversity: f64,
    /// Recency signal — weight 0.15
    pub recency: f64,
    /// Consolidation signal — weight 0.10
    pub consolidation: f64,
    /// Conceptual richness signal — weight 0.06
    pub conceptual_richness: f64,
}

impl DreamingScore {
    /// Weighted sum of all six dreaming signals.
    pub fn total_score(&self) -> f64 {
        self.relevance * 0.30
            + self.frequency * 0.24
            + self.query_diversity * 0.15
            + self.recency * 0.15
            + self.consolidation * 0.10
            + self.conceptual_richness * 0.06
    }

    /// Returns `true` if this entry passes all three promotion gates.
    pub fn passes_promotion_gates(
        &self,
        min_score: f64,
        min_recall_count: u32,
        min_unique_queries: u32,
        stats: &RecallStats,
    ) -> bool {
        self.total_score() >= min_score
            && stats.recall_count >= min_recall_count
            && stats.unique_queries >= min_unique_queries
    }
}

/// Ephemeral accumulator for raw `RecallSignal` events.
/// Consumed by the dreaming workflow during its deep-scoring phase.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RecallStore {
    pub signals: Vec<RecallSignal>,
}

impl RecallStore {
    /// Create an empty store.
    pub fn new() -> Self {
        Self { signals: Vec::new() }
    }

    /// Record a new recall signal.
    pub fn record(&mut self, signal: RecallSignal) {
        self.signals.push(signal);
    }

    /// Compute aggregate `RecallStats` for a single memory entry.
    pub fn stats_for(&self, memory_id: &MemoryId) -> RecallStats {
        let relevant: Vec<&RecallSignal> =
            self.signals.iter().filter(|s| &s.memory_id == memory_id).collect();

        let recall_count = relevant.len() as u32;

        // Unique queries by query_hash.
        let unique_queries = relevant
            .iter()
            .map(|s| s.query_hash)
            .collect::<std::collections::HashSet<_>>()
            .len() as u32;

        let last_recalled = relevant
            .iter()
            .filter_map(|s| chrono::DateTime::parse_from_rfc3339(&s.timestamp).ok())
            .map(|dt| dt.with_timezone(&chrono::Utc))
            .max();

        let average_score = if recall_count == 0 {
            0.0
        } else {
            relevant.iter().map(|s| s.score).sum::<f64>() / recall_count as f64
        };

        RecallStats { memory_id: memory_id.clone(), recall_count, unique_queries, last_recalled, average_score }
    }

    /// Compute `RecallStats` for every memory entry that has at least one signal.
    pub fn all_stats(&self) -> Vec<RecallStats> {
        let ids: std::collections::HashSet<&MemoryId> =
            self.signals.iter().map(|s| &s.memory_id).collect();
        let mut stats: Vec<RecallStats> = ids.iter().map(|id| self.stats_for(id)).collect();
        stats.sort_by(|a, b| a.memory_id.0.cmp(&b.memory_id.0));
        stats
    }

    /// Remove all signals with a timestamp strictly before `cutoff`.
    pub fn clear_before(&mut self, cutoff: chrono::DateTime<chrono::Utc>) {
        self.signals.retain(|s| {
            chrono::DateTime::parse_from_rfc3339(&s.timestamp)
                .map(|dt| dt.with_timezone(&chrono::Utc) >= cutoff)
                .unwrap_or(true) // keep signals with unparseable timestamps
        });
    }
}

// ── 2-Tier Memory Injection Model (Architecture Addendum 2026-04-16 §1) ──────

/// What produced a `MemorySegment` — drives eviction policy and rendering order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SegmentKind {
    /// Agent soul — never evicted.
    Soul,
    /// System-level prompt fragment.
    SystemPrompt,
    /// Persona definition.
    Persona,
    /// Injected skill context; carries the skill id.
    Skill(String),
    /// Recalled memory entry; carries the recall event id.
    MemoryRecall(String),
    /// Caller-defined segment type.
    Custom(String),
}

/// A unit of injectable context. Priority-ordered, budget-constrained.
///
/// Lower `priority` value = more important (priority 0 = Soul, never evicted).
/// `recency_boost` is a multiplier applied during priority sort for tiebreaking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemorySegment {
    /// Stable identifier for dedup / updates.
    pub id: String,
    /// Rendered text that will be injected.
    pub content: String,
    /// 0 = never evicted (Soul); higher values are evicted first.
    pub priority: u8,
    /// Multiplier applied during priority sort; higher boost = treated as more important.
    pub recency_boost: f32,
    /// Segment-specific soft cap (may be truncated during assembly).
    pub char_budget: usize,
    /// What produced this segment.
    pub kind: SegmentKind,
}

/// The compact Tier-1 memory block injected every turn.
///
/// Assembles `MemorySegment`s into a single string, respecting `char_budget`.
/// Soul segments (`SegmentKind::Soul`, `priority == 0`) are **never** trimmed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryBlock {
    pub segments: Vec<MemorySegment>,
    /// Total character budget for the assembled block.
    pub char_budget: usize,
    /// Consecutive-overflow threshold before `record_turn` returns `true`.
    /// Default: 6.
    pub flush_min_turns: u8,
    /// Incremented each turn the block exceeds `char_budget`.
    pub overflow_turns: u8,
}

impl MemoryBlock {
    /// Create a new `MemoryBlock` with the given `char_budget` and the default
    /// `flush_min_turns` of 6.
    pub fn new(char_budget: usize) -> Self {
        Self::with_flush_min_turns(char_budget, 6)
    }

    /// Create a new `MemoryBlock` with a custom `flush_min_turns`.
    pub fn with_flush_min_turns(char_budget: usize, flush_min_turns: u8) -> Self {
        Self {
            segments: Vec::new(),
            char_budget,
            flush_min_turns,
            overflow_turns: 0,
        }
    }

    /// Append a segment. Duplicate `id`s are not deduplicated here; callers
    /// are responsible for managing identity.
    pub fn push(&mut self, segment: MemorySegment) {
        self.segments.push(segment);
    }

    /// Render the block to a string.
    ///
    /// Algorithm:
    /// 1. Separate Soul segments (priority == 0) from evictable ones.
    /// 2. Sort evictable segments by effective priority ascending
    ///    (lower effective priority = rendered first / kept longer):
    ///    `effective = priority as f32 / recency_boost.max(f32::EPSILON)`
    ///    (higher recency_boost lowers effective priority → kept longer).
    /// 3. Walk segments in render order (Soul first, then evictable by
    ///    ascending effective priority), concatenating content.
    ///    Respect per-segment `char_budget` soft cap and the block's total
    ///    `char_budget`.  Soul segments are **never** truncated.
    pub fn render(&self) -> String {
        let mut soul_parts: Vec<&MemorySegment> =
            self.segments.iter().filter(|s| s.priority == 0).collect();

        let mut evictable: Vec<&MemorySegment> =
            self.segments.iter().filter(|s| s.priority != 0).collect();

        // Sort souls by id for determinism.
        soul_parts.sort_by(|a, b| a.id.cmp(&b.id));

        // Sort evictable: lower effective_priority = rendered first (kept longer).
        // effective_priority = priority / recency_boost  (higher boost → lower effective)
        evictable.sort_by(|a, b| {
            let eff_a = a.priority as f32 / a.recency_boost.max(f32::EPSILON);
            let eff_b = b.priority as f32 / b.recency_boost.max(f32::EPSILON);
            eff_a.partial_cmp(&eff_b).unwrap_or(std::cmp::Ordering::Equal)
        });

        let mut output = String::new();

        // Soul segments always included — not subject to char_budget trimming.
        for seg in &soul_parts {
            output.push_str(&seg.content);
            output.push('\n');
        }

        // Remaining budget for evictable segments.
        let mut remaining = self.char_budget.saturating_sub(output.len());

        for seg in &evictable {
            if remaining == 0 {
                break;
            }
            // Apply per-segment soft cap, then block budget.
            let allowed = seg.char_budget.min(remaining);
            let text = if seg.content.len() > allowed {
                &seg.content[..allowed]
            } else {
                seg.content.as_str()
            };
            output.push_str(text);
            output.push('\n');
            remaining = remaining.saturating_sub(text.len() + 1);
        }

        // Trim trailing newline added after the last segment.
        if output.ends_with('\n') {
            output.pop();
        }

        output
    }

    /// Total character count across all segments' content (unrendered).
    pub fn current_chars(&self) -> usize {
        self.segments.iter().map(|s| s.content.len()).sum()
    }

    /// Returns `true` if the rendered block exceeds `char_budget`.
    pub fn is_over_budget(&self) -> bool {
        self.render().len() > self.char_budget
    }

    /// Record one completed turn.
    ///
    /// If the block is currently over budget, increments `overflow_turns`.
    /// Otherwise resets it to 0.
    ///
    /// Returns `true` when `overflow_turns` reaches `flush_min_turns`,
    /// signalling that a `memory_pressure` event should fire.
    pub fn record_turn(&mut self) -> bool {
        if self.is_over_budget() {
            self.overflow_turns = self.overflow_turns.saturating_add(1);
        } else {
            self.overflow_turns = 0;
        }
        self.overflow_turns >= self.flush_min_turns
    }
}

#[cfg(test)]
mod memory_block_tests {
    use super::*;

    fn soul_seg(id: &str, content: &str) -> MemorySegment {
        MemorySegment {
            id: id.to_string(),
            content: content.to_string(),
            priority: 0,
            recency_boost: 1.0,
            char_budget: usize::MAX,
            kind: SegmentKind::Soul,
        }
    }

    fn evictable_seg(id: &str, content: &str, priority: u8, recency_boost: f32) -> MemorySegment {
        MemorySegment {
            id: id.to_string(),
            content: content.to_string(),
            priority,
            recency_boost,
            char_budget: usize::MAX,
            kind: SegmentKind::Custom("test".to_string()),
        }
    }

    // ── basic push + render ──────────────────────────────────────────────────

    #[test]
    fn push_and_render_basic() {
        let mut block = MemoryBlock::new(1000);
        block.push(soul_seg("soul-1", "You are a helpful assistant."));
        block.push(evictable_seg("seg-1", "Context A.", 1, 1.0));
        let rendered = block.render();
        assert!(rendered.contains("You are a helpful assistant."));
        assert!(rendered.contains("Context A."));
    }

    // ── budget truncation drops low-priority segments ────────────────────────

    #[test]
    fn budget_truncation_drops_low_priority() {
        // Budget: 30 chars. Soul = "Soul." (5+\n=6), high-priority = "Important." (10),
        // low-priority = "Expendable." (11) — should be dropped or truncated.
        let mut block = MemoryBlock::new(30);
        block.push(soul_seg("soul", "Soul."));
        block.push(evictable_seg("hi", "Important.", 1, 1.0));
        block.push(evictable_seg("lo", "Expendable.", 10, 1.0));
        let rendered = block.render();
        assert!(rendered.contains("Soul."), "soul must be present");
        assert!(rendered.contains("Important."), "high-priority must be present");
    }

    // ── soul is never trimmed even if oversized ──────────────────────────────

    #[test]
    fn soul_is_never_trimmed() {
        let long_soul = "S".repeat(500);
        let mut block = MemoryBlock::new(10); // tiny budget
        block.push(soul_seg("soul", &long_soul));
        let rendered = block.render();
        assert_eq!(rendered, long_soul, "Soul content must be fully preserved");
    }

    #[test]
    fn soul_over_budget_does_not_evict_soul() {
        let mut block = MemoryBlock::new(20);
        block.push(soul_seg("soul", "This is the soul content.")); // 25 chars
        block.push(evictable_seg("x", "Extra.", 5, 1.0));
        let rendered = block.render();
        assert!(rendered.contains("This is the soul content."));
    }

    // ── recency_boost shifts render order ────────────────────────────────────

    #[test]
    fn recency_boost_shifts_render_order() {
        // Two segments with same priority but different recency_boost.
        // Higher boost → lower effective priority → rendered first (kept longer).
        let mut block = MemoryBlock::new(1000);
        let low_boost = evictable_seg("low", "LowBoost.", 2, 1.0);
        let high_boost = evictable_seg("high", "HighBoost.", 2, 10.0);
        block.push(low_boost);
        block.push(high_boost);
        let rendered = block.render();
        let pos_high = rendered.find("HighBoost.").unwrap();
        let pos_low = rendered.find("LowBoost.").unwrap();
        assert!(pos_high < pos_low, "higher recency_boost should appear earlier");
    }

    // ── record_turn returns true at flush_min_turns ──────────────────────────

    #[test]
    fn record_turn_returns_true_at_flush_min_turns() {
        let mut block = MemoryBlock::with_flush_min_turns(5, 3); // budget=5, flush at 3
        // Soul segments are never truncated, so a long soul reliably forces is_over_budget.
        block.push(soul_seg("soul", "This soul content is way too long for the budget."));
        assert!(!block.record_turn()); // overflow_turns = 1
        assert!(!block.record_turn()); // overflow_turns = 2
        assert!(block.record_turn()); // overflow_turns = 3 = flush_min_turns → true
    }

    #[test]
    fn record_turn_resets_when_under_budget() {
        let mut block = MemoryBlock::with_flush_min_turns(5, 3);
        block.push(soul_seg("soul", "Too long soul content."));
        block.record_turn();
        block.record_turn();
        // Clear segments so block is under budget.
        block.segments.clear();
        assert!(!block.record_turn()); // resets to 0
        assert_eq!(block.overflow_turns, 0);
    }

    // ── JSON roundtrip (serde) ───────────────────────────────────────────────

    #[test]
    fn memory_segment_json_roundtrip() {
        let seg = MemorySegment {
            id: "seg-abc".to_string(),
            content: "Test content".to_string(),
            priority: 3,
            recency_boost: 1.5,
            char_budget: 500,
            kind: SegmentKind::MemoryRecall("recall-123".to_string()),
        };
        let json = serde_json::to_string(&seg).unwrap();
        let parsed: MemorySegment = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, "seg-abc");
        assert_eq!(parsed.priority, 3);
        assert!((parsed.recency_boost - 1.5).abs() < f32::EPSILON);
        assert!(
            matches!(parsed.kind, SegmentKind::MemoryRecall(ref id) if id == "recall-123")
        );
    }

    #[test]
    fn memory_block_json_roundtrip() {
        let mut block = MemoryBlock::new(2000);
        block.push(soul_seg("soul-1", "I am SERA."));
        block.push(evictable_seg("ctx-1", "Relevant context.", 2, 1.0));
        let json = serde_json::to_string(&block).unwrap();
        let parsed: MemoryBlock = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.segments.len(), 2);
        assert_eq!(parsed.char_budget, 2000);
        assert_eq!(parsed.flush_min_turns, 6);
        assert_eq!(parsed.overflow_turns, 0);
    }

    #[test]
    fn segment_kind_all_variants_roundtrip() {
        let kinds = vec![
            SegmentKind::Soul,
            SegmentKind::SystemPrompt,
            SegmentKind::Persona,
            SegmentKind::Skill("skill-42".to_string()),
            SegmentKind::MemoryRecall("recall-7".to_string()),
            SegmentKind::Custom("my-custom".to_string()),
        ];
        for kind in kinds {
            let json = serde_json::to_string(&kind).unwrap();
            let parsed: SegmentKind = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, kind);
        }
    }

    // ── current_chars and is_over_budget ─────────────────────────────────────

    #[test]
    fn current_chars_sums_all_content() {
        let mut block = MemoryBlock::new(100);
        block.push(soul_seg("s", "abc")); // 3
        block.push(evictable_seg("e", "de", 1, 1.0)); // 2
        assert_eq!(block.current_chars(), 5);
    }

    #[test]
    fn is_over_budget_respects_soul_exemption() {
        let mut block = MemoryBlock::new(10);
        // Soul content alone is 20 chars — rendered exceeds budget.
        block.push(soul_seg("s", "12345678901234567890"));
        assert!(block.is_over_budget());
    }

    #[test]
    fn is_over_budget_false_when_fits() {
        let mut block = MemoryBlock::new(100);
        block.push(evictable_seg("e", "short", 1, 1.0));
        assert!(!block.is_over_budget());
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

    // ── RecallStore tests ────────────────────────────────────────────────────

    fn make_signal(memory_id: &str, query_hash: u64, score: f64, timestamp: &str) -> RecallSignal {
        RecallSignal {
            memory_id: MemoryId::new(memory_id),
            query_text: format!("query-{query_hash}"),
            query_hash,
            score,
            timestamp: timestamp.to_string(),
        }
    }

    #[test]
    fn recall_store_record_and_stats() {
        let mut store = RecallStore::new();
        store.record(make_signal("mem-1", 1, 0.9, "2026-04-09T10:00:00Z"));
        store.record(make_signal("mem-1", 2, 0.8, "2026-04-09T11:00:00Z"));
        store.record(make_signal("mem-1", 3, 0.7, "2026-04-09T12:00:00Z"));

        let stats = store.stats_for(&MemoryId::new("mem-1"));
        assert_eq!(stats.recall_count, 3);
        assert_eq!(stats.unique_queries, 3);
        assert!((stats.average_score - 0.8).abs() < f64::EPSILON);
        assert!(stats.last_recalled.is_some());
    }

    #[test]
    fn recall_stats_unique_query_counting() {
        let mut store = RecallStore::new();
        // Same query_hash repeated — counts as one unique query.
        store.record(make_signal("mem-2", 42, 0.9, "2026-04-09T10:00:00Z"));
        store.record(make_signal("mem-2", 42, 0.85, "2026-04-09T11:00:00Z"));
        store.record(make_signal("mem-2", 99, 0.8, "2026-04-09T12:00:00Z"));

        let stats = store.stats_for(&MemoryId::new("mem-2"));
        assert_eq!(stats.recall_count, 3);
        assert_eq!(stats.unique_queries, 2); // hashes 42 and 99
    }

    #[test]
    fn dreaming_score_total_score_weighted_sum() {
        let score = DreamingScore {
            memory_id: MemoryId::new("mem-1"),
            relevance: 1.0,
            frequency: 1.0,
            query_diversity: 1.0,
            recency: 1.0,
            consolidation: 1.0,
            conceptual_richness: 1.0,
        };
        // All components 1.0 → weights sum to 1.0.
        let total = score.total_score();
        assert!((total - 1.0).abs() < 1e-10, "weights must sum to 1.0, got {total}");

        // Verify individual weights.
        let only_relevance = DreamingScore {
            relevance: 1.0,
            frequency: 0.0,
            query_diversity: 0.0,
            recency: 0.0,
            consolidation: 0.0,
            conceptual_richness: 0.0,
            ..score.clone()
        };
        assert!((only_relevance.total_score() - 0.30).abs() < f64::EPSILON);

        let only_frequency = DreamingScore { relevance: 0.0, frequency: 1.0, ..only_relevance.clone() };
        assert!((only_frequency.total_score() - 0.24).abs() < f64::EPSILON);
    }

    #[test]
    fn dreaming_score_promotion_gates_pass() {
        let score = DreamingScore {
            memory_id: MemoryId::new("mem-1"),
            relevance: 1.0,
            frequency: 1.0,
            query_diversity: 1.0,
            recency: 1.0,
            consolidation: 1.0,
            conceptual_richness: 1.0,
        };
        let stats = RecallStats {
            memory_id: MemoryId::new("mem-1"),
            recall_count: 5,
            unique_queries: 4,
            last_recalled: None,
            average_score: 0.9,
        };
        assert!(score.passes_promotion_gates(0.8, 3, 3, &stats));
    }

    #[test]
    fn dreaming_score_promotion_gates_fail_low_score() {
        let score = DreamingScore {
            memory_id: MemoryId::new("mem-1"),
            relevance: 0.1,
            frequency: 0.1,
            query_diversity: 0.1,
            recency: 0.1,
            consolidation: 0.1,
            conceptual_richness: 0.1,
        };
        let stats = RecallStats {
            memory_id: MemoryId::new("mem-1"),
            recall_count: 5,
            unique_queries: 4,
            last_recalled: None,
            average_score: 0.9,
        };
        assert!(!score.passes_promotion_gates(0.8, 3, 3, &stats));
    }

    #[test]
    fn dreaming_score_promotion_gates_fail_low_recall() {
        let score = DreamingScore {
            memory_id: MemoryId::new("mem-1"),
            relevance: 1.0,
            frequency: 1.0,
            query_diversity: 1.0,
            recency: 1.0,
            consolidation: 1.0,
            conceptual_richness: 1.0,
        };
        let stats = RecallStats {
            memory_id: MemoryId::new("mem-1"),
            recall_count: 1, // below min_recall_count=3
            unique_queries: 4,
            last_recalled: None,
            average_score: 0.9,
        };
        assert!(!score.passes_promotion_gates(0.8, 3, 3, &stats));
    }

    #[test]
    fn dreaming_score_promotion_gates_fail_low_unique_queries() {
        let score = DreamingScore {
            memory_id: MemoryId::new("mem-1"),
            relevance: 1.0,
            frequency: 1.0,
            query_diversity: 1.0,
            recency: 1.0,
            consolidation: 1.0,
            conceptual_richness: 1.0,
        };
        let stats = RecallStats {
            memory_id: MemoryId::new("mem-1"),
            recall_count: 5,
            unique_queries: 1, // below min_unique_queries=3
            last_recalled: None,
            average_score: 0.9,
        };
        assert!(!score.passes_promotion_gates(0.8, 3, 3, &stats));
    }

    #[test]
    fn recall_store_clear_before_removes_old_entries() {
        let mut store = RecallStore::new();
        store.record(make_signal("mem-1", 1, 0.9, "2026-01-01T00:00:00Z")); // old
        store.record(make_signal("mem-1", 2, 0.8, "2026-03-01T00:00:00Z")); // old
        store.record(make_signal("mem-1", 3, 0.7, "2026-04-09T10:00:00Z")); // recent

        let cutoff = chrono::DateTime::parse_from_rfc3339("2026-04-01T00:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        store.clear_before(cutoff);

        assert_eq!(store.signals.len(), 1);
        assert_eq!(store.signals[0].query_hash, 3);
    }

    #[test]
    fn recall_store_all_stats_covers_all_entries() {
        let mut store = RecallStore::new();
        store.record(make_signal("mem-a", 1, 0.9, "2026-04-09T10:00:00Z"));
        store.record(make_signal("mem-b", 1, 0.8, "2026-04-09T10:00:00Z"));
        store.record(make_signal("mem-a", 2, 0.85, "2026-04-09T11:00:00Z"));

        let all = store.all_stats();
        assert_eq!(all.len(), 2);
        let mem_a = all.iter().find(|s| s.memory_id.0 == "mem-a").unwrap();
        assert_eq!(mem_a.recall_count, 2);
        let mem_b = all.iter().find(|s| s.memory_id.0 == "mem-b").unwrap();
        assert_eq!(mem_b.recall_count, 1);
    }
}
