//! Condenser implementations — all 9 per SPEC-runtime §4.
//!
//! LLM-backed condensers are stubs in Phase 0.

use async_trait::async_trait;

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

// ── 7. LLMSummarizingCondenser (stub) ──────────────────────────────────────

/// Summarizes older messages using an LLM call.
/// TODO(P1): integrate with LLM client for actual summarization.
pub struct LlmSummarizingCondenser;

#[async_trait]
impl Condenser for LlmSummarizingCondenser {
    fn name(&self) -> &str {
        "llm_summarizing"
    }
    async fn condense(&self, messages: Vec<serde_json::Value>) -> Vec<serde_json::Value> {
        // TODO(P1): call LLM to summarize older messages
        messages
    }
}

// ── 8. LLMAttentionCondenser (stub) ─────────────────────────────────────────

/// Uses attention scores to select relevant messages.
/// TODO(P1): integrate with LLM client for attention-weighted selection.
pub struct LlmAttentionCondenser;

#[async_trait]
impl Condenser for LlmAttentionCondenser {
    fn name(&self) -> &str {
        "llm_attention"
    }
    async fn condense(&self, messages: Vec<serde_json::Value>) -> Vec<serde_json::Value> {
        // TODO(P1): use attention scores for selection
        messages
    }
}

// ── 9. StructuredSummaryCondenser (stub) ────────────────────────────────────

/// Produces a structured summary (key facts, decisions, pending items).
/// TODO(P1): integrate with LLM client for structured extraction.
pub struct StructuredSummaryCondenser;

#[async_trait]
impl Condenser for StructuredSummaryCondenser {
    fn name(&self) -> &str {
        "structured_summary"
    }
    async fn condense(&self, messages: Vec<serde_json::Value>) -> Vec<serde_json::Value> {
        // TODO(P1): extract structured summary
        messages
    }
}
