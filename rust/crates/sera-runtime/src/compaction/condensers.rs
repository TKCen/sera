//! Condenser implementations — all 9 per SPEC-runtime §4.

use std::sync::Arc;

use async_trait::async_trait;
use sera_types::model::{ModelAdapter, ModelRequest, ResponseFormat};

use super::Condenser;

// ── 1. NoOpCondenser ────────────────────────────────────────────────────────

pub struct NoOpCondenser;

#[async_trait]
impl Condenser for NoOpCondenser {
    fn name(&self) -> &str {
        "no_op"
    }
    async fn condense(&self, messages: Vec<serde_json::Value>) -> Vec<serde_json::Value> {
        messages
    }
}

// ── 2. RecentEventsCondenser ────────────────────────────────────────────────

/// Keeps only the N most recent messages.
pub struct RecentEventsCondenser {
    pub keep: usize,
}

impl RecentEventsCondenser {
    pub fn new(keep: usize) -> Self {
        Self { keep }
    }
}

#[async_trait]
impl Condenser for RecentEventsCondenser {
    fn name(&self) -> &str {
        "recent_events"
    }
    async fn condense(&self, messages: Vec<serde_json::Value>) -> Vec<serde_json::Value> {
        if messages.len() <= self.keep {
            return messages;
        }
        messages[messages.len() - self.keep..].to_vec()
    }
}

// ── 3. ConversationWindowCondenser ──────────────────────────────────────────

/// Keeps pairs of user/assistant messages to avoid orphaned tool results.
pub struct ConversationWindowCondenser {
    pub max_pairs: usize,
}

impl ConversationWindowCondenser {
    pub fn new(max_pairs: usize) -> Self {
        Self { max_pairs }
    }
}

#[async_trait]
impl Condenser for ConversationWindowCondenser {
    fn name(&self) -> &str {
        "conversation_window"
    }
    async fn condense(&self, messages: Vec<serde_json::Value>) -> Vec<serde_json::Value> {
        // Collect system messages (always keep)
        let mut system: Vec<serde_json::Value> = Vec::new();
        let mut pairs: Vec<serde_json::Value> = Vec::new();

        for msg in messages {
            let role = msg
                .get("role")
                .and_then(|r| r.as_str())
                .unwrap_or("");
            if role == "system" {
                system.push(msg);
            } else {
                pairs.push(msg);
            }
        }

        // Keep last max_pairs*2 non-system messages
        let keep = self.max_pairs * 2;
        if pairs.len() > keep {
            pairs = pairs[pairs.len() - keep..].to_vec();
        }

        // Ensure no orphaned tool_use without tool_result
        // Simple: if first message is a tool result, drop it
        while !pairs.is_empty() {
            let role = pairs[0]
                .get("role")
                .and_then(|r| r.as_str())
                .unwrap_or("");
            if role == "tool" {
                pairs.remove(0);
            } else {
                break;
            }
        }

        system.extend(pairs);
        system
    }
}

// ── 4. AmortizedForgettingCondenser ─────────────────────────────────────────

/// Progressively removes older messages with decreasing probability.
pub struct AmortizedForgettingCondenser {
    pub keep_ratio: f32,
}

impl AmortizedForgettingCondenser {
    pub fn new(keep_ratio: f32) -> Self {
        Self { keep_ratio }
    }
}

#[async_trait]
impl Condenser for AmortizedForgettingCondenser {
    fn name(&self) -> &str {
        "amortized_forgetting"
    }
    async fn condense(&self, messages: Vec<serde_json::Value>) -> Vec<serde_json::Value> {
        let keep = (messages.len() as f32 * self.keep_ratio).ceil() as usize;
        if messages.len() <= keep {
            return messages;
        }
        // Keep system messages + last `keep` non-system messages
        let mut system: Vec<serde_json::Value> = Vec::new();
        let mut rest: Vec<serde_json::Value> = Vec::new();
        for msg in messages {
            let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("");
            if role == "system" {
                system.push(msg);
            } else {
                rest.push(msg);
            }
        }
        if rest.len() > keep {
            rest = rest[rest.len() - keep..].to_vec();
        }
        system.extend(rest);
        system
    }
}

// ── 5. ObservationMaskingCondenser ──────────────────────────────────────────

/// Replaces tool result content with "[masked]" for older entries.
pub struct ObservationMaskingCondenser {
    pub keep_recent: usize,
}

impl ObservationMaskingCondenser {
    pub fn new(keep_recent: usize) -> Self {
        Self { keep_recent }
    }
}

