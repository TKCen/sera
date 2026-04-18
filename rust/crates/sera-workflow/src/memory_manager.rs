//! `WorkflowMemoryManager` — coordinator-scoped step summary store.
//!
//! Tracks per-agent [`StepSummary`] records keyed by workflow instance.
//! The coordinator calls [`WorkflowMemoryManager::record_agent_step`] after
//! each agent turn, queries [`WorkflowMemoryManager::snapshot`] to get an
//! aggregated view for routing decisions, and calls
//! [`WorkflowMemoryManager::evict`] on workflow completion to free memory.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

// =========================================================================
// Public types
// =========================================================================

/// Opaque identifier for a workflow instance (e.g. a Circle run).
pub type InstanceId = String;

/// Opaque identifier for a participating agent.
pub type AgentId = String;

/// A single agent step recorded by the coordinator.
#[derive(Debug, Clone)]
pub struct StepSummary {
    pub agent_id: AgentId,
    pub outcome: String,
    pub tokens_used: u32,
    pub key_observations: Vec<String>,
    pub timestamp: SystemTime,
}

impl StepSummary {
    pub fn new(
        agent_id: impl Into<AgentId>,
        outcome: impl Into<String>,
        tokens_used: u32,
        key_observations: Vec<String>,
    ) -> Self {
        Self {
            agent_id: agent_id.into(),
            outcome: outcome.into(),
            tokens_used,
            key_observations,
            timestamp: SystemTime::now(),
        }
    }
}

/// Aggregated coordinator view for a single workflow instance.
#[derive(Debug, Clone)]
pub struct WorkflowMemorySnapshot {
    /// Steps grouped by agent.
    pub per_agent: HashMap<AgentId, Vec<StepSummary>>,
    /// Total tokens consumed across all agents in this instance.
    pub total_tokens: u64,
    /// When the first step for this instance was recorded.
    pub started_at: SystemTime,
}

// =========================================================================
// MemoryManager trait
// =========================================================================

/// Coordinator-scoped memory interface — swappable for tests.
pub trait MemoryManager: Send + Sync {
    fn record_agent_step(&self, instance_id: &str, step: StepSummary);
    fn snapshot(&self, instance_id: &str) -> WorkflowMemorySnapshot;
    fn evict(&self, instance_id: &str);
}

// =========================================================================
// Inner per-instance bucket
// =========================================================================

#[derive(Debug, Default)]
struct InstanceBucket {
    /// Steps keyed by agent_id.
    steps: HashMap<AgentId, Vec<StepSummary>>,
    started_at: Option<SystemTime>,
}

impl InstanceBucket {
    fn push(&mut self, step: StepSummary) {
        if self.started_at.is_none() {
            self.started_at = Some(step.timestamp);
        }
        self.steps
            .entry(step.agent_id.clone())
            .or_default()
            .push(step);
    }

    fn to_snapshot(&self) -> WorkflowMemorySnapshot {
        let total_tokens = self
            .steps
            .values()
            .flat_map(|v| v.iter())
            .map(|s| s.tokens_used as u64)
            .sum();
        WorkflowMemorySnapshot {
            per_agent: self.steps.clone(),
            total_tokens,
            started_at: self.started_at.unwrap_or(SystemTime::UNIX_EPOCH),
        }
    }
}

// =========================================================================
// WorkflowMemoryManager — the canonical impl
// =========================================================================

/// Coordinator-scoped memory store for Circle workflows.
///
/// Thread-safe; cheaply cloneable via inner `Arc`.
#[derive(Clone, Default)]
pub struct WorkflowMemoryManager {
    coordinator_scoped_summary: Arc<Mutex<HashMap<InstanceId, InstanceBucket>>>,
}

impl WorkflowMemoryManager {
    pub fn new() -> Self {
        Self::default()
    }
}

impl MemoryManager for WorkflowMemoryManager {
    /// Append a [`StepSummary`] to the given workflow instance's bucket.
    fn record_agent_step(&self, instance_id: &str, step: StepSummary) {
        self.coordinator_scoped_summary
            .lock()
            .expect("memory lock poisoned")
            .entry(instance_id.to_owned())
            .or_default()
            .push(step);
    }

