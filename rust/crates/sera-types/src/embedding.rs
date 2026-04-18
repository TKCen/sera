//! Embedding service abstractions.
//!
//! Per SPEC-memory §13.2, sera-runtime hosts the concrete providers
//! (`OllamaEmbeddingService`, `OpenAIEmbeddingService`, `StubEmbeddingService`)
//! while the trait lives here so that sera-types stays the dependency-free
//! leaf crate.
//!
//! ## Contract
//!
//! - `model_id()` returns the opaque identifier of the embedding model
//!   (e.g. `nomic-embed-text`, `text-embedding-3-small`). This is used for
//!   telemetry and to guard against cross-model cosine comparisons.
//! - `dimensions()` reports the expected output vector length. Providers
//!   MUST return vectors of exactly this length.
//! - `embed(&texts)` produces one vector per input text, preserving order.
//!   Empty input slices are valid and return `Ok(vec![])`.
//! - `health()` probes the upstream provider. Implementations SHOULD avoid
//!   expensive embedding calls and prefer a cheap metadata endpoint.
//!
//! ## Error Policy
//!
//! Providers MUST NOT silently fall back to zero-vectors, mock data, or any
//! "degraded" output on failure. Every upstream problem must propagate as
//! an [`EmbeddingError`] so callers can make their own fail-loudly choice.
//! See sera-px3w for the bug this policy exists to prevent.

use async_trait::async_trait;
use thiserror::Error;

/// Health status reported by an [`EmbeddingService`] implementation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddingHealth {
    /// `true` iff the provider is reachable and the configured model is
    /// usable. `false` means `embed()` calls will fail.
    pub available: bool,
    /// Human-readable detail suitable for logs / status dashboards.
    pub detail: String,
    /// Optional round-trip latency observed during the probe, in
    /// milliseconds.
    pub latency_ms: Option<u64>,
}

/// Errors raised by an [`EmbeddingService`] implementation.
///
/// The variants mirror SPEC-memory §13.3's required error taxonomy so
/// callers can make routing decisions (retry vs. fail vs. degrade) without
/// string-matching on error messages.
#[derive(Debug, Error)]
pub enum EmbeddingError {
    /// Upstream provider returned a logical error (bad response shape,
    /// authentication rejected, model missing, rate-limited, etc).
    #[error("embedding provider error: {0}")]
    Provider(String),

    /// Transport-level failure (DNS, TCP, TLS, reqwest IO error).
    #[error("embedding transport error: {0}")]
    Transport(String),

    /// The requested model is not available on the configured provider.
    #[error("embedding model not available: {0}")]
    ModelNotAvailable(String),

    /// Provider returned a vector whose dimensionality differs from the
    /// trait's advertised `dimensions()`.
    #[error("embedding dimension mismatch: expected {expected}, got {got}")]
    DimensionMismatch { expected: usize, got: usize },
}

/// Async embedding-provider abstraction.
///
/// Implementations live in `sera-runtime::semantic::providers::*`. Callers
/// should hold a `Box<dyn EmbeddingService>` or `Arc<dyn EmbeddingService>`
/// and depend only on this trait.
#[async_trait]
pub trait EmbeddingService: Send + Sync + 'static {
    /// Opaque identifier of the configured embedding model.
    fn model_id(&self) -> &str;

    /// Expected output vector length. Vectors returned from [`Self::embed`]
    /// MUST have exactly this length.
    fn dimensions(&self) -> usize;

    /// Generate one embedding vector per input text, preserving order.
    ///
    /// An empty `texts` slice must return `Ok(vec![])` without any upstream
    /// call. Providers MUST NOT silently replace failed embeddings with
    /// zero-vectors; any error must surface as an [`EmbeddingError`].
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, EmbeddingError>;

    /// Probe the provider. Implementations SHOULD use a cheap metadata or
    /// version endpoint rather than a real embedding call.
    async fn health(&self) -> Result<EmbeddingHealth, EmbeddingError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display_includes_variant_context() {
        let e = EmbeddingError::DimensionMismatch {
            expected: 768,
            got: 384,
        };
        let s = e.to_string();
        assert!(s.contains("768"));
        assert!(s.contains("384"));
    }

    #[test]
    fn health_equality() {
        let a = EmbeddingHealth {
            available: true,
            detail: "ok".into(),
            latency_ms: Some(12),
        };
        let b = EmbeddingHealth {
            available: true,
            detail: "ok".into(),
            latency_ms: Some(12),
        };
        assert_eq!(a, b);
    }
}
