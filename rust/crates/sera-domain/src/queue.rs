//! Lane-aware FIFO queue types — per SPEC-gateway §5.
//!
//! Defines the `QueueBackend` trait and supporting types for the gateway's
//! single-writer-per-session queue. See SPEC-gateway §5.1–5.4 for full semantics.

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::event::Event;

// ── QueueMode ────────────────────────────────────────────────────────────────

/// Controls what happens when a new message arrives for a session that is
/// currently mid-turn. Configurable per-agent and overridable per-session.
///
/// SPEC-gateway §5.2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum QueueMode {
    /// Coalesce all queued messages into one follow-up turn.
    Collect,
    /// Process each queued message as a sequential follow-up turn (one per message).
    #[default]
    Followup,
    /// Inject the message at the next tool boundary, skipping remaining tool calls.
    Steer,
    /// Steer now AND preserve the message for a follow-up turn after the run completes.
    SteerBacklog,
    /// Abort the active run immediately. Legacy — not recommended for production.
    Interrupt,
}

// ── QueueError ───────────────────────────────────────────────────────────────

/// Errors returned by `QueueBackend` operations.
#[derive(Debug, Error)]
pub enum QueueError {
    /// The queue has reached its maximum capacity.
    #[error("queue is full")]
    Full,
    /// The requested lane does not exist.
    #[error("lane not found: {0}")]
    LaneNotFound(String),
    /// A backend-specific error occurred.
    #[error("backend error: {0}")]
    BackendError(String),
}

// ── QueuedEvent ──────────────────────────────────────────────────────────────

/// An event waiting in a lane queue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueuedEvent {
    /// The event payload.
    pub event: Event,
    /// The session key (lane identifier), e.g. `"agent:sera:main"`.
    pub lane: String,
    /// When this event was enqueued.
    pub enqueued_at: DateTime<Utc>,
    /// The queue mode to apply when processing this event.
    pub mode: QueueMode,
}

impl QueuedEvent {
    /// Construct a new `QueuedEvent` with the current timestamp.
    pub fn new(event: Event, lane: impl Into<String>, mode: QueueMode) -> Self {
        Self {
            lane: lane.into(),
            enqueued_at: Utc::now(),
            mode,
            event,
        }
    }
}

// ── QueueConfig ──────────────────────────────────────────────────────────────

/// Gateway queue configuration. SPEC-gateway §5.4.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueConfig {
    /// Maximum number of sessions that can be actively processing at once.
    pub max_concurrent_runs: u32,
    /// Default queue mode applied when no per-agent/session override is set.
    pub default_mode: QueueMode,
    /// Maximum number of events allowed per lane (`None` = unlimited).
    pub max_lane_depth: Option<u32>,
}

impl Default for QueueConfig {
    fn default() -> Self {
        Self {
            max_concurrent_runs: 10,
            default_mode: QueueMode::Followup,
            max_lane_depth: None,
        }
    }
}

// ── QueueBackend trait ───────────────────────────────────────────────────────

/// Pluggable storage backend for the lane-aware FIFO queue.
///
/// SPEC-gateway §5.3: Tier 1 uses SQLite, Tier 2 uses PostgreSQL. This trait
/// abstracts the backend so the gateway is not coupled to a specific store.
#[async_trait]
pub trait QueueBackend: Send + Sync {
    /// Enqueue an event into its lane. Returns `QueueError::Full` if
    /// `max_lane_depth` would be exceeded.
    async fn enqueue(&self, event: QueuedEvent) -> Result<(), QueueError>;

    /// Dequeue and remove the front event from `lane`. Returns `None` if the
    /// lane is empty.
    async fn dequeue(&self, lane: &str) -> Result<Option<QueuedEvent>, QueueError>;

    /// Inspect the front event in `lane` without removing it. Returns `None`
    /// if the lane is empty.
    async fn peek(&self, lane: &str) -> Result<Option<QueuedEvent>, QueueError>;

    /// Return the number of events currently waiting in `lane`.
    async fn lane_depth(&self, lane: &str) -> Result<usize, QueueError>;

    /// Return the total number of lanes that currently have at least one event.
    async fn active_lanes(&self) -> Result<usize, QueueError>;
}

// ── InMemoryQueueBackend ─────────────────────────────────────────────────────

/// HashMap-backed in-memory queue backend. Suitable for testing and local
/// single-process use. Not durable across restarts.
pub struct InMemoryQueueBackend {
    lanes: Mutex<HashMap<String, VecDeque<QueuedEvent>>>,
    config: QueueConfig,
}