    /// Return the coordinator's aggregated view for `instance_id`.
    /// Returns an empty snapshot when the instance has no recorded steps.
    fn snapshot(&self, instance_id: &str) -> WorkflowMemorySnapshot {
        self.coordinator_scoped_summary
            .lock()
            .expect("memory lock poisoned")
            .get(instance_id)
            .map(|b| b.to_snapshot())
            .unwrap_or(WorkflowMemorySnapshot {
                per_agent: HashMap::new(),
                total_tokens: 0,
                started_at: SystemTime::UNIX_EPOCH,
            })
    }

    /// Remove all data for `instance_id` (call on workflow completion).
    fn evict(&self, instance_id: &str) {
        self.coordinator_scoped_summary
            .lock()
            .expect("memory lock poisoned")
            .remove(instance_id);
    }
}

// =========================================================================
// Tests
// =========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn step(agent: &str, outcome: &str, tokens: u32) -> StepSummary {
        StepSummary::new(agent, outcome, tokens, vec!["obs".to_string()])
    }

    #[test]
    fn record_agent_step_appends_to_instance_bucket() {
        let mgr = WorkflowMemoryManager::new();
        mgr.record_agent_step("inst-1", step("agent-a", "ok", 10));
        mgr.record_agent_step("inst-1", step("agent-a", "ok", 20));
        let snap = mgr.snapshot("inst-1");
        let agent_steps = snap.per_agent.get("agent-a").expect("agent-a missing");
        assert_eq!(agent_steps.len(), 2);
        assert_eq!(agent_steps[0].tokens_used, 10);
        assert_eq!(agent_steps[1].tokens_used, 20);
    }

    #[test]
    fn snapshot_aggregates_per_agent() {
        let mgr = WorkflowMemoryManager::new();
        mgr.record_agent_step("inst-2", step("agent-a", "ok", 100));
        mgr.record_agent_step("inst-2", step("agent-b", "ok", 200));
        mgr.record_agent_step("inst-2", step("agent-b", "ok", 50));
        let snap = mgr.snapshot("inst-2");
        assert_eq!(snap.per_agent.len(), 2);
        assert_eq!(snap.total_tokens, 350);
        assert_eq!(snap.per_agent["agent-b"].len(), 2);
    }

    #[test]
    fn evict_removes_instance() {
        let mgr = WorkflowMemoryManager::new();
        mgr.record_agent_step("inst-3", step("agent-a", "ok", 5));
        mgr.evict("inst-3");
        let snap = mgr.snapshot("inst-3");
        assert!(snap.per_agent.is_empty());
        assert_eq!(snap.total_tokens, 0);
    }

    #[test]
    fn snapshot_empty_instance_returns_empty_map() {
        let mgr = WorkflowMemoryManager::new();
        let snap = mgr.snapshot("nonexistent-instance");
        assert!(snap.per_agent.is_empty());
        assert_eq!(snap.total_tokens, 0);
        assert_eq!(snap.started_at, SystemTime::UNIX_EPOCH);
    }

    #[tokio::test]
    async fn concurrent_record_from_multiple_tasks_safe() {
        use std::sync::Arc;
        let mgr = Arc::new(WorkflowMemoryManager::new());
        let mut handles = Vec::new();
        for i in 0u32..8 {
            let m = mgr.clone();
            handles.push(tokio::spawn(async move {
                for j in 0u32..10 {
                    m.record_agent_step(
                        "shared-inst",
                        StepSummary::new(
                            format!("agent-{i}"),
                            "ok",
                            j,
                            vec![],
                        ),
                    );
                }
            }));
        }
        for h in handles {
            h.await.expect("task panicked");
        }
        let snap = mgr.snapshot("shared-inst");
        // 8 agents * 10 steps each
        let total_steps: usize = snap.per_agent.values().map(|v| v.len()).sum();
        assert_eq!(total_steps, 80);
        assert_eq!(snap.per_agent.len(), 8);
    }
}
