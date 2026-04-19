//! Turn-lifecycle signal emission.
//!
//! Wraps a [`SignalStore`] and applies the delivery-routing rules from
//! `docs/signal-system-design.md`:
//!
//! * `MainSession` — write an inbox row for the dispatching agent.
//! * `ArtifactOnly` / `Silent` — skip the inbox entirely.
//! * `Blocked` and `Review` — always route to HITL regardless of target.
//!
//! The emitter is plumbed into [`crate::default_runtime::DefaultRuntime`] and
//! fires at each turn lifecycle point (start, progress, terminal).

use std::sync::Arc;

use sera_db::signals::SignalStore;
use sera_types::signal::{Signal, SignalTarget};

/// Inbox id used for [`Signal::Blocked`] and [`Signal::Review`]. Per the
/// design doc's invariant, attention-required signals always land here in
/// addition to whatever the configured target asks for.
pub const HITL_AGENT_ID: &str = "sera-hitl";

/// Emits signals generated during a turn, honoring [`SignalTarget`] and the
/// attention-routing invariant.
#[derive(Clone)]
pub struct SignalEmitter {
    store: Arc<dyn SignalStore>,
    to_agent_id: String,
    target: SignalTarget,
}

impl std::fmt::Debug for SignalEmitter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SignalEmitter")
            .field("to_agent_id", &self.to_agent_id)
            .field("target", &self.target)
            .finish()
    }
}

impl SignalEmitter {
    /// Build an emitter that routes signals to `to_agent_id`'s inbox with the
    /// default [`SignalTarget::MainSession`] delivery.
    pub fn new(store: Arc<dyn SignalStore>, to_agent_id: impl Into<String>) -> Self {
        Self {
            store,
            to_agent_id: to_agent_id.into(),
            target: SignalTarget::MainSession,
        }
    }

    /// Override the default delivery target for this emitter.
    pub fn with_target(mut self, target: SignalTarget) -> Self {
        self.target = target;
        self
    }

    /// Target configured for non-attention signals.
    pub fn target(&self) -> SignalTarget {
        self.target
    }

    /// Recipient agent id for non-attention signals.
    pub fn to_agent_id(&self) -> &str {
        &self.to_agent_id
    }

    /// Emit `signal` according to the routing rules. Errors are logged and
    /// swallowed — the runtime must not fail a turn because the inbox write
    /// failed.
    pub async fn emit(&self, signal: &Signal) {
        if signal.is_attention_required() {
            // Invariant: Blocked/Review always reach HITL regardless of target.
            if let Err(e) = self.store.enqueue(HITL_AGENT_ID, signal).await {
                tracing::warn!(
                    signal_kind = signal.kind(),
                    recipient = HITL_AGENT_ID,
                    error = %e,
                    "failed to enqueue HITL signal",
                );
            }
            // Also fan out to the dispatching agent when MainSession is set,
            // so the caller learns its dispatch is parked on a human.
            if self.target.writes_inbox() {
                if let Err(e) = self.store.enqueue(&self.to_agent_id, signal).await {
                    tracing::warn!(
                        signal_kind = signal.kind(),
                        recipient = %self.to_agent_id,
                        error = %e,
                        "failed to enqueue attention signal to dispatcher",
                    );
                }
            }
            return;
        }

        if !self.target.writes_inbox() {
            // ArtifactOnly / Silent: no inbox row per design doc.
            return;
        }

        if let Err(e) = self.store.enqueue(&self.to_agent_id, signal).await {
            tracing::warn!(
                signal_kind = signal.kind(),
                recipient = %self.to_agent_id,
                error = %e,
                "failed to enqueue signal",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use sera_db::signals::SqliteSignalStore;
    use sera_types::capability::AgentCapability;
    use tokio::sync::Mutex as AsyncMutex;

    fn new_store() -> Arc<dyn SignalStore> {
        let conn = Connection::open_in_memory().unwrap();
        SqliteSignalStore::init_schema(&conn).unwrap();
        Arc::new(SqliteSignalStore::new(Arc::new(AsyncMutex::new(conn))))
    }

    #[tokio::test]
    async fn main_session_writes_inbox_for_recipient() {
        let store = new_store();
        let emitter = SignalEmitter::new(Arc::clone(&store), "agent-a");
        let sig = Signal::Done {
            artifact_id: "art".into(),
            summary: "ok".into(),
            duration_ms: 1,
        };
        emitter.emit(&sig).await;
        let pending = store.peek_pending("agent-a").await.unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].signal, sig);
    }

    #[tokio::test]
    async fn artifact_only_target_skips_inbox() {
        let store = new_store();
        let emitter = SignalEmitter::new(Arc::clone(&store), "agent-a")
            .with_target(SignalTarget::ArtifactOnly);
        let sig = Signal::Done {
            artifact_id: "art".into(),
            summary: "ok".into(),
            duration_ms: 1,
        };
        emitter.emit(&sig).await;
        assert!(store.peek_pending("agent-a").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn silent_target_skips_inbox() {
        let store = new_store();
        let emitter = SignalEmitter::new(Arc::clone(&store), "agent-a")
            .with_target(SignalTarget::Silent);
        emitter
            .emit(&Signal::Progress {
                task_id: "t".into(),
                pct: 50,
                note: "".into(),
            })
            .await;
        assert!(store.peek_pending("agent-a").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn blocked_routes_to_hitl_even_when_silent() {
        let store = new_store();
        let emitter = SignalEmitter::new(Arc::clone(&store), "agent-a")
            .with_target(SignalTarget::Silent);
        let sig = Signal::Blocked {
            reason: "missing cap".into(),
            requires: vec![AgentCapability::MetaChange],
        };
        emitter.emit(&sig).await;
        // HITL got the attention-required signal.
        let hitl_pending = store.peek_pending(HITL_AGENT_ID).await.unwrap();
        assert_eq!(hitl_pending.len(), 1);
        assert_eq!(hitl_pending[0].signal, sig);
        // Silent means the dispatcher does NOT also get it.
        assert!(store.peek_pending("agent-a").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn review_routes_to_hitl_and_dispatcher_on_main_session() {
        let store = new_store();
        let emitter = SignalEmitter::new(Arc::clone(&store), "agent-a");
        let sig = Signal::Review {
            artifact_id: "art".into(),
            prompt: "check this".into(),
        };
        emitter.emit(&sig).await;
        assert_eq!(store.peek_pending(HITL_AGENT_ID).await.unwrap().len(), 1);
        assert_eq!(store.peek_pending("agent-a").await.unwrap().len(), 1);
    }
}
