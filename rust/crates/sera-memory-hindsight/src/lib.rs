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
//! ## Read-only `reflect` (RAG Q&A)
//!
//! Hindsight's `POST /v1/default/banks/{bank_id}/reflect` is a synchronous
//! RAG Q&A endpoint: it synthesises an answer from existing memories without
//! retaining anything. Because it does not match the retention semantics of
//! [`SemanticMemoryStore::put`], it is exposed as the SERA-owned inherent
//! method [`HindsightStore::reflect`] rather than on the trait surface — the
//! same separation the spec applies to `HindsightStore::list`
//! (SPEC-memory-pluggability §4). Reflect is always read-only and is allowed
//! even when the store is configured [`HindsightConfig::read_only`]. Results
//! carry a `provenance` label so injected/read answers stay attributable.
//!
//! ## Write governance
//!
//! Retain/write is governed by SERA, not passed through uncontrolled. When
//! [`HindsightConfig::read_only`] is set (the gateway default for the
//! external Hindsight backend), [`HindsightStore::put`] refuses **before** any
//! HTTP/provider mutation, returns a clear policy error to the caller, emits a
//! structured `tracing` governance event on the `sera.memory.governance`
//! target, and notifies the optional [`WriteAuditSink`] supplied via
//! [`HindsightStore::with_audit_sink`]. Recall/reflect failures degrade read
//! context truthfully; write failures are surfaced explicitly, never swallowed.

use std::sync::Arc;
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
    /// Optional fixed bank id. When set, all reads/writes use this Hindsight
    /// bank instead of deriving a bank from SERA memory scope.
    pub bank_id_override: Option<String>,
    /// When true, `put` refuses to write before making any HTTP request.
    pub read_only: bool,
    /// Hindsight recall budget (`low`, `mid`, `high`) for bounded prefetch/tool recall.
    pub recall_budget: Option<String>,
    /// Maximum tokens returned by Hindsight recall when the server supports it.
    pub recall_max_tokens: Option<usize>,
    /// Optional Hindsight memory types to request (for example `observation`).
    pub recall_types: Vec<String>,
}

impl Default for HindsightConfig {
    fn default() -> Self {
        Self {
            base_url: "http://localhost:8888".into(),
            timeout: Duration::from_secs(30),
            poll_interval: Duration::from_millis(500),
            poll_max_attempts: 20,
            bearer_token: None,
            bank_id_override: None,
            read_only: false,
            recall_budget: Some("low".to_string()),
            recall_max_tokens: Some(500),
            recall_types: vec!["observation".to_string()],
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
    #[serde(skip_serializing_if = "Option::is_none")]
    budget: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    types: Option<&'a [String]>,
}

#[derive(Debug, Deserialize)]
struct RecallResult {
    #[serde(default)]
    id: Option<String>,
    #[serde(alias = "text")]
    content: String,
    #[serde(default)]
    score: f32,
}

#[derive(Debug, Deserialize)]
struct RecallResponse {
    #[serde(default)]
    results: Vec<RecallResult>,
}

/// Serializes to `{}` — an empty `include` sub-option marker.
#[derive(Debug, Serialize)]
struct IncludeFacts {}

/// Reflect `include` controls. Requesting `facts` makes the live endpoint
/// return `based_on` evidence; without it a successful reflect carries no
/// supporting memories and [`ReflectAnswer::sources`] is empty even when
/// supporting memories exist.
#[derive(Debug, Serialize)]
struct ReflectInclude {
    facts: IncludeFacts,
}

#[derive(Debug, Serialize)]
struct ReflectBody<'a> {
    query: &'a str,
    /// Always request `facts` so `based_on` evidence is returned for
    /// [`ReflectAnswer::sources`].
    include: ReflectInclude,
    #[serde(skip_serializing_if = "Option::is_none")]
    budget: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    types: Option<&'a [String]>,
}

#[derive(Debug, Deserialize)]
struct ReflectSourceWire {
    #[serde(default)]
    id: Option<String>,
    #[serde(alias = "text", alias = "fact")]
    content: String,
    #[serde(default)]
    score: f32,
}

/// The documented live `based_on` shape: a `ReflectBasedOn` object whose cited
/// memories live under `based_on.memories`. A bare array is also tolerated for
/// compatibility with the earlier/alternate shape. `null` is handled one level
/// up by `Option<ReflectBasedOn>`, so a null `based_on` yields no sources
/// instead of a parse error.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ReflectBasedOn {
    /// `based_on: { "memories": [...] }`
    Object {
        #[serde(default)]
        memories: Vec<ReflectSourceWire>,
    },
    /// `based_on: [...]` — legacy/alternate array form.
    Array(Vec<ReflectSourceWire>),
}

