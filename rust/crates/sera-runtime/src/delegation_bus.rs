//! Delegation bus — child-session event routing for inter-agent delegation.
//!
//! The `DelegationBus` is a lightweight in-process pub/sub over child session
//! events. It backs the `session_spawn` / `session_yield` / `session_send`
//! tools (bead sera-a1u): the parent agent spawns a named child, then yields
//! and awaits the child's next [`DelegationEvent`] as a tool result.
//!
//! ## Event types
//!
//! - [`DelegationEvent::MessageEmitted`] — the child produced an intermediate
//!   user-visible message (e.g. `StreamingDelta` coalesced for the parent).
//! - [`DelegationEvent::TurnCompleted`] — the child finished one turn with a
//!   final answer.
//! - [`DelegationEvent::SessionClosed`] — the child session was terminated.
//! - [`DelegationEvent::Error`] — the child produced an error.
//!
//! ## Concurrency model
//!
//! Each `subscribe_next(session_id)` returns a fresh
//! `tokio::sync::oneshot::Receiver`. Multiple concurrent yields on the same
//! session queue as independent subscribers; a `publish` call fires each
//! pending oneshot with a clone of the event in FIFO order.
//!
//! The bus is `Clone + Send + Sync` via an internal `Arc<Mutex<...>>` so it
//! can be shared across tool constructions.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;

/// Events a child session can emit back to its parent.
///
/// All variants carry enough context for the yielding parent to render the
/// tool result without needing a separate follow-up call.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DelegationEvent {
    /// The child emitted an intermediate message (e.g. streaming delta
    /// coalesced, progress note). Carries the full text so far.
    MessageEmitted {
        /// Human-readable message content.
        content: String,
    },
    /// The child completed a turn with a final output. This is the typical
    /// "answer back to the parent" terminal event for a single delegation.
    TurnCompleted {
        /// Final text output for this turn.
        output: String,
    },
    /// The child session was closed (cancelled, completed, archived). After
    /// this event further yields on the same session will time out.
    SessionClosed {
        /// Short reason string (e.g. `"completed"`, `"cancelled"`, `"shutdown"`).
        reason: String,
    },
    /// The child session produced an error.
    Error {
        /// Machine-readable error message.
        message: String,
    },
}

/// Shared delegation bus. Cheap to clone — all clones point at the same
/// underlying subscriber map.
#[derive(Clone, Default, Debug)]
pub struct DelegationBus {
    inner: Arc<Mutex<BusInner>>,
}

#[derive(Default)]
struct BusInner {
    /// Pending subscribers keyed by child session id. Each session may have
    /// multiple concurrent subscribers (one per `session_yield` call); all
    /// fire on the next `publish`.
    subscribers: HashMap<String, Vec<oneshot::Sender<DelegationEvent>>>,
    /// Set of known child session ids — populated by `session_spawn` so the
    /// `session_yield` tool can distinguish "unknown session" from "known but
    /// no event yet".
    known_sessions: HashMap<String, ChildSessionMeta>,
}

impl std::fmt::Debug for BusInner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BusInner")
            .field("subscriber_session_count", &self.subscribers.len())
            .field("known_sessions", &self.known_sessions.len())
            .finish()
    }
}

/// Metadata tracked for each registered child session.
#[derive(Debug, Clone)]
pub struct ChildSessionMeta {
    /// The parent agent id that spawned this child.
    pub parent_agent_id: String,
    /// The child agent template/id (e.g. `researcher`, `coder`).
    pub child_agent_id: String,
    /// Initial prompt the child was spawned with (for audit/debug).
    pub initial_prompt: String,
}

impl DelegationBus {
    /// Create a fresh empty bus.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a newly spawned child session so that later `subscribe_next`
    /// calls on this id return a proper waiting subscriber instead of
    /// `SessionNotFound`.
    pub fn register_session(&self, session_id: impl Into<String>, meta: ChildSessionMeta) {
        let mut inner = self.inner.lock().expect("delegation bus mutex poisoned");
        inner.known_sessions.insert(session_id.into(), meta);
    }

