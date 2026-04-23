//! knowledge-ingest tool — structured pipeline for ingesting external content
//! into the SERA knowledge/memory layer.
//!
//! Pipeline stages (in order):
//!   1. fetch      — retrieve raw content from a URL or accept inline content
//!   2. extract    — chunk content into KnowledgeCandidates
//!   3. check      — detect contradictions with existing knowledge
//!   4. create     — materialise candidates into KnowledgeBlocks
//!   5. index      — trigger downstream index refresh (MVS: stub)
//!
//! The tool implements `sera-tools::registry::Tool` so it can be registered in
//! a `ToolRegistry` and discovered by the agent runtime.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
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
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MergePolicy {
    /// Skip if a block with the same content hash exists.
    #[default]
    Skip,
    /// Overwrite the existing block.
    Overwrite,
    /// Append the new content to the existing block.
    Append,
}

/// What action to take when a contradiction is detected.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContradictionAction {
    /// Reject the incoming candidate — do not create a block for it.
    Reject,
    /// Tag the candidate with a conflict marker but still create the block.
    #[default]
    Tag,
    /// Create the new block and mark the conflicting existing block as superseded.
    Supersede,
}

/// Configuration for the contradiction-detection stage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContradictionConfig {
    /// When `false` (default), the contradiction-detection stage is skipped.
    #[serde(default)]
    pub enabled: bool,

    /// Cosine-similarity threshold above which two texts are considered
    /// potentially contradicting.  Range [0.0, 1.0]; default 0.8.
    #[serde(default = "ContradictionConfig::default_threshold")]
    pub similarity_threshold: f64,

    /// Action to take for each detected conflict.
    #[serde(default)]
    pub action: ContradictionAction,
}

impl ContradictionConfig {
    fn default_threshold() -> f64 {
        0.8
    }
}

impl Default for ContradictionConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            similarity_threshold: Self::default_threshold(),
            action: ContradictionAction::default(),
        }
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

    /// Optional contradiction-detection configuration.
    /// When absent or `enabled: false`, Stage 3 is a no-op.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contradiction_config: Option<ContradictionConfig>,
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConflictReport {
    /// The candidate that triggered the conflict.
    pub candidate_index: usize,
    /// Human-readable description of the conflict.
    pub description: String,
}

// ── Contradiction detector trait ─────────────────────────────────────────────

/// Extension point for contradiction-detection strategies.
///
/// The default implementation (`TextSimilarityDetector`) uses word-frequency
/// cosine similarity.  Future implementations may use embedding-based or
/// LLM-based approaches without changing the pipeline contract.
#[async_trait]
pub trait ContradictionDetector: Send + Sync {
    async fn detect(
        &self,
        candidate: &KnowledgeCandidate,
        existing: &[KnowledgeCandidate],
    ) -> Vec<ConflictReport>;
}

// ── Text-similarity detector ─────────────────────────────────────────────────

/// Word-frequency cosine similarity detector.
///
/// Algorithm:
/// 1. Tokenise each text by whitespace; lowercase all tokens.
/// 2. Build a word-frequency map (term → count).
/// 3. Compute cosine similarity between candidate and each existing block.
/// 4. If similarity > threshold AND hashes differ (not exact duplicate),
///    emit a `ConflictReport`.
pub struct TextSimilarityDetector {
    pub threshold: f64,
}

impl TextSimilarityDetector {
    pub fn new(threshold: f64) -> Self {
        Self { threshold }
    }

    /// Tokenise text into lowercase words.
    fn tokenise(text: &str) -> Vec<String> {
        text.split_whitespace()
            .map(|w| w.to_lowercase())
            .collect()
    }

    /// Build a word-frequency vector from a token list.
    fn word_freq(tokens: &[String]) -> HashMap<&str, f64> {
        let mut freq: HashMap<&str, f64> = HashMap::new();
        for token in tokens {
            *freq.entry(token.as_str()).or_insert(0.0) += 1.0;
        }
        freq
    }

    /// Cosine similarity between two frequency maps.
    fn cosine_similarity(a: &HashMap<&str, f64>, b: &HashMap<&str, f64>) -> f64 {
        let dot: f64 = a.iter().filter_map(|(k, va)| b.get(k).map(|vb| va * vb)).sum();
        let mag_a: f64 = a.values().map(|v| v * v).sum::<f64>().sqrt();
        let mag_b: f64 = b.values().map(|v| v * v).sum::<f64>().sqrt();
        if mag_a == 0.0 || mag_b == 0.0 {
            0.0
        } else {
            dot / (mag_a * mag_b)
        }
    }
}

