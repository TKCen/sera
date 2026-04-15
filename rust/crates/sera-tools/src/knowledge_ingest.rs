//! knowledge-ingest tool — structured pipeline for ingesting external content
//! into the SERA knowledge/memory layer.
//!
//! Pipeline stages (in order):
//!   1. fetch      — retrieve raw content from a URL or accept inline content
//!   2. extract    — chunk content into KnowledgeCandidates
//!   3. check      — detect contradictions with existing knowledge (MVS: stub)
//!   4. create     — materialise candidates into KnowledgeBlocks
//!   5. index      — trigger downstream index refresh (MVS: stub)
//!
//! The tool implements `sera-tools::registry::Tool` so it can be registered in
//! a `ToolRegistry` and discovered by the agent runtime.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ── Public ID types ──────────────────────────────────────────────────────────

/// Opaque identifier for a created or updated knowledge block.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct KnowledgeBlockId(pub String);

impl KnowledgeBlockId {
    pub fn generate() -> Self {
        Self(Uuid::new_v4().to_string())
    }
}

impl std::fmt::Display for KnowledgeBlockId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ── Request types ────────────────────────────────────────────────────────────

/// The origin of content to ingest.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum IngestSource {
    /// Fetch content from a URL (requires network access; offline tests use
    /// `Document` with pre-fetched content instead).
    Url { url: String },

    /// Inline document content — primary path for offline/unit tests.
    Document {
        content: String,
        /// MIME type hint, e.g. `"text/plain"` or `"text/html"`.
        mime: String,
    },

    /// Local file path (resolved by the agent runtime sandbox).
    File { path: String },
}

/// How to handle candidate blocks that already exist in the circle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MergePolicy {
    /// Skip if a block with the same content hash exists.
    Skip,
    /// Overwrite the existing block.
    Overwrite,
    /// Append the new content to the existing block.
    Append,
}

impl Default for MergePolicy {
    fn default() -> Self {
        Self::Skip
    }
}

/// Top-level request for the knowledge-ingest tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestRequest {
    /// Where to obtain the raw content.
    pub source: IngestSource,

    /// Circle to write blocks into (`None` = agent's default circle).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub circle_id: Option<String>,

    /// How to handle collisions with existing blocks.
    #[serde(default)]
    pub merge_policy: MergePolicy,
}

// ── Internal pipeline types ──────────────────────────────────────────────────

/// A candidate knowledge block extracted from the raw content.
///
/// Each candidate represents one paragraph / logical chunk of text that may
/// be promoted to a `KnowledgeBlock` after the contradiction check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeCandidate {
    /// Zero-based chunk index within the source document.
    pub index: usize,
    /// The extracted text.
    pub text: String,
    /// SHA-256 content hash (hex) — used for deduplication.
    pub content_hash: String,
    /// Optional heading inferred from the surrounding document structure.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub heading: Option<String>,
}

/// A detected contradiction between a candidate and existing knowledge.
///
/// MVS: always empty — contradiction detection is a POST-MVS concern.
/// Future implementations should populate this from a semantic-diff pass.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConflictReport {
    /// The candidate that triggered the conflict.
    pub candidate_index: usize,
    /// Human-readable description of the conflict.
    pub description: String,
}

// ── Output types ─────────────────────────────────────────────────────────────

/// Summary report returned after a successful ingest run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestReport {
    pub blocks_created: Vec<KnowledgeBlockId>,
    pub blocks_updated: Vec<KnowledgeBlockId>,
    /// Conflicts detected (empty in MVS).
    pub conflicts: Vec<ConflictReport>,
}

// ── Errors ───────────────────────────────────────────────────────────────────

/// Errors that can occur during the ingest pipeline.
#[derive(Debug, thiserror::Error)]
pub enum IngestError {
    #[error("fetch failed: {reason}")]
    FetchFailed { reason: String },

    #[error("extract failed: {reason}")]
    ExtractFailed { reason: String },

    #[error("block creation failed: {reason}")]
    CreateFailed { reason: String },
}

// ── Pipeline implementation ──────────────────────────────────────────────────

/// The knowledge-ingest pipeline.
///
/// Each stage is exposed as a public method so unit tests can exercise
/// individual steps without going through the full `run()` entry point.
pub struct KnowledgeIngestPipeline;

