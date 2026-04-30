//! sera-ve9x: in-process [`AgentTurnTransport`] backed by a
//! [`sera_runtime::default_runtime::DefaultRuntime`].
//!
//! Selected by `SERA_DISPATCH_MODE=embedded`. Replaces the stdio child path
//! ([`crate::agent_transport::AgentTurnTransport`] backed by
//! `RuntimeChildSupervisor`) for the gateway-bundled runtime — there is no
//! `sera-runtime --ndjson` child process when this transport is in use.
//!
//! The default mode (`SERA_DISPATCH_MODE=runtime`) is unchanged.
//!
//! # Steer semantics
//!
//! Today's stdio backend writes a `Steer` Submission down the NDJSON pipe
//! and the runtime injects it at the next tool boundary. The embedded
//! backend stages incoming steer items in a `Mutex<Vec<Value>>` and drains
//! them into the next `send_turn`'s `TurnContext.metadata["pending_steer"]`.
//! Acceptable for the MVS — the runtime's `turn::TurnContext.pending_steer`
//! field is already populated from the same metadata key by
//! `DefaultRuntime::execute_turn`.
//!
//! # Audit / capability shape
//!
//! Tool-call audit shape (`[sera-policy] tool '…' denied …`) is produced by
//! `RegistryDispatcher` regardless of transport, so
//! [`crate::capability_enforcement`] does not need a parallel branch — the
//! transcript projected here matches what stdio surfaces.
//!
//! # Hot reload caveat
//!
//! `CapabilityRegistry` is captured by reference in the bundled
//! `RegistryDispatcher` at boot. The current stdio backend has the same
//! property (the runtime child loads the registry once on spawn), so this
//! is parity, not a regression. A future bead can add a
//! `CapabilityRegistryHandle` + per-turn lookup so `/admin/policies/reload`
//! propagates to embedded transports without a process restart.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::Mutex;

use sera_runtime::default_runtime::DefaultRuntime;
use sera_types::runtime::{AgentRuntime, TurnContext, TurnOutcome};
use sera_types::tool::ToolDefinition;

use crate::agent_transport::{AgentTurnTransport, ToolEvent, TurnEvents, UsageInfo};

/// In-process transport that calls
/// [`DefaultRuntime::execute_turn`](sera_types::runtime::AgentRuntime::execute_turn)
/// directly — no child process, no NDJSON pipe.
pub struct EmbeddedRuntimeTransport {
    agent_id: String,
    runtime: Arc<DefaultRuntime>,
    tool_defs: Vec<ToolDefinition>,
    /// Steer items staged by [`AgentTurnTransport::send_steer`] and drained
    /// into the next [`AgentTurnTransport::send_turn`] via
    /// `TurnContext.metadata["pending_steer"]`. The runtime's
    /// `turn::TurnContext.pending_steer` reads this metadata key in
    /// `DefaultRuntime::execute_turn`, mirroring the stdio backend's
    /// `Steer` Submission semantics.
    ///
    /// Keyed by `session_key` so concurrent sessions on the same agent
    /// (and the readiness probe's synthetic session) keep their steer
    /// queues isolated. The stdio backend gets this scoping for free
    /// because each `Steer` Submission carries `session_key` and the
    /// runtime routes by it; embedded mode replicates that contract here.
    pending_steer: Mutex<HashMap<String, Vec<Value>>>,
}

impl EmbeddedRuntimeTransport {
    /// Construct a new embedded transport. `runtime` is shared so multiple
    /// agents *could* share an `Arc<DefaultRuntime>`, but the standard
    /// boot path constructs one runtime per agent (mirroring today's
    /// per-agent stdio child).
    pub fn new(
        agent_id: impl Into<String>,
        runtime: Arc<DefaultRuntime>,
        tool_defs: Vec<ToolDefinition>,
    ) -> Self {
        Self {
            agent_id: agent_id.into(),
            runtime,
            tool_defs,
            pending_steer: Mutex::new(HashMap::new()),
        }
    }

    /// Test introspection: number of tool definitions exposed to the LLM.
    pub fn tool_def_count(&self) -> usize {
        self.tool_defs.len()
    }
}