#[async_trait]
impl Condenser for ObservationMaskingCondenser {
    fn name(&self) -> &str {
        "observation_masking"
    }
    async fn condense(&self, mut messages: Vec<serde_json::Value>) -> Vec<serde_json::Value> {
        let len = messages.len();
        let cutoff = len.saturating_sub(self.keep_recent);

        for msg in messages.iter_mut().take(cutoff) {
            let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("");
            if role == "tool"
                && let Some(obj) = msg.as_object_mut()
            {
                obj.insert(
                    "content".to_string(),
                    serde_json::Value::String("[masked]".to_string()),
                );
            }
        }
        messages
    }
}

// ── 6. BrowserOutputCondenser ───────────────────────────────────────────────

/// Truncates browser/shell tool outputs that exceed a size threshold.
pub struct BrowserOutputCondenser {
    pub max_chars: usize,
}

impl BrowserOutputCondenser {
    pub fn new(max_chars: usize) -> Self {
        Self { max_chars }
    }
}

#[async_trait]
impl Condenser for BrowserOutputCondenser {
    fn name(&self) -> &str {
        "browser_output"
    }
    async fn condense(&self, mut messages: Vec<serde_json::Value>) -> Vec<serde_json::Value> {
        for msg in &mut messages {
            let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("");
            if role == "tool"
                && let Some(content) = msg.get("content").and_then(|c| c.as_str())
                && content.len() > self.max_chars
            {
                let truncated = format!(
                    "{}... [truncated, {} chars total]",
                    &content[..self.max_chars],
                    content.len()
                );
                if let Some(obj) = msg.as_object_mut() {
                    obj.insert(
                        "content".to_string(),
                        serde_json::Value::String(truncated),
                    );
                }
            }
        }
        messages
    }
}

// ── 7. LlmSummarizingCondenser ──────────────────────────────────────────────

/// Summarizes older messages using an LLM call, keeping the N most recent intact.
pub struct LlmSummarizingCondenser {
    pub model: Arc<dyn ModelAdapter>,
    /// Number of recent messages to keep verbatim (default: 5).
    pub keep_recent: usize,
}

impl LlmSummarizingCondenser {
    pub fn new(model: Arc<dyn ModelAdapter>) -> Self {
        Self { model, keep_recent: 5 }
    }

    pub fn with_keep_recent(model: Arc<dyn ModelAdapter>, keep_recent: usize) -> Self {
        Self { model, keep_recent }
    }
}

#[async_trait]
impl Condenser for LlmSummarizingCondenser {
    fn name(&self) -> &str {
        "llm_summarizing"
    }

    async fn condense(&self, messages: Vec<serde_json::Value>) -> Vec<serde_json::Value> {
        if messages.len() <= self.keep_recent {
            return messages;
        }

        let split = messages.len() - self.keep_recent;
        let (to_summarize, recent) = messages.split_at(split);

        let request = ModelRequest {
            messages: vec![
                serde_json::json!({
                    "role": "system",
                    "content": "Summarize the following conversation, preserving key decisions, action items, and important context. Return a JSON object with a single 'summary' field."
                }),
                serde_json::json!({
                    "role": "user",
                    "content": serde_json::to_string(to_summarize).unwrap_or_default()
                }),
            ],
            tools: None,
            temperature: Some(0.3),
            max_tokens: Some(1024),
            stop_sequences: None,
            response_format: Some(ResponseFormat::Json),
            thinking: Default::default(),
        };

        match self.model.chat_completion(request).await {
            Ok(response) => {
                let summary_text = response.content.unwrap_or_default();
                let summary_msg = serde_json::json!({
                    "role": "system",
                    "content": format!("[Conversation summary]\n{}", summary_text)
                });
                let mut result = vec![summary_msg];
                result.extend_from_slice(recent);
                result
            }
            Err(err) => {
                tracing::warn!(condenser = "llm_summarizing", %err, "LLM call failed, returning input unchanged");
                messages
            }
        }
    }
}

// ── 8. LlmAttentionCondenser ─────────────────────────────────────────────────

/// Selects the most relevant messages for the current query using an LLM call.
pub struct LlmAttentionCondenser {
    pub model: Arc<dyn ModelAdapter>,
    /// Maximum number of non-system messages to keep (default: 10).
    pub max_select: usize,
}

impl LlmAttentionCondenser {
    pub fn new(model: Arc<dyn ModelAdapter>) -> Self {
        Self { model, max_select: 10 }
    }

    pub fn with_max_select(model: Arc<dyn ModelAdapter>, max_select: usize) -> Self {
        Self { model, max_select }
    }
}

#[async_trait]
impl Condenser for LlmAttentionCondenser {
    fn name(&self) -> &str {
        "llm_attention"
    }