impl KnowledgeIngestPipeline {
    /// **Stage 1 — Fetch**
    ///
    /// Returns the raw content string for the given source.
    ///
    /// * `IngestSource::Document` — returns the inline content immediately
    ///   (primary path for offline unit tests).
    /// * `IngestSource::File` — reads from the local path.
    /// * `IngestSource::Url` — performs an HTTP GET via `reqwest`.
    ///   If the `reqwest` feature is unavailable the call returns an error
    ///   directing the caller to use `Document` instead.
    pub async fn fetch(source: &IngestSource) -> Result<String, IngestError> {
        match source {
            IngestSource::Document { content, .. } => Ok(content.clone()),

            IngestSource::File { path } => std::fs::read_to_string(path).map_err(|e| {
                IngestError::FetchFailed {
                    reason: format!("could not read file '{}': {}", path, e),
                }
            }),

            IngestSource::Url { url } => {
                // reqwest is a workspace dep — use it for live URL fetching.
                // Tests should use IngestSource::Document to stay offline.
                #[cfg(feature = "http_fetch")]
                {
                    let body = reqwest::get(url.as_str())
                        .await
                        .map_err(|e| IngestError::FetchFailed { reason: e.to_string() })?
                        .text()
                        .await
                        .map_err(|e| IngestError::FetchFailed { reason: e.to_string() })?;
                    Ok(body)
                }
                #[cfg(not(feature = "http_fetch"))]
                {
                    Err(IngestError::FetchFailed {
                        reason: format!(
                            "HTTP fetch not enabled (url={url}); \
                             use IngestSource::Document with pre-fetched content"
                        ),
                    })
                }
            }
        }
    }

    /// **Stage 2 — Extract**
    ///
    /// Splits raw text into `KnowledgeCandidate` chunks.
    ///
    /// MVS strategy: split on blank lines (paragraph boundaries).  Each
    /// non-empty paragraph becomes one candidate.  A SHA-256 content hash is
    /// computed for each chunk to support deduplication in stage 4.
    ///
    /// *Hook for LLM extraction*: replace this function body with an LLM call
    /// that returns structured facts, citations, and entity mentions once the
    /// inference backend is wired in (POST-MVS).
    pub fn extract_facts(raw: &str) -> Result<Vec<KnowledgeCandidate>, IngestError> {
        use sha2::{Digest, Sha256};

        let candidates: Vec<KnowledgeCandidate> = raw
            .split("\n\n")
            .enumerate()
            .filter_map(|(idx, chunk)| {
                let trimmed = chunk.trim();
                if trimmed.is_empty() {
                    return None;
                }

                // Detect a leading markdown heading on the first line.
                let mut lines = trimmed.lines();
                let first_line = lines.next().unwrap_or("");
                let heading = if first_line.starts_with('#') {
                    Some(first_line.trim_start_matches('#').trim().to_string())
                } else {
                    None
                };

                let mut hasher = Sha256::new();
                hasher.update(trimmed.as_bytes());
                let hash = hex::encode(hasher.finalize());

                Some(KnowledgeCandidate {
                    index: idx,
                    text: trimmed.to_string(),
                    content_hash: hash,
                    heading,
                })
            })
            .collect();

        Ok(candidates)
    }

    /// **Stage 3 — Check contradictions**
    ///
    /// MVS stub: always returns an empty conflict list.
    ///
    /// Future implementation should query the circle's knowledge index with
    /// each candidate's embedding and run a semantic-diff to surface
    /// contradicting claims.
    pub fn check_contradictions(
        _candidates: &[KnowledgeCandidate],
        _existing: &[KnowledgeCandidate],
    ) -> Vec<ConflictReport> {
        // POST-MVS: implement via semantic similarity + LLM judgment.
        vec![]
    }

    /// **Stage 4 — Create blocks**
    ///
    /// Materialises each candidate into a `KnowledgeBlockId`.
    ///
    /// MVS: generates a fresh UUID for every candidate that survives the
    /// contradiction check.  Production implementation should persist to the
    /// circle's `FileMemory` / vector-store backend and honour `MergePolicy`.
    pub fn create_blocks(
        candidates: &[KnowledgeCandidate],
        _merge_policy: MergePolicy,
    ) -> Result<Vec<KnowledgeBlockId>, IngestError> {
        let ids = candidates.iter().map(|_| KnowledgeBlockId::generate()).collect();
        Ok(ids)
    }

