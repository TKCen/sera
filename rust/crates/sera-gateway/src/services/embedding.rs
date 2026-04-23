//! Ad-hoc gateway-side embedding + Qdrant client (legacy scaffold).
//!
//! This module predates the [`sera_types::EmbeddingService`] trait and the
//! provider implementations in `sera-runtime::semantic::providers`. It
//! remains here only until the gateway is fully migrated to the new
//! trait-driven pipeline (tracked by the Tier-2 memory plan); a future
//! bead retires it outright.
//!
//! NOTE: The earlier implementation silently returned a zero-vector
//! placeholder on any Ollama error, shipping degenerate embeddings into
//! downstream cosine-similarity calls. Per SPEC-memory §13.3 and bug
//! `sera-px3w`, that fallback has been removed: every failure now
//! bubbles as an [`EmbeddingError`] and callers must handle the `Err`
//! path explicitly (return 500/503, do NOT re-introduce zero-vectors).

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::RwLock;

#[derive(Debug, Error)]
pub enum EmbeddingError {
    #[error("Ollama service unavailable: {0}")]
    OllamaUnavailable(String),

    #[error("Qdrant error: {0}")]
    QdrantError(String),

    #[error("HTTP request failed: {0}")]
    HttpError(String),

    #[error("Cache error: {0}")]
    CacheError(String),

    #[error("Invalid response format: {0}")]
    InvalidResponse(String),
}

/// Search result from Qdrant semantic search
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SearchResult {
    pub id: String,
    pub score: f32,
    pub payload: serde_json::Value,
}

/// LRU cache for embedding vectors, keyed by text hash
struct EmbeddingCache {
    // Maps text -> vector
    cache: HashMap<String, Vec<f32>>,
    // LRU order: oldest first
    lru_order: VecDeque<String>,
    max_entries: usize,
}

impl EmbeddingCache {
    fn new(max_entries: usize) -> Self {
        Self {
            cache: HashMap::new(),
            lru_order: VecDeque::new(),
            max_entries,
        }
    }

    fn get(&mut self, text: &str) -> Option<Vec<f32>> {
        if let Some(vector) = self.cache.get(text) {
            // Move to end (most recently used)
            self.lru_order.retain(|t| t != text);
            self.lru_order.push_back(text.to_string());
            Some(vector.clone())
        } else {
            None
        }
    }

    fn insert(&mut self, text: String, vector: Vec<f32>) {
        if self.cache.contains_key(&text) {
            return; // Already exists, don't need to evict
        }

        // Evict oldest if at capacity
        if self.cache.len() >= self.max_entries
            && let Some(oldest) = self.lru_order.pop_front()
        {
            self.cache.remove(&oldest);
        }

        self.lru_order.push_back(text.clone());
        self.cache.insert(text, vector);
    }

    fn len(&self) -> usize {
        self.cache.len()
    }
}

pub struct AdHocEmbeddingClient {
    client: reqwest::Client,
    qdrant_url: String,
    ollama_url: String,
    cache: Arc<RwLock<EmbeddingCache>>,
}

