//! Source ingestion pipeline — fetch → extract → assemble [`KnowledgeBlock`].
//!
//! This module provides the structural pipeline with stub-ready traits so that
//! LLM-backed implementations can be plugged in without changing the pipeline.

use anyhow::anyhow;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Core data types
// ---------------------------------------------------------------------------

/// Where a piece of source content comes from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceRef {
    pub uri: String,
    pub kind: SourceKind,
}

/// Discriminant for a [`SourceRef`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    Url,
    File,
    Text,
}

/// A single extracted fact with its provenance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestedFact {
    pub text: String,
    /// Confidence in [0.0, 1.0].
    pub confidence: f32,
    pub source: SourceRef,
}

/// A fully assembled knowledge block produced by the ingestion pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeBlock {
    /// UUID v4 identifier.
    pub id: String,
    /// Short human-readable title derived from the source URI.
    pub title: String,
    /// Full raw body text fetched from the source.
    pub body: String,
    /// Extracted facts from the body.
    pub facts: Vec<IngestedFact>,
    /// IDs of related or contradicting knowledge blocks.
    pub cross_refs: Vec<String>,
}

// ---------------------------------------------------------------------------
// Pipeline traits
// ---------------------------------------------------------------------------

/// Fetches raw text content from a [`SourceRef`].
///
/// Implementors may perform HTTP requests, read files, or return inline text.
/// Stub implementations return canned content for testing.
#[async_trait]
pub trait SourceFetcher: Send + Sync {
    async fn fetch(&self, source: &SourceRef) -> anyhow::Result<String>;
}

/// Extracts facts from raw text content.
///
/// Production implementations will call an LLM; stubs return fixed slices.
#[async_trait]
pub trait FactExtractor: Send + Sync {
    async fn extract(&self, text: &str, source: &SourceRef) -> anyhow::Result<Vec<IngestedFact>>;
}

// ---------------------------------------------------------------------------
// Pipeline
// ---------------------------------------------------------------------------

/// Composes a [`SourceFetcher`] and a [`FactExtractor`] into a single
/// fetch → extract → assemble pipeline.
pub struct IngestionPipeline<F: SourceFetcher, E: FactExtractor> {
    pub fetcher: F,
    pub extractor: E,
}

impl<F: SourceFetcher, E: FactExtractor> IngestionPipeline<F, E> {
    /// Run the full ingestion pipeline for `source`.
    ///
    /// Steps:
    /// 1. Fetch raw text via [`SourceFetcher::fetch`].
    /// 2. Extract facts via [`FactExtractor::extract`].
    /// 3. Assemble a [`KnowledgeBlock`] with a fresh UUID and a title derived
    ///    from the last path segment of the URI.
    pub async fn ingest(&self, source: SourceRef) -> anyhow::Result<KnowledgeBlock> {
        // Step 1 — fetch.
        let body = self
            .fetcher
            .fetch(&source)
            .await
            .map_err(|e| anyhow!("fetch failed for '{}': {e:#}", source.uri))?;

        // Step 2 — extract facts.
        let facts = self
            .extractor
            .extract(&body, &source)
            .await
            .map_err(|e| anyhow!("extraction failed for '{}': {e:#}", source.uri))?;

        // Step 3 — assemble.
        let id = Uuid::new_v4().to_string();
        let title = derive_title(&source.uri);

        Ok(KnowledgeBlock {
            id,
            title,
            body,
            facts,
            cross_refs: Vec::new(),
        })
    }
}

/// Derive a short title from a URI by taking the last non-empty path segment.
fn derive_title(uri: &str) -> String {
    // Strip query / fragment first.
    let base = uri.split(['?', '#']).next().unwrap_or(uri);
    base.trim_end_matches('/')
        .rsplit('/')
        .find(|s| !s.is_empty())
        .unwrap_or(uri)
        .to_string()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- Mock implementations -----------------------------------------------

    struct MockFetcher {
        content: String,
    }

    #[async_trait]
    impl SourceFetcher for MockFetcher {
        async fn fetch(&self, _source: &SourceRef) -> anyhow::Result<String> {
            Ok(self.content.clone())
        }
    }

    struct FailingFetcher;

    #[async_trait]
    impl SourceFetcher for FailingFetcher {
        async fn fetch(&self, source: &SourceRef) -> anyhow::Result<String> {
            Err(anyhow!("network unreachable: {}", source.uri))
        }
    }

    struct MockExtractor;

    #[async_trait]
    impl FactExtractor for MockExtractor {
        async fn extract(
            &self,
            _text: &str,
            source: &SourceRef,
        ) -> anyhow::Result<Vec<IngestedFact>> {
            Ok(vec![
                IngestedFact {
                    text: "SERA supports Docker-native sandboxing".to_string(),
                    confidence: 0.95,
                    source: source.clone(),
                },
                IngestedFact {
                    text: "SERA agents communicate via Centrifugo".to_string(),
                    confidence: 0.88,
                    source: source.clone(),
                },
            ])
        }
    }

    fn make_url_source(uri: &str) -> SourceRef {
        SourceRef {
            uri: uri.to_string(),
            kind: SourceKind::Url,
        }
    }

    // --- Tests ---------------------------------------------------------------

    #[tokio::test]
    async fn test_ingest_produces_knowledge_block_with_two_facts() {
        let pipeline = IngestionPipeline {
            fetcher: MockFetcher {
                content: "SERA is a multi-agent orchestration platform.".to_string(),
            },
            extractor: MockExtractor,
        };

        let source = make_url_source("https://example.com/docs/sera-overview");
        let block = pipeline.ingest(source).await.expect("ingest should succeed");

        assert_eq!(block.facts.len(), 2);
        assert!(!block.id.is_empty(), "id must be populated");
        assert_eq!(block.title, "sera-overview");
        assert!(block.body.contains("multi-agent"));
        assert!(block.cross_refs.is_empty());
    }

    #[tokio::test]
    async fn test_ingest_error_propagated_when_fetch_fails() {
        let pipeline = IngestionPipeline {
            fetcher: FailingFetcher,
            extractor: MockExtractor,
        };

        let source = make_url_source("https://unreachable.internal/data");
        let err = pipeline
            .ingest(source)
            .await
            .expect_err("should fail when fetcher errors");

        let msg = err.to_string();
        assert!(
            msg.contains("fetch failed"),
            "error message should mention fetch: {msg}"
        );
        assert!(
            msg.contains("unreachable.internal"),
            "error message should include URI: {msg}"
        );
    }

    #[tokio::test]
    async fn test_derive_title_strips_query_and_fragment() {
        assert_eq!(derive_title("https://example.com/page?foo=bar"), "page");
        assert_eq!(derive_title("https://example.com/section#anchor"), "section");
        assert_eq!(derive_title("https://example.com/a/b/c/"), "c");
        assert_eq!(derive_title("plain-text"), "plain-text");
    }

    #[tokio::test]
    async fn test_ingest_body_equals_fetcher_output() {
        let expected_body = "raw content from source";
        let pipeline = IngestionPipeline {
            fetcher: MockFetcher {
                content: expected_body.to_string(),
            },
            extractor: MockExtractor,
        };

        let block = pipeline
            .ingest(make_url_source("https://example.com/item"))
            .await
            .expect("ingest should succeed");

        assert_eq!(block.body, expected_body);
    }
}
