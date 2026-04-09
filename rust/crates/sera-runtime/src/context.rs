//! Context manager — tracks message token usage and compacts when needed.
//!
//! Ported from the TypeScript implementation in `core/agent-runtime/src/contextManager.ts`.
//! Uses a chars/4 heuristic for token estimation (no tiktoken dependency).

use crate::types::ChatMessage;

// ── Defaults ────────────────────────────────────────────────────────────────

const DEFAULT_CONTEXT_WINDOW: usize = 128_000;
const DEFAULT_HIGH_WATER_PCT: f64 = 0.95;
const DEFAULT_CLEAR_TARGET_PCT: f64 = 0.80;
const DEFAULT_TOOL_OUTPUT_MAX_TOKENS: usize = 4_000;
const DEFAULT_PRESERVE_RECENT: usize = 4;
const COMPACTION_STEP: usize = 4;
const OVERHEAD_CHARS_PER_MSG: usize = 20;

// ── CompactionResult ────────────────────────────────────────────────────────

/// Result of a compaction operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactionResult {
    /// Number of messages dropped during compaction.
    pub dropped_count: usize,
    /// Total estimated tokens before compaction.
    pub tokens_before: usize,
    /// Total estimated tokens after compaction.
    pub tokens_after: usize,
}

// ── ContextManager ──────────────────────────────────────────────────────────

/// Manages conversation context to stay within the model's token limit.
///
/// Provides token estimation, high-water-mark detection, tool output truncation,
/// and sliding-window compaction of message histories.
pub struct ContextManager {
    /// Total tokens available in the context window.
    context_window: usize,
    /// Trigger compaction when token count reaches this level (95% of window).
    high_water_mark: usize,
    /// Target token count after compaction (80% of window).
    clear_target: usize,
    /// Truncate individual tool outputs above this many tokens.
    tool_output_max_tokens: usize,
    /// Always preserve at least this many recent non-system messages.
    preserve_recent: usize,
}

impl ContextManager {
    /// Create a new `ContextManager` for the given context window size.
    pub fn new(context_window: usize) -> Self {
        let window = if context_window == 0 {
            DEFAULT_CONTEXT_WINDOW
        } else {
            context_window
        };
        Self {
            context_window: window,
            high_water_mark: (window as f64 * DEFAULT_HIGH_WATER_PCT) as usize,
            clear_target: (window as f64 * DEFAULT_CLEAR_TARGET_PCT) as usize,
            tool_output_max_tokens: DEFAULT_TOOL_OUTPUT_MAX_TOKENS,
            preserve_recent: DEFAULT_PRESERVE_RECENT,
        }
    }

    /// Create a `ContextManager` with custom parameters.
    pub fn with_params(
        context_window: usize,
        high_water_pct: f64,
        clear_target_pct: f64,
        tool_output_max_tokens: usize,
        preserve_recent: usize,
    ) -> Self {
        let window = if context_window == 0 {
            DEFAULT_CONTEXT_WINDOW
        } else {
            context_window
        };
        Self {
            context_window: window,
            high_water_mark: (window as f64 * high_water_pct) as usize,
            clear_target: (window as f64 * clear_target_pct) as usize,
            tool_output_max_tokens,
            preserve_recent,
        }
    }

    // ── Token estimation ────────────────────────────────────────────────

    /// Estimate token count for a string using the chars/4 heuristic.
    pub fn estimate_tokens(text: &str) -> usize {
        // Ceiling division to avoid undercount on short strings.
        (text.len() + 3) / 4
    }

    /// Estimate tokens for a single `ChatMessage`.
    pub fn estimate_message_tokens(msg: &ChatMessage) -> usize {
        let content_len = msg.content.as_deref().map(|s| s.len()).unwrap_or(0);
        let tool_len = msg
            .tool_calls
            .as_ref()
            .map(|tcs| {
                tcs.iter()
                    .map(|tc| tc.function.arguments.len() + tc.function.name.len())
                    .sum::<usize>()
            })
            .unwrap_or(0);
        // +OVERHEAD for role, delimiters, and metadata.
        (content_len + tool_len + OVERHEAD_CHARS_PER_MSG) / 4
    }

    /// Count total estimated tokens across all messages.
    pub fn count_message_tokens(messages: &[ChatMessage]) -> usize {
        messages.iter().map(Self::estimate_message_tokens).sum()
    }

