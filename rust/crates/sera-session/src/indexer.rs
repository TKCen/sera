//! Session transcript indexing into Tier-2 semantic memory.
//!
//! On session close, the runtime asks an implementation of [`TranscriptIndexer`]
//! to extract a compact text blob from the transcript (user prompts + final
//! assistant responses + tool-call *summaries* only — no raw tool I/O) and
//! persist it into a [`SemanticMemoryStore`] so future sessions can recall the
//! conversation through the standard [`ContextEnricher`] path.
//!
//! Issue reference: #512 (session transcript indexing).
//!
//! ## Failure policy
//!
//! Indexing is best-effort. A failure MUST NOT block the session close path —
//! callers log at `warn` and continue. The indexer module itself makes no
//! retry attempts; idempotency is deferred until a higher-tier policy lands.
//!
//! ## Size policy
//!
//! Tool results can be megabytes (file reads, web fetches, command output).
//! This module never stores raw tool I/O. Instead it emits a terse summary
//! per tool call of the form `[tool:<name>] <one-line gist>`, truncating any
//! accidental multi-line content to the first line. Per-entry content is
//! capped at [`MAX_ENTRY_CHARS`] and the composite blob at [`MAX_BLOB_CHARS`].

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sera_types::{
    MemoryId, PutRequest, SemanticError, SemanticMemoryStore,
    memory::SegmentKind,
};
#[cfg(test)]
use uuid::Uuid;

use crate::transcript::{ContentBlock, Role, Transcript};

/// Maximum characters emitted per transcript entry before truncation.
pub const MAX_ENTRY_CHARS: usize = 2_000;

/// Hard cap on the composite blob size. Keeps a runaway transcript from
/// flooding the semantic store when the indexer is wired against an
/// unbounded session.
pub const MAX_BLOB_CHARS: usize = 32_000;

/// Tier tag used for stored session transcripts. Consumers can filter on
/// this in a [`sera_types::SemanticQuery::tier_filter`] to scope recall to
/// archived sessions.
pub const TRANSCRIPT_TIER_LABEL: &str = "session_transcript";

/// Metadata-key constant used on the [`SemanticEntry::tags`] vector to
/// identify transcript rows. Downstream tooling that wants to locate
/// transcript entries should look for this tag.
pub const TRANSCRIPT_TAG: &str = "kind:session_transcript";

/// A transcript indexer hooks into the session close path and writes a
/// compact summary of the conversation into a semantic memory backend.
///
/// Implementations are expected to be cheap to clone (typically holding an
/// `Arc<dyn SemanticMemoryStore>`). Failures MUST surface as
/// [`IndexerError`] so callers can decide whether to log-and-continue.
#[async_trait]
pub trait TranscriptIndexer: Send + Sync {
    /// Index the given transcript, assigning it to `agent_id`.
    ///
    /// `session_id` and `started_at` are preserved on the stored entry's
    /// tags/content so recall hits can be traced back to the source
    /// session.
    async fn index_transcript(
        &self,
        agent_id: &str,
        session_id: &str,
        started_at: DateTime<Utc>,
        transcript: &Transcript,
    ) -> Result<MemoryId, IndexerError>;
}

/// Errors surfaced by a [`TranscriptIndexer`].
#[derive(Debug, thiserror::Error)]
pub enum IndexerError {
    /// The underlying semantic store rejected the write.
    #[error("semantic memory store error: {0}")]
    Store(#[from] SemanticError),
    /// The transcript was empty — nothing to index.
    #[error("transcript empty; nothing to index")]
    Empty,
}

/// Concrete [`TranscriptIndexer`] backed by a shared
/// [`SemanticMemoryStore`].
///
/// The indexer writes one [`SemanticEntry`] per session with
/// `tier = SegmentKind::Custom("session_transcript")`. The embedding is
/// always an empty vector — a future enhancement will plug in an
/// [`sera_types::EmbeddingService`] so stored transcripts participate in
/// vector similarity. For now the entry is retrievable by tag/agent
/// filtering only.
pub struct SemanticTranscriptIndexer {
    store: Arc<dyn SemanticMemoryStore>,
}

impl SemanticTranscriptIndexer {
    /// Build a new indexer wrapping `store`.
    pub fn new(store: Arc<dyn SemanticMemoryStore>) -> Self {
        Self { store }
    }

