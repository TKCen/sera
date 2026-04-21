//! Hindsight HTTP adapter for [`SemanticMemoryStore`].
//!
//! Hindsight is a server-side memory service. It owns embeddings entirely;
//! callers supply plain text and hindsight embeds and stores it. There is
//! no concept of a caller-supplied embedding vector.
//!
//! ## Bank-id encoding
//!
//! Hindsight organises memories into named "banks". This adapter maps a
//! [`sera_memory::Scope`] to a bank id with the scheme:
//!
//! ```text
//! scope_kind:scope_key   →   bank_id
//! Agent("agent-1")       →   "agent:agent-1"
//! Circle("circle-a")     →   "circle:circle-a"
//! Org("my-org")          →   "org:my-org"
//! Global                 →   "global:"
//! ```
//!
//! The encoding is stable and reversible. All bank-id construction goes
//! through [`scope_to_bank_id`] so future changes remain in one place.
//!
//! ## Put-polling
//!
//! `POST /v1/default/banks/{bank_id}/memories` may return an
//! `operation_id` in its response body. When non-null the adapter polls
//! `GET /v1/default/banks/{bank_id}/operations/{id}` until the operation
//! reaches a terminal state (`completed` or `failed`), honouring
//! [`HindsightConfig::poll_interval`] and [`HindsightConfig::poll_max_attempts`].
//! When `operation_id` is null the put is considered immediately complete.
//!
//! ## Unsupported operations
//!
//! Hindsight does not support per-memory delete, bulk-evict, or stats queries.
//! Those methods return [`SemanticError::Backend`] with a clear message.
//! `promote`, `touch`, and `maintenance` inherit the trait defaults (no-op /
//! `Backend` not-implemented).
//!
//! ## `reflect` is NOT mapped
//!
//! Hindsight's `POST /v1/default/banks/{bank_id}/reflect` is a synchronous
//! RAG Q&A endpoint, not a memory-retention primitive. It does not belong in
//! the `SemanticMemoryStore` surface. A future bead may expose it separately.

use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};
use uuid::Uuid;

use sera_memory::store::{
    EvictionPolicy, MemoryId, PutRequest, Scope, ScoredEntry, SemanticEntry, SemanticError,
    SemanticMemoryStore, SemanticQuery, SemanticStats,
};
use sera_types::memory::SegmentKind;

// ── Config ─────────────────────────────────────────────────────────────────────

/// Configuration for the Hindsight HTTP adapter.
#[derive(Debug, Clone)]
pub struct HindsightConfig {
    /// Base URL of the Hindsight service. Default: `"http://localhost:8888"`.
    pub base_url: String,
    /// HTTP request timeout. Default: 30 seconds.
    pub timeout: Duration,
    /// How long to wait between operation-status polls. Default: 500 ms.
    pub poll_interval: Duration,
    /// Maximum number of poll attempts before giving up. Default: 20.
    pub poll_max_attempts: u32,
    /// Optional Bearer token for Hindsight authentication.
    pub bearer_token: Option<String>,
}

impl Default for HindsightConfig {
    fn default() -> Self {
        Self {
            base_url: "http://localhost:8888".into(),
            timeout: Duration::from_secs(30),
            poll_interval: Duration::from_millis(500),
            poll_max_attempts: 20,
            bearer_token: None,
        }
    }
}

// ── Bank-id encoding ───────────────────────────────────────────────────────────

/// Convert a [`Scope`] to a Hindsight bank id.
///
/// Encoding: `"{kind}:{key}"` where `kind` is the stable discriminant
/// (`"agent"`, `"circle"`, `"org"`, `"global"`) and `key` is the scope's
/// associated string (empty for `Global`).
///
/// Examples:
/// - `Agent("agent-1")` → `"agent:agent-1"`
/// - `Circle("c")` → `"circle:c"`
/// - `Global` → `"global:"`
pub fn scope_to_bank_id(scope: &Scope) -> String {
    format!("{}:{}", scope.kind_str(), scope.key_str())
}

/// Derive a bank id from a [`PutRequest`].
///
/// Uses `req.scope` when present; falls back to `Agent(req.agent_id)`.
fn bank_id_for_put(req: &PutRequest) -> String {
    match &req.scope {
        Some(scope) => scope_to_bank_id(scope),
        None => scope_to_bank_id(&Scope::Agent(req.agent_id.clone())),
    }
}

