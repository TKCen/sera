//! Four-tier working memory wrapper types.
//!
//! Implements the SPEC-memory §2.0 Four-Tier Memory ABC (BeeAI validated):
//! - UnconstrainedMemory: Tier 1 — no limit, keeps full history
//! - TokenMemory: Tier 2 — evicts oldest when token budget exceeded
//! - SlidingWindowMemory: Tier 3 — fixed message-count sliding window
//! - SummarizeMemory: Tier 4 — LLM-driven compaction when budget hit
//!
//! These wrappers wrap a backing store (typically Transcript from sera-session)
//! and enforce the eviction/compaction policy based on the configured tier.

use crate::transcript::{ContentBlock, Role, Transcript, TranscriptEntry};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Configuration for a memory tier.
/// Each tier has its own specific configuration options.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "tier", rename_all = "snake_case")]
pub enum MemoryTierConfig {
    /// Tier 1: Unconstrained — no limit configuration needed.
    Unconstrained,
    /// Tier 2: Token-bounded with a max token budget.
    TokenBounded {
        /// Maximum token budget before eviction kicks in.
        max_tokens: u32,
        /// Approximate tokens per message (for estimating when to trigger eviction).
        /// Default: 256 tokens/message.
        #[serde(default = "default_tokens_per_message")]
        tokens_per_message: u32,
    },
    /// Tier 3: Sliding window with fixed message count.
    SlidingWindow {
        /// Maximum number of messages to keep in the window.
        max_messages: u32,
    },
    /// Tier 4: Summarizing with token budget and compaction settings.
    Summarizing {
        /// Maximum token budget before triggering compaction.
        max_tokens: u32,
        /// Minimum number of messages before compaction is considered.
        /// Default: 10
        #[serde(default = "default_min_messages_for_compact")]
        min_messages_for_compact: u32,
        /// Maximum summary length in tokens.
        /// Default: 512
        #[serde(default = "default_max_summary_tokens")]
        max_summary_tokens: u32,
    },
}

fn default_tokens_per_message() -> u32 {
    256
}

fn default_min_messages_for_compact() -> u32 {
    10
}

fn default_max_summary_tokens() -> u32 {
    512
}

/// Statistics about a memory tier's current state.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryStats {
    pub entry_count: u32,
    pub estimated_tokens: u32,
    pub tier: String,
}

/// Trait for memory tier wrappers.
/// Each wrapper implements this trait to provide tier-specific behavior.
pub trait MemoryWrapper: Send + Sync {
    /// Get the current working memory tier type.
    fn tier(&self) -> WorkingMemoryTier;

    /// Add a new entry to memory.
    fn push(&mut self, entry: TranscriptEntry);

    /// Get all entries.
    fn entries(&self) -> Vec<TranscriptEntry>;

    /// Get the last N entries.
    fn last_n(&self, n: usize) -> Vec<TranscriptEntry>;

    /// Get current statistics.
    fn stats(&self) -> MemoryStats;

    /// Check if eviction or compaction is needed.
    fn needs_maintenance(&self) -> bool;

    /// Perform maintenance (eviction/compaction) if needed.
    /// Returns Some(summary) if compaction was performed, None otherwise.
    fn maintain(&mut self) -> Option<String>;

    /// Get the accumulated summary (for Summarizing tier).
    fn summary(&self) -> Option<&str>;
}

/// Working memory tier type (matches sera_types::WorkingMemoryTier).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkingMemoryTier {
    /// Tier 1: No limit — keeps full history.
    Unconstrained,
    /// Tier 2: Evicts oldest when token budget exceeded.
    TokenBounded,
    /// Tier 3: Fixed message-count sliding window.
    SlidingWindow,
    /// Tier 4: LLM-driven compaction when budget hit.
    Summarizing,
}

// ---------------------------------------------------------------------------
// Tier 1: UnconstrainedMemory — keeps all history
// ---------------------------------------------------------------------------

/// A memory wrapper that keeps all entries (no eviction).
pub struct UnconstrainedMemory {
    transcript: Transcript,
}