    async fn condense(&self, messages: Vec<serde_json::Value>) -> Vec<serde_json::Value> {
        // Always keep system messages.
        let system_msgs: Vec<serde_json::Value> = messages
            .iter()
            .filter(|m| m.get("role").and_then(|r| r.as_str()) == Some("system"))
            .cloned()
            .collect();

        // Extract the last user message as the query.
        let query = messages
            .iter()
            .rev()
            .find(|m| m.get("role").and_then(|r| r.as_str()) == Some("user"))
            .and_then(|m| m.get("content").and_then(|c| c.as_str()))
            .unwrap_or("")
            .to_string();

        if query.is_empty() {
            return messages;
        }

        let indexed: Vec<serde_json::Value> = messages
            .iter()
            .enumerate()
            .map(|(i, m)| serde_json::json!({"index": i, "message": m}))
            .collect();

        let request = ModelRequest {
            messages: vec![
                serde_json::json!({
                    "role": "system",
                    "content": "Given a query and conversation history, select the most relevant messages. Return a JSON array of message indices (0-based) that are most relevant to the current conversation."
                }),
                serde_json::json!({
                    "role": "user",
                    "content": format!(
                        "Query: {}\n\nConversation history:\n{}",
                        query,
                        serde_json::to_string(&indexed).unwrap_or_default()
                    )
                }),
            ],
            tools: None,
            temperature: Some(0.1),
            max_tokens: Some(512),
            stop_sequences: None,
            response_format: Some(ResponseFormat::Json),
            thinking: Default::default(),
        };

        match self.model.chat_completion(request).await {
            Ok(response) => {
                let content = response.content.unwrap_or_default();
                let indices: Vec<usize> = serde_json::from_str::<Vec<serde_json::Value>>(&content)
                    .unwrap_or_default()
                    .into_iter()
                    .filter_map(|v| v.as_u64().map(|n| n as usize))
                    .take(self.max_select)
                    .collect();

                if indices.is_empty() {
                    tracing::warn!(condenser = "llm_attention", "LLM returned no indices, returning input unchanged");
                    return messages;
                }

                let mut selected: Vec<serde_json::Value> = system_msgs;
                for idx in &indices {
                    if let Some(msg) = messages.get(*idx) {
                        let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("");
                        if role != "system" {
                            selected.push(msg.clone());
                        }
                    }
                }
                selected
            }
            Err(err) => {
                tracing::warn!(condenser = "llm_attention", %err, "LLM call failed, returning input unchanged");
                messages
            }
        }
    }
}

// ── 9. StructuredSummaryCondenser ────────────────────────────────────────────

/// Extracts a structured summary (key facts, decisions, pending items) via LLM.
pub struct StructuredSummaryCondenser {
    pub model: Arc<dyn ModelAdapter>,
    /// Number of recent messages to keep verbatim (default: 3).
    pub keep_recent: usize,
}

impl StructuredSummaryCondenser {
    pub fn new(model: Arc<dyn ModelAdapter>) -> Self {
        Self { model, keep_recent: 3 }
    }

    pub fn with_keep_recent(model: Arc<dyn ModelAdapter>, keep_recent: usize) -> Self {
        Self { model, keep_recent }
    }
}

#[async_trait]
impl Condenser for StructuredSummaryCondenser {
    fn name(&self) -> &str {
        "structured_summary"
    }