/// Derive a bank id from a [`SemanticQuery`].
///
/// Uses `query.scope` when present; falls back to `Agent(query.agent_id)`.
fn bank_id_for_query(query: &SemanticQuery) -> String {
    match &query.scope {
        Some(scope) => scope_to_bank_id(scope),
        None => scope_to_bank_id(&Scope::Agent(query.agent_id.clone())),
    }
}

// ── Hindsight wire types ───────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct PutMemoryItem<'a> {
    content: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    metadata: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
struct PutMemoryBody<'a> {
    items: Vec<PutMemoryItem<'a>>,
}

#[derive(Debug, Deserialize)]
struct PutMemoryResponse {
    /// Hindsight assigns a canonical id to the stored memory.
    #[serde(default)]
    id: Option<String>,
    /// Non-null when the put is processed asynchronously.
    #[serde(default)]
    operation_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OperationStatus {
    /// `"pending"`, `"completed"`, or `"failed"`.
    state: String,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Debug, Serialize)]
struct RecallBody<'a> {
    query: &'a str,
    top_k: usize,
}

#[derive(Debug, Deserialize)]
struct RecallResult {
    #[serde(default)]
    id: Option<String>,
    content: String,
    #[serde(default)]
    score: f32,
}

#[derive(Debug, Deserialize)]
struct RecallResponse {
    #[serde(default)]
    results: Vec<RecallResult>,
}

// ── Adapter ────────────────────────────────────────────────────────────────────

/// [`SemanticMemoryStore`] backed by the Hindsight HTTP API.
///
/// Hindsight owns embeddings; `supplied_embedding` on [`PutRequest`] is always
/// ignored. See module-level documentation for the full API mapping.
pub struct HindsightStore {
    client: Client,
    config: HindsightConfig,
}

impl HindsightStore {
    /// Construct a store with the given configuration.
    ///
    /// Returns an error if the `reqwest` client cannot be built (e.g.
    /// invalid TLS configuration).
    pub fn new(config: HindsightConfig) -> Result<Self, SemanticError> {
        let mut builder = Client::builder().timeout(config.timeout);
        if let Some(token) = &config.bearer_token {
            let mut headers = reqwest::header::HeaderMap::new();
            let value = reqwest::header::HeaderValue::from_str(&format!("Bearer {token}"))
                .map_err(|e| SemanticError::Backend(format!("invalid bearer token: {e}")))?;
            headers.insert(reqwest::header::AUTHORIZATION, value);
            builder = builder.default_headers(headers);
        }
        let client = builder
            .build()
            .map_err(|e| SemanticError::Backend(format!("reqwest client build failed: {e}")))?;
        Ok(Self { client, config })
    }