impl ReflectBasedOn {
    fn into_memories(self) -> Vec<ReflectSourceWire> {
        match self {
            ReflectBasedOn::Object { memories } => memories,
            ReflectBasedOn::Array(memories) => memories,
        }
    }
}

#[derive(Debug, Deserialize)]
struct ReflectResponse {
    /// Hindsight's live reflect endpoint returns the generated answer under
    /// `text`; `answer` is accepted as a compatible alias.
    #[serde(default, alias = "text")]
    answer: String,
    /// Array-shaped evidence under `sources` (with `results` as a compatible
    /// alias). Used as-is and merged with any `based_on.memories`.
    #[serde(default, alias = "results")]
    sources: Vec<ReflectSourceWire>,
    /// Documented live evidence container: an object `{ memories: [...] }`, a
    /// bare array, or `null`. Parsed as its own field so a null/object value
    /// never turns a successful reflect into a parse error.
    #[serde(default)]
    based_on: Option<ReflectBasedOn>,
}

// ── Public reflect types ─────────────────────────────────────────────────────────

/// A single source memory cited by a [`HindsightStore::reflect`] answer.
///
/// Carried verbatim so callers can render or audit the evidence behind a
/// synthesised answer.
#[derive(Debug, Clone)]
pub struct ReflectSource {
    /// Hindsight memory id, when the server returns one.
    pub id: Option<String>,
    /// Memory content backing the answer.
    pub content: String,
    /// Relevance score reported by Hindsight (`0.0` when absent).
    pub score: f32,
}

/// Result of a read-only Hindsight `reflect` (synchronous RAG Q&A).
#[derive(Debug, Clone)]
pub struct ReflectAnswer {
    /// The synthesised answer text.
    pub answer: String,
    /// Source memories cited by Hindsight, preserved for provenance.
    pub sources: Vec<ReflectSource>,
    /// Provenance label identifying the backend + bank that produced this
    /// answer (for example `"hindsight:reflect:agent:agent-1"`). Injected /
    /// read results stay attributable to their origin.
    pub provenance: String,
}

// ── Write governance ─────────────────────────────────────────────────────────────

/// A write-governance decision recorded when a retain/write is refused
/// **before** any provider mutation.
#[derive(Debug, Clone)]
pub struct WriteDenial {
    /// Hindsight bank the write targeted.
    pub bank_id: String,
    /// Caller agent identity.
    pub agent_id: String,
    /// Operation that was refused (for example `"put"`).
    pub operation: &'static str,
    /// Machine-stable reason (for example `"read_only"`).
    pub reason: &'static str,
}

/// Sink for SERA-owned write-governance decisions.
///
/// The gateway supplies an implementation that bridges denials to its OCSF
/// audit log; tests use a recording stub. When no sink is set, denials are
/// still emitted via `tracing` on the `sera.memory.governance` target and the
/// caller still receives the policy error — the sink only adds a programmatic
/// hook for callers that own audit primitives.
pub trait WriteAuditSink: Send + Sync {
    /// Called exactly once per refused write, before the policy error is
    /// returned to the caller.
    fn record_denial(&self, denial: &WriteDenial);
}

// ── Adapter ────────────────────────────────────────────────────────────────────