    /// Return `true` if the session id is known to the bus.
    pub fn is_known(&self, session_id: &str) -> bool {
        let inner = self.inner.lock().expect("delegation bus mutex poisoned");
        inner.known_sessions.contains_key(session_id)
    }

    /// Return metadata for a known session, or `None` if unknown.
    pub fn session_meta(&self, session_id: &str) -> Option<ChildSessionMeta> {
        let inner = self.inner.lock().expect("delegation bus mutex poisoned");
        inner.known_sessions.get(session_id).cloned()
    }

    /// Subscribe to the next event on a child session. Returns a oneshot
    /// receiver that fires on the next [`Self::publish`] for that session.
    ///
    /// Returns `Err` if the session id is not known — parents must call
    /// `session_spawn` before yielding.
    pub fn subscribe_next(
        &self,
        session_id: &str,
    ) -> Result<oneshot::Receiver<DelegationEvent>, BusError> {
        let mut inner = self.inner.lock().expect("delegation bus mutex poisoned");
        if !inner.known_sessions.contains_key(session_id) {
            return Err(BusError::SessionNotFound(session_id.to_string()));
        }
        let (tx, rx) = oneshot::channel();
        inner
            .subscribers
            .entry(session_id.to_string())
            .or_default()
            .push(tx);
        Ok(rx)
    }

    /// Publish an event to a child session. All pending subscribers fire
    /// with a clone of the event in FIFO order. Subscribers with dropped
    /// receivers are silently skipped.
    ///
    /// Returns the number of subscribers that received the event.
    pub fn publish(&self, session_id: &str, event: DelegationEvent) -> usize {
        let subs = {
            let mut inner = self.inner.lock().expect("delegation bus mutex poisoned");
            inner.subscribers.remove(session_id).unwrap_or_default()
        };
        let mut delivered = 0usize;
        for tx in subs {
            if tx.send(event.clone()).is_ok() {
                delivered += 1;
            }
        }
        delivered
    }

    /// Remove a child session from the bus (e.g. on graceful shutdown). All
    /// pending subscribers receive a `SessionClosed` event before removal.
    pub fn close_session(&self, session_id: &str, reason: impl Into<String>) {
        let reason = reason.into();
        // Drain subscribers first so closed-session publish logic is
        // self-contained.
        let subs = {
            let mut inner = self.inner.lock().expect("delegation bus mutex poisoned");
            inner.known_sessions.remove(session_id);
            inner.subscribers.remove(session_id).unwrap_or_default()
        };
        for tx in subs {
            let _ = tx.send(DelegationEvent::SessionClosed {
                reason: reason.clone(),
            });
        }
    }

    /// Return the number of pending subscribers for a session (mostly for
    /// tests and observability).
    pub fn subscriber_count(&self, session_id: &str) -> usize {
        let inner = self.inner.lock().expect("delegation bus mutex poisoned");
        inner
            .subscribers
            .get(session_id)
            .map(|v| v.len())
            .unwrap_or(0)
    }
}