    /// `POST /v1/default/banks/{bank_id}/memories`
    async fn put_memory(
        &self,
        bank_id: &str,
        req: &PutRequest,
    ) -> Result<PutMemoryResponse, SemanticError> {
        let url = format!(
            "{}/v1/default/banks/{}/memories",
            self.config.base_url, bank_id
        );
        let metadata = serde_json::json!({
            "agent_id": req.agent_id,
            "tier": format!("{:?}", req.tier),
            "tags": req.tags,
            "promoted": req.promoted,
        });
        let body = PutMemoryBody {
            items: vec![PutMemoryItem {
                content: &req.content,
                metadata: Some(metadata),
            }],
        };
        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| SemanticError::Backend(format!("hindsight put request failed: {e}")))?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(SemanticError::Backend(format!(
                "hindsight put returned {status}: {text}"
            )));
        }

        resp.json::<PutMemoryResponse>()
            .await
            .map_err(|e| SemanticError::Backend(format!("hindsight put response parse: {e}")))
    }

    /// `GET /v1/default/banks/{bank_id}/operations/{op_id}`
    ///
    /// Polls until the operation is terminal or `poll_max_attempts` is
    /// exhausted.
    async fn poll_operation(&self, bank_id: &str, operation_id: &str) -> Result<(), SemanticError> {
        let url = format!(
            "{}/v1/default/banks/{}/operations/{}",
            self.config.base_url, bank_id, operation_id
        );
        for attempt in 1..=self.config.poll_max_attempts {
            tokio::time::sleep(self.config.poll_interval).await;
            let resp = self.client.get(&url).send().await.map_err(|e| {
                SemanticError::Backend(format!("hindsight poll request failed: {e}"))
            })?;

            let status = resp.status();
            if !status.is_success() {
                let text = resp.text().await.unwrap_or_default();
                return Err(SemanticError::Backend(format!(
                    "hindsight operation poll returned {status}: {text}"
                )));
            }

            let op: OperationStatus = resp
                .json()
                .await
                .map_err(|e| SemanticError::Backend(format!("hindsight operation parse: {e}")))?;

            debug!(
                operation_id,
                attempt,
                state = %op.state,
                "hindsight operation poll"
            );

            match op.state.as_str() {
                "completed" => return Ok(()),
                "failed" => {
                    return Err(SemanticError::Backend(format!(
                        "hindsight operation {operation_id} failed: {}",
                        op.error.unwrap_or_else(|| "no details".into())
                    )));
                }
                _ => {
                    // still pending — keep polling
                }
            }
        }

        Err(SemanticError::Backend(format!(
            "hindsight operation {operation_id} did not complete after {} attempts",
            self.config.poll_max_attempts
        )))
    }

    /// `POST /v1/default/banks/{bank_id}/memories/recall`
    async fn recall_memories(
        &self,
        bank_id: &str,
        query_text: &str,
        top_k: usize,
    ) -> Result<Vec<RecallResult>, SemanticError> {
        let url = format!(
            "{}/v1/default/banks/{}/memories/recall",
            self.config.base_url, bank_id
        );
        let body = RecallBody {
            query: query_text,
            top_k,
        };
        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| SemanticError::Backend(format!("hindsight recall request failed: {e}")))?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(SemanticError::Backend(format!(
                "hindsight recall returned {status}: {text}"
            )));
        }

        let recall: RecallResponse = resp
            .json()
            .await
            .map_err(|e| SemanticError::Backend(format!("hindsight recall parse: {e}")))?;

        Ok(recall.results)
    }
}

// ── SemanticMemoryStore impl ───────────────────────────────────────────────────

#[async_trait]
impl SemanticMemoryStore for HindsightStore {
    /// Persist content to Hindsight. `supplied_embedding` is always ignored —
    /// Hindsight owns embeddings server-side (SPEC-memory-pluggability §3).
    ///
    /// If the response carries a non-null `operation_id`, the adapter polls
    /// the operations endpoint until the operation reaches a terminal state.
    async fn put(&self, req: PutRequest) -> Result<MemoryId, SemanticError> {
        let bank_id = bank_id_for_put(&req);
        let put_resp = self.put_memory(&bank_id, &req).await?;

        // Opportunistic async path: poll until terminal.
        if let Some(op_id) = &put_resp.operation_id {
            self.poll_operation(&bank_id, op_id).await?;
        }

        // Return the server-assigned id, or a client-generated UUID when the
        // server does not echo one back.
        let id = put_resp.id.unwrap_or_else(|| Uuid::new_v4().to_string());
        Ok(MemoryId::new(id))
    }

    /// Semantic search via Hindsight recall. Embeddings are server-side;
    /// this method requires [`SemanticQuery::text`] to be set and ignores
    /// `query_embedding`. Returns [`SemanticError::Backend`] when no query
    /// text is supplied (Hindsight has no raw-vector recall endpoint).
    ///
    /// All returned [`SemanticEntry`]s have `embedding: None` (server-owned).
    async fn query(&self, query: SemanticQuery) -> Result<Vec<ScoredEntry>, SemanticError> {
        let query_text = query.text.as_deref().ok_or_else(|| {
            SemanticError::Backend(
                "hindsight backend requires query.text; raw-vector recall is not supported".into(),
            )
        })?;

        let bank_id = bank_id_for_query(&query);
        let results = self
            .recall_memories(&bank_id, query_text, query.top_k)
            .await?;

        let now = Utc::now();
        let scored: Vec<ScoredEntry> = results
            .into_iter()
            .map(|r| {
                let id = r.id.unwrap_or_else(|| Uuid::new_v4().to_string());
                ScoredEntry {
                    entry: SemanticEntry {
                        id: MemoryId::new(id),
                        agent_id: query.agent_id.clone(),
                        content: r.content,
                        embedding: None, // server-owned; not returned
                        tier: SegmentKind::MemoryRecall(String::new()),
                        tags: Vec::new(),
                        created_at: now,
                        last_accessed_at: None,
                        promoted: false,
                        scope: query.scope.clone(),
                    },
                    score: r.score,
                    index_score: 0.0,
                    vector_score: r.score,
                    recency_score: 0.0,
                }
            })
            .collect();

        Ok(scored)
    }

