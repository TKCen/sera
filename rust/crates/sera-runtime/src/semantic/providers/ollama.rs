//! Ollama-backed embedding provider.
//!
//! Talks to a local or remote Ollama daemon via:
//!
//! - `GET  {base}/api/version`  for health probes
//! - `POST {base}/api/embeddings` for embedding generation
//!
//! Per SPEC-memory §13.3 this provider NEVER returns a silent zero-vector
//! fallback. Any transport, decode, or upstream failure propagates as an
//! [`EmbeddingError`]; it is the caller's job to decide whether to retry
//! or to degrade.

use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;
use std::time::Duration;

use sera_types::{EmbeddingError, EmbeddingHealth, EmbeddingService};

/// Default Ollama base URL used when `OLLAMA_BASE_URL` is unset.
pub const DEFAULT_OLLAMA_BASE_URL: &str = "http://localhost:11434";
/// Default embedding model used when `SERA_EMBEDDING_MODEL` is unset.
pub const DEFAULT_OLLAMA_MODEL: &str = "nomic-embed-text";
/// `nomic-embed-text` output dimensionality. Other models need a custom
/// builder call; see [`OllamaEmbeddingService::with_dimensions`].
pub const DEFAULT_OLLAMA_DIMENSIONS: usize = 768;

/// Live-API Ollama embedding provider.
#[derive(Debug, Clone)]
pub struct OllamaEmbeddingService {
    client: Client,
    base_url: String,
    model_id: String,
    dimensions: usize,
}

impl OllamaEmbeddingService {
    /// Build a provider with the given base URL / model / dimensions.
    ///
    /// Returns `EmbeddingError::Transport` if the underlying `reqwest`
    /// client cannot be constructed (e.g. TLS backend init failure).
    pub fn new(
        base_url: impl Into<String>,
        model_id: impl Into<String>,
        dimensions: usize,
    ) -> Result<Self, EmbeddingError> {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| EmbeddingError::Transport(format!("build reqwest client: {e}")))?;
        Ok(Self {
            client,
            base_url: base_url.into(),
            model_id: model_id.into(),
            dimensions,
        })
    }

    /// Convenience constructor reading `OLLAMA_BASE_URL` and
    /// `SERA_EMBEDDING_MODEL` from the environment, with the standard
    /// `nomic-embed-text` defaults.
    pub fn from_env() -> Result<Self, EmbeddingError> {
        let base = std::env::var("OLLAMA_BASE_URL")
            .unwrap_or_else(|_| DEFAULT_OLLAMA_BASE_URL.to_string());
        let model = std::env::var("SERA_EMBEDDING_MODEL")
            .unwrap_or_else(|_| DEFAULT_OLLAMA_MODEL.to_string());
        Self::new(base, model, DEFAULT_OLLAMA_DIMENSIONS)
    }

    /// Override the advertised dimensionality. Use this when targeting an
    /// Ollama model whose output length differs from `nomic-embed-text`.
    pub fn with_dimensions(mut self, dimensions: usize) -> Self {
        self.dimensions = dimensions;
        self
    }

    fn base(&self) -> &str {
        self.base_url.trim_end_matches('/')
    }
}

#[derive(Debug, Deserialize)]
struct OllamaEmbedResponse {
    #[serde(default)]
    embedding: Option<Vec<f32>>,
    /// Newer Ollama builds return `embeddings: [[...]]` for batch mode.
    #[serde(default)]
    embeddings: Option<Vec<Vec<f32>>>,
}

#[derive(Debug, Deserialize)]
struct OllamaVersionResponse {
    #[serde(default)]
    version: Option<String>,
}

#[async_trait]
impl EmbeddingService for OllamaEmbeddingService {
    fn model_id(&self) -> &str {
        &self.model_id
    }

    fn dimensions(&self) -> usize {
        self.dimensions
    }

    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        if texts.is_empty() {
            return Ok(vec![]);
        }

        let url = format!("{}/api/embeddings", self.base());
        let mut out = Vec::with_capacity(texts.len());