impl InMemoryQueueBackend {
    /// Create a new in-memory backend with the supplied configuration.
    pub fn new(config: QueueConfig) -> Self {
        Self {
            lanes: Mutex::new(HashMap::new()),
            config,
        }
    }

    /// Create a backend with default configuration (no depth limit).
    pub fn with_defaults() -> Self {
        Self::new(QueueConfig::default())
    }
}

#[async_trait]
impl QueueBackend for InMemoryQueueBackend {
    async fn enqueue(&self, event: QueuedEvent) -> Result<(), QueueError> {
        let mut lanes = self
            .lanes
            .lock()
            .map_err(|e| QueueError::BackendError(e.to_string()))?;

        let lane = lanes.entry(event.lane.clone()).or_default();

        if let Some(max) = self.config.max_lane_depth
            && lane.len() >= max as usize
        {
            return Err(QueueError::Full);
        }

        lane.push_back(event);
        Ok(())
    }

    async fn dequeue(&self, lane: &str) -> Result<Option<QueuedEvent>, QueueError> {
        let mut lanes = self
            .lanes
            .lock()
            .map_err(|e| QueueError::BackendError(e.to_string()))?;

        let event = lanes.get_mut(lane).and_then(VecDeque::pop_front);

        // Remove empty lanes to keep active_lanes() accurate.
        if lanes.get(lane).is_some_and(VecDeque::is_empty) {
            lanes.remove(lane);
        }

        Ok(event)
    }

    async fn peek(&self, lane: &str) -> Result<Option<QueuedEvent>, QueueError> {
        let lanes = self
            .lanes
            .lock()
            .map_err(|e| QueueError::BackendError(e.to_string()))?;

        Ok(lanes.get(lane).and_then(|q| q.front()).cloned())
    }

    async fn lane_depth(&self, lane: &str) -> Result<usize, QueueError> {
        let lanes = self
            .lanes
            .lock()
            .map_err(|e| QueueError::BackendError(e.to_string()))?;

        Ok(lanes.get(lane).map_or(0, VecDeque::len))
    }