    /// Per-memory delete is not supported by Hindsight.
    async fn delete(&self, id: &MemoryId) -> Result<(), SemanticError> {
        warn!(memory_id = %id, "hindsight does not support per-memory delete");
        Err(SemanticError::Backend(
            "hindsight does not support per-memory delete".into(),
        ))
    }

    /// Bulk eviction is not supported by Hindsight.
    async fn evict(&self, _policy: &EvictionPolicy) -> Result<usize, SemanticError> {
        Err(SemanticError::Backend(
            "hindsight does not support bulk eviction".into(),
        ))
    }

    /// Aggregate stats are not supported by Hindsight.
    async fn stats(&self) -> Result<SemanticStats, SemanticError> {
        Err(SemanticError::Backend(
            "hindsight does not support aggregate stats".into(),
        ))
    }
    // promote(), touch(), maintenance() inherit trait defaults:
    //   promote()     → Backend("promote() not implemented for this backend")
    //   touch()       → Ok(())   (no-op)
    //   maintenance() → Ok(())   (no-op)
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    // ── helpers ──────────────────────────────────────────────────────────────

    fn fast_config(base_url: String) -> HindsightConfig {
        HindsightConfig {
            base_url,
            timeout: Duration::from_secs(5),
            poll_interval: Duration::from_millis(10),
            poll_max_attempts: 5,
            bearer_token: None,
        }
    }

    fn put_req(agent_id: &str) -> PutRequest {
        PutRequest::new(
            agent_id,
            "Hello, Hindsight!",
            SegmentKind::MemoryRecall("r-1".into()),
        )
    }

    // ── scope_to_bank_id ─────────────────────────────────────────────────────

    #[test]
    fn bank_id_agent() {
        assert_eq!(
            scope_to_bank_id(&Scope::Agent("agent-1".into())),
            "agent:agent-1"
        );
    }

    #[test]
    fn bank_id_circle() {
        assert_eq!(scope_to_bank_id(&Scope::Circle("c".into())), "circle:c");
    }

    #[test]
    fn bank_id_org() {
        assert_eq!(scope_to_bank_id(&Scope::Org("my-org".into())), "org:my-org");
    }

    #[test]
    fn bank_id_global() {
        assert_eq!(scope_to_bank_id(&Scope::Global), "global:");
    }

    #[test]
    fn bank_id_encoding_is_stable() {
        // Ensure the colon separator never leaks into the kind component.
        let id = scope_to_bank_id(&Scope::Agent("a:b".into()));
        assert_eq!(id, "agent:a:b");
        let (kind, _key) = id.split_once(':').unwrap();
        assert_eq!(kind, "agent");
    }

    // ── put — sync path (operation_id null) ──────────────────────────────────