        // Ollama's /api/embeddings takes one `prompt` per call. We issue
        // one request per text and fail loudly on the first error — no
        // silent partials, no zero-vector fallback.
        for text in texts {
            let body = serde_json::json!({
                "model": self.model_id,
                "prompt": text,
            });

            let resp = self
                .client
                .post(&url)
                .json(&body)
                .send()
                .await
                .map_err(|e| EmbeddingError::Transport(e.to_string()))?;

            let status = resp.status();
            if !status.is_success() {
                let body = resp.text().await.unwrap_or_default();
                if status.as_u16() == 404 {
                    return Err(EmbeddingError::ModelNotAvailable(format!(
                        "ollama model {} not found: {body}",
                        self.model_id
                    )));
                }
                return Err(EmbeddingError::Provider(format!(
                    "ollama HTTP {status}: {body}"
                )));
            }

            let parsed: OllamaEmbedResponse = resp
                .json()
                .await
                .map_err(|e| EmbeddingError::Provider(format!("decode ollama response: {e}")))?;

            let vec = parsed
                .embedding
                .or_else(|| parsed.embeddings.and_then(|mut v| v.pop()))
                .ok_or_else(|| {
                    EmbeddingError::Provider(
                        "ollama response missing `embedding` field".to_string(),
                    )
                })?;

            if vec.len() != self.dimensions {
                return Err(EmbeddingError::DimensionMismatch {
                    expected: self.dimensions,
                    got: vec.len(),
                });
            }
            out.push(vec);
        }

        Ok(out)
    }

    async fn health(&self) -> Result<EmbeddingHealth, EmbeddingError> {
        let url = format!("{}/api/version", self.base());
        let start = std::time::Instant::now();
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| EmbeddingError::Transport(e.to_string()))?;

        let latency_ms = start.elapsed().as_millis() as u64;
        let status = resp.status();
        if !status.is_success() {
            return Ok(EmbeddingHealth {
                available: false,
                detail: format!("ollama /api/version returned HTTP {status}"),
                latency_ms: Some(latency_ms),
            });
        }

        let parsed: OllamaVersionResponse = resp
            .json()
            .await
            .map_err(|e| EmbeddingError::Provider(format!("decode ollama version: {e}")))?;

        Ok(EmbeddingHealth {
            available: true,
            detail: format!(
                "ollama ok (version={})",
                parsed.version.as_deref().unwrap_or("unknown")
            ),
            latency_ms: Some(latency_ms),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_roundtrip() {
        let svc = OllamaEmbeddingService::new(
            "http://localhost:11434".to_string(),
            "nomic-embed-text".to_string(),
            768,
        )
        .unwrap();
        assert_eq!(svc.model_id(), "nomic-embed-text");
        assert_eq!(svc.dimensions(), 768);
    }

    #[test]
    fn builder_override_applies() {
        let svc = OllamaEmbeddingService::new("http://localhost:11434", "bge-small", 768)
            .unwrap()
            .with_dimensions(384);
        assert_eq!(svc.dimensions(), 384);
    }

    #[tokio::test]
    async fn transport_failure_bubbles_as_error() {
        // Route DNS to a port we know nothing listens on.
        let svc =
            OllamaEmbeddingService::new("http://127.0.0.1:1", "nomic-embed-text", 768).unwrap();
        let err = svc
            .embed(&["hi".to_string()])
            .await
            .expect_err("no service should be listening");
        match err {
            EmbeddingError::Transport(_) => {}
            other => panic!("expected Transport error, got {other:?}"),
        }
    }

    #[cfg(feature = "integration")]
    mod integration {
        use super::*;

        #[tokio::test]
        async fn health_probe_against_live_ollama() {
            let svc = OllamaEmbeddingService::from_env().unwrap();
            let h = svc.health().await.expect("health probe");
            assert!(h.available, "live ollama expected; got {h:?}");
        }

        #[tokio::test]
        async fn embed_against_live_ollama() {
            let svc = OllamaEmbeddingService::from_env().unwrap();
            let v = svc.embed(&["hello world".to_string()]).await.unwrap();
            assert_eq!(v.len(), 1);
            assert_eq!(v[0].len(), svc.dimensions());
        }

        #[tokio::test]
        async fn embed_empty_is_no_op_against_live_ollama() {
            let svc = OllamaEmbeddingService::from_env().unwrap();
            let v = svc.embed(&[]).await.unwrap();
            assert!(v.is_empty());
        }
    }
}