    /// Count tokens in a slice of `serde_json::Value` messages.
    pub fn count_json_message_tokens(messages: &[serde_json::Value]) -> usize {
        messages.iter().map(|v| Self::estimate_json_msg_tokens(v)).sum()
    }

    /// Estimate tokens for a JSON-encoded message.
    fn estimate_json_msg_tokens(val: &serde_json::Value) -> usize {
        let content_len = val
            .get("content")
            .and_then(|c| c.as_str())
            .map(|s| s.len())
            .unwrap_or(0);
        let tool_len = val
            .get("tool_calls")
            .and_then(|t| t.as_array())
            .map(|arr| {
                arr.iter()
                    .map(|tc| {
                        let f = tc.get("function").cloned().unwrap_or_default();
                        let name_len = f
                            .get("name")
                            .and_then(|n| n.as_str())
                            .map(|s| s.len())
                            .unwrap_or(0);
                        let args_len = f
                            .get("arguments")
                            .and_then(|a| a.as_str())
                            .map(|s| s.len())
                            .unwrap_or(0);
                        name_len + args_len
                    })
                    .sum::<usize>()
            })
            .unwrap_or(0);
        (content_len + tool_len + OVERHEAD_CHARS_PER_MSG) / 4
    }

    // ── Limit detection ─────────────────────────────────────────────────

    /// Returns `true` when the messages are at or above the high-water mark.
    pub fn is_near_limit(&self, messages: &[ChatMessage]) -> bool {
        Self::count_message_tokens(messages) >= self.high_water_mark
    }

    /// Returns `true` when JSON messages are at or above the high-water mark.
    pub fn is_near_limit_json(&self, messages: &[serde_json::Value]) -> bool {
        Self::count_json_message_tokens(messages) >= self.high_water_mark
    }

    // ── Tool output truncation ──────────────────────────────────────────

    /// Truncate a tool output string if it exceeds `tool_output_max_tokens`.
    ///
    /// When truncated, appends a notice indicating the truncation.
    pub fn truncate_tool_output(&self, content: &str) -> String {
        let tokens = Self::estimate_tokens(content);
        if tokens <= self.tool_output_max_tokens {
            return content.to_string();
        }

        let notice = format!(
            "\n\n[SERA: output truncated — exceeded {} tokens]",
            self.tool_output_max_tokens
        );
        let notice_tokens = Self::estimate_tokens(&notice);
        let target_chars = (self.tool_output_max_tokens.saturating_sub(notice_tokens)) * 4;

        // Truncate at a char boundary.
        let truncated = if target_chars >= content.len() {
            content.to_string()
        } else {
            let end = content
                .char_indices()
                .take_while(|(i, _)| *i < target_chars)
                .last()
                .map(|(i, c)| i + c.len_utf8())
                .unwrap_or(0);
            content[..end].to_string()
        };

        format!("{truncated}{notice}")
    }

    // ── Compaction ──────────────────────────────────────────────────────

