//! Circle coordination — runtime state for active circles.
//!
//! CircleState tracks agent membership, shared memory (KV store),
//! and cross-agent message routing for circles that are actively
//! participating in a session.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::RwLock;
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

/// Runtime state for an active circle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CircleState {
    pub circle_id: String,
    /// Agent IDs currently participating in this circle.
    pub active_agents: HashSet<String>,
    /// Circle-scoped KV store shared across agents.
    pub shared_memory: SharedMemory,
    /// Pending cross-agent messages.
    pub message_queue: Vec<CircleMessage>,
    pub created_at: DateTime<Utc>,
}

impl CircleState {
    fn new(circle_id: String, agents: Vec<String>) -> Self {
        Self {
            circle_id,
            active_agents: agents.into_iter().collect(),
            shared_memory: SharedMemory::default(),
            message_queue: Vec::new(),
            created_at: Utc::now(),
        }
    }
}

/// Simple typed KV store for circle-scoped state.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SharedMemory {
    pub entries: HashMap<String, MemoryEntry>,
}

impl SharedMemory {
    /// Get a memory entry by key.
    pub fn get(&self, key: &str) -> Option<&MemoryEntry> {
        self.entries.get(key)
    }

    /// Set a memory entry.
    pub fn set(&mut self, key: String, value: serde_json::Value, written_by: String) {
        self.entries.insert(key, MemoryEntry {
            value,
            written_by,
            written_at: Utc::now(),
        });
    }

    /// Delete a memory entry. Returns true if the key existed.
    pub fn delete(&mut self, key: &str) -> bool {
        self.entries.remove(key).is_some()
    }

    /// List all keys in the store.
    pub fn list_keys(&self) -> Vec<&str> {
        self.entries.keys().map(String::as_str).collect()
    }
}

/// A single entry in the shared memory KV store.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub value: serde_json::Value,
    /// ID of the agent that wrote this entry.
    pub written_by: String,
    pub written_at: DateTime<Utc>,
}

/// Cross-agent message within a circle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CircleMessage {
    /// Unique message ID (UUID v4).
    pub id: String,
    pub from_agent: String,
    /// Target agent ID. `None` means broadcast to all circle members.
    pub to_agent: Option<String>,
    pub content: serde_json::Value,
    pub sent_at: DateTime<Utc>,
}

impl CircleMessage {
    /// Create a new message with a generated UUID.
    pub fn new(
        from_agent: String,
        to_agent: Option<String>,
        content: serde_json::Value,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            from_agent,
            to_agent,
            content,
            sent_at: Utc::now(),
        }
    }
}

/// Manages runtime state for all active circles.
#[derive(Debug, Clone)]
pub struct CircleCoordinator {
    /// Active circle states keyed by circle_id.
    states: Arc<RwLock<HashMap<String, CircleState>>>,
}