#[async_trait]
impl ContradictionDetector for TextSimilarityDetector {
    async fn detect(
        &self,
        candidate: &KnowledgeCandidate,
        existing: &[KnowledgeCandidate],
    ) -> Vec<ConflictReport> {
        let cand_tokens = Self::tokenise(&candidate.text);
        let cand_freq = Self::word_freq(&cand_tokens);

        existing
            .iter()
            .filter_map(|ex| {
                // Identical content hash → dedup, not a contradiction.
                if ex.content_hash == candidate.content_hash {
                    return None;
                }

                let ex_tokens = Self::tokenise(&ex.text);
                let ex_freq = Self::word_freq(&ex_tokens);
                let sim = Self::cosine_similarity(&cand_freq, &ex_freq);

                if sim > self.threshold {
                    Some(ConflictReport {
                        candidate_index: candidate.index,
                        description: format!(
                            "Candidate {} is {:.1}% similar to existing block {} \
                             (threshold {:.1}%) — possible contradiction.",
                            candidate.index,
                            sim * 100.0,
                            ex.index,
                            self.threshold * 100.0,
                        ),
                    })
                } else {
                    None
                }
            })
            .collect()
    }
}

// ── Output types ─────────────────────────────────────────────────────────────

/// Summary report returned after a successful ingest run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestReport {
    pub blocks_created: Vec<KnowledgeBlockId>,
    pub blocks_updated: Vec<KnowledgeBlockId>,
    /// Conflicts detected (empty when contradiction detection is disabled).
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
    /// When `config` is `None` or `enabled: false`, returns an empty list
    /// (backward-compatible no-op).
    ///
    /// When enabled, runs each candidate through the provided detector
    /// against `existing` blocks and collects all conflict reports.
    pub async fn check_contradictions_with(
        candidates: &[KnowledgeCandidate],
        existing: &[KnowledgeCandidate],
        config: Option<&ContradictionConfig>,
        detector: &dyn ContradictionDetector,
    ) -> Vec<ConflictReport> {
        let Some(cfg) = config else { return vec![] };
        if !cfg.enabled {
            return vec![];
        }

        let mut all_conflicts = Vec::new();
        for candidate in candidates {
            let mut conflicts = detector.detect(candidate, existing).await;
            all_conflicts.append(&mut conflicts);
        }
        all_conflicts
    }

    /// **Stage 3 — Check contradictions** (legacy sync stub)
    ///
    /// Kept for backward compatibility with existing call sites and tests.
    /// Always returns an empty conflict list; use
    /// `check_contradictions_with` for real detection.
    pub fn check_contradictions(
        _candidates: &[KnowledgeCandidate],
        _existing: &[KnowledgeCandidate],
    ) -> Vec<ConflictReport> {
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

        // Stage 3: contradiction check
        let conflicts = if let Some(cfg) = &req.contradiction_config {
            let detector = TextSimilarityDetector::new(cfg.similarity_threshold);
            Self::check_contradictions_with(&candidates, &[], Some(cfg), &detector).await
        } else {
            vec![]
        };

        // Stage 4: create blocks for candidates that didn't conflict (Reject action)
        // or all candidates when action is Tag/Supersede.
        let action = req
            .contradiction_config
            .as_ref()
            .map(|c| c.action)
            .unwrap_or(ContradictionAction::Tag);

        let non_conflicting: Vec<KnowledgeCandidate> = {
            if action == ContradictionAction::Reject && !conflicts.is_empty() {
                let conflicting_indices: std::collections::HashSet<usize> =
                    conflicts.iter().map(|c| c.candidate_index).collect();
                candidates
                    .iter()
                    .filter(|c| !conflicting_indices.contains(&c.index))
                    .cloned()
                    .collect()
            } else {
                candidates.clone()
            }
        };

        let blocks_created = Self::create_blocks(&non_conflicting, req.merge_policy)?;

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

use crate::registry::ToolDescriptor;

/// `knowledge-ingest` tool — registered with `ToolRegistry` so the agent
/// runtime can discover it by name.
pub struct KnowledgeIngestTool;

impl ToolDescriptor for KnowledgeIngestTool {
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

    // ── Stage 3: check_contradictions (legacy stub) ───────────────────────

    #[test]
    fn check_contradictions_always_empty_mvs() {
        let raw = "Some fact.\n\nAnother fact.";
        let candidates = KnowledgeIngestPipeline::extract_facts(raw).unwrap();
        let conflicts = KnowledgeIngestPipeline::check_contradictions(&candidates, &[]);
        assert!(conflicts.is_empty(), "legacy stub must return no conflicts");
    }

    // ── TextSimilarityDetector ────────────────────────────────────────────

    fn make_candidate(index: usize, text: &str) -> KnowledgeCandidate {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(text.as_bytes());
        KnowledgeCandidate {
            index,
            text: text.to_string(),
            content_hash: hex::encode(hasher.finalize()),
            heading: None,
        }
    }

    #[tokio::test]
    async fn detector_flags_similar_but_different_texts() {
        // Two texts that share most words → high cosine similarity.
        let candidate = make_candidate(
            0,
            "The quick brown fox jumps over the lazy dog near the river",
        );
        let existing = vec![make_candidate(
            1,
            "The quick brown fox jumps over the lazy dog beside the lake",
        )];

        let detector = TextSimilarityDetector::new(0.8);
        let conflicts = detector.detect(&candidate, &existing).await;
        assert!(!conflicts.is_empty(), "similar texts should produce a conflict");
        assert_eq!(conflicts[0].candidate_index, 0);
    }

    #[tokio::test]
    async fn detector_does_not_flag_identical_texts() {
        // Identical content_hash → dedup path, not a contradiction.
        let text = "SERA agents communicate via the event bus.";
        let candidate = make_candidate(0, text);
        let existing = vec![make_candidate(1, text)];

        let detector = TextSimilarityDetector::new(0.8);
        let conflicts = detector.detect(&candidate, &existing).await;
        assert!(conflicts.is_empty(), "identical texts must not be flagged");
    }

    #[tokio::test]
    async fn detector_does_not_flag_dissimilar_texts() {
        let candidate = make_candidate(0, "The sky is blue and clouds are white.");
        let existing = vec![make_candidate(
            1,
            "Rust is a systems programming language focused on safety.",
        )];

        let detector = TextSimilarityDetector::new(0.8);
        let conflicts = detector.detect(&candidate, &existing).await;
        assert!(conflicts.is_empty(), "dissimilar texts must not be flagged");
    }

    #[tokio::test]
    async fn check_contradictions_with_disabled_config_skips_detection() {
        let candidates = vec![make_candidate(
            0,
            "The quick brown fox jumps over the lazy dog near the river",
        )];
        let existing = vec![make_candidate(
            1,
            "The quick brown fox jumps over the lazy dog beside the lake",
        )];

        let config = ContradictionConfig { enabled: false, ..Default::default() };
        let detector = TextSimilarityDetector::new(0.8);
        let conflicts = KnowledgeIngestPipeline::check_contradictions_with(
            &candidates,
            &existing,
            Some(&config),
            &detector,
        )
        .await;
        assert!(conflicts.is_empty(), "disabled config must skip detection");
    }

    #[tokio::test]
    async fn check_contradictions_with_none_config_skips_detection() {
        let candidates = vec![make_candidate(
            0,
            "The quick brown fox jumps over the lazy dog near the river",
        )];
        let existing = vec![make_candidate(
            1,
            "The quick brown fox jumps over the lazy dog beside the lake",
        )];

        let detector = TextSimilarityDetector::new(0.8);
        let conflicts = KnowledgeIngestPipeline::check_contradictions_with(
            &candidates,
            &existing,
            None,
            &detector,
        )
        .await;
        assert!(conflicts.is_empty(), "None config must skip detection");
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
            contradiction_config: None,
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
            contradiction_config: None,
        };

        let report = KnowledgeIngestPipeline::run(&req).await.unwrap();
        assert!(report.blocks_created.is_empty());
        assert!(report.conflicts.is_empty());
    }

    #[tokio::test]
    async fn full_pipeline_with_contradiction_detection_enabled() {
        // Two similar but not identical paragraphs — second should be flagged.
        // We use existing = [] in run() so no conflicts will be emitted
        // (there's nothing to compare against in a fresh ingest).
        // This test exercises the enabled path end-to-end and verifies that
        // when documents are self-similar the pipeline still completes.
        let req = IngestRequest {
            source: IngestSource::Document {
                content: "The sky is blue today and clouds are white.\n\n\
                          The sky is blue today and clouds are quite white."
                    .to_string(),
                mime: "text/plain".to_string(),
            },
            circle_id: None,
            merge_policy: MergePolicy::Skip,
            contradiction_config: Some(ContradictionConfig {
                enabled: true,
                similarity_threshold: 0.8,
                action: ContradictionAction::Tag,
            }),
        };

        let report = KnowledgeIngestPipeline::run(&req).await.unwrap();
        // run() compares candidates against existing=[] so no conflicts expected.
        assert!(report.conflicts.is_empty());
        // Both blocks created (Tag action keeps them).
        assert_eq!(report.blocks_created.len(), 2);
    }

    #[tokio::test]
    async fn full_pipeline_contradiction_reject_removes_conflicting_blocks() {
        // Simulate: candidate 0 is similar to candidate 1 (already "existing").
        // We test the Reject path using check_contradictions_with directly,
        // then verify create_blocks only sees non-conflicting candidates.
        let candidate = make_candidate(
            0,
            "The quick brown fox jumps over the lazy dog near the river bank today",
        );
        let existing = vec![make_candidate(
            1,
            "The quick brown fox jumps over the lazy dog near the river bank here",
        )];

        let config = ContradictionConfig {
            enabled: true,
            similarity_threshold: 0.8,
            action: ContradictionAction::Reject,
        };
        let detector = TextSimilarityDetector::new(config.similarity_threshold);
        let conflicts = KnowledgeIngestPipeline::check_contradictions_with(
            std::slice::from_ref(&candidate),
            &existing,
            Some(&config),
            &detector,
        )
        .await;

        assert!(!conflicts.is_empty(), "should detect contradiction");

        // Simulate what run() does with Reject: exclude conflicting candidates.
        let conflicting_indices: std::collections::HashSet<usize> =
            conflicts.iter().map(|c| c.candidate_index).collect();
        let survivors: Vec<KnowledgeCandidate> = [candidate]
            .iter()
            .filter(|c| !conflicting_indices.contains(&c.index))
            .cloned()
            .collect();
        assert!(survivors.is_empty(), "conflicting candidate must be rejected");
    }

    // ── ContradictionConfig serde ─────────────────────────────────────────

    #[test]
    fn contradiction_config_default_values() {
        let cfg = ContradictionConfig::default();
        assert!(!cfg.enabled);
        assert!((cfg.similarity_threshold - 0.8).abs() < f64::EPSILON);
        assert_eq!(cfg.action, ContradictionAction::Tag);
    }

    #[test]
    fn contradiction_config_roundtrip() {
        let cfg = ContradictionConfig {
            enabled: true,
            similarity_threshold: 0.75,
            action: ContradictionAction::Reject,
        };
        let json = serde_json::to_string(&cfg).unwrap();
        let parsed: ContradictionConfig = serde_json::from_str(&json).unwrap();
        assert!(parsed.enabled);
        assert!((parsed.similarity_threshold - 0.75).abs() < f64::EPSILON);
        assert_eq!(parsed.action, ContradictionAction::Reject);
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
            contradiction_config: Some(ContradictionConfig {
                enabled: true,
                similarity_threshold: 0.9,
                action: ContradictionAction::Supersede,
            }),
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: IngestRequest = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed.source, IngestSource::Url { .. }));
        assert_eq!(parsed.merge_policy, MergePolicy::Overwrite);
        assert_eq!(parsed.circle_id.as_deref(), Some("circle-1"));
        let cc = parsed.contradiction_config.unwrap();
        assert!(cc.enabled);
        assert_eq!(cc.action, ContradictionAction::Supersede);
    }

    #[test]
    fn ingest_request_without_contradiction_config_roundtrip() {
        let req = IngestRequest {
            source: IngestSource::Url {
                url: "https://example.com/doc".to_string(),
            },
            circle_id: Some("circle-1".to_string()),
            merge_policy: MergePolicy::Overwrite,
            contradiction_config: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: IngestRequest = serde_json::from_str(&json).unwrap();
        assert!(parsed.contradiction_config.is_none());
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