    async fn condense(&self, messages: Vec<serde_json::Value>) -> Vec<serde_json::Value> {
        if messages.len() <= self.keep_recent {
            return messages;
        }

        let split = messages.len() - self.keep_recent;
        let (to_summarize, recent) = messages.split_at(split);

        let request = ModelRequest {
            messages: vec![
                serde_json::json!({
                    "role": "system",
                    "content": "Extract a structured summary from the conversation. Return JSON with fields: 'key_facts' (array of strings), 'decisions' (array of strings), 'pending_items' (array of strings), 'context' (string with overall context)."
                }),
                serde_json::json!({
                    "role": "user",
                    "content": serde_json::to_string(to_summarize).unwrap_or_default()
                }),
            ],
            tools: None,
            temperature: Some(0.2),
            max_tokens: Some(1024),
            stop_sequences: None,
            response_format: Some(ResponseFormat::Json),
            thinking: Default::default(),
        };

        match self.model.chat_completion(request).await {
            Ok(response) => {
                let summary_json = response.content.unwrap_or_default();
                let summary_msg = serde_json::json!({
                    "role": "system",
                    "content": format!("[Structured summary]\n{}", summary_json)
                });
                let mut result = vec![summary_msg];
                result.extend_from_slice(recent);
                result
            }
            Err(err) => {
                tracing::warn!(condenser = "structured_summary", %err, "LLM call failed, returning input unchanged");
                messages
            }
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use async_trait::async_trait;
    use sera_types::model::{FinishReason, ModelAdapter, ModelError, ModelRequest, ModelResponse};
    use sera_types::runtime::TokenUsage;

    use super::*;

    // ── Mock adapter ─────────────────────────────────────────────────────────

    /// A mock adapter whose response is determined by a provided closure.
    struct MockAdapter {
        // The full trait-object type is unavoidable for a single-field test mock.
        #[allow(clippy::type_complexity)]
        response: Box<dyn Fn(&ModelRequest) -> Result<ModelResponse, ModelError> + Send + Sync>,
    }

    impl MockAdapter {
        fn ok(content: impl Into<String>) -> Arc<Self> {
            let s = content.into();
            Arc::new(Self {
                response: Box::new(move |_| {
                    Ok(ModelResponse {
                        content: Some(s.clone()),
                        tool_calls: vec![],
                        usage: TokenUsage { prompt_tokens: 10, completion_tokens: 5, total_tokens: 15 },
                        model: "mock".to_string(),
                        finish_reason: FinishReason::Stop,
                    })
                }),
            })
        }

        fn err() -> Arc<Self> {
            Arc::new(Self {
                response: Box::new(|_| Err(ModelError::Timeout)),
            })
        }
    }

    #[async_trait]
    impl ModelAdapter for MockAdapter {
        async fn chat_completion(&self, request: ModelRequest) -> Result<ModelResponse, ModelError> {
            (self.response)(&request)
        }
        fn model_name(&self) -> &str { "mock" }
        fn supports_tools(&self) -> bool { false }
        fn supports_streaming(&self) -> bool { false }
        fn max_context_tokens(&self) -> u32 { 4096 }
    }

    fn make_messages(n: usize) -> Vec<serde_json::Value> {
        (0..n)
            .map(|i| serde_json::json!({"role": if i % 2 == 0 { "user" } else { "assistant" }, "content": format!("msg {}", i)}))
            .collect()
    }

    // ── Helpers ──────────────────────────────────────────────────────────────

    fn tool_msg(content: &str) -> serde_json::Value {
        serde_json::json!({"role": "tool", "content": content})
    }

    fn system_msg(content: &str) -> serde_json::Value {
        serde_json::json!({"role": "system", "content": content})
    }

    // ── NoOpCondenser ────────────────────────────────────────────────────────

    #[tokio::test]
    async fn noop_empty_input() {
        let c = NoOpCondenser;
        let result = c.condense(vec![]).await;
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn noop_single_message() {
        let c = NoOpCondenser;
        let msgs = vec![serde_json::json!({"role": "user", "content": "hi"})];
        let result = c.condense(msgs.clone()).await;
        assert_eq!(result, msgs);
    }

    #[tokio::test]
    async fn noop_preserves_all_messages() {
        let c = NoOpCondenser;
        let msgs = make_messages(10);
        let result = c.condense(msgs.clone()).await;
        assert_eq!(result, msgs);
    }

    #[tokio::test]
    async fn noop_name() {
        assert_eq!(NoOpCondenser.name(), "no_op");
    }

    // ── RecentEventsCondenser ────────────────────────────────────────────────

    #[tokio::test]
    async fn recent_events_empty_input() {
        let c = RecentEventsCondenser::new(5);
        let result = c.condense(vec![]).await;
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn recent_events_single_message() {
        let c = RecentEventsCondenser::new(5);
        let msgs = vec![serde_json::json!({"role": "user", "content": "hi"})];
        let result = c.condense(msgs.clone()).await;
        assert_eq!(result, msgs);
    }

    #[tokio::test]
    async fn recent_events_below_budget_passthrough() {
        let c = RecentEventsCondenser::new(10);
        let msgs = make_messages(5);
        let result = c.condense(msgs.clone()).await;
        assert_eq!(result.len(), 5);
        assert_eq!(result, msgs);
    }

    #[tokio::test]
    async fn recent_events_at_budget_passthrough() {
        let c = RecentEventsCondenser::new(5);
        let msgs = make_messages(5);
        let result = c.condense(msgs.clone()).await;
        assert_eq!(result.len(), 5);
        assert_eq!(result, msgs);
    }

    #[tokio::test]
    async fn recent_events_above_budget_keeps_tail() {
        let c = RecentEventsCondenser::new(3);
        let msgs = make_messages(7);
        let result = c.condense(msgs.clone()).await;
        assert_eq!(result.len(), 3);
        assert_eq!(result, msgs[4..]);
    }

    #[tokio::test]
    async fn recent_events_deterministic() {
        let c = RecentEventsCondenser::new(3);
        let msgs = make_messages(6);
        let r1 = c.condense(msgs.clone()).await;
        let r2 = c.condense(msgs.clone()).await;
        assert_eq!(r1, r2);
    }

    #[tokio::test]
    async fn recent_events_keep_zero_returns_empty() {
        let c = RecentEventsCondenser::new(0);
        let msgs = make_messages(5);
        let result = c.condense(msgs).await;
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn recent_events_name() {
        assert_eq!(RecentEventsCondenser::new(1).name(), "recent_events");
    }

    // ── ConversationWindowCondenser ──────────────────────────────────────────

    #[tokio::test]
    async fn conversation_window_empty_input() {
        let c = ConversationWindowCondenser::new(3);
        let result = c.condense(vec![]).await;
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn conversation_window_single_user_message() {
        let c = ConversationWindowCondenser::new(3);
        let msgs = vec![serde_json::json!({"role": "user", "content": "hello"})];
        let result = c.condense(msgs.clone()).await;
        assert_eq!(result, msgs);
    }

    #[tokio::test]
    async fn conversation_window_only_system_messages_preserved() {
        let c = ConversationWindowCondenser::new(1);
        let msgs = vec![
            system_msg("sys1"),
            system_msg("sys2"),
        ];
        let result = c.condense(msgs.clone()).await;
        assert_eq!(result, msgs);
    }

    #[tokio::test]
    async fn conversation_window_trims_to_max_pairs() {
        let c = ConversationWindowCondenser::new(2); // keep 4 non-system msgs
        let msgs = make_messages(8); // 8 non-system
        let result = c.condense(msgs).await;
        // Should have at most 4 non-system messages
        let non_system: Vec<_> = result.iter().filter(|m| m["role"] != "system").collect();
        assert_eq!(non_system.len(), 4);
    }

    #[tokio::test]
    async fn conversation_window_drops_leading_tool_result() {
        let c = ConversationWindowCondenser::new(10);
        let msgs = vec![
            tool_msg("result1"),
            serde_json::json!({"role": "user", "content": "hi"}),
            serde_json::json!({"role": "assistant", "content": "hello"}),
        ];
        let result = c.condense(msgs).await;
        // Leading tool result should be dropped
        assert!(result.iter().all(|m| m["role"] != "tool"));
    }

    #[tokio::test]
    async fn conversation_window_preserves_system_and_pairs() {
        let c = ConversationWindowCondenser::new(2);
        let mut msgs = vec![system_msg("sys")];
        msgs.extend(make_messages(6));
        let result = c.condense(msgs).await;
        assert!(result.iter().any(|m| m["role"] == "system"));
    }

    #[tokio::test]
    async fn conversation_window_deterministic() {
        let c = ConversationWindowCondenser::new(3);
        let msgs = make_messages(10);
        let r1 = c.condense(msgs.clone()).await;
        let r2 = c.condense(msgs.clone()).await;
        assert_eq!(r1, r2);
    }

    #[tokio::test]
    async fn conversation_window_name() {
        assert_eq!(ConversationWindowCondenser::new(1).name(), "conversation_window");
    }

    // ── AmortizedForgettingCondenser ─────────────────────────────────────────

    #[tokio::test]
    async fn amortized_forgetting_empty_input() {
        let c = AmortizedForgettingCondenser::new(0.5);
        let result = c.condense(vec![]).await;
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn amortized_forgetting_single_message() {
        let c = AmortizedForgettingCondenser::new(0.5);
        let msgs = vec![serde_json::json!({"role": "user", "content": "hi"})];
        let result = c.condense(msgs.clone()).await;
        assert_eq!(result, msgs);
    }

    #[tokio::test]
    async fn amortized_forgetting_ratio_one_keeps_all() {
        let c = AmortizedForgettingCondenser::new(1.0);
        let msgs = make_messages(10);
        let result = c.condense(msgs.clone()).await;
        // keep = ceil(10 * 1.0) = 10 => no truncation
        let non_system: Vec<_> = result.iter().filter(|m| m["role"] != "system").collect();
        assert_eq!(non_system.len(), 10);
    }

    #[tokio::test]
    async fn amortized_forgetting_ratio_half_keeps_half() {
        let c = AmortizedForgettingCondenser::new(0.5);
        let msgs = make_messages(10); // all non-system
        let result = c.condense(msgs).await;
        // keep = ceil(10 * 0.5) = 5
        assert_eq!(result.len(), 5);
    }

    #[tokio::test]
    async fn amortized_forgetting_system_messages_always_kept() {
        let c = AmortizedForgettingCondenser::new(0.1);
        let mut msgs = vec![system_msg("sys")];
        msgs.extend(make_messages(10));
        let result = c.condense(msgs).await;
        assert!(result.iter().any(|m| m["role"] == "system"));
    }

    #[tokio::test]
    async fn amortized_forgetting_deterministic() {
        let c = AmortizedForgettingCondenser::new(0.6);
        let msgs = make_messages(8);
        let r1 = c.condense(msgs.clone()).await;
        let r2 = c.condense(msgs.clone()).await;
        assert_eq!(r1, r2);
    }

    #[tokio::test]
    async fn amortized_forgetting_name() {
        assert_eq!(AmortizedForgettingCondenser::new(0.5).name(), "amortized_forgetting");
    }

    // ── ObservationMaskingCondenser ──────────────────────────────────────────

    #[tokio::test]
    async fn observation_masking_empty_input() {
        let c = ObservationMaskingCondenser::new(5);
        let result = c.condense(vec![]).await;
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn observation_masking_single_tool_message_below_keep_recent() {
        let c = ObservationMaskingCondenser::new(5);
        let msgs = vec![tool_msg("result")];
        let result = c.condense(msgs.clone()).await;
        // Only 1 message, keep_recent=5 => cutoff=0, nothing masked
        assert_eq!(result[0]["content"], "result");
    }

    #[tokio::test]
    async fn observation_masking_masks_old_tool_results() {
        let c = ObservationMaskingCondenser::new(2);
        let msgs = vec![
            tool_msg("old result"),
            tool_msg("another old"),
            tool_msg("recent 1"),
            tool_msg("recent 2"),
        ];
        let result = c.condense(msgs).await;
        // cutoff = 4 - 2 = 2: indices 0 and 1 masked
        assert_eq!(result[0]["content"], "[masked]");
        assert_eq!(result[1]["content"], "[masked]");
        assert_eq!(result[2]["content"], "recent 1");
        assert_eq!(result[3]["content"], "recent 2");
    }

    #[tokio::test]
    async fn observation_masking_does_not_mask_non_tool_messages() {
        let c = ObservationMaskingCondenser::new(1);
        let msgs = vec![
            serde_json::json!({"role": "user", "content": "old user msg"}),
            serde_json::json!({"role": "assistant", "content": "old assistant msg"}),
            tool_msg("recent"),
        ];
        let result = c.condense(msgs).await;
        // cutoff = 3 - 1 = 2: indices 0 and 1 in cutoff range but not role=tool
        assert_eq!(result[0]["content"], "old user msg");
        assert_eq!(result[1]["content"], "old assistant msg");
        assert_eq!(result[2]["content"], "recent");
    }

    #[tokio::test]
    async fn observation_masking_keep_recent_ge_len_no_masking() {
        let c = ObservationMaskingCondenser::new(10);
        let msgs = vec![tool_msg("a"), tool_msg("b"), tool_msg("c")];
        let result = c.condense(msgs).await;
        // cutoff = 3 - 10 = 0 (saturating), nothing masked
        assert_eq!(result[0]["content"], "a");
        assert_eq!(result[1]["content"], "b");
        assert_eq!(result[2]["content"], "c");
    }

    #[tokio::test]
    async fn observation_masking_deterministic() {
        let c = ObservationMaskingCondenser::new(2);
        let msgs = vec![tool_msg("x"), tool_msg("y"), tool_msg("z"), tool_msg("w")];
        let r1 = c.condense(msgs.clone()).await;
        let r2 = c.condense(msgs.clone()).await;
        assert_eq!(r1, r2);
    }

    #[tokio::test]
    async fn observation_masking_name() {
        assert_eq!(ObservationMaskingCondenser::new(1).name(), "observation_masking");
    }

    // ── BrowserOutputCondenser ────────────────────────────────────────────────

    #[tokio::test]
    async fn browser_output_empty_input() {
        let c = BrowserOutputCondenser::new(100);
        let result = c.condense(vec![]).await;
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn browser_output_single_short_message_unchanged() {
        let c = BrowserOutputCondenser::new(100);
        let msgs = vec![tool_msg("short")];
        let result = c.condense(msgs.clone()).await;
        assert_eq!(result[0]["content"], "short");
    }

    #[tokio::test]
    async fn browser_output_below_threshold_unchanged() {
        let c = BrowserOutputCondenser::new(100);
        let content = "a".repeat(50);
        let msgs = vec![tool_msg(&content)];
        let result = c.condense(msgs).await;
        assert_eq!(result[0]["content"], content.as_str());
    }

    #[tokio::test]
    async fn browser_output_at_threshold_unchanged() {
        let c = BrowserOutputCondenser::new(50);
        let content = "a".repeat(50);
        let msgs = vec![tool_msg(&content)];
        let result = c.condense(msgs).await;
        // exactly at threshold => not above => unchanged
        assert_eq!(result[0]["content"].as_str().unwrap().len(), 50);
    }

    #[tokio::test]
    async fn browser_output_above_threshold_truncated() {
        let c = BrowserOutputCondenser::new(10);
        let content = "a".repeat(100);
        let msgs = vec![tool_msg(&content)];
        let result = c.condense(msgs).await;
        let truncated = result[0]["content"].as_str().unwrap();
        assert!(truncated.contains("... [truncated, 100 chars total]"));
        assert!(truncated.starts_with(&"a".repeat(10)));
    }

    #[tokio::test]
    async fn browser_output_only_truncates_tool_messages() {
        let c = BrowserOutputCondenser::new(5);
        let long_content = "x".repeat(100);
        let msgs = vec![
            serde_json::json!({"role": "user", "content": long_content.clone()}),
            serde_json::json!({"role": "assistant", "content": long_content.clone()}),
            tool_msg(&long_content),
        ];
        let result = c.condense(msgs).await;
        // user and assistant messages should NOT be truncated
        assert_eq!(result[0]["content"].as_str().unwrap().len(), 100);
        assert_eq!(result[1]["content"].as_str().unwrap().len(), 100);
        // tool message should be truncated
        assert!(result[2]["content"].as_str().unwrap().contains("[truncated"));
    }

    #[tokio::test]
    async fn browser_output_deterministic() {
        let c = BrowserOutputCondenser::new(20);
        let msgs = vec![tool_msg(&"z".repeat(100))];
        let r1 = c.condense(msgs.clone()).await;
        let r2 = c.condense(msgs.clone()).await;
        assert_eq!(r1, r2);
    }

    #[tokio::test]
    async fn browser_output_name() {
        assert_eq!(BrowserOutputCondenser::new(100).name(), "browser_output");
    }

    // ── LlmSummarizingCondenser ───────────────────────────────────────────────

    #[tokio::test]
    async fn summarizing_condenser_empty_input() {
        let model = MockAdapter::ok("{}");
        let condenser = LlmSummarizingCondenser::new(model);
        let result = condenser.condense(vec![]).await;
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn summarizing_condenser_single_message() {
        let model = MockAdapter::ok("{}");
        let condenser = LlmSummarizingCondenser::with_keep_recent(model, 5);
        let msgs = vec![serde_json::json!({"role": "user", "content": "hi"})];
        let result = condenser.condense(msgs.clone()).await;
        assert_eq!(result, msgs);
    }

    #[tokio::test]
    async fn summarizing_condenser_at_boundary_no_llm_call() {
        // keep_recent == msgs.len() => passthrough, LLM should NOT be called
        // Use the error adapter — if LLM were called, it would change behavior
        let model = MockAdapter::err();
        let condenser = LlmSummarizingCondenser::with_keep_recent(model, 3);
        let msgs = make_messages(3);
        let result = condenser.condense(msgs.clone()).await;
        assert_eq!(result, msgs);
    }

    #[tokio::test]
    async fn summarizing_condenser_name() {
        let model = MockAdapter::ok("{}");
        assert_eq!(LlmSummarizingCondenser::new(model).name(), "llm_summarizing");
    }

    #[tokio::test]
    async fn summarizing_condenser_replaces_old_messages() {
        let model = MockAdapter::ok(r#"{"summary": "A concise summary."}"#);
        let condenser = LlmSummarizingCondenser::with_keep_recent(model, 2);
        let msgs = make_messages(5);
        let result = condenser.condense(msgs.clone()).await;

        // Should have: 1 summary message + 2 recent
        assert_eq!(result.len(), 3);
        assert_eq!(result[0]["role"], "system");
        let content = result[0]["content"].as_str().unwrap();
        assert!(content.contains("[Conversation summary]"));
    }

    #[tokio::test]
    async fn summarizing_condenser_passthrough_when_short() {
        let model = MockAdapter::ok("{}");
        let condenser = LlmSummarizingCondenser::with_keep_recent(model, 10);
        let msgs = make_messages(5);
        let result = condenser.condense(msgs.clone()).await;
        assert_eq!(result.len(), 5);
    }

    #[tokio::test]
    async fn summarizing_condenser_fallback_on_error() {
        let model = MockAdapter::err();
        let condenser = LlmSummarizingCondenser::with_keep_recent(model, 2);
        let msgs = make_messages(5);
        let result = condenser.condense(msgs.clone()).await;
        // Falls back to original on error
        assert_eq!(result.len(), 5);
    }

    // ── LlmAttentionCondenser ─────────────────────────────────────────────────

    #[tokio::test]
    async fn attention_condenser_empty_input() {
        let model = MockAdapter::ok("[]");
        let condenser = LlmAttentionCondenser::new(model);
        let result = condenser.condense(vec![]).await;
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn attention_condenser_no_user_message_passthrough() {
        // No user message → query is empty → return original unchanged
        let model = MockAdapter::ok("[0]");
        let condenser = LlmAttentionCondenser::new(model);
        let msgs = vec![
            serde_json::json!({"role": "assistant", "content": "something"}),
        ];
        let result = condenser.condense(msgs.clone()).await;
        assert_eq!(result, msgs);
    }

    #[tokio::test]
    async fn attention_condenser_empty_indices_fallback() {
        // LLM returns empty array → fallback to original
        let model = MockAdapter::ok("[]");
        let condenser = LlmAttentionCondenser::new(model);
        let msgs = make_messages(4); // has user messages
        let result = condenser.condense(msgs.clone()).await;
        assert_eq!(result, msgs);
    }

    #[tokio::test]
    async fn attention_condenser_name() {
        let model = MockAdapter::ok("[]");
        assert_eq!(LlmAttentionCondenser::new(model).name(), "llm_attention");
    }

    #[tokio::test]
    async fn attention_condenser_selects_indices() {
        // LLM returns indices [0, 2] — two non-system messages to keep
        let model = MockAdapter::ok("[0, 2]");
        let condenser = LlmAttentionCondenser::with_max_select(model, 10);
        let msgs = make_messages(5);
        let result = condenser.condense(msgs.clone()).await;
        // 0 system messages + 2 selected
        assert_eq!(result.len(), 2);
    }

    #[tokio::test]
    async fn attention_condenser_always_keeps_system() {
        let model = MockAdapter::ok("[1]");
        let condenser = LlmAttentionCondenser::new(model);
        let mut msgs = make_messages(3);
        msgs.insert(0, serde_json::json!({"role": "system", "content": "system prompt"}));
        let result = condenser.condense(msgs).await;
        // system message always kept + 1 selected non-system
        assert!(result.iter().any(|m| m["role"] == "system" && m["content"] == "system prompt"));
    }

    #[tokio::test]
    async fn attention_condenser_fallback_on_error() {
        let model = MockAdapter::err();
        let condenser = LlmAttentionCondenser::new(model);
        let msgs = make_messages(4);
        // Need a user message for query extraction
        let result = condenser.condense(msgs.clone()).await;
        assert_eq!(result.len(), 4);
    }

    // ── StructuredSummaryCondenser ────────────────────────────────────────────

    #[tokio::test]
    async fn structured_summary_condenser_empty_input() {
        let model = MockAdapter::ok("{}");
        let condenser = StructuredSummaryCondenser::new(model);
        let result = condenser.condense(vec![]).await;
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn structured_summary_condenser_single_message() {
        let model = MockAdapter::ok("{}");
        let condenser = StructuredSummaryCondenser::with_keep_recent(model, 5);
        let msgs = vec![serde_json::json!({"role": "user", "content": "hi"})];
        let result = condenser.condense(msgs.clone()).await;
        assert_eq!(result, msgs);
    }

    #[tokio::test]
    async fn structured_summary_condenser_at_boundary_no_llm_call() {
        let model = MockAdapter::err();
        let condenser = StructuredSummaryCondenser::with_keep_recent(model, 3);
        let msgs = make_messages(3);
        let result = condenser.condense(msgs.clone()).await;
        assert_eq!(result, msgs);
    }

    #[tokio::test]
    async fn structured_summary_condenser_name() {
        let model = MockAdapter::ok("{}");
        assert_eq!(StructuredSummaryCondenser::new(model).name(), "structured_summary");
    }

    #[tokio::test]
    async fn structured_summary_condenser_replaces_old_messages() {
        let model = MockAdapter::ok(r#"{"key_facts":["fact1"],"decisions":[],"pending_items":[],"context":"ctx"}"#);
        let condenser = StructuredSummaryCondenser::with_keep_recent(model, 2);
        let msgs = make_messages(5);
        let result = condenser.condense(msgs.clone()).await;

        assert_eq!(result.len(), 3);
        assert_eq!(result[0]["role"], "system");
        let content = result[0]["content"].as_str().unwrap();
        assert!(content.contains("[Structured summary]"));
    }

    #[tokio::test]
    async fn structured_summary_condenser_passthrough_when_short() {
        let model = MockAdapter::ok("{}");
        let condenser = StructuredSummaryCondenser::with_keep_recent(model, 10);
        let msgs = make_messages(3);
        let result = condenser.condense(msgs.clone()).await;
        assert_eq!(result.len(), 3);
    }

    #[tokio::test]
    async fn structured_summary_condenser_fallback_on_error() {
        let model = MockAdapter::err();
        let condenser = StructuredSummaryCondenser::with_keep_recent(model, 2);
        let msgs = make_messages(5);
        let result = condenser.condense(msgs.clone()).await;
        assert_eq!(result.len(), 5);
    }
}
