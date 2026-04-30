//! sera-ve9x: per-agent runtime backend abstraction.
//!
//! Today the gateway dispatches every turn through a `sera-runtime --ndjson`
//! child process supervised by `RuntimeChildSupervisor` (see `bin/sera.rs`).
//! This trait factors that single backend out of the gateway's call sites
//! (`execute_turn`, `execute_steer`, `probe_runtime_ready`) so PR 2 can
//! drop in an in-process `EmbeddedRuntimeTransport` without forking the
//! boot loop. PR 1 introduces the trait and routes the existing supervisor
//! through it; behaviour is unchanged. See
//! `docs/plan/decisions/2026-04-29-dispatch-ownership.md` and
//! `artifacts/reports/research/ve9x-embedded-dispatch-implementation-plan-2026-04-30.md`.
//!
//! Cancellation and per-turn timeout still live at the gateway call site —
//! `tokio::select!` on the cancellation token + `tokio::time::timeout` —
//! and the trait methods themselves are non-cancellable in the contract
//! sense. The implementor is responsible only for the backend round-trip.

use async_trait::async_trait;
use serde::Serialize;
use serde_json::Value;

/// A tool call event captured from the runtime's NDJSON output (today's
/// stdio backend) or projected from the runtime transcript (future
/// embedded backend). Consumed by the gateway's audit / persistence path
/// (`enforce_tool_events`, `persist_tool_events`).
#[derive(Debug, Clone)]
pub enum ToolEvent {
    Begin {
        call_id: String,
        tool: String,
        arguments: Value,
    },
    End {
        call_id: String,
        content: String,
    },
}

/// Provider-reported token usage extracted from the terminal
/// `TurnCompleted` frame.
#[derive(Serialize, Debug, Clone, Copy, Default)]
pub struct UsageInfo {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}

/// Result from a single turn dispatched through an [`AgentTurnTransport`].
#[derive(Debug, Default)]
pub struct TurnEvents {
    pub response: String,
    pub tool_events: Vec<ToolEvent>,
    pub usage: UsageInfo,
}

/// Abstraction over the per-agent runtime backend.
///
/// Implementors own whatever IPC, child supervision, or in-process state
/// the backend needs. Cancellation and per-turn timeouts stay at the
/// gateway call site (`execute_turn`, `execute_steer`).
#[async_trait]
pub trait AgentTurnTransport: Send + Sync {
    /// Dispatch a single user turn. Returns the assembled response text,
    /// any tool events observed, and the provider-reported usage.
    async fn send_turn(
        &self,
        messages: Vec<Value>,
        session_key: &str,
    ) -> anyhow::Result<TurnEvents>;

    /// Inject a steer (mid-turn user content) at the next tool boundary.
    /// The stdio backend writes a `Steer` Submission down the NDJSON pipe
    /// and drains until `turn_completed`; the future embedded backend
    /// stages the items for the next `send_turn`.
    async fn send_steer(
        &self,
        items: Vec<Value>,
        session_key: &str,
    ) -> anyhow::Result<()>;

    /// Best-effort graceful shutdown. Called from the drain phase after
    /// SIGTERM/Ctrl+C; any I/O failure is logged upstream rather than
    /// stalling the drain.
    async fn shutdown(&self) -> anyhow::Result<()>;

    /// End-to-end probe used by `/api/health/ready`. Must succeed only
    /// when the backend is reachable AND its underlying LLM provider
    /// answered a trivial turn (today's stdio path: a non-empty
    /// streaming reply on a `__sera_readiness_probe__` session).
    async fn liveness_probe(&self) -> anyhow::Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    struct DummyTransport;

    #[async_trait]
    impl AgentTurnTransport for DummyTransport {
        async fn send_turn(
            &self,
            _messages: Vec<Value>,
            _session_key: &str,
        ) -> anyhow::Result<TurnEvents> {
            Ok(TurnEvents::default())
        }
        async fn send_steer(
            &self,
            _items: Vec<Value>,
            _session_key: &str,
        ) -> anyhow::Result<()> {
            Ok(())
        }
        async fn shutdown(&self) -> anyhow::Result<()> {
            Ok(())
        }
        async fn liveness_probe(&self) -> anyhow::Result<()> {
            Ok(())
        }
    }

    /// Compile-only check that `AgentTurnTransport` is object-safe — the
    /// gateway stores it as `Arc<dyn AgentTurnTransport>` in `AppState`,
    /// so any non-object-safe addition would be caught here at build time.
    #[test]
    fn trait_is_object_safe() {
        let _: Arc<dyn AgentTurnTransport> = Arc::new(DummyTransport);
    }
}