    /// Borrow the inner semantic store (for diagnostics / tests).
    pub fn store(&self) -> &Arc<dyn SemanticMemoryStore> {
        &self.store
    }
}

impl std::fmt::Debug for SemanticTranscriptIndexer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SemanticTranscriptIndexer").finish()
    }
}

#[async_trait]
impl TranscriptIndexer for SemanticTranscriptIndexer {
    async fn index_transcript(
        &self,
        agent_id: &str,
        session_id: &str,
        started_at: DateTime<Utc>,
        transcript: &Transcript,
    ) -> Result<MemoryId, IndexerError> {
        let blob = extract_transcript_text(transcript);
        if blob.trim().is_empty() {
            return Err(IndexerError::Empty);
        }

        let header = format!(
            "session={session_id} started={started_at} agent={agent_id}\n"
        );
        let content = truncate(&format!("{header}{blob}"), MAX_BLOB_CHARS);

        let req = PutRequest {
            agent_id: agent_id.to_string(),
            content,
            scope: None,
            tier: SegmentKind::Custom(TRANSCRIPT_TIER_LABEL.to_string()),
            tags: vec![
                TRANSCRIPT_TAG.to_string(),
                format!("session_id:{session_id}"),
                format!("started_at:{started_at}"),
            ],
            promoted: false,
            supplied_embedding: None,
        };

        let id = self.store.put(req).await?;
        tracing::debug!(
            agent_id,
            session_id,
            memory_id = %id,
            "indexed session transcript into semantic memory"
        );
        Ok(id)
    }
}

/// Extract a compact text blob suitable for semantic recall.
///
/// Emits one block per [`TranscriptEntry`], tagged with the role. For
/// assistant entries, tool-call invocations are summarised as a single-line
/// `[tool:<name>]` marker plus any argument key hints. Tool results are
/// entirely skipped — they are often huge and rarely useful as recall
/// context.
pub fn extract_transcript_text(transcript: &Transcript) -> String {
    let mut out = String::new();
    for entry in transcript.entries() {
        let role_prefix = match entry.role {
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::System => "system",
            Role::Tool => {
                // Skip tool-role entries entirely — they carry raw results.
                continue;
            }
        };

        let mut segment = String::new();
        for block in &entry.blocks {
            match block {
                ContentBlock::Text { text } => {
                    let line = first_line(text);
                    if !line.trim().is_empty() {
                        segment.push_str(&line);
                        segment.push('\n');
                    }
                }
                ContentBlock::ToolUse { name, input, .. } => {
                    segment.push_str(&format!(
                        "[tool:{name}] {}\n",
                        summarise_tool_input(input)
                    ));
                }
                ContentBlock::ToolResult { .. } => {
                    // Always skipped — can be huge, rarely useful for recall.
                }
                ContentBlock::Image { media_type, .. } => {
                    segment.push_str(&format!("[image:{media_type}]\n"));
                }
                ContentBlock::Thinking { .. } => {
                    // Chain-of-thought is not persisted to memory.
                }
            }
        }

        let trimmed = segment.trim();
        if trimmed.is_empty() {
            continue;
        }
        let truncated = truncate(trimmed, MAX_ENTRY_CHARS);
        out.push_str(&format!("{role_prefix}: {truncated}\n"));
    }
    out
}

