//! Circle activity log — lightweight in-memory peer activity tracking.
//!
//! Agents within a circle can record brief summaries of their actions.
//! When a system prompt is assembled, up to `CIRCLE_ACTIVITY_LIMIT` peer
//! entries are injected as a `## Circle Activity` section so each agent
//! has situational awareness of what its circle-mates are doing.

use serde::{Deserialize, Serialize};
use std::time::SystemTime;

/// Maximum entries returned by [`InMemoryCircleActivityLog::recent_for_circle`]
/// when the caller does not specify a limit.
pub const CIRCLE_ACTIVITY_DEFAULT_LIMIT: usize = 10;

/// A single recorded activity entry from one agent in a circle.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CircleActivityEntry {
    /// The agent that produced this entry.
    pub agent_id: String,
    /// The circle this agent belongs to.
    pub circle_id: String,
    /// A brief human-readable summary of what the agent did.
    pub summary: String,
    /// Unix timestamp (seconds) when the entry was recorded.
    pub timestamp: u64,
}

impl CircleActivityEntry {
    /// Create a new entry using the current wall-clock time.
    pub fn new(
        agent_id: impl Into<String>,
        circle_id: impl Into<String>,
        summary: impl Into<String>,
    ) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        Self {
            agent_id: agent_id.into(),
            circle_id: circle_id.into(),
            summary: summary.into(),
            timestamp,
        }
    }

    /// Create a new entry with an explicit timestamp (useful for tests).
    pub fn with_timestamp(
        agent_id: impl Into<String>,
        circle_id: impl Into<String>,
        summary: impl Into<String>,
        timestamp: u64,
    ) -> Self {
        Self {
            agent_id: agent_id.into(),
            circle_id: circle_id.into(),
            summary: summary.into(),
            timestamp,
        }
    }

    /// Format the entry as a prompt bullet:
    /// `- [agent-id @ ISO-like-timestamp] summary`
    pub fn format_for_prompt(&self) -> String {
        format!(
            "- [{} @ {}] {}",
            self.agent_id, self.timestamp, self.summary
        )
    }
}

/// In-memory circle activity log.
///
/// Thread-safety is NOT provided here — callers must wrap in `Arc<Mutex<_>>`
/// when sharing across async tasks.
///
/// Entries are stored in insertion order; `recent_for_circle` returns the
/// most-recent N entries in descending timestamp order (newest first).
#[derive(Debug, Default, Clone)]
pub struct InMemoryCircleActivityLog {
    entries: Vec<CircleActivityEntry>,
}

impl InMemoryCircleActivityLog {
    /// Create an empty log.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a new entry.
    pub fn record(
        &mut self,
        agent_id: impl Into<String>,
        circle_id: impl Into<String>,
        summary: impl Into<String>,
    ) {
        self.entries
            .push(CircleActivityEntry::new(agent_id, circle_id, summary));
    }

    /// Record an entry with an explicit timestamp (useful for deterministic tests).
    pub fn record_at(
        &mut self,
        agent_id: impl Into<String>,
        circle_id: impl Into<String>,
        summary: impl Into<String>,
        timestamp: u64,
    ) {
        self.entries.push(CircleActivityEntry::with_timestamp(
            agent_id, circle_id, summary, timestamp,
        ));
    }

    /// Return up to `limit` most-recent entries for `circle_id`, excluding `exclude_agent`.
    ///
    /// Results are sorted newest-first by timestamp.
    pub fn recent_for_circle(
        &self,
        circle_id: &str,
        exclude_agent: &str,
        limit: usize,
    ) -> Vec<CircleActivityEntry> {
        let mut matching: Vec<&CircleActivityEntry> = self
            .entries
            .iter()
            .filter(|e| e.circle_id == circle_id && e.agent_id != exclude_agent)
            .collect();

        // Sort newest-first.
        matching.sort_by_key(|e| std::cmp::Reverse(e.timestamp));
        matching.into_iter().take(limit).cloned().collect()
    }

    /// Total number of entries (across all circles).
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns true when no entries are recorded.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_stores_entry_and_recent_returns_it() {
        let mut log = InMemoryCircleActivityLog::new();
        log.record("agent-a", "circle-1", "called tool X");

        let results = log.recent_for_circle("circle-1", "other-agent", 10);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].agent_id, "agent-a");
        assert_eq!(results[0].circle_id, "circle-1");
        assert_eq!(results[0].summary, "called tool X");
    }

    #[test]
    fn own_agent_entries_excluded() {
        let mut log = InMemoryCircleActivityLog::new();
        log.record_at("agent-self", "circle-1", "my action", 100);
        log.record_at("agent-peer", "circle-1", "peer action", 101);

        let results = log.recent_for_circle("circle-1", "agent-self", 10);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].agent_id, "agent-peer");
    }

    #[test]
    fn limit_honored_newest_first() {
        let mut log = InMemoryCircleActivityLog::new();
        for i in 0u64..8 {
            log.record_at("agent-a", "circle-1", format!("action-{i}"), i);
        }

        let results = log.recent_for_circle("circle-1", "other", 3);
        assert_eq!(results.len(), 3);
        // Newest first: timestamps 7, 6, 5
        assert_eq!(results[0].timestamp, 7);
        assert_eq!(results[1].timestamp, 6);
        assert_eq!(results[2].timestamp, 5);
    }

    #[test]
    fn empty_circle_returns_empty_vec() {
        let log = InMemoryCircleActivityLog::new();
        let results = log.recent_for_circle("circle-nonexistent", "any-agent", 10);
        assert!(results.is_empty());
    }

    #[test]
    fn entries_from_different_circles_not_mixed() {
        let mut log = InMemoryCircleActivityLog::new();
        log.record_at("agent-a", "circle-1", "c1 action", 1);
        log.record_at("agent-b", "circle-2", "c2 action", 2);

        let c1 = log.recent_for_circle("circle-1", "nobody", 10);
        assert_eq!(c1.len(), 1);
        assert_eq!(c1[0].circle_id, "circle-1");

        let c2 = log.recent_for_circle("circle-2", "nobody", 10);
        assert_eq!(c2.len(), 1);
        assert_eq!(c2[0].circle_id, "circle-2");
    }

    #[test]
    fn format_for_prompt_contains_agent_and_summary() {
        let entry = CircleActivityEntry::with_timestamp("bot-1", "circle-x", "ran grep", 42);
        let formatted = entry.format_for_prompt();
        assert!(formatted.contains("bot-1"));
        assert!(formatted.contains("42"));
        assert!(formatted.contains("ran grep"));
    }
}