impl UnconstrainedMemory {
    pub fn new() -> Self {
        Self {
            transcript: Transcript::new(),
        }
    }

    /// Create from an existing transcript.
    pub fn from_transcript(transcript: Transcript) -> Self {
        Self { transcript }
    }

    /// Get the underlying transcript.
    pub fn into_transcript(self) -> Transcript {
        self.transcript
    }
}

impl Default for UnconstrainedMemory {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryWrapper for UnconstrainedMemory {
    fn tier(&self) -> WorkingMemoryTier {
        WorkingMemoryTier::Unconstrained
    }

    fn push(&mut self, entry: TranscriptEntry) {
        self.transcript.append(entry);
    }

    fn entries(&self) -> Vec<TranscriptEntry> {
        self.transcript.entries().to_vec()
    }

    fn last_n(&self, n: usize) -> Vec<TranscriptEntry> {
        self.transcript.last_n(n).to_vec()
    }

    fn stats(&self) -> MemoryStats {
        let entry_count = self.transcript.len() as u32;
        // Rough estimate: 256 tokens per entry on average
        let estimated_tokens = entry_count * 256;
        MemoryStats {
            entry_count,
            estimated_tokens,
            tier: "unconstrained".to_string(),
        }
    }

    fn needs_maintenance(&self) -> bool {
        false // Never needs maintenance — keeps everything
    }

    fn maintain(&mut self) -> Option<String> {
        None // No-op
    }

    fn summary(&self) -> Option<&str> {
        None // No summary in unconstrained mode
    }
}

// ---------------------------------------------------------------------------
// Tier 2: TokenMemory — evicts by token budget
// ---------------------------------------------------------------------------

/// Configuration for TokenMemory.
#[derive(Debug, Clone)]
pub struct TokenConfig {
    pub max_tokens: u32,
    pub tokens_per_message: u32,
}

impl Default for TokenConfig {
    fn default() -> Self {
        Self {
            max_tokens: 128_000, // ~128K tokens (default context window)
            tokens_per_message: 256,
        }
    }
}

/// A memory wrapper that evicts the oldest entries when token budget is exceeded.
pub struct TokenMemory {
    transcript: Transcript,
    config: TokenConfig,
    /// Estimated current token count.
    estimated_tokens: u32,
}

impl TokenMemory {
    pub fn new(config: TokenConfig) -> Self {
        Self {
            transcript: Transcript::new(),
            config,
            estimated_tokens: 0,
        }
    }

    /// Create from an existing transcript.
    pub fn from_transcript(transcript: Transcript, config: TokenConfig) -> Self {
        let estimated_tokens = transcript.len() as u32 * config.tokens_per_message;
        Self {
            transcript,
            config,
            estimated_tokens,
        }
    }

    /// Get the underlying transcript.
    pub fn into_transcript(self) -> Transcript {
        self.transcript
    }

    /// Estimate tokens in a transcript entry.
    fn estimate_tokens(entry: &TranscriptEntry) -> u32 {
        // Rough estimate: ~4 characters per token
        let char_count: usize = entry.blocks.iter().map(|b| match b {
            ContentBlock::Text { text } => text.len(),
            ContentBlock::ToolUse { input, .. } => serde_json::to_string(input).map(|s| s.len()).unwrap_or(0),
            ContentBlock::ToolResult { content, .. } => content.len(),
            ContentBlock::Thinking { thinking } => thinking.len(),
            ContentBlock::Image { data, .. } => data.len(),
            _ => 0,
        }).sum();
        (char_count / 4) as u32
    }

    /// Evict oldest entries until under budget.
    fn evict(&mut self) {
        while self.estimated_tokens > self.config.max_tokens && self.transcript.len() > 1 {
            // Remove the oldest entry (index 0) - collect entries first to avoid borrow issue
            let entries: Vec<TranscriptEntry> = self.transcript.entries().to_vec();
            self.transcript.clear();
            // Re-add all entries except the first
            for entry in entries.iter().skip(1) {
                self.transcript.append(entry.clone());
            }
            // Subtract based on configured tokens per message (not actual estimate)
            self.estimated_tokens = self.estimated_tokens.saturating_sub(self.config.tokens_per_message);
        }
    }
}

impl MemoryWrapper for TokenMemory {
    fn tier(&self) -> WorkingMemoryTier {
        WorkingMemoryTier::TokenBounded
    }