    /// Compact the message history using sliding-window compaction.
    ///
    /// Algorithm:
    /// 1. Separate system messages (indices) from non-system messages.
    /// 2. If total tokens < high_water_mark, return early (no compaction needed).
    /// 3. Drop oldest non-system messages in groups of [`COMPACTION_STEP`] until
    ///    total tokens fall below [`clear_target`] (80% of window).
    /// 4. Always preserve the last [`preserve_recent`] non-system messages.
    /// 5. Insert a compaction summary system message replacing the dropped block.
    ///
    /// Returns the compaction result and mutates `messages` in place.
    pub fn compact(&self, messages: &mut Vec<ChatMessage>) -> CompactionResult {
        let tokens_before = Self::count_message_tokens(messages);

        if tokens_before < self.high_water_mark {
            return CompactionResult {
                dropped_count: 0,
                tokens_before,
                tokens_after: tokens_before,
            };
        }

        // Partition into system and non-system, preserving order.
        let mut system_messages: Vec<ChatMessage> = Vec::new();
        let mut non_system_messages: Vec<ChatMessage> = Vec::new();
        for msg in messages.drain(..) {
            if msg.role == "system" {
                system_messages.push(msg);
            } else {
                non_system_messages.push(msg);
            }
        }

        let keep_limit = non_system_messages
            .len()
            .min(self.preserve_recent);
        let mut dropped_count: usize = 0;

        // Drop oldest non-system messages in groups until under target.
        while non_system_messages.len() > keep_limit {
            let current_tokens = Self::count_message_tokens(&system_messages)
                + Self::count_message_tokens(&non_system_messages);
            if current_tokens < self.clear_target {
                break;
            }
            let to_drop = COMPACTION_STEP.min(non_system_messages.len() - keep_limit);
            if to_drop == 0 {
                break;
            }
            // Remove from the front (oldest).
            non_system_messages.drain(..to_drop);
            dropped_count += to_drop;
        }

        // Build the compacted message list.
        if dropped_count > 0 {
            let summary = ChatMessage {
                role: "system".to_string(),
                content: Some(format!(
                    "[Context compacted: {} earlier messages removed to fit within context window.]",
                    dropped_count
                )),
                ..Default::default()
            };
            // system messages, then summary, then remaining non-system.
            messages.extend(system_messages);
            messages.push(summary);
        } else {
            messages.extend(system_messages);
        }
        messages.extend(non_system_messages);

        let tokens_after = Self::count_message_tokens(messages);

        CompactionResult {
            dropped_count,
            tokens_before,
            tokens_after,
        }
    }

    // ── Budget ──────────────────────────────────────────────────────────

    /// Get the remaining token budget before hitting the high-water mark.
    pub fn available_budget(&self, messages: &[ChatMessage]) -> usize {
        self.high_water_mark
            .saturating_sub(Self::count_message_tokens(messages))
    }

    /// Get the context window size.
    pub fn context_window(&self) -> usize {
        self.context_window
    }

    /// Get the high-water mark.
    pub fn high_water_mark(&self) -> usize {
        self.high_water_mark
    }

    // ── Legacy interface (backward compat with reasoning_loop.rs) ───────