impl CircleCoordinator {
    /// Create a new, empty CircleCoordinator.
    pub fn new() -> Self {
        Self {
            states: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Activate a circle with an initial set of agents.
    ///
    /// If the circle is already active its state is replaced.
    pub async fn activate_circle(&self, circle_id: String, agents: Vec<String>) -> CircleState {
        let state = CircleState::new(circle_id.clone(), agents);
        self.states.write().await.insert(circle_id, state.clone());
        state
    }

    /// Get a clone of the current state for a circle.
    pub async fn get_state(&self, circle_id: &str) -> Option<CircleState> {
        self.states.read().await.get(circle_id).cloned()
    }

    /// Add an agent to an active circle. No-op if the circle does not exist.
    pub async fn join_agent(&self, circle_id: &str, agent_id: String) {
        if let Some(state) = self.states.write().await.get_mut(circle_id) {
            state.active_agents.insert(agent_id);
        }
    }

    /// Remove an agent from an active circle. No-op if the circle or agent does not exist.
    pub async fn leave_agent(&self, circle_id: &str, agent_id: &str) {
        if let Some(state) = self.states.write().await.get_mut(circle_id) {
            state.active_agents.remove(agent_id);
        }
    }

    /// Enqueue a message into a circle's message queue.
    ///
    /// No-op if the circle does not exist.
    pub async fn send_message(&self, circle_id: &str, msg: CircleMessage) {
        if let Some(state) = self.states.write().await.get_mut(circle_id) {
            state.message_queue.push(msg);
        }
    }

    /// Return all messages addressed to `agent_id` (broadcast + directed).
    ///
    /// Messages are not consumed — callers are responsible for tracking
    /// which messages they have already processed if needed.
    pub async fn read_messages(&self, circle_id: &str, agent_id: &str) -> Vec<CircleMessage> {
        let guard = self.states.read().await;
        let Some(state) = guard.get(circle_id) else {
            return Vec::new();
        };
        state
            .message_queue
            .iter()
            .filter(|msg| {
                msg.to_agent.is_none()
                    || msg.to_agent.as_deref() == Some(agent_id)
            })
            .cloned()
            .collect()
    }

    /// Write a value into the shared memory of a circle.
    ///
    /// No-op if the circle does not exist.
    pub async fn write_memory(
        &self,
        circle_id: &str,
        key: String,
        value: serde_json::Value,
        agent_id: String,
    ) {
        if let Some(state) = self.states.write().await.get_mut(circle_id) {
            state.shared_memory.set(key, value, agent_id);
        }
    }

    /// Read a single memory entry from a circle's shared memory.
    pub async fn read_memory(&self, circle_id: &str, key: &str) -> Option<MemoryEntry> {
        self.states
            .read()
            .await
            .get(circle_id)?
            .shared_memory
            .get(key)
            .cloned()
    }
}

impl Default for CircleCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn coordinator() -> CircleCoordinator {
        CircleCoordinator::new()
    }

    #[tokio::test]
    async fn test_activate_circle() {
        let coord = coordinator();
        let state = coord
            .activate_circle("c1".into(), vec!["agent-a".into(), "agent-b".into()])
            .await;

        assert_eq!(state.circle_id, "c1");
        assert!(state.active_agents.contains("agent-a"));
        assert!(state.active_agents.contains("agent-b"));
        assert!(state.message_queue.is_empty());

        // get_state returns the same data
        let fetched = coord.get_state("c1").await.expect("state missing");
        assert_eq!(fetched.circle_id, "c1");
    }

    #[tokio::test]
    async fn test_get_state_missing_circle() {
        let coord = coordinator();
        assert!(coord.get_state("nonexistent").await.is_none());
    }

    #[tokio::test]
    async fn test_join_and_leave_agent() {
        let coord = coordinator();
        coord.activate_circle("c1".into(), vec![]).await;

        coord.join_agent("c1", "agent-a".into()).await;
        coord.join_agent("c1", "agent-b".into()).await;

        let state = coord.get_state("c1").await.unwrap();
        assert!(state.active_agents.contains("agent-a"));
        assert!(state.active_agents.contains("agent-b"));

        coord.leave_agent("c1", "agent-a").await;

        let state = coord.get_state("c1").await.unwrap();
        assert!(!state.active_agents.contains("agent-a"));
        assert!(state.active_agents.contains("agent-b"));
    }

    #[tokio::test]
    async fn test_join_leave_noop_on_missing_circle() {
        let coord = coordinator();
        // Neither should panic
        coord.join_agent("nope", "agent-a".into()).await;
        coord.leave_agent("nope", "agent-a").await;
    }

    #[tokio::test]
    async fn test_send_and_read_messages_broadcast() {
        let coord = coordinator();
        coord
            .activate_circle("c1".into(), vec!["agent-a".into(), "agent-b".into()])
            .await;

        let msg = CircleMessage::new("agent-a".into(), None, json!("hello everyone"));
        coord.send_message("c1", msg).await;

        // Both agents should see the broadcast
        let for_a = coord.read_messages("c1", "agent-a").await;
        let for_b = coord.read_messages("c1", "agent-b").await;

        assert_eq!(for_a.len(), 1);
        assert_eq!(for_b.len(), 1);
        assert_eq!(for_a[0].content, json!("hello everyone"));
    }

    #[tokio::test]
    async fn test_send_and_read_messages_directed() {
        let coord = coordinator();
        coord
            .activate_circle("c1".into(), vec!["agent-a".into(), "agent-b".into()])
            .await;

        let msg = CircleMessage::new("agent-a".into(), Some("agent-b".into()), json!({"key": "val"}));
        coord.send_message("c1", msg).await;

        // Only agent-b sees it
        let for_a = coord.read_messages("c1", "agent-a").await;
        let for_b = coord.read_messages("c1", "agent-b").await;

        assert_eq!(for_a.len(), 0);
        assert_eq!(for_b.len(), 1);
        assert_eq!(for_b[0].from_agent, "agent-a");
    }

    #[tokio::test]
    async fn test_read_messages_missing_circle() {
        let coord = coordinator();
        let msgs = coord.read_messages("nope", "agent-a").await;
        assert!(msgs.is_empty());
    }

    #[tokio::test]
    async fn test_write_and_read_shared_memory() {
        let coord = coordinator();
        coord.activate_circle("c1".into(), vec![]).await;

        coord
            .write_memory("c1", "config".into(), json!({"timeout": 30}), "agent-a".into())
            .await;

        let entry = coord.read_memory("c1", "config").await.expect("entry missing");
        assert_eq!(entry.value, json!({"timeout": 30}));
        assert_eq!(entry.written_by, "agent-a");
    }

    #[tokio::test]
    async fn test_read_memory_missing_key() {
        let coord = coordinator();
        coord.activate_circle("c1".into(), vec![]).await;
        assert!(coord.read_memory("c1", "no-such-key").await.is_none());
    }

    #[tokio::test]
    async fn test_read_memory_missing_circle() {
        let coord = coordinator();
        assert!(coord.read_memory("nope", "key").await.is_none());
    }

    #[tokio::test]
    async fn test_shared_memory_overwrite() {
        let coord = coordinator();
        coord.activate_circle("c1".into(), vec![]).await;

        coord
            .write_memory("c1", "k".into(), json!(1), "agent-a".into())
            .await;
        coord
            .write_memory("c1", "k".into(), json!(2), "agent-b".into())
            .await;

        let entry = coord.read_memory("c1", "k").await.unwrap();
        assert_eq!(entry.value, json!(2));
        assert_eq!(entry.written_by, "agent-b");
    }

    #[tokio::test]
    async fn test_shared_memory_delete() {
        let mut mem = SharedMemory::default();
        mem.set("key".into(), json!("val"), "agent".into());
        assert!(mem.get("key").is_some());

        let deleted = mem.delete("key");
        assert!(deleted);
        assert!(mem.get("key").is_none());

        // Deleting again returns false
        assert!(!mem.delete("key"));
    }

    #[tokio::test]
    async fn test_shared_memory_list_keys() {
        let mut mem = SharedMemory::default();
        mem.set("a".into(), json!(1), "ag".into());
        mem.set("b".into(), json!(2), "ag".into());

        let mut keys = mem.list_keys();
        keys.sort_unstable();
        assert_eq!(keys, vec!["a", "b"]);
    }

    #[test]
    fn test_circle_message_new_generates_id() {
        let m1 = CircleMessage::new("a".into(), None, json!(null));
        let m2 = CircleMessage::new("a".into(), None, json!(null));
        assert_ne!(m1.id, m2.id);
    }
}