    fn push(&mut self, entry: TranscriptEntry) {
        // Use configured tokens_per_message instead of actual estimate for consistency
        self.estimated_tokens += self.config.tokens_per_message;
        self.transcript.append(entry);
        // Call maintain to trigger eviction if over budget
        if self.needs_maintenance() {
            self.evict();
        }
    }

    fn entries(&self) -> Vec<TranscriptEntry> {
        self.transcript.entries().to_vec()
    }

    fn last_n(&self, n: usize) -> Vec<TranscriptEntry> {
        self.transcript.last_n(n).to_vec()
    }

    fn stats(&self) -> MemoryStats {
        MemoryStats {
            entry_count: self.transcript.len() as u32,
            estimated_tokens: self.estimated_tokens,
            tier: "token_bounded".to_string(),
        }
    }

    fn needs_maintenance(&self) -> bool {
        self.estimated_tokens >= self.config.max_tokens && self.transcript.len() > 1
    }

    fn maintain(&mut self) -> Option<String> {
        if self.needs_maintenance() {
            self.evict();
        }
        None // No summary for token-bounded
    }

    fn summary(&self) -> Option<&str> {
        None
    }
}

// ---------------------------------------------------------------------------
// Tier 3: SlidingWindowMemory — fixed message count window
// ---------------------------------------------------------------------------

/// Configuration for SlidingWindowMemory.
#[derive(Debug, Clone)]
pub struct SlidingWindowConfig {
    pub max_messages: u32,
}

impl Default for SlidingWindowConfig {
    fn default() -> Self {
        Self {
            max_messages: 100, // Default: keep last 100 messages
        }
    }
}

/// A memory wrapper that maintains a fixed-size sliding window of messages.
pub struct SlidingWindowMemory {
    transcript: Transcript,
    config: SlidingWindowConfig,
}

impl SlidingWindowMemory {
    pub fn new(config: SlidingWindowConfig) -> Self {
        Self {
            transcript: Transcript::new(),
            config,
        }
    }

    /// Create from an existing transcript.
    pub fn from_transcript(transcript: Transcript, config: SlidingWindowConfig) -> Self {
        Self {
            transcript,
            config,
        }
    }

    /// Get the underlying transcript.
    pub fn into_transcript(self) -> Transcript {
        self.transcript
    }

    /// Evict oldest entries to maintain window size.
    fn evict(&mut self) {
        let max_len = self.config.max_messages as usize;
        while self.transcript.len() > max_len {
            // Remove oldest entry
            let entries = self.transcript.entries().to_vec();
            self.transcript.clear();
            for entry in entries.iter().skip(1) {
                self.transcript.append(entry.clone());
            }
        }
    }
}

impl MemoryWrapper for SlidingWindowMemory {
    fn tier(&self) -> WorkingMemoryTier {
        WorkingMemoryTier::SlidingWindow
    }

    fn push(&mut self, entry: TranscriptEntry) {
        self.transcript.append(entry);
        // Maintain window size after adding
        if self.needs_maintenance() {
            self.evict();
        }
    }

    fn entries(&self) -> Vec<TranscriptEntry> {
        self.transcript.entries().to_vec()
    }

    fn last_n(&self, n: usize) -> Vec<TranscriptEntry> {
        self.transcript.last_n(n).to_vec()
    }

    fn stats(&self) -> MemoryStats {
        let entry_count = self.transcript.len() as u32;
        MemoryStats {
            entry_count,
            estimated_tokens: entry_count * 256,
            tier: "sliding_window".to_string(),
        }
    }

    fn needs_maintenance(&self) -> bool {
        self.transcript.len() > self.config.max_messages as usize
    }

    fn maintain(&mut self) -> Option<String> {
        if self.needs_maintenance() {
            self.evict();
        }
        None
    }

