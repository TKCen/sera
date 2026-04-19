//! End-to-end integration test for turn-lifecycle signal emission.
//!
//! Closes the emission gap left by PR #947: the `Signal` / `SignalTarget` /
//! `Dispatch` types and the `agent_signals` SQLite table existed, but nothing
//! in `sera-runtime` ever enqueued a signal during a turn. This test runs a
//! mock turn with the signal emitter wired in and asserts that `Started`
//! and `Done` both land in the dispatching agent's inbox when the target is
//! `SignalTarget::MainSession`.

use std::sync::Arc;

use rusqlite::Connection;
use sera_db::signals::{SignalStore, SqliteSignalStore};
use sera_runtime::context_engine::pipeline::ContextPipeline;
use sera_runtime::default_runtime::DefaultRuntime;
use sera_runtime::signal_emit::{SignalEmitter, HITL_AGENT_ID};
use sera_types::runtime::{AgentRuntime, TurnContext, TurnOutcome};
use sera_types::signal::{Signal, SignalTarget};
use tokio::sync::Mutex as AsyncMutex;

fn make_store() -> Arc<dyn SignalStore> {
    let conn = Connection::open_in_memory().unwrap();
    SqliteSignalStore::init_schema(&conn).unwrap();
    Arc::new(SqliteSignalStore::new(Arc::new(AsyncMutex::new(conn))))
}

fn make_turn_context(agent_id: &str) -> TurnContext {
    TurnContext {
        event_id: "evt-signal-emit".into(),
        agent_id: agent_id.into(),
        session_key: format!("session:{agent_id}:u1"),
        messages: vec![serde_json::json!({
            "role": "user",
            "content": "emit lifecycle signals for this turn",
        })],
        available_tools: vec![],
        metadata: std::collections::HashMap::new(),
        change_artifact: None,
        parent_session_key: None,
        tool_use_behavior: Default::default(),
    }
}

/// Full end-to-end: a mock turn with no tool calls (think stub returns a
/// final response) must emit `Started` at the top of the turn and `Done`
/// on success, both landing in the dispatching agent's inbox under
/// `SignalTarget::MainSession`.
#[tokio::test]
async fn started_and_done_land_in_inbox_with_main_session_target() {
    let store = make_store();
    let dispatcher_id = "agent-dispatcher";

    let emitter = SignalEmitter::new(Arc::clone(&store), dispatcher_id)
        .with_target(SignalTarget::MainSession);

    let runtime = DefaultRuntime::new(Box::new(ContextPipeline::new()))
        .with_signal_emitter(emitter);

    let outcome = runtime
        .execute_turn(make_turn_context("agent-worker"))
        .await
        .expect("turn must succeed");

    // The think stub produces a FinalOutput; that path emits Signal::Done.
    assert!(
        matches!(outcome, TurnOutcome::FinalOutput { .. }),
        "expected FinalOutput, got {outcome:?}"
    );

    let inbox = store.peek_pending(dispatcher_id).await.unwrap();
    assert_eq!(inbox.len(), 2, "expected Started + Done, got {inbox:?}");

    // Ordering in the inbox falls back to UUID tiebreak when both rows share
    // the same whole-second `created_at`, so assert by kind rather than
    // position. Both must be present.
    let started = inbox
        .iter()
        .find_map(|row| match &row.signal {
            Signal::Started { description, .. } => Some(description.clone()),
            _ => None,
        })
        .expect("Started signal must be in the inbox");
    assert!(
        started.contains("emit lifecycle signals"),
        "Started description should carry the last user message, got {started:?}"
    );

    let summary = inbox
        .iter()
        .find_map(|row| match &row.signal {
            Signal::Done { summary, .. } => Some(summary.clone()),
            _ => None,
        })
        .expect("Done signal must be in the inbox");
    assert!(!summary.is_empty(), "Done summary should carry the final response");

    // HITL inbox stays empty — neither signal is attention-required.
    let hitl = store.peek_pending(HITL_AGENT_ID).await.unwrap();
    assert!(
        hitl.is_empty(),
        "HITL inbox must stay empty for non-attention signals, got {hitl:?}"
    );
}

/// `SignalTarget::Silent` must NOT write an inbox row for non-attention
/// signals, per the design doc's routing rules.
#[tokio::test]
async fn silent_target_skips_inbox_entirely() {
    let store = make_store();
    let emitter = SignalEmitter::new(Arc::clone(&store), "agent-dispatcher")
        .with_target(SignalTarget::Silent);

    let runtime = DefaultRuntime::new(Box::new(ContextPipeline::new()))
        .with_signal_emitter(emitter);

    let _ = runtime
        .execute_turn(make_turn_context("agent-worker"))
        .await
        .expect("turn must succeed");

    assert!(
        store.peek_pending("agent-dispatcher").await.unwrap().is_empty(),
        "Silent target must skip inbox writes"
    );
    assert!(
        store.peek_pending(HITL_AGENT_ID).await.unwrap().is_empty(),
        "non-attention signals must not reach HITL"
    );
}
