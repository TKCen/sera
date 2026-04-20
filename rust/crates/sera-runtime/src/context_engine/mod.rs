//! Context engine — pluggable context assembly and compaction.

pub mod enricher;
pub mod hybrid;
pub mod kvcache;
pub mod pipeline;

pub use enricher::{
    ContextEnricher, ContextEnricherConfig, EnrichmentResult, MAX_RECALL_SEGMENTS,
    MEMORY_RECALL_PRIORITY,
};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Token budget for context assembly.
#[derive(Debug, Clone)]
pub struct TokenBudget {
    pub max_tokens: u32,
    pub reserved_for_output: u32,
}

/// Assembled context window.
#[derive(Debug, Clone)]
pub struct ContextWindow {
    pub messages: Vec<serde_json::Value>,
    pub estimated_tokens: u32,
}

/// Compaction checkpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactionCheckpoint {
    pub checkpoint_id: uuid::Uuid,
    pub session_key: String,
    pub reason: CheckpointReason,
    pub tokens_before: u32,
    pub tokens_after: u32,
    pub summary: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Reason for creating a compaction checkpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckpointReason {
    Manual,
    AutoThreshold,
    OverflowRetry,
    TimeoutRetry,
}

/// Maximum compaction checkpoints per session.
pub const MAX_COMPACTION_CHECKPOINTS_PER_SESSION: u32 = 25;

/// Context engine descriptor.
#[derive(Debug, Clone)]
pub struct ContextEngineDescriptor {
    pub name: String,
    pub version: String,
}

/// Context errors.
#[derive(Debug, thiserror::Error)]
pub enum ContextError {
    #[error("context error: {0}")]
    Internal(String),
    #[error("token budget exceeded: {limit} max, {actual} actual")]
    BudgetExceeded { limit: u32, actual: u32 },
}

/// Pluggable context engine trait — orthogonal to AgentRuntime.
#[async_trait]
pub trait ContextEngine: Send + Sync {
    async fn ingest(&mut self, msg: serde_json::Value) -> Result<(), ContextError>;
    async fn assemble(&self, budget: TokenBudget) -> Result<ContextWindow, ContextError>;
    async fn compact(
        &mut self,
        trigger: CheckpointReason,
    ) -> Result<CompactionCheckpoint, ContextError>;
    async fn maintain(&mut self) -> Result<(), ContextError>;
    fn describe(&self) -> ContextEngineDescriptor;
}

// ─── Pluggability: optional drill + diagnostic traits (SPEC-context-engine-pluggability) ───

/// Opaque identifier for a compaction / summary node produced by a
/// `ContextEngine`. Backends pick their own encoding — LCM uses a
/// stringified `INTEGER PRIMARY KEY`, another engine could use a UUID.
/// Consumers treat it as opaque and never parse the inner string.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ContextNodeId(pub String);

impl ContextNodeId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Sort policy for a `ContextQuery::search` request.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchSort {
    #[default]
    Recency,
    Relevance,
    Hybrid,
}

/// Scope of a `ContextQuery::search` request.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchScope {
    #[default]
    CurrentSession,
    AllSessions,
}

/// Input to `ContextQuery::search`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextSearchRequest {
    pub query: String,
    pub limit: usize,
    #[serde(default)]
    pub sort: SearchSort,
    #[serde(default)]
    pub scope: SearchScope,
    /// Optional source/platform filter (e.g. `"cli"`, `"discord"`). Engines
    /// that do not track a source field ignore this.
    pub source: Option<String>,
}

/// A single hit from `ContextQuery::search`.
///
/// At least one of `node_id` or `externalized_ref` is populated so callers
/// can hand the hit back to an `expand_*` call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextSearchHit {
    pub node_id: Option<ContextNodeId>,
    pub externalized_ref: Option<String>,
    pub snippet: String,
    /// Free-form depth label. LCM returns e.g. `"D0"`; flat engines return
    /// `""`. Rendered verbatim by consumers — there is no shared depth enum.
    pub depth_label: String,
    /// Rank used for ordering (lower = stronger, SQLite FTS5 convention).
    /// `None` means the backend did not compute a rank.
    pub rank: Option<f64>,
    /// Forward-compatible slot for backend-specific fields.
    #[serde(default)]
    pub metadata: serde_json::Value,
}

