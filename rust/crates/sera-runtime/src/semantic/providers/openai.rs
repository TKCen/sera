//! OpenAI-backed embedding provider.
//!
//! Talks to `https://api.openai.com/v1/embeddings` (or any
//! OpenAI-compatible base URL via [`OpenAIEmbeddingService::new`]).
//! Reads `OPENAI_API_KEY` and `SERA_EMBEDDING_MODEL` from the environment
//! in [`OpenAIEmbeddingService::from_env`].
//!
//! Per SPEC-memory §13.3, this provider NEVER returns silent zero-vector
//! fallbacks. Every failure is an [`EmbeddingError`]; callers decide
//! whether to retry, queue, or fail the enclosing operation.

use async_trait::async_trait;
use reqwest::Client;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use serde::Deserialize;
use std::time::Duration;

use sera_types::{EmbeddingError, EmbeddingHealth, EmbeddingService};

/// Default OpenAI base URL.
pub const DEFAULT_OPENAI_BASE_URL: &str = "https://api.openai.com";
/// Default embedding model (`text-embedding-3-small`, 1536 dims).
pub const DEFAULT_OPENAI_MODEL: &str = "text-embedding-3-small";
/// `text-embedding-3-small` output dimensionality.
pub const DEFAULT_OPENAI_DIMENSIONS: usize = 1536;

/// Live-API OpenAI embedding provider.
#[derive(Debug, Clone)]
pub struct OpenAIEmbeddingService {
    client: Client,
    base_url: String,
    api_key: String,
    model_id: String,
    dimensions: usize,
}

impl OpenAIEmbeddingService {
    /// Build a provider pointing at an arbitrary OpenAI-compatible
    /// endpoint. The API key is required; empty keys are rejected so a
    /// misconfiguration fails loudly at construction time.
    pub fn new(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        model_id: impl Into<String>,
        dimensions: usize,
    ) -> Result<Self, EmbeddingError> {
        let api_key: String = api_key.into();
        if api_key.trim().is_empty() {
            return Err(EmbeddingError::Provider(
                "OpenAI API key is empty".to_string(),
            ));
        }
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| EmbeddingError::Transport(format!("build reqwest client: {e}")))?;
        Ok(Self {
            client,
            base_url: base_url.into(),
            api_key,
            model_id: model_id.into(),
            dimensions,
        })
    }

    /// Convenience constructor reading `OPENAI_API_KEY` and
    /// `SERA_EMBEDDING_MODEL` from the environment, defaulting to the
    /// canonical OpenAI host + `text-embedding-3-small`.
    pub fn from_env() -> Result<Self, EmbeddingError> {
        let key = std::env::var("OPENAI_API_KEY").map_err(|_| {
            EmbeddingError::Provider("OPENAI_API_KEY not set".to_string())
        })?;
        let model = std::env::var("SERA_EMBEDDING_MODEL")
            .unwrap_or_else(|_| DEFAULT_OPENAI_MODEL.to_string());
        Self::new(
            DEFAULT_OPENAI_BASE_URL.to_string(),
            key,
            model,
            DEFAULT_OPENAI_DIMENSIONS,
        )
    }

    /// Override the advertised dimensionality. Needed for `text-embedding-3-large`
    /// (3072) or when using OpenAI's `dimensions` request parameter.
    pub fn with_dimensions(mut self, dimensions: usize) -> Self {
        self.dimensions = dimensions;
        self
    }

    fn base(&self) -> &str {
        self.base_url.trim_end_matches('/')
    }

    fn auth_headers(&self) -> Result<HeaderMap, EmbeddingError> {
        let mut headers = HeaderMap::new();
        let auth = HeaderValue::from_str(&format!("Bearer {}", self.api_key))
            .map_err(|e| EmbeddingError::Provider(format!("invalid api key for header: {e}")))?;
        headers.insert(AUTHORIZATION, auth);
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        Ok(headers)
    }
}

#[derive(Debug, Deserialize)]
struct OpenAIEmbedResponse {
    data: Vec<OpenAIEmbedItem>,
}

#[derive(Debug, Deserialize)]
struct OpenAIEmbedItem {
    #[serde(default)]
    index: usize,
    embedding: Vec<f32>,
}

#[async_trait]
impl EmbeddingService for OpenAIEmbeddingService {
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

        let url = format!("{}/v1/embeddings", self.base());
        let body = serde_json::json!({
            "model": self.model_id,
            "input": texts,
        });