#[async_trait]
impl AgentTurnTransport for EmbeddedRuntimeTransport {
    async fn send_turn(
        &self,
        messages: Vec<Value>,
        session_key: &str,
    ) -> anyhow::Result<TurnEvents> {
        // Drain any staged steer items for *this* session into the turn
        // metadata. The runtime's `DefaultRuntime::execute_turn` (and
        // `turn::TurnContext.pending_steer`) looks for them under
        // `metadata["pending_steer"]` in the same shape an NDJSON `Steer`
        // submission would surface.
        //
        // Keying on `session_key` matters: concurrent sessions on the same
        // agent — including the readiness probe's
        // `__sera_readiness_probe__` session running between a real user's
        // steer and their next turn — must not cross-pollinate steer
        // payloads. The stdio backend gets this for free via per-Submission
        // `session_key` routing inside the runtime.
        let mut metadata: HashMap<String, Value> = HashMap::new();
        {
            let mut pending = self.pending_steer.lock().await;
            if let Some(drained) = pending.remove(session_key)
                && !drained.is_empty()
            {
                metadata.insert("pending_steer".to_string(), Value::Array(drained));
            }
        }

        let ctx = TurnContext {
            event_id: uuid::Uuid::new_v4().to_string(),
            agent_id: self.agent_id.clone(),
            session_key: session_key.to_string(),
            messages,
            available_tools: self.tool_defs.clone(),
            metadata,
            change_artifact: None,
            parent_session_key: None,
            tool_use_behavior: Default::default(),
        };

        let outcome = self
            .runtime
            .execute_turn(ctx)
            .await
            .map_err(|e| anyhow::anyhow!("embedded runtime: {e}"))?;

        Ok(turn_events_from_outcome(outcome))
    }

    async fn send_steer(
        &self,
        items: Vec<Value>,
        session_key: &str,
    ) -> anyhow::Result<()> {
        if items.is_empty() {
            return Ok(());
        }
        let mut pending = self.pending_steer.lock().await;
        pending
            .entry(session_key.to_string())
            .or_default()
            .extend(items);
        Ok(())
    }

    async fn shutdown(&self) -> anyhow::Result<()> {
        // Nothing to clean up — `Arc<DefaultRuntime>` drops with the
        // transport. There is no child process and no open IPC handle.
        Ok(())
    }

    /// Liveness probe semantics: round-trip a trivial turn through the
    /// in-process runtime, mirroring the stdio backend
    /// (`RuntimeChildSupervisor::liveness_probe`). A non-empty streaming
    /// reply proves both the runtime and its LLM provider are reachable.
    async fn liveness_probe(&self) -> anyhow::Result<()> {
        let messages = vec![serde_json::json!({"role": "user", "content": "ping"})];
        let events = self
            .send_turn(messages, "__sera_readiness_probe__")
            .await?;
        if events.response.trim().is_empty() {
            anyhow::bail!("liveness probe returned empty response");
        }
        Ok(())
    }
}

/// Project a runtime [`TurnOutcome`] into the gateway's [`TurnEvents`].
///
/// The resulting tool events match what the stdio backend yields when it
/// reads `tool_call_begin` / `tool_call_end` frames off the NDJSON pipe —
/// the runtime emits both shapes from the same transcript. The gateway's
/// audit / persistence path (`enforce_tool_events`) is byte-stable across
/// transports.
fn turn_events_from_outcome(outcome: TurnOutcome) -> TurnEvents {
    match outcome {
        TurnOutcome::FinalOutput {
            response,
            tokens_used,
            transcript,
            ..
        } => TurnEvents {
            response,
            tool_events: project_tool_events(&transcript),
            usage: usage_from_tokens(&tokens_used),
        },
        TurnOutcome::RunAgain { tokens_used, .. } => TurnEvents {
            response: "[run_again — tool calls dispatched]".to_string(),
            tool_events: vec![],
            usage: usage_from_tokens(&tokens_used),
        },
        TurnOutcome::Handoff {
            target_agent_id,
            tokens_used,
            ..
        } => TurnEvents {
            response: format!("[handoff -> {target_agent_id}]"),
            tool_events: vec![],
            usage: usage_from_tokens(&tokens_used),
        },
        TurnOutcome::Compact { tokens_used, .. } => TurnEvents {
            response: "[compact — context condensed]".to_string(),
            tool_events: vec![],
            usage: usage_from_tokens(&tokens_used),
        },
        TurnOutcome::Interruption { reason, .. } => TurnEvents {
            response: format!("[interrupted: {reason}]"),
            tool_events: vec![],
            usage: UsageInfo::default(),
        },
        TurnOutcome::Stop {
            summary,
            tokens_used,
            ..
        } => TurnEvents {
            response: format!("[stop: {summary}]"),
            tool_events: vec![],
            usage: usage_from_tokens(&tokens_used),
        },
        TurnOutcome::WaitingForApproval {
            ticket_id,
            tokens_used,
            ..
        } => TurnEvents {
            response: format!("[waiting_for_approval: ticket={ticket_id}]"),
            tool_events: vec![],
            usage: usage_from_tokens(&tokens_used),
        },
        TurnOutcome::PlanEmitted {
            plan_tool_calls,
            rationale,
            tokens_used,
            ..
        } => TurnEvents {
            response: format!(
                "[plan_emitted: {} tool call(s); rationale={rationale:?}]",
                plan_tool_calls.len()
            ),
            tool_events: vec![],
            usage: usage_from_tokens(&tokens_used),
        },
    }
}