/// The result of resolving a `node_id` or `externalized_ref` to its content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextExpansion {
    pub content: String,
    pub estimated_tokens: u32,
    #[serde(default)]
    pub metadata: serde_json::Value,
}

/// Metadata about a node's subtree — returned WITHOUT loading full content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextSubtreeDescription {
    pub node_id: ContextNodeId,
    pub depth_label: String,
    pub token_count: u32,
    pub source_token_count: u32,
    pub children: Vec<ContextChildRef>,
    pub expand_hint: Option<String>,
    #[serde(default)]
    pub metadata: serde_json::Value,
}

/// One child in a `ContextSubtreeDescription`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextChildRef {
    pub node_id: ContextNodeId,
    pub depth_label: String,
    pub token_count: u32,
    pub expand_hint: Option<String>,
}

/// Optional — implement when the engine exposes agent-facing drill tools
/// (search / describe / expand). Engines that only do pure context
/// assembly (e.g. a ring buffer) do not implement this.
///
/// Shape mirrors LCM's public tool surface so an LCM adapter maps 1:1.
/// See `docs/plan/specs/SPEC-context-engine-pluggability.md` §4.
#[async_trait]
pub trait ContextQuery: Send + Sync {
    async fn search(
        &self,
        req: ContextSearchRequest,
    ) -> Result<Vec<ContextSearchHit>, ContextError>;

    /// Inspect a node WITHOUT loading content.
    async fn describe_node(
        &self,
        node_id: &ContextNodeId,
    ) -> Result<ContextSubtreeDescription, ContextError>;

    /// Recover source material behind a node. `max_tokens` is a soft
    /// budget the backend MUST respect — it may return less.
    async fn expand_node(
        &self,
        node_id: &ContextNodeId,
        max_tokens: u32,
    ) -> Result<ContextExpansion, ContextError>;

    /// Inspect an externalized payload ref — backend-owned blob reference
    /// for large tool results moved out of the main context. Defaults to
    /// an error for engines without externalization.
    async fn describe_ref(
        &self,
        ref_name: &str,
    ) -> Result<ContextSubtreeDescription, ContextError> {
        let _ = ref_name;
        Err(ContextError::Internal(
            "backend does not support externalized payload refs".into(),
        ))
    }

    /// Load an externalized payload ref. Defaults to an error for engines
    /// without externalization.
    async fn expand_ref(
        &self,
        ref_name: &str,
        max_tokens: u32,
    ) -> Result<ContextExpansion, ContextError> {
        let _ = (ref_name, max_tokens);
        Err(ContextError::Internal(
            "backend does not support externalized payload refs".into(),
        ))
    }
}

/// A status snapshot returned by `ContextDiagnostics::status`.
///
/// `fields` is `serde_json::Value` so backends report their own metrics
/// (compression count, depth distribution, externalization stats, DB
/// size) without schema churn on the trait.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextStatus {
    pub engine: ContextEngineDescriptor,
    pub session_id: Option<String>,
    pub fields: serde_json::Value,
}

/// One row of a `ContextDiagnostics::doctor` report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoctorCheck {
    pub name: String,
    pub passed: bool,
    pub detail: String,
}

/// Optional — implement when the engine has health introspection
/// (DB integrity, orphan detection, config validation). Engines without
/// meaningful operational state do not implement this.
#[async_trait]
pub trait ContextDiagnostics: Send + Sync {
    async fn status(
        &self,
        session_id: Option<&str>,
    ) -> Result<ContextStatus, ContextError>;
    async fn doctor(&self) -> Result<Vec<DoctorCheck>, ContextError>;
}

