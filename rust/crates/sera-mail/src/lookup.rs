//! [`sera_workflow::MailLookup`] implementation backed by correlator output.
//!
//! `InMemoryMailLookup` is the bridge between the ingress correlator and the
//! workflow scheduler. The correlator calls [`InMemoryMailLookup::notify`]
//! (via the [`crate::correlator::NotifySink`] trait) when a reply resolves;
//! the scheduler polls via [`sera_workflow::MailLookup::thread_event`] at its
//! own cadence.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use sera_workflow::task::{MailEvent, MailThreadId};
use sera_workflow::MailLookup;

use crate::correlator::NotifySink;
use crate::envelope::GateId;
use crate::error::MailCorrelationError;

/// One recorded event on a mail thread.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadEventRecord {
    /// Monotonic sequence number — starts at 1, increments per event on this
    /// thread. Tests can assert ordering via this field.
    pub seq: u64,
    /// Which gate the event applies to (same gate may raise multiple events,
    /// e.g. an initial ReplyReceived followed later by a Closed).
    pub gate_id: GateId,
    /// Scheduler-visible event state.
    pub event: MailEvent,
}

/// In-memory implementation of [`MailLookup`].
///
/// The scheduler needs synchronous lookups so we materialise a
/// `HashMap<MailThreadId, latest MailEvent>` plus, per thread, the full event
/// timeline to support the `after: seq` filter used by replay / audit paths.
///
/// Locking discipline: all mutations take an exclusive lock for the minimal
/// time needed to mutate the maps. Reads (`thread_event`) share the same
/// `Mutex`, which is fine because events arrive at human-interaction cadence
/// (email reply latency ≫ lock contention).
#[derive(Debug, Default)]
pub struct InMemoryMailLookup {
    inner: Mutex<LookupInner>,
}

#[derive(Debug, Default)]
struct LookupInner {
    /// Most-recent event state per thread.
    latest: HashMap<String, MailEvent>,
    /// Full timeline per thread. Used by [`InMemoryMailLookup::thread_event`]
    /// which takes an `after` seq filter.
    timeline: HashMap<String, Vec<ThreadEventRecord>>,
    /// Monotonic counter shared across threads — each new event gets
    /// `next_seq` and then we increment. Makes per-thread timelines globally
    /// orderable.
    next_seq: u64,
}

impl InMemoryMailLookup {
    /// Construct an empty lookup.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a new event for `thread_id` originating from `gate_id`.
    ///
    /// This is what the correlator calls when it resolves an inbound reply,
    /// and what administrative paths call when they close a thread without a
    /// reply (e.g. HITL timeout).
    pub fn notify(
        &self,
        gate_id: GateId,
        thread_id: &MailThreadId,
        event: MailEvent,
    ) -> Result<(), MailCorrelationError> {
        let mut inner = self.inner.lock().map_err(|_| MailCorrelationError::IndexPoisoned)?;
        inner.next_seq += 1;
        let seq = inner.next_seq;
        let key = thread_id.as_str().to_string();
        inner.latest.insert(key.clone(), event.clone());
        inner
            .timeline
            .entry(key)
            .or_default()
            .push(ThreadEventRecord { seq, gate_id, event });
        Ok(())
    }

    /// Return every event recorded for `thread_id` with `seq > after`,
    /// in-order.
    ///
    /// `after = 0` returns the full timeline. Callers that only care about
    /// the latest event should use [`MailLookup::thread_event`] instead.
    pub fn events_after(&self, thread_id: &MailThreadId, after: u64) -> Vec<ThreadEventRecord> {
        let inner = match self.inner.lock() {
            Ok(i) => i,
            Err(_) => return Vec::new(),
        };
        inner
            .timeline
            .get(thread_id.as_str())
            .map(|events| events.iter().filter(|e| e.seq > after).cloned().collect())
            .unwrap_or_default()
    }

    /// Number of recorded threads. Useful for tests / metrics.
    pub fn thread_count(&self) -> usize {
        let inner = match self.inner.lock() {
            Ok(i) => i,
            Err(_) => return 0,
        };
        inner.latest.len()
    }
}

impl MailLookup for InMemoryMailLookup {
    fn thread_event(&self, id: &MailThreadId) -> Option<MailEvent> {
        let inner = self.inner.lock().ok()?;
        inner.latest.get(id.as_str()).cloned()
    }
}

/// Adapter so `Arc<InMemoryMailLookup>` can be used as a
/// [`NotifySink`] directly. This lets the correlator push events into the
/// lookup without an intermediate wrapper.
#[async_trait]
impl NotifySink for InMemoryMailLookup {
    async fn on_resolved(
        &self,
        gate_id: &GateId,
        thread_id: &MailThreadId,
        event: MailEvent,
    ) -> Result<(), MailCorrelationError> {
        self.notify(gate_id.clone(), thread_id, event)
    }
}

// Allow `Arc<InMemoryMailLookup>` to act as the sink by borrowing through.
#[async_trait]
impl NotifySink for Arc<InMemoryMailLookup> {
    async fn on_resolved(
        &self,
        gate_id: &GateId,
        thread_id: &MailThreadId,
        event: MailEvent,
    ) -> Result<(), MailCorrelationError> {
        (**self).on_resolved(gate_id, thread_id, event).await
    }
}
