use std::collections::{HashMap, VecDeque};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Controls how an event is placed into the lane queue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum QueueMode {
    /// Normal FIFO accumulation.
    Collect,
    /// Follow-up message — appended to the back of the lane (FIFO).
    Followup,
    /// Steering override — replaces any pending steer item.
    Steer,
    /// Steering override that also keeps a backlog copy.
    SteerBacklog,
    /// Highest-priority interrupt — clears the lane backlog immediately.
    Interrupt,
}

/// A single event that has been placed in a lane.
#[derive(Debug, Clone)]
pub struct QueuedEvent {
    pub id: String,
    pub lane: String,
    pub payload: serde_json::Value,
    pub enqueued_at: DateTime<Utc>,
    pub mode: QueueMode,
}

/// Result returned from a successful enqueue operation.
#[derive(Debug, Clone)]
pub struct EnqueueResult {
    pub id: String,
    pub depth: usize,
}

/// Per-lane in-memory queue with steering and interrupt semantics.
#[derive(Default)]
pub struct LaneQueue {
    lanes: HashMap<String, VecDeque<QueuedEvent>>,
    /// The current steer item per lane (newest wins).
    steer: HashMap<String, QueuedEvent>,
}

impl LaneQueue {
    pub fn new() -> Self {
        Self::default()
    }

    /// Enqueue an event into the named lane according to `mode`.
    pub fn enqueue(
        &mut self,
        lane: impl Into<String>,
        payload: serde_json::Value,
        mode: QueueMode,
    ) -> EnqueueResult {
        let lane = lane.into();
        let id = uuid::Uuid::new_v4().to_string();
        let event = QueuedEvent {
            id: id.clone(),
            lane: lane.clone(),
            payload,
            enqueued_at: Utc::now(),
            mode: mode.clone(),
        };

        match mode {
            QueueMode::Interrupt => {
                // Clear the backlog and place the interrupt as the only item.
                let queue = self.lanes.entry(lane.clone()).or_default();
                queue.clear();
                self.steer.remove(&lane);
                queue.push_back(event);
            }
            QueueMode::Steer | QueueMode::SteerBacklog => {
                // Newest steer wins — replace any existing steer entry.
                self.steer.insert(lane.clone(), event.clone());
                if mode == QueueMode::SteerBacklog {
                    self.lanes.entry(lane.clone()).or_default().push_back(event);
                }
            }
            QueueMode::Collect | QueueMode::Followup => {
                self.lanes.entry(lane.clone()).or_default().push_back(event);
            }
        }

        let depth = self.depth(&lane);
        EnqueueResult { id, depth }
    }

    /// Dequeue the next event from the front of the lane (FIFO).
    pub fn dequeue(&mut self, lane: &str) -> Option<QueuedEvent> {
        self.lanes.get_mut(lane)?.pop_front()
    }

    /// Return the current steer item for the lane (newest wins), if any.
    pub fn take_steer(&mut self, lane: &str) -> Option<QueuedEvent> {
        self.steer.remove(lane)
    }

    /// Clear all pending events in the lane and return how many were removed.
    pub fn interrupt_clear(&mut self, lane: &str) -> usize {
        let queue_removed = self
            .lanes
            .get_mut(lane)
            .map(|q| {
                let n = q.len();
                q.clear();
                n
            })
            .unwrap_or(0);
        let steer_removed = if self.steer.remove(lane).is_some() {
            1
        } else {
            0
        };
        queue_removed + steer_removed
    }

    /// Current number of items queued in the lane (steer slot not counted).
    pub fn depth(&self, lane: &str) -> usize {
        self.lanes.get(lane).map(|q| q.len()).unwrap_or(0)
    }

    /// Called after a lane's run completes — hook point for throttle integration.
    pub fn complete_run(&mut self, _lane: &str) {
        // Intentionally left as a hook; throttle permit release is handled by the caller.
    }
}
