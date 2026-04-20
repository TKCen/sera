//! Runtime configuration from environment variables.
#![allow(dead_code)]

use sera_types::llm::ThinkingLevel;

/// Runtime configuration — read from env vars set by sera-core when spawning the container.
#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    pub llm_base_url: String,
    pub llm_model: String,
    pub llm_api_key: String,
    pub chat_port: u16,
    pub agent_id: String,
    pub lifecycle_mode: String,
    pub core_url: String,
    pub api_key: String,
    pub context_window: usize,
    pub compaction_strategy: String,
    pub max_tokens: u32,
    /// When `true`, inject a `## Circle Activity` section into the system prompt.
    /// Defaults to `false` so existing prompts are unchanged.
    pub circle_activity_enabled: bool,
    /// Master flag for Tier-2 semantic enrichment (sera-0yqq).
    ///
    /// When `false` (default), [`crate::context_engine::ContextEnricher`] is a
    /// pure no-op: no embedding calls, no store queries, no promoted recall
    /// segments. Turns proceed with lexical-only scoring in `HybridScorer`.
    pub semantic_enrichment_enabled: bool,
    /// How many store hits to request before hybrid rerank.
    ///
    /// Final promotion is still capped at
    /// [`crate::context_engine::MAX_RECALL_SEGMENTS`] (3).
    pub semantic_top_k: usize,
    /// Minimum composite score for a store hit to survive initial filtering.
    /// Forwarded to [`sera_memory::SemanticQuery::similarity_threshold`].
    pub semantic_similarity_threshold: Option<f32>,
    /// Wall-clock timeout on the combined embedding + store query, in
    /// milliseconds. Past this budget the enricher degrades silently to an
    /// empty segment list.
    pub semantic_enrichment_timeout_ms: u64,
    /// GH#140 — toggle hierarchical scope recall in
    /// [`crate::context_engine::ContextEnricher`].
    ///
    /// When `true`, the enricher calls
    /// [`sera_memory::SemanticMemoryStore::query_hierarchical`] with the
    /// agent's scope chain (Agent → Circle → Org → Global). When `false`
    /// (the default), the enricher keeps its pre-GH#140 agent-only
    /// `query()` behaviour. Safe rollout kill-switch — no code path other
    /// than the enricher branches on this flag.
    pub hierarchical_scopes_enabled: bool,
    /// When `true`, `TraitToolRegistry::execute` runs a per-tool PDP check via
    /// `ToolContext::authz` before the `ToolPolicy` check.
    ///
    /// Defaults to `false` — safe rollout kill-switch.  Set
    /// `TOOL_AUTHZ_ENABLED=true` (or `=1`) to enable enforcement.
    pub tool_authz_enabled: bool,
    /// Optional inline role → action-kind grants for `RoleBasedAuthzProvider`.
    ///
    /// Format: `TOOL_AUTHZ_ROLES=<role>:<action_kind>[,<action_kind>...][;<role>:...]`
    ///
    /// Example: `TOOL_AUTHZ_ROLES=operator:tool_call,read;admin:tool_call,read,write,admin`
    ///
    /// When absent, an allow-all `DefaultAuthzProvider` stub is installed.
    pub tool_authz_roles: Option<String>,
    /// Provider-agnostic reasoning intensity (sera-1rv8).
    ///
    /// Reads `SERA_THINKING_LEVEL` env var (values: `none`, `low`, `medium`,
    /// `high`, `xhigh`).  Defaults to `None` — no reasoning overhead.
    ///
    /// When `Some`, [`crate::llm_client::LlmClient`] applies the matching
    /// provider-native parameter before sending each request.
    pub thinking_level: Option<ThinkingLevel>,
}

impl RuntimeConfig {
    pub fn from_env() -> Self {
        Self {
            llm_base_url: std::env::var("LLM_BASE_URL")
                .unwrap_or_else(|_| "http://sera-core:3001/v1/llm".to_string()),
            llm_model: std::env::var("LLM_MODEL")
                .unwrap_or_else(|_| "lmstudio-local".to_string()),
            llm_api_key: std::env::var("LLM_API_KEY")
                .unwrap_or_else(|_| "lm-studio".to_string()),
            chat_port: std::env::var("AGENT_CHAT_PORT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(8080),
            agent_id: std::env::var("AGENT_ID").unwrap_or_default(),
            lifecycle_mode: std::env::var("LIFECYCLE_MODE")
                .unwrap_or_else(|_| "task".to_string()),
            core_url: std::env::var("SERA_CORE_URL")
                .unwrap_or_else(|_| "http://sera-core:3001".to_string()),
            api_key: std::env::var("SERA_API_KEY")
                .unwrap_or_else(|_| "sera_bootstrap_dev_123".to_string()),
            context_window: std::env::var("CONTEXT_WINDOW")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(128_000),
            compaction_strategy: std::env::var("CONTEXT_COMPACTION_STRATEGY")
                .unwrap_or_else(|_| "summarize".to_string()),
            max_tokens: std::env::var("MAX_TOKENS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(crate::llm_client::DEFAULT_MAX_TOKENS),
            circle_activity_enabled: std::env::var("CIRCLE_ACTIVITY_ENABLED")
                .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
                .unwrap_or(false),
            semantic_enrichment_enabled: std::env::var("SEMANTIC_ENRICHMENT_ENABLED")
                .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
                .unwrap_or(false),
            semantic_top_k: std::env::var("SEMANTIC_TOP_K")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(3),
            semantic_similarity_threshold: std::env::var("SEMANTIC_SIMILARITY_THRESHOLD")
                .ok()
                .and_then(|v| v.parse().ok()),
            semantic_enrichment_timeout_ms: std::env::var("SEMANTIC_ENRICHMENT_TIMEOUT_MS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(150),
            hierarchical_scopes_enabled: std::env::var("HIERARCHICAL_SCOPES_ENABLED")
                .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
                .unwrap_or(false),
            tool_authz_enabled: std::env::var("TOOL_AUTHZ_ENABLED")
                .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
                .unwrap_or(false),
            tool_authz_roles: std::env::var("TOOL_AUTHZ_ROLES").ok(),
            thinking_level: std::env::var("SERA_THINKING_LEVEL")
                .ok()
                .and_then(|v| v.parse::<ThinkingLevel>().ok()),
        }
    }
}