    /// Prepare messages for the LLM, compacting if needed.
    ///
    /// This preserves the old `prepare()` API used by the reasoning loop.
    /// Internally it delegates to [`compact`] when the high-water mark is reached.
    pub fn prepare(&self, messages: &[ChatMessage]) -> Vec<ChatMessage> {
        let mut msgs = messages.to_vec();
        self.compact(&mut msgs);
        msgs
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ChatMessage;

    fn make_msg(role: &str, content: &str) -> ChatMessage {
        ChatMessage {
            role: role.to_string(),
            content: Some(content.to_string()),
            ..Default::default()
        }
    }

    fn make_msg_sized(role: &str, size_chars: usize) -> ChatMessage {
        ChatMessage {
            role: role.to_string(),
            content: Some("x".repeat(size_chars)),
            ..Default::default()
        }
    }

    // ── Token estimation ────────────────────────────────────────────

    #[test]
    fn estimate_tokens_empty_string() {
        assert_eq!(ContextManager::estimate_tokens(""), 0);
    }

    #[test]
    fn estimate_tokens_short_string() {
        // "hello" = 5 chars → (5+3)/4 = 2
        assert_eq!(ContextManager::estimate_tokens("hello"), 2);
    }

    #[test]
    fn estimate_tokens_exact_multiple() {
        // 8 chars → (8+3)/4 = 2
        assert_eq!(ContextManager::estimate_tokens("12345678"), 2);
    }

    #[test]
    fn estimate_tokens_long_string() {
        let text = "a".repeat(1000);
        // 1000 chars → (1000+3)/4 = 250
        assert_eq!(ContextManager::estimate_tokens(&text), 250);
    }

    #[test]
    fn estimate_message_tokens_with_content() {
        let msg = make_msg("user", "Hello, world!"); // 13 chars content
        let tokens = ContextManager::estimate_message_tokens(&msg);
        // (13 + 0 + 20) / 4 = 8
        assert_eq!(tokens, 8);
    }

    #[test]
    fn estimate_message_tokens_no_content() {
        let msg = ChatMessage {
            role: "assistant".to_string(),
            content: None,
            ..Default::default()
        };
        let tokens = ContextManager::estimate_message_tokens(&msg);
        // (0 + 0 + 20) / 4 = 5
        assert_eq!(tokens, 5);
    }

    #[test]
    fn count_message_tokens_multiple() {
        let messages = vec![
            make_msg("system", "You are helpful"),
            make_msg("user", "Hi"),
        ];
        let total = ContextManager::count_message_tokens(&messages);
        // system: (15+20)/4 = 8, user: (2+20)/4 = 5 → 13
        assert_eq!(total, 13);
    }

    // ── Near-limit detection ────────────────────────────────────────

    #[test]
    fn is_near_limit_below_threshold() {
        let cm = ContextManager::new(1000);
        let messages = vec![make_msg("user", "short")];
        assert!(!cm.is_near_limit(&messages));
    }

    #[test]
    fn is_near_limit_at_threshold() {
        // Window = 100, high water = 95
        let cm = ContextManager::new(100);
        // Need ~95 tokens. Each token ≈ 4 chars, plus 20 overhead per msg.
        // One message with (95*4 - 20) = 360 chars → (360+20)/4 = 95 tokens.
        let messages = vec![make_msg_sized("user", 360)];
        assert!(cm.is_near_limit(&messages));
    }

    #[test]
    fn is_near_limit_above_threshold() {
        let cm = ContextManager::new(100);
        // 500 chars → (500+20)/4 = 130 tokens > 95
        let messages = vec![make_msg_sized("user", 500)];
        assert!(cm.is_near_limit(&messages));
    }

    // ── Tool output truncation ──────────────────────────────────────

    #[test]
    fn truncate_tool_output_under_limit() {
        let cm = ContextManager::new(128_000);
        let short = "Hello, world!";
        assert_eq!(cm.truncate_tool_output(short), short);
    }

    #[test]
    fn truncate_tool_output_over_limit() {
        let cm = ContextManager::with_params(128_000, 0.95, 0.80, 10, 4);
        // 10 tokens max → ~40 chars. Create a 200-char string.
        let long = "a".repeat(200);
        let result = cm.truncate_tool_output(&long);
        assert!(result.contains("[SERA: output truncated"));
        assert!(result.len() < long.len() + 60); // truncated + notice
    }

    #[test]
    fn truncate_tool_output_exact_limit() {
        let cm = ContextManager::with_params(128_000, 0.95, 0.80, 100, 4);
        // 100 tokens ≈ 400 chars. Build a string with (100*4 - 20) = 380 chars
        // so message estimate = (380+20)/4 = 100 tokens.
        // But estimate_tokens for raw string: (380+3)/4 = 95 tokens. Under 100, no truncation.
        let content = "x".repeat(380);
        let result = cm.truncate_tool_output(&content);
        assert_eq!(result, content); // should not truncate
    }

    // ── Compaction ──────────────────────────────────────────────────

    #[test]
    fn compact_no_compaction_needed() {
        let cm = ContextManager::new(10_000);
        let mut messages = vec![
            make_msg("system", "Be helpful"),
            make_msg("user", "Hi"),
            make_msg("assistant", "Hello!"),
        ];
        let result = cm.compact(&mut messages);
        assert_eq!(result.dropped_count, 0);
        assert_eq!(result.tokens_before, result.tokens_after);
        assert_eq!(messages.len(), 3);
    }

    #[test]
    fn compact_drops_old_messages() {
        // Tiny window to force compaction.
        // Window = 200, high water = 190, clear target = 160.
        let cm = ContextManager::with_params(200, 0.95, 0.80, 4000, 2);

        let mut messages = vec![
            make_msg("system", "System prompt"),
            make_msg("user", "First user message with some content that takes tokens"),
            make_msg("assistant", "First assistant response with some content"),
            make_msg("user", "Second user message"),
            make_msg("assistant", "Second assistant response"),
            make_msg("user", "Third user message"),
            make_msg("assistant", "Third assistant response"),
            // Add more to push over the limit.
            make_msg("user", &"x".repeat(400)),
            make_msg("assistant", &"y".repeat(400)),
        ];

        let original_count = messages.len();
        let result = cm.compact(&mut messages);

        // Should have dropped some messages.
        assert!(result.dropped_count > 0, "Expected some messages to be dropped");
        assert!(
            messages.len() < original_count,
            "Expected fewer messages after compaction: {} < {}",
            messages.len(),
            original_count
        );
        assert!(result.tokens_after <= result.tokens_before);
    }

    #[test]
    fn compact_preserves_system_messages() {
        let cm = ContextManager::with_params(100, 0.95, 0.80, 4000, 2);
        let mut messages = vec![
            make_msg("system", "System prompt"),
            make_msg("user", &"x".repeat(200)),
            make_msg("assistant", &"y".repeat(200)),
            make_msg("user", "Recent 1"),
            make_msg("assistant", "Recent 2"),
        ];

        cm.compact(&mut messages);

        // System message must survive.
        assert!(
            messages.iter().any(|m| m.role == "system"
                && m.content.as_deref().is_some_and(|c| c.contains("System prompt"))),
            "Original system message must be preserved"
        );
    }

    #[test]
    fn compact_preserves_recent_messages() {
        let cm = ContextManager::with_params(100, 0.95, 0.80, 4000, 2);
        let mut messages = vec![
            make_msg("system", "Sys"),
            make_msg("user", &"old".repeat(50)),
            make_msg("assistant", &"old".repeat(50)),
            make_msg("user", "recent-user"),
            make_msg("assistant", "recent-assistant"),
        ];

        cm.compact(&mut messages);

        // The last 2 non-system messages should be preserved.
        let non_system: Vec<_> = messages.iter().filter(|m| m.role != "system").collect();
        assert!(non_system.len() >= 2);
        let last = non_system.last().unwrap();
        assert_eq!(last.content.as_deref(), Some("recent-assistant"));
    }

    #[test]
    fn compact_inserts_summary_when_dropping() {
        let cm = ContextManager::with_params(100, 0.95, 0.80, 4000, 2);
        let mut messages = vec![
            make_msg("system", "Sys"),
            make_msg("user", &"x".repeat(200)),
            make_msg("assistant", &"y".repeat(200)),
            make_msg("user", "Recent"),
            make_msg("assistant", "Recent"),
        ];

        let result = cm.compact(&mut messages);
        if result.dropped_count > 0 {
            // Should have a compaction summary system message.
            assert!(
                messages.iter().any(|m| {
                    m.role == "system"
                        && m.content
                            .as_deref()
                            .is_some_and(|c| c.contains("Context compacted"))
                }),
                "Expected a compaction summary message"
            );
        }
    }

    // ── Available budget ────────────────────────────────────────────

    #[test]
    fn available_budget_empty_history() {
        let cm = ContextManager::new(1000);
        let messages: Vec<ChatMessage> = vec![];
        // high water = 950
        assert_eq!(cm.available_budget(&messages), 950);
    }

    #[test]
    fn available_budget_partial_usage() {
        let cm = ContextManager::new(1000);
        let messages = vec![make_msg("user", "short")]; // ~6 tokens
        let budget = cm.available_budget(&messages);
        let used = ContextManager::count_message_tokens(&messages);
        assert_eq!(budget, 950 - used);
    }

    #[test]
    fn available_budget_over_limit() {
        let cm = ContextManager::new(100);
        // 500 chars → well over the 95-token high-water mark.
        let messages = vec![make_msg_sized("user", 500)];
        assert_eq!(cm.available_budget(&messages), 0);
    }

    // ── Constructor edge cases ──────────────────────────────────────

    #[test]
    fn new_with_zero_window_uses_default() {
        let cm = ContextManager::new(0);
        assert_eq!(cm.context_window(), DEFAULT_CONTEXT_WINDOW);
    }

    #[test]
    fn prepare_returns_compacted_copy() {
        let cm = ContextManager::new(10_000);
        let messages = vec![
            make_msg("system", "Be helpful"),
            make_msg("user", "Hi"),
        ];
        let prepared = cm.prepare(&messages);
        // Should return same messages (no compaction needed).
        assert_eq!(prepared.len(), messages.len());
    }

    // ── JSON message token counting ─────────────────────────────────

    #[test]
    fn count_json_message_tokens_basic() {
        let msgs: Vec<serde_json::Value> = vec![
            serde_json::json!({"role": "user", "content": "Hello"}),
            serde_json::json!({"role": "assistant", "content": "Hi there"}),
        ];
        let total = ContextManager::count_json_message_tokens(&msgs);
        // "Hello" = 5 chars, "Hi there" = 8 chars
        // (5+20)/4 + (8+20)/4 = 6 + 7 = 13
        assert_eq!(total, 13);
    }
}