/// Produce a terse summary of a tool-call input JSON value.
///
/// Emits the top-level keys in a compact form; never serialises values,
/// which can be arbitrarily large.
fn summarise_tool_input(input: &serde_json::Value) -> String {
    match input {
        serde_json::Value::Object(map) => {
            let keys: Vec<&str> = map.keys().map(String::as_str).collect();
            if keys.is_empty() {
                String::from("(no args)")
            } else {
                format!("args={{{}}}", keys.join(","))
            }
        }
        serde_json::Value::Null => String::from("(null)"),
        _ => String::from("(scalar)"),
    }
}

fn first_line(s: &str) -> String {
    match s.split_once('\n') {
        Some((head, _tail)) => head.to_string(),
        None => s.to_string(),
    }
}

fn truncate(s: &str, max_chars: usize) -> String {
    if s.len() <= max_chars {
        return s.to_string();
    }
    let mut end = max_chars;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &s[..end])
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use async_trait::async_trait;
    use sera_types::{
        EvictionPolicy, PutRequest, ScoredEntry, SemanticEntry, SemanticError,
        SemanticMemoryStore, SemanticQuery, SemanticStats,
    };

    use super::*;
    use crate::transcript::{ContentBlock, Role, Transcript, TranscriptEntry};

    /// Minimal mock of a [`SemanticMemoryStore`] sufficient for indexer tests.
    #[derive(Default)]
    struct MockStore {
        rows: Mutex<Vec<SemanticEntry>>,
        fail_on_put: bool,
    }

    impl MockStore {
        fn new() -> Self {
            Self::default()
        }

        fn with_failure() -> Self {
            Self {
                rows: Mutex::new(Vec::new()),
                fail_on_put: true,
            }
        }

        fn snapshot(&self) -> Vec<SemanticEntry> {
            self.rows.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl SemanticMemoryStore for MockStore {
        async fn put(&self, req: PutRequest) -> Result<MemoryId, SemanticError> {
            if self.fail_on_put {
                return Err(SemanticError::Backend("mock failure".into()));
            }
            let id = MemoryId::new(format!("session-transcript:{}", Uuid::new_v4()));
            let entry = SemanticEntry {
                id: id.clone(),
                agent_id: req.agent_id,
                content: req.content,
                embedding: req.supplied_embedding,
                tier: req.tier,
                tags: req.tags,
                created_at: Utc::now(),
                last_accessed_at: None,
                promoted: req.promoted,
                scope: req.scope,
            };
            self.rows.lock().unwrap().push(entry);
            Ok(id)
        }
        async fn query(
            &self,
            query: SemanticQuery,
        ) -> Result<Vec<ScoredEntry>, SemanticError> {
            let rows = self.rows.lock().unwrap().clone();
            let scored: Vec<ScoredEntry> = rows
                .into_iter()
                .filter(|e| e.agent_id == query.agent_id)
                .map(|entry| ScoredEntry {
                    entry,
                    score: 1.0,
                    index_score: 1.0,
                    vector_score: 0.0,
                    recency_score: 1.0,
                })
                .take(query.top_k)
                .collect();
            Ok(scored)
        }
        async fn delete(&self, _id: &MemoryId) -> Result<(), SemanticError> {
            Ok(())
        }
        async fn evict(&self, _p: &EvictionPolicy) -> Result<usize, SemanticError> {
            Ok(0)
        }
        async fn stats(&self) -> Result<SemanticStats, SemanticError> {
            Ok(SemanticStats {
                total_rows: self.rows.lock().unwrap().len(),
                per_agent_top: vec![],
                oldest: Utc::now(),
                newest: Utc::now(),
            })
        }
    }

    fn text_entry(role: Role, text: &str) -> TranscriptEntry {
        TranscriptEntry {
            id: Uuid::new_v4(),
            role,
            blocks: vec![ContentBlock::Text {
                text: text.to_string(),
            }],
            timestamp: Utc::now(),
            cause_by: None,
        }
    }

    fn assistant_with_tool_use(name: &str, args: serde_json::Value) -> TranscriptEntry {
        TranscriptEntry {
            id: Uuid::new_v4(),
            role: Role::Assistant,
            blocks: vec![ContentBlock::ToolUse {
                id: "tu_1".to_string(),
                name: name.to_string(),
                input: args,
            }],
            timestamp: Utc::now(),
            cause_by: None,
        }
    }

    fn tool_result(payload: &str) -> TranscriptEntry {
        TranscriptEntry {
            id: Uuid::new_v4(),
            role: Role::Tool,
            blocks: vec![ContentBlock::ToolResult {
                tool_use_id: "tu_1".to_string(),
                content: payload.to_string(),
                is_error: false,
            }],
            timestamp: Utc::now(),
            cause_by: None,
        }
    }

    fn sample_transcript() -> Transcript {
        let mut t = Transcript::new();
        t.append(text_entry(Role::User, "What is the invoice for 42?"));
        t.append(assistant_with_tool_use(
            "lookup_invoice",
            serde_json::json!({ "invoice_id": "42" }),
        ));
        t.append(tool_result(
            "GIANT JSON BLOB that should not be stored verbatim ".repeat(200).as_str(),
        ));
        t.append(text_entry(Role::Assistant, "Invoice 42 totals $100."));
        t
    }

    // ── extract_transcript_text ──────────────────────────────────────────────

    #[test]
    fn extract_includes_user_and_assistant_text() {
        let t = sample_transcript();
        let blob = extract_transcript_text(&t);
        assert!(blob.contains("user: What is the invoice for 42?"));
        assert!(blob.contains("assistant: Invoice 42 totals $100."));
    }

    #[test]
    fn extract_summarises_tool_calls_without_raw_io() {
        let t = sample_transcript();
        let blob = extract_transcript_text(&t);
        assert!(
            blob.contains("[tool:lookup_invoice]"),
            "expected tool-call summary, got: {blob}"
        );
        // The GIANT JSON BLOB tool_result content must not appear.
        assert!(
            !blob.contains("GIANT JSON BLOB"),
            "raw tool result leaked into transcript blob: {blob}"
        );
    }

    #[test]
    fn extract_skips_thinking_blocks() {
        let mut t = Transcript::new();
        t.append(TranscriptEntry {
            id: Uuid::new_v4(),
            role: Role::Assistant,
            blocks: vec![
                ContentBlock::Thinking {
                    thinking: "internal chain-of-thought must NEVER appear".to_string(),
                },
                ContentBlock::Text {
                    text: "Public answer.".to_string(),
                },
            ],
            timestamp: Utc::now(),
            cause_by: None,
        });
        let blob = extract_transcript_text(&t);
        assert!(!blob.contains("chain-of-thought"));
        assert!(blob.contains("Public answer."));
    }

    #[test]
    fn extract_handles_empty_transcript() {
        let t = Transcript::new();
        let blob = extract_transcript_text(&t);
        assert!(blob.is_empty());
    }

    #[test]
    fn extract_truncates_per_entry() {
        let long = "a".repeat(MAX_ENTRY_CHARS + 500);
        let mut t = Transcript::new();
        t.append(text_entry(Role::User, &long));
        let blob = extract_transcript_text(&t);
        // First-line truncation happens before the MAX_ENTRY_CHARS cap,
        // but the cap still bounds the emitted length.
        assert!(blob.len() <= MAX_ENTRY_CHARS + 64, "blob={blob}");
        assert!(blob.ends_with("…\n") || blob.ends_with("…"));
    }

    // ── SemanticTranscriptIndexer ────────────────────────────────────────────

    #[tokio::test]
    async fn indexer_puts_entry_with_archive_tier() {
        let store: Arc<dyn SemanticMemoryStore> = Arc::new(MockStore::new());
        let indexer = SemanticTranscriptIndexer::new(store.clone());

        let id = indexer
            .index_transcript(
                "agent-alice",
                "sess-123",
                Utc::now(),
                &sample_transcript(),
            )
            .await
            .unwrap();
        assert!(id.as_str().starts_with("session-transcript:"));
    }

    #[tokio::test]
    async fn indexer_entry_metadata_propagated() {
        let store: Arc<MockStore> = Arc::new(MockStore::new());
        let indexer = SemanticTranscriptIndexer::new(store.clone() as Arc<dyn SemanticMemoryStore>);

        indexer
            .index_transcript(
                "agent-alice",
                "sess-xyz",
                Utc::now(),
                &sample_transcript(),
            )
            .await
            .unwrap();

        let rows = store.snapshot();
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(row.agent_id, "agent-alice");
        assert!(row.tags.iter().any(|t| t == TRANSCRIPT_TAG));
        assert!(row.tags.iter().any(|t| t == "session_id:sess-xyz"));
        assert!(matches!(row.tier, SegmentKind::Custom(ref s) if s == TRANSCRIPT_TIER_LABEL));
        assert!(row.content.contains("user: What is the invoice"));
    }

    #[tokio::test]
    async fn indexer_empty_transcript_returns_empty_error() {
        let store: Arc<dyn SemanticMemoryStore> = Arc::new(MockStore::new());
        let indexer = SemanticTranscriptIndexer::new(store);
        let err = indexer
            .index_transcript("agent-a", "sess-empty", Utc::now(), &Transcript::new())
            .await
            .unwrap_err();
        assert!(matches!(err, IndexerError::Empty));
    }

    #[tokio::test]
    async fn indexer_propagates_store_error() {
        let store: Arc<dyn SemanticMemoryStore> = Arc::new(MockStore::with_failure());
        let indexer = SemanticTranscriptIndexer::new(store);
        let err = indexer
            .index_transcript("agent-a", "sess-fail", Utc::now(), &sample_transcript())
            .await
            .unwrap_err();
        assert!(matches!(err, IndexerError::Store(_)));
    }

    #[tokio::test]
    async fn indexed_transcript_retrievable_by_query() {
        let mock: Arc<MockStore> = Arc::new(MockStore::new());
        let indexer = SemanticTranscriptIndexer::new(mock.clone() as Arc<dyn SemanticMemoryStore>);

        indexer
            .index_transcript(
                "agent-recall",
                "sess-recall",
                Utc::now(),
                &sample_transcript(),
            )
            .await
            .unwrap();

        let hits = mock
            .query(SemanticQuery {
                agent_id: "agent-recall".into(),
                tier_filter: None,
                text: None,
                query_embedding: None,
                top_k: 10,
                similarity_threshold: None,
                scope: None,
            })
            .await
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert!(hits[0].entry.tags.iter().any(|t| t == TRANSCRIPT_TAG));
    }

    #[tokio::test]
    async fn indexer_failure_does_not_panic_caller() {
        // Mimics the session-close call site: caller ignores the error to
        // preserve the close path. This test documents that contract.
        let store: Arc<dyn SemanticMemoryStore> = Arc::new(MockStore::with_failure());
        let indexer = SemanticTranscriptIndexer::new(store);
        let result = indexer
            .index_transcript("a", "s", Utc::now(), &sample_transcript())
            .await;
        // Caller pattern: log-and-continue.
        if let Err(err) = result {
            let _ = err.to_string();
        }
    }

    // ── summarise_tool_input ─────────────────────────────────────────────────

    #[test]
    fn summarise_tool_input_lists_keys() {
        let input = serde_json::json!({ "a": 1, "b": 2 });
        let s = summarise_tool_input(&input);
        assert!(s.contains("args={"));
        assert!(s.contains('a'));
        assert!(s.contains('b'));
    }

    #[test]
    fn summarise_tool_input_handles_empty_object() {
        let input = serde_json::json!({});
        assert_eq!(summarise_tool_input(&input), "(no args)");
    }

    #[test]
    fn summarise_tool_input_scalar() {
        let input = serde_json::json!("hello");
        assert_eq!(summarise_tool_input(&input), "(scalar)");
    }
}