impl AdHocEmbeddingClient {
    /// Create a new ad-hoc client with Qdrant and Ollama URLs.
    pub fn new(qdrant_url: String, ollama_url: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            qdrant_url,
            ollama_url,
            cache: Arc::new(RwLock::new(EmbeddingCache::new(10_000))),
        }
    }

    /// Generate an embedding for the given text.
    ///
    /// Any transport or decode failure is returned as [`EmbeddingError`].
    /// This method MUST NOT silently fall back to zero-vectors — see
    /// SPEC-memory §13.3 and bug `sera-px3w` for the rationale.
    pub async fn embed_text(&self, text: &str) -> Result<Vec<f32>, EmbeddingError> {
        // Check cache first
        {
            let mut cache = self.cache.write().await;
            if let Some(vector) = cache.get(text) {
                return Ok(vector);
            }
        }

        // Call Ollama
        let url = format!("{}/api/embeddings", self.ollama_url);
        let req_body = serde_json::json!({
            "model": "nomic-embed-text",
            "prompt": text
        });

        let response = self
            .client
            .post(&url)
            .json(&req_body)
            .send()
            .await
            .map_err(|e| EmbeddingError::OllamaUnavailable(e.to_string()))?;

        let body = response
            .json::<serde_json::Value>()
            .await
            .map_err(|e| EmbeddingError::InvalidResponse(e.to_string()))?;

        let embedding = body
            .get("embedding")
            .and_then(|e| e.as_array())
            .ok_or_else(|| {
                EmbeddingError::InvalidResponse(
                    "Missing 'embedding' array in Ollama response".to_string(),
                )
            })?
            .iter()
            .filter_map(|v| v.as_f64().map(|f| f as f32))
            .collect::<Vec<_>>();

        // Cache the result
        {
            let mut cache = self.cache.write().await;
            cache.insert(text.to_string(), embedding.clone());
        }

        Ok(embedding)
    }

    /// Upsert a point (document) into Qdrant collection.
    pub async fn upsert_point(
        &self,
        collection: &str,
        id: &str,
        vector: Vec<f32>,
        payload: serde_json::Value,
    ) -> Result<(), EmbeddingError> {
        let url = format!("{}/collections/{}/points", self.qdrant_url, collection);

        let point = serde_json::json!({
            "id": id,
            "vector": vector,
            "payload": payload
        });

        let req_body = serde_json::json!({
            "points": [point]
        });

        match self.client.put(&url).json(&req_body).send().await {
            Ok(response) => {
                if response.status().is_success() {
                    Ok(())
                } else {
                    let status = response.status();
                    let text = response.text().await.unwrap_or_default();
                    Err(EmbeddingError::QdrantError(format!(
                        "HTTP {}: {}",
                        status, text
                    )))
                }
            }
            Err(e) => Err(EmbeddingError::HttpError(e.to_string())),
        }
    }

    /// Search for similar vectors in Qdrant collection.
    pub async fn search_semantic(
        &self,
        collection: &str,
        query_vector: Vec<f32>,
        limit: usize,
    ) -> Result<Vec<SearchResult>, EmbeddingError> {
        let url = format!(
            "{}/collections/{}/points/search",
            self.qdrant_url, collection
        );

        let req_body = serde_json::json!({
            "vector": query_vector,
            "limit": limit,
            "with_payload": true
        });

        match self.client.post(&url).json(&req_body).send().await {
            Ok(response) => {
                if !response.status().is_success() {
                    let status = response.status();
                    let text = response.text().await.unwrap_or_default();
                    return Err(EmbeddingError::QdrantError(format!(
                        "HTTP {}: {}",
                        status, text
                    )));
                }

                match response.json::<serde_json::Value>().await {
                    Ok(body) => {
                        let results = body
                            .get("result")
                            .and_then(|r| r.as_array())
                            .ok_or_else(|| {
                                EmbeddingError::InvalidResponse(
                                    "Missing 'result' array in Qdrant response".to_string(),
                                )
                            })?
                            .iter()
                            .filter_map(|item| {
                                let id = item
                                    .get("id")
                                    .and_then(|v| v.as_str())
                                    .map(|s| s.to_string())?;
                                let score = item
                                    .get("score")
                                    .and_then(|v| v.as_f64())
                                    .map(|f| f as f32)?;
                                let payload = item
                                    .get("payload")
                                    .cloned()
                                    .unwrap_or(serde_json::json!({}));
                                Some(SearchResult { id, score, payload })
                            })
                            .collect();

                        Ok(results)
                    }
                    Err(e) => Err(EmbeddingError::InvalidResponse(e.to_string())),
                }
            }
            Err(e) => Err(EmbeddingError::HttpError(e.to_string())),
        }
    }

    /// Get cache statistics (for debugging/testing)
    pub async fn cache_size(&self) -> usize {
        let cache = self.cache.read().await;
        cache.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lru_cache_hit() {
        let mut cache = EmbeddingCache::new(5);
        let vector = vec![1.0, 2.0, 3.0];
        cache.insert("test".to_string(), vector.clone());

        let result = cache.get("test");
        assert_eq!(result, Some(vector));
    }

    #[test]
    fn test_lru_cache_miss() {
        let mut cache = EmbeddingCache::new(5);
        let result = cache.get("nonexistent");
        assert_eq!(result, None);
    }

    #[test]
    fn test_lru_cache_eviction() {
        let mut cache = EmbeddingCache::new(3);

        // Fill cache
        cache.insert("a".to_string(), vec![1.0]);
        cache.insert("b".to_string(), vec![2.0]);
        cache.insert("c".to_string(), vec![3.0]);

        assert_eq!(cache.len(), 3);

        // Add one more — should evict 'a' (oldest)
        cache.insert("d".to_string(), vec![4.0]);

        assert_eq!(cache.len(), 3);
        assert_eq!(cache.get("a"), None);
        assert_eq!(cache.get("b"), Some(vec![2.0]));
    }

    #[test]
    fn test_lru_cache_recency() {
        let mut cache = EmbeddingCache::new(3);

        cache.insert("a".to_string(), vec![1.0]);
        cache.insert("b".to_string(), vec![2.0]);
        cache.insert("c".to_string(), vec![3.0]);

        // Access 'a' to make it recent
        cache.get("a");

        // Add 'd' — should evict 'b' (oldest)
        cache.insert("d".to_string(), vec![4.0]);

        assert_eq!(cache.get("a"), Some(vec![1.0]));
        assert_eq!(cache.get("b"), None);
    }

    #[test]
    fn test_search_result_serialization() {
        let result = SearchResult {
            id: "test-id".to_string(),
            score: 0.95,
            payload: serde_json::json!({"key": "value"}),
        };

        let json = serde_json::to_string(&result).expect("Should serialize");
        let parsed: SearchResult = serde_json::from_str(&json).expect("Should deserialize");

        assert_eq!(parsed.id, "test-id");
        assert_eq!(parsed.score, 0.95);
    }

    #[tokio::test]
    async fn test_embedding_service_creation() {
        let service = AdHocEmbeddingClient::new(
            "http://localhost:6333".to_string(),
            "http://localhost:11434".to_string(),
        );

        assert_eq!(service.cache_size().await, 0);
    }
}