    async fn active_lanes(&self) -> Result<usize, QueueError> {
        let lanes = self
            .lanes
            .lock()
            .map_err(|e| QueueError::BackendError(e.to_string()))?;

        Ok(lanes.values().filter(|q| !q.is_empty()).count())
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::Event;
    use crate::principal::{PrincipalId, PrincipalKind, PrincipalRef};

    fn make_principal() -> PrincipalRef {
        PrincipalRef {
            id: PrincipalId::new("test-user"),
            kind: PrincipalKind::Human,
        }
    }

    fn make_event(agent_id: &str, session_key: &str) -> Event {
        Event::message(agent_id, session_key, make_principal(), "hello")
    }

    fn make_queued(lane: &str) -> QueuedEvent {
        QueuedEvent::new(make_event("sera", lane), lane, QueueMode::Followup)
    }

    // ── FIFO ordering ────────────────────────────────────────────────────────

    #[tokio::test]
    async fn enqueue_dequeue_fifo_order() {
        let backend = InMemoryQueueBackend::with_defaults();
        let lane = "agent:sera:main";

        let e1 = QueuedEvent::new(make_event("sera", lane), lane, QueueMode::Followup);
        let e2 = QueuedEvent::new(make_event("sera", lane), lane, QueueMode::Followup);
        let id1 = e1.event.id.clone();
        let id2 = e2.event.id.clone();

        backend.enqueue(e1).await.unwrap();
        backend.enqueue(e2).await.unwrap();

        let first = backend.dequeue(lane).await.unwrap().unwrap();
        let second = backend.dequeue(lane).await.unwrap().unwrap();

        assert_eq!(first.event.id, id1, "first dequeued should be first enqueued");
        assert_eq!(second.event.id, id2, "second dequeued should be second enqueued");

        // Lane is now empty.
        assert!(backend.dequeue(lane).await.unwrap().is_none());
    }

    // ── Lane isolation ───────────────────────────────────────────────────────

    #[tokio::test]
    async fn lane_isolation() {
        let backend = InMemoryQueueBackend::with_defaults();
        let lane_a = "agent:sera:main";
        let lane_b = "agent:reviewer:main";

        let ea = make_queued(lane_a);
        let eb = make_queued(lane_b);
        let id_a = ea.event.id.clone();
        let id_b = eb.event.id.clone();

        backend.enqueue(ea).await.unwrap();
        backend.enqueue(eb).await.unwrap();

        // Dequeuing lane_a should not affect lane_b.
        let got_a = backend.dequeue(lane_a).await.unwrap().unwrap();
        assert_eq!(got_a.event.id, id_a);

        let got_b = backend.dequeue(lane_b).await.unwrap().unwrap();
        assert_eq!(got_b.event.id, id_b);
    }

    // ── Peek doesn't consume ─────────────────────────────────────────────────

    #[tokio::test]
    async fn peek_does_not_consume() {
        let backend = InMemoryQueueBackend::with_defaults();
        let lane = "agent:sera:main";

        let ev = make_queued(lane);
        let id = ev.event.id.clone();

        backend.enqueue(ev).await.unwrap();

        // Peek twice — should return the same event each time.
        let p1 = backend.peek(lane).await.unwrap().unwrap();
        let p2 = backend.peek(lane).await.unwrap().unwrap();
        assert_eq!(p1.event.id, id);
        assert_eq!(p2.event.id, id);

        // Dequeue should still return it.
        let d = backend.dequeue(lane).await.unwrap().unwrap();
        assert_eq!(d.event.id, id);

        // Now empty.
        assert!(backend.peek(lane).await.unwrap().is_none());
    }

    // ── Lane depth ───────────────────────────────────────────────────────────

    #[tokio::test]
    async fn lane_depth_counting() {
        let backend = InMemoryQueueBackend::with_defaults();
        let lane = "agent:sera:main";

        assert_eq!(backend.lane_depth(lane).await.unwrap(), 0);

        backend.enqueue(make_queued(lane)).await.unwrap();
        assert_eq!(backend.lane_depth(lane).await.unwrap(), 1);

        backend.enqueue(make_queued(lane)).await.unwrap();
        assert_eq!(backend.lane_depth(lane).await.unwrap(), 2);

        backend.dequeue(lane).await.unwrap();
        assert_eq!(backend.lane_depth(lane).await.unwrap(), 1);

        backend.dequeue(lane).await.unwrap();
        assert_eq!(backend.lane_depth(lane).await.unwrap(), 0);
    }

    // ── Active lanes ─────────────────────────────────────────────────────────

    #[tokio::test]
    async fn active_lanes_count() {
        let backend = InMemoryQueueBackend::with_defaults();

        assert_eq!(backend.active_lanes().await.unwrap(), 0);

        backend.enqueue(make_queued("lane:1")).await.unwrap();
        assert_eq!(backend.active_lanes().await.unwrap(), 1);

        backend.enqueue(make_queued("lane:2")).await.unwrap();
        assert_eq!(backend.active_lanes().await.unwrap(), 2);

        // Draining lane:1 should reduce active lanes.
        backend.dequeue("lane:1").await.unwrap();
        assert_eq!(backend.active_lanes().await.unwrap(), 1);
    }

    // ── Max lane depth ───────────────────────────────────────────────────────

    #[tokio::test]
    async fn max_lane_depth_enforced() {
        let config = QueueConfig {
            max_lane_depth: Some(2),
            ..Default::default()
        };
        let backend = InMemoryQueueBackend::new(config);
        let lane = "agent:sera:main";

        backend.enqueue(make_queued(lane)).await.unwrap();
        backend.enqueue(make_queued(lane)).await.unwrap();

        // Third enqueue should fail with Full.
        let err = backend.enqueue(make_queued(lane)).await.unwrap_err();
        assert!(matches!(err, QueueError::Full));
    }

    // ── QueueMode serde roundtrip ────────────────────────────────────────────

    #[test]
    fn queue_mode_serde_roundtrip() {
        let modes = [
            (QueueMode::Collect, "collect"),
            (QueueMode::Followup, "followup"),
            (QueueMode::Steer, "steer"),
            (QueueMode::SteerBacklog, "steer-backlog"),
            (QueueMode::Interrupt, "interrupt"),
        ];

        for (mode, expected) in modes {
            let json = serde_json::to_string(&mode).unwrap();
            assert_eq!(json, format!("\"{expected}\""), "serialized form mismatch for {mode:?}");
            let parsed: QueueMode = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, mode, "deserialized form mismatch for {mode:?}");
        }
    }

    // ── Empty peek on unknown lane ───────────────────────────────────────────

    #[tokio::test]
    async fn peek_unknown_lane_returns_none() {
        let backend = InMemoryQueueBackend::with_defaults();
        assert!(backend.peek("no-such-lane").await.unwrap().is_none());
    }
}