    /// **Stage 5 — Update index**
    ///
    /// MVS stub: no-op.
    ///
    /// Future implementation should trigger a re-index of the circle's
    /// knowledge store (e.g. rebuild HNSW graph, recompute BM25 inverted
    /// index) and update `IndexStatus` to `Building`.
    pub fn update_index() {
        // POST-MVS: emit an index-rebuild event to the event bus.
    }

    /// Run the full pipeline end-to-end.
    pub async fn run(req: &IngestRequest) -> Result<IngestReport, IngestError> {
        // Stage 1: fetch
        let raw = Self::fetch(&req.source).await?;

        // Stage 2: extract
        let candidates = Self::extract_facts(&raw)?;

        // Stage 3: contradiction check (MVS: always empty)
        let conflicts = Self::check_contradictions(&candidates, &[]);

        // Stage 4: create blocks for candidates that didn't conflict
        let non_conflicting: Vec<&KnowledgeCandidate> = {
            let conflicting_indices: std::collections::HashSet<usize> =
                conflicts.iter().map(|c| c.candidate_index).collect();
            candidates
                .iter()
                .filter(|c| !conflicting_indices.contains(&c.index))
                .collect()
        };
        let non_conflicting_owned: Vec<KnowledgeCandidate> =
            non_conflicting.into_iter().cloned().collect();
        let blocks_created = Self::create_blocks(&non_conflicting_owned, req.merge_policy)?;

        // Stage 5: update index
        Self::update_index();

        Ok(IngestReport {
            blocks_created,
            blocks_updated: vec![],
            conflicts,
        })
    }
}

// ── Tool trait integration ────────────────────────────────────────────────────

use crate::registry::Tool;

/// `knowledge-ingest` tool — registered with `ToolRegistry` so the agent
/// runtime can discover it by name.
pub struct KnowledgeIngestTool;

impl Tool for KnowledgeIngestTool {
    fn name(&self) -> &str {
        "knowledge-ingest"
    }