/// Reconstruct `Begin` / `End` tool events from the runtime transcript
/// (assistant `tool_calls` arrays + `tool` result messages). Mirrors the
/// shape that `sera_runtime::stdio::emit_tool_events_from_transcript`
/// produces over NDJSON, then re-parses on the stdio backend's read side.
fn project_tool_events(transcript: &[Value]) -> Vec<ToolEvent> {
    let mut events = Vec::new();
    for msg in transcript {
        let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("");
        if role == "assistant" {
            if let Some(tool_calls) = msg.get("tool_calls").and_then(|tc| tc.as_array()) {
                for tc in tool_calls {
                    let call_id = tc
                        .get("id")
                        .and_then(|id| id.as_str())
                        .unwrap_or("")
                        .to_string();
                    let tool_name = tc
                        .get("function")
                        .and_then(|f| f.get("name"))
                        .and_then(|n| n.as_str())
                        .unwrap_or("")
                        .to_string();
                    let arguments_str = tc
                        .get("function")
                        .and_then(|f| f.get("arguments"))
                        .and_then(|a| a.as_str())
                        .unwrap_or("{}");
                    let arguments = serde_json::from_str(arguments_str)
                        .unwrap_or(Value::Object(Default::default()));
                    events.push(ToolEvent::Begin {
                        call_id,
                        tool: tool_name,
                        arguments,
                    });
                }
            }
        } else if role == "tool" {
            let call_id = msg
                .get("tool_call_id")
                .and_then(|id| id.as_str())
                .unwrap_or("")
                .to_string();
            let content = msg
                .get("content")
                .and_then(|c| c.as_str())
                .unwrap_or("")
                .to_string();
            events.push(ToolEvent::End { call_id, content });
        }
    }
    events
}