// Manual `Clone` on `ContextEngineDescriptor` was derived already; add
// `Serialize`/`Deserialize` via a thin wrapper so `ContextStatus` can
// round-trip cleanly.
impl Serialize for ContextEngineDescriptor {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut st = s.serialize_struct("ContextEngineDescriptor", 2)?;
        st.serialize_field("name", &self.name)?;
        st.serialize_field("version", &self.version)?;
        st.end()
    }
}

impl<'de> Deserialize<'de> for ContextEngineDescriptor {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Raw {
            name: String,
            version: String,
        }
        let Raw { name, version } = Raw::deserialize(d)?;
        Ok(Self { name, version })
    }
}

#[cfg(test)]
mod pluggability_tests {
    use super::*;
    use async_trait::async_trait;

    /// A `ContextQuery` impl that only supports node-addressed lookups.
    /// Used to prove the default `describe_ref` / `expand_ref` error
    /// cleanly for engines without externalization.
    struct NodeOnlyQuery;

    #[async_trait]
    impl ContextQuery for NodeOnlyQuery {
        async fn search(
            &self,
            _req: ContextSearchRequest,
        ) -> Result<Vec<ContextSearchHit>, ContextError> {
            Ok(vec![ContextSearchHit {
                node_id: Some(ContextNodeId::new("42")),
                externalized_ref: None,
                snippet: "hit".into(),
                depth_label: "D0".into(),
                rank: Some(0.1),
                metadata: serde_json::Value::Null,
            }])
        }

        async fn describe_node(
            &self,
            node_id: &ContextNodeId,
        ) -> Result<ContextSubtreeDescription, ContextError> {
            Ok(ContextSubtreeDescription {
                node_id: node_id.clone(),
                depth_label: "D0".into(),
                token_count: 100,
                source_token_count: 100,
                children: vec![],
                expand_hint: None,
                metadata: serde_json::Value::Null,
            })
        }

        async fn expand_node(
            &self,
            _node_id: &ContextNodeId,
            _max_tokens: u32,
        ) -> Result<ContextExpansion, ContextError> {
            Ok(ContextExpansion {
                content: "expanded".into(),
                estimated_tokens: 10,
                metadata: serde_json::Value::Null,
            })
        }
    }

    #[tokio::test]
    async fn default_describe_ref_errors_when_not_overridden() {
        let q = NodeOnlyQuery;
        let err = q.describe_ref("some/ref").await.unwrap_err();
        assert!(
            matches!(err, ContextError::Internal(ref s) if s.contains("externalized")),
            "expected internal error about externalized refs, got {err:?}"
        );
    }

    #[tokio::test]
    async fn default_expand_ref_errors_when_not_overridden() {
        let q = NodeOnlyQuery;
        let err = q.expand_ref("some/ref", 4000).await.unwrap_err();
        assert!(matches!(err, ContextError::Internal(_)));
    }

    #[tokio::test]
    async fn search_request_defaults_round_trip_through_json() {
        let req: ContextSearchRequest = serde_json::from_str(r#"{"query":"hi","limit":10}"#).unwrap();
        assert!(matches!(req.sort, SearchSort::Recency));
        assert!(matches!(req.scope, SearchScope::CurrentSession));
        assert!(req.source.is_none());
    }

    #[test]
    fn context_node_id_accessors() {
        let id = ContextNodeId::new("42");
        assert_eq!(id.as_str(), "42");
        let s = serde_json::to_string(&id).unwrap();
        let round: ContextNodeId = serde_json::from_str(&s).unwrap();
        assert_eq!(round, id);
    }

    #[test]
    fn context_status_round_trips() {
        let status = ContextStatus {
            engine: ContextEngineDescriptor {
                name: "test".into(),
                version: "0.1.0".into(),
            },
            session_id: Some("sess-1".into()),
            fields: serde_json::json!({"depth_nodes": {"D0": 5, "D1": 1}}),
        };
        let s = serde_json::to_string(&status).unwrap();
        let back: ContextStatus = serde_json::from_str(&s).unwrap();
        assert_eq!(back.engine.name, "test");
        assert_eq!(back.session_id.as_deref(), Some("sess-1"));
    }
}