/// [`SemanticMemoryStore`] backed by the Hindsight HTTP API.
///
/// Hindsight owns embeddings; `supplied_embedding` on [`PutRequest`] is always
/// ignored. See module-level documentation for the full API mapping.
pub struct HindsightStore {
    client: Client,
    config: HindsightConfig,
    audit_sink: Option<Arc<dyn WriteAuditSink>>,
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
        Ok(Self {
            client,
            config,
            audit_sink: None,
        })
    }

    /// Attach a [`WriteAuditSink`] that is notified whenever write governance
    /// refuses a retain/write before any provider mutation.
    pub fn with_audit_sink(mut self, sink: Arc<dyn WriteAuditSink>) -> Self {
        self.audit_sink = Some(sink);
        self
    }

    /// SERA-owned write-governance gate. Returns `Ok(())` when writes are
    /// permitted; otherwise emits a structured governance event, notifies the
    /// optional [`WriteAuditSink`], and returns a clear policy error — all
    /// **before** any HTTP/provider mutation.
    fn ensure_write_allowed(&self, agent_id: &str, bank_id: &str) -> Result<(), SemanticError> {
        if !self.config.read_only {
            return Ok(());
        }
        let denial = WriteDenial {
            bank_id: bank_id.to_string(),
            agent_id: agent_id.to_string(),
            operation: "put",
            reason: "read_only",
        };
        warn!(
            target: "sera.memory.governance",
            decision = "denied",
            reason = denial.reason,
            operation = denial.operation,
            bank_id = %denial.bank_id,
            agent_id = %denial.agent_id,
            "hindsight write refused by SERA governance"
        );
        if let Some(sink) = &self.audit_sink {
            sink.record_denial(&denial);
        }
        Err(SemanticError::Backend(format!(
            "hindsight write refused by SERA governance (read_only): \
             retain/write disabled for bank {bank_id}; no provider mutation attempted"
        )))
    }

    fn bank_id_for_put(&self, req: &PutRequest) -> String {
        self.config
            .bank_id_override
            .clone()
            .unwrap_or_else(|| bank_id_for_put(req))
    }

    fn bank_id_for_query(&self, query: &SemanticQuery) -> String {
        self.config
            .bank_id_override
            .clone()
            .unwrap_or_else(|| bank_id_for_query(query))
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
            budget: self.config.recall_budget.as_deref(),
            max_tokens: self.config.recall_max_tokens,
            types: if self.config.recall_types.is_empty() {
                None
            } else {
                Some(self.config.recall_types.as_slice())
            },
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

    /// Read-only RAG Q&A via Hindsight `reflect`.
    ///
    /// `POST /v1/default/banks/{bank_id}/reflect` synthesises an answer from
    /// existing memories; it never retains anything, so it is permitted even
    /// when the store is configured [`HindsightConfig::read_only`]. Requires
    /// [`SemanticQuery::text`] (Hindsight reflect has no raw-vector form) and
    /// returns [`SemanticError::Backend`] when text is absent.
    ///
    /// Failures degrade truthfully: a provider error surfaces as
    /// [`SemanticError::Backend`] rather than an empty answer. The returned
    /// [`ReflectAnswer::provenance`] labels the backend + bank so injected
    /// answers stay attributable.
    pub async fn reflect(&self, query: &SemanticQuery) -> Result<ReflectAnswer, SemanticError> {
        let query_text = query.text.as_deref().ok_or_else(|| {
            SemanticError::Backend(
                "hindsight reflect requires query.text; raw-vector reflect is not supported".into(),
            )
        })?;

        let bank_id = self.bank_id_for_query(query);
        let url = format!(
            "{}/v1/default/banks/{}/reflect",
            self.config.base_url, bank_id
        );
        let body = ReflectBody {
            query: query_text,
            include: ReflectInclude {
                facts: IncludeFacts {},
            },
            budget: self.config.recall_budget.as_deref(),
            max_tokens: self.config.recall_max_tokens,
            types: if self.config.recall_types.is_empty() {
                None
            } else {
                Some(self.config.recall_types.as_slice())
            },
        };
        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| SemanticError::Backend(format!("hindsight reflect request failed: {e}")))?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(SemanticError::Backend(format!(
                "hindsight reflect returned {status}: {text}"
            )));
        }

        let reflect: ReflectResponse = resp
            .json()
            .await
            .map_err(|e| SemanticError::Backend(format!("hindsight reflect parse: {e}")))?;

        // Merge top-level array evidence with the documented `based_on`
        // container (object `{memories}`, array, or null). The shapes are
        // mutually exclusive in practice; merging is lossless either way.
        let mut sources_wire = reflect.sources;
        if let Some(based_on) = reflect.based_on {
            sources_wire.extend(based_on.into_memories());
        }
        let sources = sources_wire
            .into_iter()
            .map(|s| ReflectSource {
                id: s.id,
                content: s.content,
                score: s.score,
            })
            .collect();

        Ok(ReflectAnswer {
            answer: reflect.answer,
            sources,
            provenance: format!("hindsight:reflect:{bank_id}"),
        })
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
        // SERA-owned write governance: refuse before any provider mutation.
        // Bank derivation is pure (no HTTP), so computing it here keeps the
        // governance event/audit attributable without touching Hindsight.
        let bank_id = self.bank_id_for_put(&req);
        self.ensure_write_allowed(&req.agent_id, &bank_id)?;
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

        let bank_id = self.bank_id_for_query(&query);
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
    use wiremock::matchers::{body_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    // ── helpers ──────────────────────────────────────────────────────────────

    fn fast_config(base_url: String) -> HindsightConfig {
        HindsightConfig {
            base_url,
            timeout: Duration::from_secs(5),
            poll_interval: Duration::from_millis(10),
            poll_max_attempts: 5,
            bearer_token: None,
            bank_id_override: None,
            read_only: false,
            recall_budget: Some("low".to_string()),
            recall_max_tokens: Some(500),
            recall_types: vec!["observation".to_string()],
        }
    }

    fn put_req(agent_id: &str) -> PutRequest {
        PutRequest::new(
            agent_id,
            "Hello, Hindsight!",
            SegmentKind::MemoryRecall("r-1".into()),
        )
    }

    fn reflect_query(text: &str) -> SemanticQuery {
        SemanticQuery {
            agent_id: "agent-1".into(),
            scope: None,
            tier_filter: None,
            text: Some(text.into()),
            query_embedding: None,
            top_k: 5,
            similarity_threshold: None,
        }
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
            .and(path("/v1/default/banks/hermes/memories/recall"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "results": [
                    {"id": "r-1", "text": "first result", "score": 0.9},
                    {"id": "r-2", "content": "second result", "score": 0.7},
                ]
            })))
            .mount(&server)
            .await;

        let mut config = fast_config(server.uri());
        config.bank_id_override = Some("hermes".to_string());
        let store = HindsightStore::new(config).unwrap();
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

    // ── read-only refuses put before any provider mutation ────────────────────

    /// A recording [`WriteAuditSink`] for governance assertions.
    #[derive(Default)]
    struct RecordingSink {
        denials: std::sync::Mutex<Vec<WriteDenial>>,
    }

    impl WriteAuditSink for RecordingSink {
        fn record_denial(&self, denial: &WriteDenial) {
            self.denials.lock().unwrap().push(denial.clone());
        }
    }

    #[tokio::test]
    async fn read_only_put_refuses_before_provider_mutation() {
        // If the governance gate let the request through, this mock would be
        // hit and return a recognisable sentinel body. We assert the sentinel
        // never appears in the error, proving no provider mutation occurred.
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/default/banks/agent:agent-1/memories"))
            .respond_with(ResponseTemplate::new(500).set_body_string("PROVIDER_WAS_CALLED"))
            .mount(&server)
            .await;

        let mut config = fast_config(server.uri());
        config.read_only = true;
        let sink = Arc::new(RecordingSink::default());
        let store = HindsightStore::new(config).unwrap().with_audit_sink(sink.clone());

        let err = store.put(put_req("agent-1")).await.unwrap_err();
        match &err {
            SemanticError::Backend(msg) => {
                assert!(msg.contains("read_only"), "expected read_only in: {msg}");
                assert!(
                    msg.contains("no provider mutation attempted"),
                    "expected explicit no-mutation marker in: {msg}"
                );
                assert!(
                    !msg.contains("PROVIDER_WAS_CALLED"),
                    "provider must not be reached on a governed refusal: {msg}"
                );
            }
            other => panic!("expected Backend error, got: {other:?}"),
        }

        // The denial is auditable: the sink received exactly one record with
        // the attributable bank/agent/reason.
        let denials = sink.denials.lock().unwrap();
        assert_eq!(denials.len(), 1, "exactly one denial recorded");
        assert_eq!(denials[0].bank_id, "agent:agent-1");
        assert_eq!(denials[0].agent_id, "agent-1");
        assert_eq!(denials[0].operation, "put");
        assert_eq!(denials[0].reason, "read_only");
    }

    // ── reflect — read-only RAG Q&A ───────────────────────────────────────────

    #[tokio::test]
    async fn reflect_returns_answer_sources_and_provenance() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/default/banks/agent:agent-1/reflect"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "answer": "You prefer concise replies.",
                "sources": [
                    {"id": "m-1", "text": "user asked for brevity", "score": 0.8},
                    {"id": "m-2", "content": "short answers preferred", "score": 0.6},
                ]
            })))
            .mount(&server)
            .await;

        let store = HindsightStore::new(fast_config(server.uri())).unwrap();
        let q = reflect_query("what tone do I prefer?");
        let ans = store.reflect(&q).await.unwrap();

        assert_eq!(ans.answer, "You prefer concise replies.");
        assert_eq!(ans.sources.len(), 2);
        assert_eq!(ans.sources[0].id.as_deref(), Some("m-1"));
        assert_eq!(ans.sources[0].content, "user asked for brevity");
        assert!((ans.sources[1].score - 0.6).abs() < 1e-6);
        assert_eq!(ans.provenance, "hindsight:reflect:agent:agent-1");
    }

    #[tokio::test]
    async fn reflect_reads_live_text_and_based_on_shape() {
        // Regression: the live Hindsight reflect endpoint returns the answer
        // under `text` and evidence under `based_on` (not `answer`/`sources`).
        // Before the alias fix both fields defaulted, silently yielding an
        // empty answer and no sources.
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/default/banks/agent:agent-1/reflect"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "text": "You favour terse, direct answers.",
                "based_on": [
                    {"id": "m-1", "text": "user asked to keep it short", "score": 0.91},
                    {"id": "m-2", "content": "dislikes preamble", "score": 0.42},
                ]
            })))
            .mount(&server)
            .await;

        let store = HindsightStore::new(fast_config(server.uri())).unwrap();
        let ans = store.reflect(&reflect_query("how should I answer?")).await.unwrap();

        assert_eq!(ans.answer, "You favour terse, direct answers.");
        assert_eq!(ans.sources.len(), 2, "based_on evidence must be parsed");
        assert_eq!(ans.sources[0].id.as_deref(), Some("m-1"));
        assert_eq!(ans.sources[0].content, "user asked to keep it short");
        assert!((ans.sources[0].score - 0.91).abs() < 1e-6);
        assert_eq!(ans.sources[1].content, "dislikes preamble");
        assert_eq!(ans.provenance, "hindsight:reflect:agent:agent-1");
    }

    #[tokio::test]
    async fn reflect_reads_based_on_object_with_memories() {
        // Documented live shape: `based_on` is a ReflectBasedOn object whose
        // cited memories live under `based_on.memories`, not a top-level array.
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/default/banks/agent:agent-1/reflect"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "text": "You ship small, verified PRs.",
                "based_on": {
                    "memories": [
                        {"id": "m-1", "text": "prefers narrow slices", "score": 0.88},
                        {"id": "m-2", "fact": "verifies before claiming done", "score": 0.5},
                    ]
                }
            })))
            .mount(&server)
            .await;

        let store = HindsightStore::new(fast_config(server.uri())).unwrap();
        let ans = store.reflect(&reflect_query("how do I work?")).await.unwrap();

        assert_eq!(ans.answer, "You ship small, verified PRs.");
        assert_eq!(ans.sources.len(), 2, "based_on.memories must be extracted");
        assert_eq!(ans.sources[0].content, "prefers narrow slices");
        assert!((ans.sources[0].score - 0.88).abs() < 1e-6);
        // `fact` is accepted as a content alias for memory objects.
        assert_eq!(ans.sources[1].content, "verifies before claiming done");
        assert_eq!(ans.provenance, "hindsight:reflect:agent:agent-1");
    }

    #[tokio::test]
    async fn reflect_accepts_null_based_on() {
        // `based_on: null` must yield a successful reflect with an empty
        // source list — not a deserialization failure.
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/default/banks/agent:agent-1/reflect"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "text": "No supporting memories were found.",
                "based_on": null
            })))
            .mount(&server)
            .await;

        let store = HindsightStore::new(fast_config(server.uri())).unwrap();
        let ans = store.reflect(&reflect_query("anything?")).await.unwrap();

        assert_eq!(ans.answer, "No supporting memories were found.");
        assert!(ans.sources.is_empty(), "null based_on yields no sources");
        assert_eq!(ans.provenance, "hindsight:reflect:agent:agent-1");
    }

    #[tokio::test]
    async fn reflect_request_body_requests_facts_and_carries_controls() {
        // The reflect request must ask for evidence (`include.facts`) so the
        // live endpoint returns `based_on`, alongside the existing
        // query/budget/max_tokens/types controls.
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/default/banks/agent:agent-1/reflect"))
            .and(body_json(serde_json::json!({
                "query": "what do I know?",
                "include": {"facts": {}},
                "budget": "low",
                "max_tokens": 500,
                "types": ["observation"],
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "text": "ok",
                "based_on": {"memories": []}
            })))
            .mount(&server)
            .await;

        let store = HindsightStore::new(fast_config(server.uri())).unwrap();
        let ans = store.reflect(&reflect_query("what do I know?")).await.unwrap();
        assert_eq!(ans.answer, "ok");
    }

    #[tokio::test]
    async fn reflect_requires_query_text() {
        let store = HindsightStore::new(HindsightConfig::default()).unwrap();
        let mut q = reflect_query("ignored");
        q.text = None;
        let err = store.reflect(&q).await.unwrap_err();
        match &err {
            SemanticError::Backend(msg) => {
                assert!(msg.contains("requires query.text"), "got: {msg}");
            }
            other => panic!("expected Backend error, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn reflect_propagates_backend_error_truthfully() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/default/banks/agent:agent-1/reflect"))
            .respond_with(ResponseTemplate::new(503).set_body_string("hindsight down"))
            .mount(&server)
            .await;

        let store = HindsightStore::new(fast_config(server.uri())).unwrap();
        let err = store.reflect(&reflect_query("anything")).await.unwrap_err();
        match &err {
            SemanticError::Backend(msg) => {
                assert!(msg.contains("503"), "expected status in: {msg}");
                assert!(msg.contains("hindsight down"), "expected body in: {msg}");
            }
            other => panic!("expected Backend error, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn reflect_allowed_when_read_only() {
        // Reflect never retains, so the read-only governance gate must not
        // block it.
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/default/banks/agent:agent-1/reflect"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "answer": "ok",
                "sources": []
            })))
            .mount(&server)
            .await;

        let mut config = fast_config(server.uri());
        config.read_only = true;
        let store = HindsightStore::new(config).unwrap();
        let ans = store.reflect(&reflect_query("q")).await.unwrap();
        assert_eq!(ans.answer, "ok");
        assert!(ans.sources.is_empty());
    }

    // ── recall request body carries budget / max_tokens / types ───────────────

    #[tokio::test]
    async fn recall_request_body_includes_budget_max_tokens_and_types() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/default/banks/agent:agent-1/memories/recall"))
            .and(body_json(serde_json::json!({
                "query": "what do I know?",
                "top_k": 5,
                "budget": "low",
                "max_tokens": 500,
                "types": ["observation"],
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "results": [{"id": "r-1", "content": "hit", "score": 0.5}]
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
        assert_eq!(results.len(), 1);
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