fn usage_from_tokens(tokens: &sera_types::runtime::TokenUsage) -> UsageInfo {
    UsageInfo {
        prompt_tokens: tokens.prompt_tokens as u64,
        completion_tokens: tokens.completion_tokens as u64,
        total_tokens: tokens.total_tokens as u64,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sera_runtime::context_engine::pipeline::ContextPipeline;
    use sera_runtime::turn::{LlmProvider, ThinkError, ThinkResult};

    /// Minimal `LlmProvider` that returns a fixed assistant reply with a
    /// canned token count. Used to exercise the embedded transport without
    /// depending on a network LLM.
    struct ScriptedLlm {
        reply: String,
    }

    #[async_trait]
    impl LlmProvider for ScriptedLlm {
        async fn chat(
            &self,
            _messages: &[Value],
            _tools: &[Value],
        ) -> Result<ThinkResult, ThinkError> {
            Ok(ThinkResult {
                response: serde_json::json!({
                    "role": "assistant",
                    "content": self.reply,
                }),
                tool_calls: vec![],
                tokens: sera_types::runtime::TokenUsage {
                    prompt_tokens: 7,
                    completion_tokens: 5,
                    total_tokens: 12,
                },
                plan: None,
            })
        }
    }

    fn build_runtime(reply: &'static str) -> Arc<DefaultRuntime> {
        let runtime = DefaultRuntime::new(Box::new(ContextPipeline::new()))
            .with_llm(Box::new(ScriptedLlm {
                reply: reply.to_string(),
            }))
            .with_allow_missing_constitutional_gate(true);
        Arc::new(runtime)
    }

    #[tokio::test]
    async fn send_turn_returns_response_and_usage() {
        let runtime = build_runtime("hello from embedded");
        let transport = EmbeddedRuntimeTransport::new("agent-1", runtime, vec![]);

        let events = transport
            .send_turn(
                vec![serde_json::json!({"role": "user", "content": "hi"})],
                "session:agent-1:t-1",
            )
            .await
            .expect("turn ok");

        assert_eq!(events.response, "hello from embedded");
        assert_eq!(events.usage.prompt_tokens, 7);
        assert_eq!(events.usage.completion_tokens, 5);
        assert_eq!(events.usage.total_tokens, 12);
        assert!(events.tool_events.is_empty());
    }

    #[tokio::test]
    async fn liveness_probe_succeeds_when_runtime_replies() {
        let runtime = build_runtime("pong");
        let transport = EmbeddedRuntimeTransport::new("agent-1", runtime, vec![]);
        transport.liveness_probe().await.expect("liveness ok");
    }

    #[tokio::test]
    async fn liveness_probe_fails_on_empty_response() {
        let runtime = build_runtime("");
        let transport = EmbeddedRuntimeTransport::new("agent-1", runtime, vec![]);
        let res = transport.liveness_probe().await;
        assert!(res.is_err(), "empty response should fail liveness");
    }

    #[tokio::test]
    async fn send_steer_stages_items_for_next_turn() {
        let runtime = build_runtime("ack");
        let transport = EmbeddedRuntimeTransport::new("agent-1", runtime, vec![]);
        let session = "session:agent-1:t-1";

        transport
            .send_steer(
                vec![serde_json::json!({"role": "user", "content": "steer"})],
                session,
            )
            .await
            .expect("steer ok");

        // Pending queue holds the staged item under the session key until drained.
        {
            let pending = transport.pending_steer.lock().await;
            assert_eq!(pending.get(session).map(Vec::len), Some(1));
        }

        // First send_turn on the same session drains the staged item.
        transport
            .send_turn(
                vec![serde_json::json!({"role": "user", "content": "next"})],
                session,
            )
            .await
            .expect("turn ok");

        let pending = transport.pending_steer.lock().await;
        assert!(
            !pending.contains_key(session),
            "pending steer queue should drain into TurnContext.metadata"
        );
    }

    #[tokio::test]
    async fn send_steer_is_scoped_per_session() {
        // Codex review on PR #1130: a steer staged for session A must not
        // be drained by an unrelated turn (session B / readiness probe)
        // running on the same agent. Stdio gets this for free via
        // per-Submission session_key routing; embedded must replicate it.
        let runtime = build_runtime("ack");
        let transport = EmbeddedRuntimeTransport::new("agent-1", runtime, vec![]);
        let session_a = "session:agent-1:alpha";
        let session_b = "session:agent-1:beta";

        transport
            .send_steer(
                vec![serde_json::json!({"role": "user", "content": "for-A"})],
                session_a,
            )
            .await
            .expect("steer ok");

        // A turn on session B (or the readiness probe) must NOT drain
        // session A's steer queue.
        transport
            .send_turn(
                vec![serde_json::json!({"role": "user", "content": "B"})],
                session_b,
            )
            .await
            .expect("turn ok");

        {
            let pending = transport.pending_steer.lock().await;
            assert_eq!(
                pending.get(session_a).map(Vec::len),
                Some(1),
                "session A steer must survive an unrelated session's turn"
            );
            assert!(
                !pending.contains_key(session_b),
                "session B has no pending steer"
            );
        }

        // The next turn on session A drains its own queue.
        transport
            .send_turn(
                vec![serde_json::json!({"role": "user", "content": "A"})],
                session_a,
            )
            .await
            .expect("turn ok");

        let pending = transport.pending_steer.lock().await;
        assert!(
            !pending.contains_key(session_a),
            "session A's pending queue should drain on its own turn"
        );
    }

    #[tokio::test]
    async fn liveness_probe_does_not_drain_real_session_steer() {
        // The readiness probe runs `send_turn` against the synthetic
        // `__sera_readiness_probe__` session. It must not consume steer
        // staged for a real user session running concurrently on the same
        // agent.
        let runtime = build_runtime("pong");
        let transport = EmbeddedRuntimeTransport::new("agent-1", runtime, vec![]);
        let user_session = "session:agent-1:user-42";

        transport
            .send_steer(
                vec![serde_json::json!({"role": "user", "content": "guidance"})],
                user_session,
            )
            .await
            .expect("steer ok");

        transport.liveness_probe().await.expect("probe ok");

        let pending = transport.pending_steer.lock().await;
        assert_eq!(
            pending.get(user_session).map(Vec::len),
            Some(1),
            "readiness probe must not drain a real session's steer queue"
        );
    }

    #[test]
    fn project_tool_events_extracts_begin_and_end() {
        let transcript = vec![
            serde_json::json!({
                "role": "assistant",
                "content": null,
                "tool_calls": [{
                    "id": "call_1",
                    "function": {
                        "name": "echo",
                        "arguments": "{\"msg\":\"hello\"}",
                    },
                }],
            }),
            serde_json::json!({
                "role": "tool",
                "tool_call_id": "call_1",
                "content": "ok",
            }),
        ];
        let events = project_tool_events(&transcript);
        assert_eq!(events.len(), 2);
        match &events[0] {
            ToolEvent::Begin { call_id, tool, arguments } => {
                assert_eq!(call_id, "call_1");
                assert_eq!(tool, "echo");
                assert_eq!(arguments.get("msg").and_then(|v| v.as_str()), Some("hello"));
            }
            other => panic!("expected Begin, got {other:?}"),
        }
        match &events[1] {
            ToolEvent::End { call_id, content } => {
                assert_eq!(call_id, "call_1");
                assert_eq!(content, "ok");
            }
            other => panic!("expected End, got {other:?}"),
        }
    }
}