    #[tokio::test]
    async fn put_sync_returns_immediately_when_no_operation_id() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/default/banks/agent:agent-1/memories"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"id": "mem-abc", "operation_id": null})),
            )
            .mount(&server)
            .await;

        let store = HindsightStore::new(fast_config(server.uri())).unwrap();
        let id = store.put(put_req("agent-1")).await.unwrap();
        assert_eq!(id.as_str(), "mem-abc");
    }

    // ── put — async path (operation_id set, polls until completed) ───────────

    #[tokio::test]
    async fn put_async_polls_until_completed() {
        let server = MockServer::start().await;

        // The put returns an operation_id.
        Mock::given(method("POST"))
            .and(path("/v1/default/banks/agent:agent-1/memories"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"id": "mem-xyz", "operation_id": "op-99"})),
            )
            .mount(&server)
            .await;

        // First poll → pending; second poll → completed.
        Mock::given(method("GET"))
            .and(path("/v1/default/banks/agent:agent-1/operations/op-99"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"state": "pending"})),
            )
            .up_to_n_times(1)
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/v1/default/banks/agent:agent-1/operations/op-99"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"state": "completed"})),
            )
            .mount(&server)
            .await;

        let store = HindsightStore::new(fast_config(server.uri())).unwrap();
        let id = store.put(put_req("agent-1")).await.unwrap();
        assert_eq!(id.as_str(), "mem-xyz");
    }

    // ── query returns entries with embedding: None ────────────────────────────

    #[tokio::test]
    async fn query_returns_entries_with_no_embedding() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/default/banks/agent:agent-1/memories/recall"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "results": [
                    {"id": "r-1", "content": "first result", "score": 0.9},
                    {"id": "r-2", "content": "second result", "score": 0.7},
                ]
            })))
            .mount(&server)
            .await;

        let store = HindsightStore::new(fast_config(server.uri())).unwrap();
        let q = SemanticQuery {
            agent_id: "agent-1".into(),
            scope: None,
            tier_filter: None,
            text: Some("what do I know?".into()),
            query_embedding: None,
            top_k: 5,
            similarity_threshold: None,
        };
        let results = store.query(q).await.unwrap();
        assert_eq!(results.len(), 2);
        for scored in &results {
            assert!(
                scored.entry.embedding.is_none(),
                "embedding must be None for hindsight results"
            );
        }
        assert_eq!(results[0].entry.content, "first result");
        assert!((results[0].score - 0.9).abs() < 1e-6);
    }

    // ── delete returns Backend error ──────────────────────────────────────────

    #[tokio::test]
    async fn delete_returns_backend_error() {
        let store = HindsightStore::new(HindsightConfig::default()).unwrap();
        let err = store.delete(&MemoryId::new("any-id")).await.unwrap_err();
        match &err {
            SemanticError::Backend(msg) => {
                assert!(
                    msg.contains("per-memory delete"),
                    "expected 'per-memory delete' in: {msg}"
                );
            }
            other => panic!("expected Backend error, got: {other:?}"),
        }
    }

    // ── evict returns Backend error ───────────────────────────────────────────

    #[tokio::test]
    async fn evict_returns_backend_error() {
        let store = HindsightStore::new(HindsightConfig::default()).unwrap();
        let err = store.evict(&EvictionPolicy::default()).await.unwrap_err();
        match &err {
            SemanticError::Backend(msg) => {
                assert!(
                    msg.contains("bulk eviction"),
                    "expected 'bulk eviction' in: {msg}"
                );
            }
            other => panic!("expected Backend error, got: {other:?}"),
        }
    }

    // ── stats returns Backend error ───────────────────────────────────────────

    #[tokio::test]
    async fn stats_returns_backend_error() {
        let store = HindsightStore::new(HindsightConfig::default()).unwrap();
        let err = store.stats().await.unwrap_err();
        match &err {
            SemanticError::Backend(msg) => {
                assert!(
                    msg.contains("aggregate stats"),
                    "expected 'aggregate stats' in: {msg}"
                );
            }
            other => panic!("expected Backend error, got: {other:?}"),
        }
    }

    // ── optional integration test (gated on HINDSIGHT_URL) ───────────────────

    /// Live integration test — runs only when `HINDSIGHT_URL` is set.
    ///
    /// ```bash
    /// HINDSIGHT_URL=http://localhost:8888 cargo test -p sera-memory-hindsight -- --ignored
    /// ```
    #[tokio::test]
    #[ignore]
    async fn integration_put_and_recall() {
        let base_url =
            std::env::var("HINDSIGHT_URL").unwrap_or_else(|_| "http://localhost:8888".into());
        let config = HindsightConfig {
            base_url,
            ..HindsightConfig::default()
        };
        let store = HindsightStore::new(config).unwrap();
        let req = PutRequest::new(
            "integration-agent",
            "The quick brown fox jumps over the lazy dog.",
            SegmentKind::MemoryRecall("integ-1".into()),
        );
        let id = store.put(req).await.expect("put should succeed");
        assert!(!id.as_str().is_empty());

        let q = SemanticQuery {
            agent_id: "integration-agent".into(),
            scope: None,
            tier_filter: None,
            text: Some("quick fox".into()),
            query_embedding: None,
            top_k: 3,
            similarity_threshold: None,
        };
        let results = store.query(q).await.expect("recall should succeed");
        assert!(!results.is_empty(), "expected at least one result");
    }
}