/// Errors returned by `DelegationBus` operations.
#[derive(Debug, thiserror::Error)]
pub enum BusError {
    /// The requested session id is not registered with the bus.
    #[error("delegation session not found: {0}")]
    SessionNotFound(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(parent: &str, child: &str) -> ChildSessionMeta {
        ChildSessionMeta {
            parent_agent_id: parent.into(),
            child_agent_id: child.into(),
            initial_prompt: "hello".into(),
        }
    }

    #[tokio::test]
    async fn subscribe_receives_published_event() {
        let bus = DelegationBus::new();
        bus.register_session("s1", meta("parent", "child"));
        let rx = bus.subscribe_next("s1").unwrap();

        assert_eq!(bus.subscriber_count("s1"), 1);

        bus.publish(
            "s1",
            DelegationEvent::TurnCompleted {
                output: "done".into(),
            },
        );
        let ev = rx.await.unwrap();
        assert_eq!(
            ev,
            DelegationEvent::TurnCompleted {
                output: "done".into()
            }
        );
        assert_eq!(bus.subscriber_count("s1"), 0);
    }

    #[tokio::test]
    async fn subscribe_unknown_session_errors() {
        let bus = DelegationBus::new();
        let err = bus.subscribe_next("ghost").unwrap_err();
        assert!(matches!(err, BusError::SessionNotFound(_)));
    }

    #[tokio::test]
    async fn multiple_concurrent_subscribers_each_get_a_copy() {
        let bus = DelegationBus::new();
        bus.register_session("s2", meta("parent", "child"));

        let rx1 = bus.subscribe_next("s2").unwrap();
        let rx2 = bus.subscribe_next("s2").unwrap();
        let rx3 = bus.subscribe_next("s2").unwrap();
        assert_eq!(bus.subscriber_count("s2"), 3);

        let delivered = bus.publish(
            "s2",
            DelegationEvent::MessageEmitted {
                content: "hi".into(),
            },
        );
        assert_eq!(delivered, 3);

        for rx in [rx1, rx2, rx3] {
            let ev = rx.await.unwrap();
            assert_eq!(
                ev,
                DelegationEvent::MessageEmitted {
                    content: "hi".into()
                }
            );
        }
    }

    #[tokio::test]
    async fn publish_without_subscribers_is_noop() {
        let bus = DelegationBus::new();
        bus.register_session("s3", meta("parent", "child"));
        let n = bus.publish(
            "s3",
            DelegationEvent::Error {
                message: "no one listening".into(),
            },
        );
        assert_eq!(n, 0);
    }

    #[tokio::test]
    async fn close_session_delivers_closed_event() {
        let bus = DelegationBus::new();
        bus.register_session("s4", meta("parent", "child"));
        let rx = bus.subscribe_next("s4").unwrap();
        bus.close_session("s4", "completed");
        let ev = rx.await.unwrap();
        match ev {
            DelegationEvent::SessionClosed { reason } => assert_eq!(reason, "completed"),
            other => panic!("unexpected event: {other:?}"),
        }
        // After close, session is no longer known.
        assert!(!bus.is_known("s4"));
    }

    #[tokio::test]
    async fn dropped_subscriber_is_skipped() {
        let bus = DelegationBus::new();
        bus.register_session("s5", meta("parent", "child"));
        let rx1 = bus.subscribe_next("s5").unwrap();
        let rx2 = bus.subscribe_next("s5").unwrap();
        drop(rx1); // simulate the yield-side dropping the receiver
        let delivered = bus.publish(
            "s5",
            DelegationEvent::TurnCompleted {
                output: "x".into(),
            },
        );
        // Only rx2 should have received.
        assert_eq!(delivered, 1);
        let ev = rx2.await.unwrap();
        assert!(matches!(ev, DelegationEvent::TurnCompleted { .. }));
    }

    #[tokio::test]
    async fn event_serde_roundtrip() {
        let cases = vec![
            DelegationEvent::MessageEmitted {
                content: "x".into(),
            },
            DelegationEvent::TurnCompleted {
                output: "y".into(),
            },
            DelegationEvent::SessionClosed {
                reason: "done".into(),
            },
            DelegationEvent::Error {
                message: "boom".into(),
            },
        ];
        for ev in cases {
            let json = serde_json::to_string(&ev).unwrap();
            let back: DelegationEvent = serde_json::from_str(&json).unwrap();
            assert_eq!(back, ev);
        }
    }

    #[tokio::test]
    async fn session_meta_roundtrip() {
        let bus = DelegationBus::new();
        bus.register_session(
            "s6",
            ChildSessionMeta {
                parent_agent_id: "p".into(),
                child_agent_id: "researcher".into(),
                initial_prompt: "find X".into(),
            },
        );
        let got = bus.session_meta("s6").unwrap();
        assert_eq!(got.parent_agent_id, "p");
        assert_eq!(got.child_agent_id, "researcher");
        assert_eq!(got.initial_prompt, "find X");
        assert!(bus.is_known("s6"));
        assert!(!bus.is_known("s-missing"));
    }
}
