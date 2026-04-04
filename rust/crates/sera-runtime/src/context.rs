//! Context manager — tracks message token usage and compacts when needed.

use crate::types::ChatMessage;

/// Estimates tokens in a message (rough: 4 chars ≈ 1 token).
fn estimate_tokens(msg: &ChatMessage) -> usize {
    let content_len = msg.content.as_deref().map(|s| s.len()).unwrap_or(0);
    let tool_len = msg.tool_calls.as_ref().map(|tcs| {
        tcs.iter().map(|tc| tc.function.arguments.len() + tc.function.name.len()).sum::<usize>()
    }).unwrap_or(0);
    (content_len + tool_len + 20) / 4 // +20 for role/metadata overhead
}

/// Manages conversation context to stay within token limits.
pub struct ContextManager {
    max_tokens: usize,
    strategy: String,
}

impl ContextManager {
    pub fn new(max_tokens: usize, strategy: String) -> Self {
        Self { max_tokens, strategy }
    }

    /// Prepare messages for the LLM, compacting if needed.
    /// Returns a potentially shortened message list.
    pub fn prepare(&self, messages: &[ChatMessage]) -> Vec<ChatMessage> {
        let total: usize = messages.iter().map(estimate_tokens).sum();

        // Use 70% of context window as high-water mark
        let limit = (self.max_tokens as f64 * 0.7) as usize;

        if total <= limit {
            return messages.to_vec();
        }

        match self.strategy.as_str() {
            "truncate" => self.truncate(messages, limit),
            _ => self.summarize_compact(messages, limit),
        }
    }

    /// Truncate strategy: keep system message + last N messages.
    fn truncate(&self, messages: &[ChatMessage], limit: usize) -> Vec<ChatMessage> {
        let mut result = Vec::new();

        // Always keep the system message
        if let Some(sys) = messages.first() {
            if sys.role == "system" {
                result.push(sys.clone());
            }
        }

        // Add messages from the end until we hit the limit
        let mut tokens = result.iter().map(estimate_tokens).sum::<usize>();
        let mut tail = Vec::new();

        for msg in messages.iter().rev() {
            if msg.role == "system" { continue; }
            let msg_tokens = estimate_tokens(msg);
            if tokens + msg_tokens > limit { break; }
            tokens += msg_tokens;
            tail.push(msg.clone());
        }

        tail.reverse();
        result.extend(tail);
        result
    }

    /// Summarize strategy: replace early conversation with a summary message.
    fn summarize_compact(&self, messages: &[ChatMessage], limit: usize) -> Vec<ChatMessage> {
        let mut result = Vec::new();

        // Keep system message
        if let Some(sys) = messages.first() {
            if sys.role == "system" {
                result.push(sys.clone());
            }
        }

        // Insert a summary of dropped messages
        let keep_count = messages.len() / 2; // Keep the latter half
        let dropped = &messages[1..messages.len().saturating_sub(keep_count)];
        if !dropped.is_empty() {
            let summary = format!(
                "[Context compacted: {} earlier messages summarized. The conversation involved tool calls and reasoning steps.]",
                dropped.len()
            );
            result.push(ChatMessage {
                role: "system".to_string(),
                content: Some(summary),
                ..Default::default()
            });
        }

        // Keep the tail messages
        let tail_start = messages.len().saturating_sub(keep_count);
        for msg in &messages[tail_start..] {
            result.push(msg.clone());
        }

        // Final check — if still over limit, fall back to truncate
        let total: usize = result.iter().map(estimate_tokens).sum();
        if total > limit {
            return self.truncate(messages, limit);
        }

        result
    }
}
