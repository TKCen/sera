//! Circle activity integration for sera-runtime.
//!
//! Provides [`SharedCircleActivityLog`] — a thread-safe wrapper around
//! [`InMemoryCircleActivityLog`] that can be stored in runtime state and
//! accessed from the turn loop and prompt assembly path.

use std::sync::{Arc, Mutex};

use sera_types::circle_activity::{CircleActivityEntry, InMemoryCircleActivityLog, CIRCLE_ACTIVITY_DEFAULT_LIMIT};

/// Thread-safe, cheaply-cloneable handle to the in-memory circle activity log.
#[derive(Debug, Clone, Default)]
pub struct SharedCircleActivityLog {
    inner: Arc<Mutex<InMemoryCircleActivityLog>>,
}

impl SharedCircleActivityLog {
    /// Create a new, empty shared log.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a peer activity event.
    pub fn record(&self, agent_id: impl Into<String>, circle_id: impl Into<String>, summary: impl Into<String>) {
        if let Ok(mut log) = self.inner.lock() {
            log.record(agent_id, circle_id, summary);
        }
    }

    /// Return up to `limit` most-recent peer entries for `circle_id`, excluding `exclude_agent`.
    /// Uses [`CIRCLE_ACTIVITY_DEFAULT_LIMIT`] when `limit` is `None`.
    pub fn recent_for_circle(
        &self,
        circle_id: &str,
        exclude_agent: &str,
        limit: Option<usize>,
    ) -> Vec<CircleActivityEntry> {
        let l = limit.unwrap_or(CIRCLE_ACTIVITY_DEFAULT_LIMIT);
        self.inner
            .lock()
            .map(|log| log.recent_for_circle(circle_id, exclude_agent, l))
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_log_record_and_retrieve() {
        let log = SharedCircleActivityLog::new();
        log.record("agent-x", "circle-a", "did something");

        let entries = log.recent_for_circle("circle-a", "other", None);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].agent_id, "agent-x");
        assert_eq!(entries[0].summary, "did something");
    }

    #[test]
    fn shared_log_clone_shares_state() {
        let log = SharedCircleActivityLog::new();
        let log2 = log.clone();
        log.record("agent-a", "circle-b", "event from original");

        let entries = log2.recent_for_circle("circle-b", "nobody", None);
        assert_eq!(entries.len(), 1, "clone should see entries added via original");
    }

    #[test]
    fn shared_log_excludes_own_agent() {
        let log = SharedCircleActivityLog::new();
        log.record("self", "circle-c", "my own action");
        log.record("peer", "circle-c", "peer action");

        let entries = log.recent_for_circle("circle-c", "self", None);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].agent_id, "peer");
    }

    #[test]
    fn shared_log_custom_limit() {
        let log = SharedCircleActivityLog::new();
        for i in 0u64..5 {
            // Use the inner log directly via a block to set explicit timestamps.
            log.inner.lock().unwrap().record_at("agent-a", "circle-d", format!("e{i}"), i);
        }

        let entries = log.recent_for_circle("circle-d", "nobody", Some(2));
        assert_eq!(entries.len(), 2);
        // Newest first: timestamps 4, 3
        assert_eq!(entries[0].timestamp, 4);
        assert_eq!(entries[1].timestamp, 3);
    }
}