    fn description(&self) -> &str {
        "Ingest external content (URL, document, or file) into the agent's \
         knowledge store. Runs a structured pipeline: fetch → extract_facts → \
         check_contradictions → create_blocks → update_index."
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Stage 2: extract_facts ────────────────────────────────────────────

    #[test]
    fn extract_single_paragraph() {
        let raw = "SERA uses a trait-based architecture for all tools.";
        let candidates = KnowledgeIngestPipeline::extract_facts(raw).unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].index, 0);
        assert!(candidates[0].text.contains("trait-based"));
        assert!(!candidates[0].content_hash.is_empty());
        assert!(candidates[0].heading.is_none());
    }

    #[test]
    fn extract_multiple_paragraphs() {
        let raw = "First paragraph with some content.\n\nSecond paragraph here.\n\nThird one.";
        let candidates = KnowledgeIngestPipeline::extract_facts(raw).unwrap();
        assert_eq!(candidates.len(), 3);
        assert!(candidates[0].text.contains("First"));
        assert!(candidates[1].text.contains("Second"));
        assert!(candidates[2].text.contains("Third"));
    }

    #[test]
    fn extract_skips_blank_paragraphs() {
        let raw = "Real content.\n\n\n\nMore content.";
        let candidates = KnowledgeIngestPipeline::extract_facts(raw).unwrap();
        assert_eq!(candidates.len(), 2);
    }

    #[test]
    fn extract_detects_markdown_heading() {
        // Heading and body on the same paragraph (no blank line between them).
        let raw = "# Introduction\nThis is the intro.\n\n## Details\nMore here.";
        let candidates = KnowledgeIngestPipeline::extract_facts(raw).unwrap();
        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[0].heading.as_deref(), Some("Introduction"));
        assert_eq!(candidates[1].heading.as_deref(), Some("Details"));
    }

    #[test]
    fn extract_content_hashes_differ_for_different_text() {
        let raw = "Alpha content.\n\nBeta content.";
        let candidates = KnowledgeIngestPipeline::extract_facts(raw).unwrap();
        assert_ne!(candidates[0].content_hash, candidates[1].content_hash);
    }

    #[test]
    fn extract_content_hash_is_stable() {
        let raw = "Stable content.";
        let c1 = KnowledgeIngestPipeline::extract_facts(raw).unwrap();
        let c2 = KnowledgeIngestPipeline::extract_facts(raw).unwrap();
        assert_eq!(c1[0].content_hash, c2[0].content_hash);
    }

    // ── Stage 3: check_contradictions ────────────────────────────────────

    #[test]
    fn check_contradictions_always_empty_mvs() {
        let raw = "Some fact.\n\nAnother fact.";
        let candidates = KnowledgeIngestPipeline::extract_facts(raw).unwrap();
        let conflicts = KnowledgeIngestPipeline::check_contradictions(&candidates, &[]);
        assert!(conflicts.is_empty(), "MVS stub must return no conflicts");
    }

    // ── Stage 4: create_blocks ────────────────────────────────────────────

    #[test]
    fn create_blocks_returns_one_id_per_candidate() {
        let raw = "Block one.\n\nBlock two.\n\nBlock three.";
        let candidates = KnowledgeIngestPipeline::extract_facts(raw).unwrap();
        let ids = KnowledgeIngestPipeline::create_blocks(&candidates, MergePolicy::Skip).unwrap();
        assert_eq!(ids.len(), 3);
    }

    #[test]
    fn create_blocks_ids_are_unique() {
        let raw = "Alpha.\n\nBeta.\n\nGamma.";
        let candidates = KnowledgeIngestPipeline::extract_facts(raw).unwrap();
        let ids = KnowledgeIngestPipeline::create_blocks(&candidates, MergePolicy::Skip).unwrap();
        let unique: std::collections::HashSet<&KnowledgeBlockId> = ids.iter().collect();
        assert_eq!(unique.len(), ids.len(), "all block IDs must be unique");
    }

    // ── Full pipeline (inline Document source, offline) ───────────────────

    #[tokio::test]
    async fn full_pipeline_inline_document() {
        let req = IngestRequest {
            source: IngestSource::Document {
                content: "SERA agents communicate via the event bus.\n\n\
                          Each agent has a sandboxed workspace.\n\n\
                          Kill switches are CON-04 compliant."
                    .to_string(),
                mime: "text/plain".to_string(),
            },
            circle_id: Some("circle-test".to_string()),
            merge_policy: MergePolicy::Skip,
        };

        let report = KnowledgeIngestPipeline::run(&req).await.unwrap();
        assert_eq!(report.blocks_created.len(), 3);
        assert!(report.blocks_updated.is_empty());
        assert!(report.conflicts.is_empty());
    }

    #[tokio::test]
    async fn full_pipeline_empty_content_produces_no_blocks() {
        let req = IngestRequest {
            source: IngestSource::Document {
                content: "   \n\n   ".to_string(),
                mime: "text/plain".to_string(),
            },
            circle_id: None,
            merge_policy: MergePolicy::default(),
        };

        let report = KnowledgeIngestPipeline::run(&req).await.unwrap();
        assert!(report.blocks_created.is_empty());
        assert!(report.conflicts.is_empty());
    }

    // ── Tool registration ─────────────────────────────────────────────────

    #[test]
    fn tool_name_and_description() {
        let tool = KnowledgeIngestTool;
        assert_eq!(tool.name(), "knowledge-ingest");
        assert!(!tool.description().is_empty());
    }

    #[test]
    fn tool_registers_in_registry() {
        use crate::registry::ToolRegistry;
        use std::sync::Arc;

        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(KnowledgeIngestTool));

        let names = registry.list();
        assert!(names.contains(&"knowledge-ingest".to_string()));

        let retrieved = registry.get("knowledge-ingest");
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().name(), "knowledge-ingest");
    }

    // ── IngestRequest serde ───────────────────────────────────────────────

    #[test]
    fn ingest_request_roundtrip() {
        let req = IngestRequest {
            source: IngestSource::Url {
                url: "https://example.com/doc".to_string(),
            },
            circle_id: Some("circle-1".to_string()),
            merge_policy: MergePolicy::Overwrite,
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: IngestRequest = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed.source, IngestSource::Url { .. }));
        assert_eq!(parsed.merge_policy, MergePolicy::Overwrite);
        assert_eq!(parsed.circle_id.as_deref(), Some("circle-1"));
    }

    #[test]
    fn merge_policy_default_is_skip() {
        assert_eq!(MergePolicy::default(), MergePolicy::Skip);
    }

    #[test]
    fn knowledge_block_id_display() {
        let id = KnowledgeBlockId("test-id".to_string());
        assert_eq!(id.to_string(), "test-id");
    }
}