        let resp = self
            .client
            .post(&url)
            .headers(self.auth_headers()?)
            .json(&body)
            .send()
            .await
            .map_err(|e| EmbeddingError::Transport(e.to_string()))?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            if status.as_u16() == 404 {
                return Err(EmbeddingError::ModelNotAvailable(format!(
                    "openai model {} not found: {body}",
                    self.model_id
                )));
            }
            return Err(EmbeddingError::Provider(format!(
                "openai HTTP {status}: {body}"
            )));
        }

        let mut parsed: OpenAIEmbedResponse = resp
            .json()
            .await
            .map_err(|e| EmbeddingError::Provider(format!("decode openai response: {e}")))?;

        // Preserve caller order by sorting on `index`.
        parsed.data.sort_by_key(|d| d.index);

        if parsed.data.len() != texts.len() {
            return Err(EmbeddingError::Provider(format!(
                "openai returned {} embeddings for {} inputs",
                parsed.data.len(),
                texts.len()
            )));
        }

        let mut out = Vec::with_capacity(parsed.data.len());
        for item in parsed.data {
            if item.embedding.len() != self.dimensions {
                return Err(EmbeddingError::DimensionMismatch {
                    expected: self.dimensions,
                    got: item.embedding.len(),
                });
            }
            out.push(item.embedding);
        }
        Ok(out)
    }

    async fn health(&self) -> Result<EmbeddingHealth, EmbeddingError> {
        let url = format!("{}/v1/models/{}", self.base(), self.model_id);
        let start = std::time::Instant::now();
        let resp = self
            .client
            .get(&url)
            .headers(self.auth_headers()?)
            .send()
            .await
            .map_err(|e| EmbeddingError::Transport(e.to_string()))?;

        let latency_ms = start.elapsed().as_millis() as u64;
        let status = resp.status();
        if !status.is_success() {
            let available = false;
            let detail = format!("openai /v1/models/{} returned HTTP {status}", self.model_id);
            return Ok(EmbeddingHealth {
                available,
                detail,
                latency_ms: Some(latency_ms),
            });
        }

        Ok(EmbeddingHealth {
            available: true,
            detail: format!("openai ok (model={})", self.model_id),
            latency_ms: Some(latency_ms),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_roundtrip() {
        let svc = OpenAIEmbeddingService::new(
            DEFAULT_OPENAI_BASE_URL,
            "sk-test",
            "text-embedding-3-small",
            1536,
        )
        .unwrap();
        assert_eq!(svc.model_id(), "text-embedding-3-small");
        assert_eq!(svc.dimensions(), 1536);
    }

    #[test]
    fn empty_api_key_is_rejected() {
        let err = OpenAIEmbeddingService::new(
            DEFAULT_OPENAI_BASE_URL,
            "   ",
            "text-embedding-3-small",
            1536,
        )
        .expect_err("empty key must fail construction");
        match err {
            EmbeddingError::Provider(_) => {}
            other => panic!("expected Provider error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn transport_failure_bubbles_as_error() {
        let svc = OpenAIEmbeddingService::new(
            "http://127.0.0.1:1",
            "sk-test",
            "text-embedding-3-small",
            1536,
        )
        .unwrap();
        let err = svc
            .embed(&["hi".to_string()])
            .await
            .expect_err("no listener should accept");
        match err {
            EmbeddingError::Transport(_) => {}
            other => panic!("expected Transport error, got {other:?}"),
        }
    }

    #[cfg(feature = "integration")]
    mod integration {
        use super::*;

        #[tokio::test]
        async fn health_probe_against_live_openai() {
            let svc = OpenAIEmbeddingService::from_env().unwrap();
            let h = svc.health().await.expect("health probe");
            assert!(h.available, "live openai expected; got {h:?}");
        }

        #[tokio::test]
        async fn embed_against_live_openai() {
            let svc = OpenAIEmbeddingService::from_env().unwrap();
            let v = svc.embed(&["hello world".to_string()]).await.unwrap();
            assert_eq!(v.len(), 1);
            assert_eq!(v[0].len(), svc.dimensions());
        }

        #[tokio::test]
        async fn embed_empty_is_no_op_against_live_openai() {
            let svc = OpenAIEmbeddingService::from_env().unwrap();
            let v = svc.embed(&[]).await.unwrap();
            assert!(v.is_empty());
        }
    }
}
