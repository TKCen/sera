//! Deterministic stub embedding provider for tests.
//!
//! `StubEmbeddingService` derives vectors from a SipHash-equivalent
//! hash of each input text. The result is deterministic, cheap, and
//! stable across runs — ideal for unit tests that want to exercise
//! callers without standing up a real embedding backend.
//!
//! The output is NOT a meaningful semantic embedding. Do not use this
//! in production code paths.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use async_trait::async_trait;
use sera_types::{EmbeddingError, EmbeddingHealth, EmbeddingService};

/// Deterministic embedding provider that derives vectors from a hash of
/// the input text. Values lie in `[-1, 1)` so callers can sanity-check
/// cosine-similarity consumers with representative magnitudes.
#[derive(Debug, Clone)]
pub struct StubEmbeddingService {
    model_id: String,
    dimensions: usize,
}

impl StubEmbeddingService {
    /// Construct a stub with the default `stub-embed` model id and 384
    /// dimensions (matching `all-minilm-l6-v2`, the smallest model in
    /// SERA's supported set).
    pub fn new() -> Self {
        Self::with_dimensions(384)
    }

    /// Construct a stub with a caller-chosen dimensionality. Useful for
    /// exercising callers that expect a specific model shape (e.g. 1536
    /// for `text-embedding-3-small`, 768 for `nomic-embed-text`).
    pub fn with_dimensions(dimensions: usize) -> Self {
        Self {
            model_id: "stub-embed".to_string(),
            dimensions,
        }
    }

    /// Construct a stub with both a custom model id and dimensionality.
    pub fn with_model(model_id: impl Into<String>, dimensions: usize) -> Self {
        Self {
            model_id: model_id.into(),
            dimensions,
        }
    }

    fn embed_one(&self, text: &str) -> Vec<f32> {
        let mut out = Vec::with_capacity(self.dimensions);
        // Seed from the text hash; each dimension is derived from a
        // second hash that mixes in its index, giving a stable, spread
        // signature without pulling in a crypto dependency.
        let mut seeder = DefaultHasher::new();
        text.hash(&mut seeder);
        let seed = seeder.finish();

        for i in 0..self.dimensions {
            let mut h = DefaultHasher::new();
            seed.hash(&mut h);
            (i as u64).hash(&mut h);
            let raw = h.finish();
            // Map to [-1, 1) by taking the u64, normalising into [0, 1),
            // then shifting.
            let unit = (raw as f64) / (u64::MAX as f64);
            let v = (unit * 2.0) - 1.0;
            out.push(v as f32);
        }
        out
    }
}

impl Default for StubEmbeddingService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl EmbeddingService for StubEmbeddingService {
    fn model_id(&self) -> &str {
        &self.model_id
    }

    fn dimensions(&self) -> usize {
        self.dimensions
    }

    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        Ok(texts.iter().map(|t| self.embed_one(t)).collect())
    }

    async fn health(&self) -> Result<EmbeddingHealth, EmbeddingError> {
        Ok(EmbeddingHealth {
            available: true,
            detail: "stub embedding service".to_string(),
            latency_ms: Some(0),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn metadata_reports_configured_values() {
        let svc = StubEmbeddingService::with_model("stub-test", 512);
        assert_eq!(svc.model_id(), "stub-test");
        assert_eq!(svc.dimensions(), 512);
    }

    #[tokio::test]
    async fn embed_returns_vectors_of_correct_shape() {
        let svc = StubEmbeddingService::with_dimensions(384);
        let texts = vec!["hello".to_string(), "world".to_string()];
        let vectors = svc.embed(&texts).await.unwrap();
        assert_eq!(vectors.len(), 2);
        assert_eq!(vectors[0].len(), 384);
        assert_eq!(vectors[1].len(), 384);
    }

    #[tokio::test]
    async fn embed_is_deterministic_for_same_text() {
        let svc = StubEmbeddingService::new();
        let texts = vec!["reproducible".to_string()];
        let a = svc.embed(&texts).await.unwrap();
        let b = svc.embed(&texts).await.unwrap();
        assert_eq!(a, b);
    }

    #[tokio::test]
    async fn embed_differs_across_inputs() {
        let svc = StubEmbeddingService::new();
        let a = svc.embed(&["alpha".to_string()]).await.unwrap();
        let b = svc.embed(&["beta".to_string()]).await.unwrap();
        assert_ne!(a, b);
    }

    #[tokio::test]
    async fn embed_values_are_within_unit_range() {
        let svc = StubEmbeddingService::with_dimensions(32);
        let vectors = svc.embed(&["sample".to_string()]).await.unwrap();
        for v in &vectors[0] {
            assert!((-1.0..1.0).contains(v), "value {v} out of range");
        }
    }

    #[tokio::test]
    async fn empty_input_returns_empty_output() {
        let svc = StubEmbeddingService::new();
        let vectors = svc.embed(&[]).await.unwrap();
        assert!(vectors.is_empty());
    }

    #[tokio::test]
    async fn health_is_always_available() {
        let svc = StubEmbeddingService::new();
        let h = svc.health().await.unwrap();
        assert!(h.available);
    }
}