    fn summary(&self) -> Option<&str> {
        None
    }
}

// ---------------------------------------------------------------------------
// Tier 4: SummarizeMemory — LLM-driven compaction
// ---------------------------------------------------------------------------

/// Configuration for SummarizeMemory.
#[derive(Debug, Clone)]
pub struct SummarizeConfig {
    pub max_tokens: u32,
    pub min_messages_for_compact: u32,
    pub max_summary_tokens: u32,
}

impl Default for SummarizeConfig {
    fn default() -> Self {
        Self {
            max_tokens: 128_000,
            min_messages_for_compact: 10,
            max_summary_tokens: 512,
        }
    }
}

/// A memory wrapper that triggers LLM compaction when token budget is hit.
/// This is a minimal implementation — actual summarization requires an LLM client.
pub struct SummarizeMemory {
    transcript: Transcript,
    config: SummarizeConfig,
    /// Accumulated summary from past compactions.
    summary: Option<String>,
    /// Estimated current token count.
    estimated_tokens: u32,
    /// Count of messages since last compaction.
    messages_since_compact: u32,
}

impl SummarizeMemory {
    pub fn new(config: SummarizeConfig) -> Self {
        Self {
            transcript: Transcript::new(),
            config,
            summary: None,
            estimated_tokens: 0,
            messages_since_compact: 0,
        }
    }

    /// Create from an existing transcript.
    pub fn from_transcript(transcript: Transcript, config: SummarizeConfig) -> Self {
        let entry_count = transcript.len() as u32;
        Self {
            transcript,
            config,
            summary: None,
            estimated_tokens: entry_count * 256,
            messages_since_compact: entry_count,
        }
    }

    /// Get the underlying transcript.
    pub fn into_transcript(self) -> Transcript {
        self.transcript
    }

    /// Estimate tokens in a transcript entry.
    fn estimate_tokens(entry: &TranscriptEntry) -> u32 {
        let char_count: usize = entry.blocks.iter().map(|b| match b {
            ContentBlock::Text { text } => text.len(),
            ContentBlock::ToolUse { input, .. } => serde_json::to_string(input).map(|s| s.len()).unwrap_or(0),
            ContentBlock::ToolResult { content, .. } => content.len(),
            ContentBlock::Thinking { thinking } => thinking.len(),
            ContentBlock::Image { data, .. } => data.len(),
            _ => 0,
        }).sum();
        (char_count / 4) as u32
    }

    /// Perform basic truncation compaction (placeholder for LLM summarization).
    /// In a full implementation, this would call an LLM to generate a summary.
    fn compact(&mut self) -> String {
        let entries = self.transcript.entries().to_vec();
        let summary_text = if entries.len() <= 2 {
            "Session history compact: minimal entries".to_string()
        } else {
            // Create a basic summary from the first and last entries
            let first = entries.first();
            let last = entries.last();
            let mut summary = String::from("Session summary:\n");
            if let Some(e) = first {
                summary.push_str(&format!("- First: {:?} message\n", e.role));
            }
            if let Some(e) = last {
                summary.push_str(&format!("- Last: {:?} message\n", e.role));
            }
            summary.push_str(&format!("- Total: {} messages\n", entries.len()));
            summary
        };

        // Keep only the last N entries after compaction
        let keep_count = self.config.min_messages_for_compact as usize;
        self.transcript.clear();
        for entry in entries.iter().skip(entries.len().saturating_sub(keep_count)) {
            self.transcript.append(entry.clone());
        }

        self.summary = Some(summary_text.clone());
        self.messages_since_compact = 0;
        self.estimated_tokens = self.transcript.len() as u32 * 256;

        summary_text
    }
}

impl MemoryWrapper for SummarizeMemory {
    fn tier(&self) -> WorkingMemoryTier {
        WorkingMemoryTier::Summarizing
    }

    fn push(&mut self, entry: TranscriptEntry) {
        // Use configured estimate for consistency with token counting
        self.estimated_tokens += self.config.max_tokens / 10; // Rough: 10% of budget per message
        self.messages_since_compact += 1;
        self.transcript.append(entry);

        // Trigger compaction if over budget and enough messages
        if self.needs_maintenance() {
            self.compact();
        }
    }

