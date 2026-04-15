//! SERA Session — session state machine and transcript management.
//!
//! Provides the SessionState finite state machine (6 states) and
//! ContentBlock-based transcript storage.
//!
//! Also provides the four-tier working memory wrapper types:
//! - UnconstrainedMemory: Tier 1 — no limit, keeps full history
//! - TokenMemory: Tier 2 — evicts oldest when token budget exceeded
//! - SlidingWindowMemory: Tier 3 — fixed message-count sliding window
//! - SummarizeMemory: Tier 4 — LLM-driven compaction when budget hit

pub mod memory_wrapper;
pub mod state;
pub mod transcript;