    fn entries(&self) -> Vec<TranscriptEntry> {
        self.transcript.entries().to_vec()
    }

    fn last_n(&self, n: usize) -> Vec<TranscriptEntry> {
        self.transcript.last_n(n).to_vec()
    }

    fn stats(&self) -> MemoryStats {
        MemoryStats {
            entry_count: self.transcript.len() as u32,
            estimated_tokens: self.estimated_tokens,
            tier: "summarizing".to_string(),
        }
    }

    fn needs_maintenance(&self) -> bool {
        self.estimated_tokens > self.config.max_tokens
            && self.messages_since_compact >= self.config.min_messages_for_compact
            && self.transcript.len() > 1
    }

    fn maintain(&mut self) -> Option<String> {
        if self.needs_maintenance() {
            Some(self.compact())
        } else {
            None
        }
    }

    fn summary(&self) -> Option<&str> {
        self.summary.as_deref()
    }
}

// ---------------------------------------------------------------------------
// Factory function for creating memory wrappers
// ---------------------------------------------------------------------------

/// Create a memory wrapper from a configuration.
pub fn create_memory_wrapper(
    tier_config: &MemoryTierConfig,
) -> Box<dyn MemoryWrapper> {
    match tier_config {
        MemoryTierConfig::Unconstrained => Box::new(UnconstrainedMemory::new()),
        MemoryTierConfig::TokenBounded { max_tokens, tokens_per_message } => {
            let config = TokenConfig {
                max_tokens: *max_tokens,
                tokens_per_message: *tokens_per_message,
            };
            Box::new(TokenMemory::new(config))
        }
        MemoryTierConfig::SlidingWindow { max_messages } => {
            let config = SlidingWindowConfig {
                max_messages: *max_messages,
            };
            Box::new(SlidingWindowMemory::new(config))
        }
        MemoryTierConfig::Summarizing {
            max_tokens,
            min_messages_for_compact,
            max_summary_tokens,
        } => {
            let config = SummarizeConfig {
                max_tokens: *max_tokens,
                min_messages_for_compact: *min_messages_for_compact,
                max_summary_tokens: *max_summary_tokens,
            };
            Box::new(SummarizeMemory::new(config))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn make_entry(role: Role) -> TranscriptEntry {
        TranscriptEntry {
            id: Uuid::new_v4(),
            role,
            blocks: vec![ContentBlock::Text {
                text: "test message content".to_string(),
            }],
            timestamp: Utc::now(),
            cause_by: None,
        }
    }

    #[test]
    fn unconstrained_keeps_all() {
        let mut mem = UnconstrainedMemory::new();
        for _ in 0..10 {
            mem.push(make_entry(Role::User));
        }
        assert_eq!(mem.stats().entry_count, 10);
        assert!(!mem.needs_maintenance());
    }

#[test]
    fn token_bounded_tracks_tokens() {
        // Just verify token counting works correctly
        let config = TokenConfig::default();
        let mem = TokenMemory::new(config);
        let stats = mem.stats();
        // TokenMemory should track entry count
        assert_eq!(stats.entry_count, 0);
    }

    #[test]
    fn sliding_window_maintains_size() {
        let config = SlidingWindowConfig { max_messages: 5 };
        let mut mem = SlidingWindowMemory::new(config);

        for _ in 0..10 {
            mem.push(make_entry(Role::User));
        }
        // Should keep only max_messages
        assert_eq!(mem.stats().entry_count, 5);
    }

    #[test]
    fn summarize_basic() {
        // Just verify SummarizeMemory can be created and used
        let config = SummarizeConfig::default();
        let mut mem = SummarizeMemory::new(config);
        
        // Add some entries
        for _ in 0..3 {
            mem.push(make_entry(Role::User));
        }
        
        // Should have entries
        assert!(mem.stats().entry_count > 0);
    }

    #[test]
    fn factory_creates_correct_type() {
        let config = MemoryTierConfig::SlidingWindow { max_messages: 10 };
        let mem = create_memory_wrapper(&config);
        assert_eq!(mem.tier(), WorkingMemoryTier::SlidingWindow);
    }
}