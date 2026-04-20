//! NDJSON Submission/Event stdio transport for sera-runtime (P0-6).
//!
//! This module implements the runtime's half of the `AppServerTransport::Stdio`
//! contract: one Submission JSON object per line on stdin, one Event JSON object
//! per line on stdout. The first frame emitted is a canonical
//! [`HandshakeFrame`] from `sera_types::envelope`; subsequent frames are the
//! protocol-v1 [`Event`] envelope defined below (serde-compatible with
//! `sera_gateway::bin::sera::StdioHarness`).
//!
//! The local [`Submission`], [`Event`], [`Op`], [`SystemOp`] and [`EventMsg`]
//! types are intentionally duplicated here rather than pulled from
//! `sera_types::envelope`:
//!
//! * `sera-runtime` must not depend on `sera-gateway` (cycle), and the gateway's
//!   [`StdioHarness`](sera_gateway::bin::sera::StdioHarness) currently emits the
//!   v1 shape. Switching both sides to canonical types simultaneously is P1
//!   follow-up work — tracked via `TODO(P0-6/P1-canonical-envelope)`.
//! * The handshake is already on the canonical `HandshakeFrame`, so v2
//!   consumers can negotiate without reading any event body.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use sera_types::envelope::HandshakeFrame;
use sera_types::runtime::{AgentRuntime, TokenUsage, TurnContext, TurnOutcome};

use crate::config::RuntimeConfig;
use crate::default_runtime::DefaultRuntime;

// ── Protocol-v1 envelope (runtime ↔ gateway stdio) ──────────────────────────

/// Local NDJSON submission type — serde-compatible with `sera-gateway`'s
/// `Submission`. Defined locally to avoid a cyclic dependency (`sera-gateway`
/// depends on `sera-runtime`). See module-level docs for the P1 plan to
/// migrate onto `sera_types::envelope`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Submission {
    pub id: uuid::Uuid,
    pub op: Op,
}

/// Local operation enum — mirrors `sera-gateway`'s `Op`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Op {
    UserTurn {
        items: Vec<serde_json::Value>,
        #[serde(default)]
        model_override: Option<String>,
        /// Session key provided by the gateway for per-session context tracking.
        #[serde(default)]
        session_key: Option<String>,
        /// Parent session key — set when this turn belongs to a child session.
        #[serde(default)]
        parent_session_key: Option<String>,
    },
    Steer {
        items: Vec<serde_json::Value>,
        #[serde(default)]
        session_key: Option<String>,
        /// Parent session key — propagated from the spawning session.
        #[serde(default)]
        parent_session_key: Option<String>,
    },
    Interrupt,
    System(SystemOp),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "system_op", rename_all = "snake_case")]
pub enum SystemOp {
    Shutdown,
    HealthCheck,
}

/// Local NDJSON event type — serde-compatible with `sera-gateway`'s `Event`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub id: uuid::Uuid,
    /// Nil UUID for handshake frames (no associated submission).
    pub submission_id: uuid::Uuid,
    pub msg: EventMsg,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// Parent session key carried on every frame so consumers can route child events.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_session_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EventMsg {
    TurnStarted {
        turn_id: uuid::Uuid,
    },
    /// Terminal turn frame carrying the provider-reported token usage for this
    /// turn. Consumers (e.g. the gateway) parse `tokens` to report usage back
    /// through `/api/chat` responses.
    TurnCompleted {
        turn_id: uuid::Uuid,
        #[serde(default)]
        tokens: TokenUsage,
    },
    StreamingDelta {
        delta: String,
    },
    /// Tool call started — emitted for each tool invocation during the turn.
    ToolCallBegin {
        turn_id: uuid::Uuid,
        call_id: String,
        tool: String,
        arguments: serde_json::Value,
    },
    /// Tool call completed — emitted after tool execution with the result.
    ToolCallEnd {
        turn_id: uuid::Uuid,
        call_id: String,
        result: String,
    },
    Error {
        code: String,
        message: String,
    },
}

// ── NDJSON loop ─────────────────────────────────────────────────────────────

/// Read [`Submission`] frames from stdin, dispatch each through the runtime,
/// and stream [`Event`] frames back on stdout. Exits on
/// `Op::System(SystemOp::Shutdown)` or EOF (stdin closed).
///
/// First frame emitted is the canonical [`HandshakeFrame::v2`] so any v2-aware
/// consumer can negotiate protocol capabilities without parsing an `Event`
/// body.
pub async fn run_ndjson_loop(
    config: &RuntimeConfig,
    runtime: &DefaultRuntime,
    tool_defs: &[sera_types::tool::ToolDefinition],
) -> anyhow::Result<()> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let stdin = tokio::io::stdin();
    let mut reader = BufReader::new(stdin);
    let mut stdout = tokio::io::stdout();
    let mut line = String::new();

    // Emit canonical HandshakeFrame — v2-aware consumers negotiate capabilities
    // here; legacy consumers treat the first line as an informational frame and
    // skip it (non-Event JSON).
    let handshake = HandshakeFrame::v2(config.agent_id.clone(), None);
    let mut handshake_json = serde_json::to_string(&handshake)?;
    handshake_json.push('\n');
    stdout.write_all(handshake_json.as_bytes()).await?;
    stdout.flush().await?;

    loop {
        line.clear();
        let n = reader.read_line(&mut line).await?;
        if n == 0 {
            tracing::info!("stdin closed, exiting NDJSON loop");
            break;
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let submission: Submission = match serde_json::from_str(trimmed) {
            Ok(s) => s,
            Err(e) => {
                emit(
                    &mut stdout,
                    Event {
                        id: uuid::Uuid::new_v4(),
                        submission_id: uuid::Uuid::nil(),
                        msg: EventMsg::Error {
                            code: "parse_error".to_string(),
                            message: format!("failed to parse submission: {e}"),
                        },
                        timestamp: chrono::Utc::now(),
                        parent_session_key: None,
                    },
                )
                .await?;
                continue;
            }
        };

        // Check for shutdown
        if matches!(&submission.op, Op::System(SystemOp::Shutdown)) {
            tracing::info!("received shutdown command, exiting");
            break;
        }

        process_submission(&mut stdout, config, runtime, tool_defs, submission).await?;
    }

    Ok(())
}

/// Dispatch a single submission through the runtime and stream the resulting
/// [`Event`] frames to `stdout`.
async fn process_submission(
    stdout: &mut tokio::io::Stdout,
    config: &RuntimeConfig,
    runtime: &DefaultRuntime,
    tool_defs: &[sera_types::tool::ToolDefinition],
    submission: Submission,
) -> anyhow::Result<()> {
    let turn_id = uuid::Uuid::new_v4();

    // Extract parent_session_key from the submission op for propagation.
    let submission_parent_key = match &submission.op {
        Op::UserTurn { parent_session_key, .. } => parent_session_key.clone(),
        Op::Steer { parent_session_key, .. } => parent_session_key.clone(),
        _ => None,
    };

    // Emit TurnStarted
    emit(
        stdout,
        Event {
            id: uuid::Uuid::new_v4(),
            submission_id: submission.id,
            msg: EventMsg::TurnStarted { turn_id },
            timestamp: chrono::Utc::now(),
            parent_session_key: submission_parent_key.clone(),
        },
    )
    .await?;

    let turn_ctx = submission_to_turn_context(&submission, &config.agent_id, turn_id, tool_defs);
    let outcome = runtime.execute_turn(turn_ctx).await;

    // Extract tokens_used for the terminal TurnCompleted frame. `Interruption`
    // is the only outcome variant without a `tokens_used` field; errors and
    // the `Err` arm report zeroed usage (the LLM call never completed).
    let tokens_for_completion = match &outcome {
        Ok(TurnOutcome::FinalOutput { tokens_used, .. })
        | Ok(TurnOutcome::RunAgain { tokens_used, .. })
        | Ok(TurnOutcome::Handoff { tokens_used, .. })
        | Ok(TurnOutcome::Compact { tokens_used, .. })
        | Ok(TurnOutcome::Stop { tokens_used, .. })
        | Ok(TurnOutcome::WaitingForApproval { tokens_used, .. })
        | Ok(TurnOutcome::PlanEmitted { tokens_used, .. }) => tokens_used.clone(),
        Ok(TurnOutcome::Interruption { .. }) | Err(_) => TokenUsage::default(),
    };

    match outcome {
        Ok(TurnOutcome::FinalOutput { response, transcript, .. }) => {
            emit_tool_events_from_transcript(stdout, &submission, turn_id, &submission_parent_key, &transcript)
                .await?;
            emit(
                stdout,
                Event {
                    id: uuid::Uuid::new_v4(),
                    submission_id: submission.id,
                    msg: EventMsg::StreamingDelta { delta: response },
                    timestamp: chrono::Utc::now(),
                    parent_session_key: submission_parent_key.clone(),
                },
            )
            .await?;
        }
        Ok(TurnOutcome::RunAgain { .. }) => {
            emit_delta(
                stdout,
                &submission,
                &submission_parent_key,
                "[run_again — tool calls dispatched]".into(),
            )
            .await?;
        }
        Ok(TurnOutcome::Handoff { target_agent_id, .. }) => {
            emit_delta(
                stdout,
                &submission,
                &submission_parent_key,
                format!("[handoff -> {target_agent_id}]"),
            )
            .await?;
        }
        Ok(TurnOutcome::Compact { .. }) => {
            emit_delta(
                stdout,
                &submission,
                &submission_parent_key,
                "[compact — context condensed]".into(),
            )
            .await?;
        }
        Ok(TurnOutcome::Interruption { reason, .. }) => {
            emit_delta(
                stdout,
                &submission,
                &submission_parent_key,
                format!("[interrupted: {reason}]"),
            )
            .await?;
        }
        Ok(TurnOutcome::Stop { summary, .. }) => {
            emit_delta(
                stdout,
                &submission,
                &submission_parent_key,
                format!("[stop: {summary}]"),
            )
            .await?;
        }
        Ok(TurnOutcome::WaitingForApproval { ticket_id, .. }) => {
            emit_delta(
                stdout,
                &submission,
                &submission_parent_key,
                format!("[waiting_for_approval: ticket={ticket_id}]"),
            )
            .await?;
        }
        Ok(TurnOutcome::PlanEmitted { plan_tool_calls, rationale, .. }) => {
            let summary = format!(
                "[plan_emitted: {} tool call(s); rationale={:?}]",
                plan_tool_calls.len(),
                rationale
            );
            emit_delta(stdout, &submission, &submission_parent_key, summary).await?;
        }
        Err(e) => {
            tracing::error!("execute_turn failed: {e:?}");
            emit(
                stdout,
                Event {
                    id: uuid::Uuid::new_v4(),
                    submission_id: submission.id,
                    msg: EventMsg::Error {
                        code: "turn_error".to_string(),
                        message: format!("{e:?}"),
                    },
                    timestamp: chrono::Utc::now(),
                    parent_session_key: submission_parent_key.clone(),
                },
            )
            .await?;
        }
    }

    // Emit TurnCompleted with the usage the LLM reported for this turn.
    emit(
        stdout,
        Event {
            id: uuid::Uuid::new_v4(),
            submission_id: submission.id,
            msg: EventMsg::TurnCompleted {
                turn_id,
                tokens: tokens_for_completion,
            },
            timestamp: chrono::Utc::now(),
            parent_session_key: submission_parent_key,
        },
    )
    .await?;

    use tokio::io::AsyncWriteExt;
    stdout.flush().await?;
    Ok(())
}

/// Fan out `ToolCallBegin` / `ToolCallEnd` frames reconstructed from the
/// runtime transcript (assistant tool_calls + tool result messages).
async fn emit_tool_events_from_transcript(
    stdout: &mut tokio::io::Stdout,
    submission: &Submission,
    turn_id: uuid::Uuid,
    parent_session_key: &Option<String>,
    transcript: &[serde_json::Value],
) -> anyhow::Result<()> {
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
                        .unwrap_or(serde_json::Value::Object(Default::default()));
                    emit(
                        stdout,
                        Event {
                            id: uuid::Uuid::new_v4(),
                            submission_id: submission.id,
                            msg: EventMsg::ToolCallBegin {
                                turn_id,
                                call_id,
                                tool: tool_name,
                                arguments,
                            },
                            timestamp: chrono::Utc::now(),
                            parent_session_key: parent_session_key.clone(),
                        },
                    )
                    .await?;
                }
            }
        } else if role == "tool" {
            let call_id = msg
                .get("tool_call_id")
                .and_then(|id| id.as_str())
                .unwrap_or("")
                .to_string();
            let result_content = msg
                .get("content")
                .and_then(|c| c.as_str())
                .unwrap_or("")
                .to_string();
            emit(
                stdout,
                Event {
                    id: uuid::Uuid::new_v4(),
                    submission_id: submission.id,
                    msg: EventMsg::ToolCallEnd {
                        turn_id,
                        call_id,
                        result: result_content,
                    },
                    timestamp: chrono::Utc::now(),
                    parent_session_key: parent_session_key.clone(),
                },
            )
            .await?;
        }
    }
    Ok(())
}

async fn emit_delta(
    stdout: &mut tokio::io::Stdout,
    submission: &Submission,
    parent_session_key: &Option<String>,
    delta: String,
) -> anyhow::Result<()> {
    emit(
        stdout,
        Event {
            id: uuid::Uuid::new_v4(),
            submission_id: submission.id,
            msg: EventMsg::StreamingDelta { delta },
            timestamp: chrono::Utc::now(),
            parent_session_key: parent_session_key.clone(),
        },
    )
    .await
}

async fn emit(stdout: &mut tokio::io::Stdout, event: Event) -> anyhow::Result<()> {
    use tokio::io::AsyncWriteExt;
    let mut json = serde_json::to_string(&event)?;
    json.push('\n');
    stdout.write_all(json.as_bytes()).await?;
    Ok(())
}

/// Convert a local [`Submission`] into a [`TurnContext`] for the runtime.
fn submission_to_turn_context(
    submission: &Submission,
    agent_id: &str,
    turn_id: uuid::Uuid,
    tool_defs: &[sera_types::tool::ToolDefinition],
) -> TurnContext {
    let (messages, session_key_override, parent_session_key) = match &submission.op {
        Op::UserTurn { items, session_key, parent_session_key, .. } => {
            (items.clone(), session_key.clone(), parent_session_key.clone())
        }
        Op::Steer { items, session_key, parent_session_key } => {
            (items.clone(), session_key.clone(), parent_session_key.clone())
        }
        Op::Interrupt | Op::System(_) => (vec![], None, None),
    };

    // Use gateway-provided session_key when available, otherwise generate one.
    let session_key = session_key_override
        .unwrap_or_else(|| format!("session:{agent_id}:{}", submission.id));

    TurnContext {
        event_id: turn_id.to_string(),
        agent_id: agent_id.to_string(),
        session_key,
        messages,
        available_tools: tool_defs.to_vec(),
        metadata: HashMap::new(),
        change_artifact: None,
        parent_session_key,
        tool_use_behavior: Default::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handshake_first_line_is_canonical_v2() {
        let frame = HandshakeFrame::v2("agent-demo", None);
        let json = serde_json::to_string(&frame).unwrap();
        let parsed: HandshakeFrame = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.protocol_version, "2.0");
        assert_eq!(parsed.frame_type, "handshake");
        assert!(parsed.capabilities.supports("steer"));
        assert!(parsed.capabilities.supports("hitl"));
    }

    #[test]
    fn submission_user_turn_with_session_key_roundtrip() {
        let json = r#"{
            "id": "00000000-0000-0000-0000-000000000000",
            "op": {
                "type": "user_turn",
                "items": [{"role":"user","content":"hi"}],
                "session_key": "session:agent-x:abc"
            }
        }"#;
        let sub: Submission = serde_json::from_str(json).unwrap();
        match sub.op {
            Op::UserTurn { items, session_key, .. } => {
                assert_eq!(items.len(), 1);
                assert_eq!(session_key.as_deref(), Some("session:agent-x:abc"));
            }
            _ => panic!("expected UserTurn"),
        }
    }

    #[test]
    fn submission_system_shutdown_roundtrip() {
        let json = r#"{
            "id": "00000000-0000-0000-0000-000000000000",
            "op": {"type":"system","system_op":"shutdown"}
        }"#;
        let sub: Submission = serde_json::from_str(json).unwrap();
        assert!(matches!(sub.op, Op::System(SystemOp::Shutdown)));
    }

    #[test]
    fn event_turn_completed_serializes_tokens() {
        let ev = Event {
            id: uuid::Uuid::nil(),
            submission_id: uuid::Uuid::nil(),
            msg: EventMsg::TurnCompleted {
                turn_id: uuid::Uuid::nil(),
                tokens: TokenUsage {
                    prompt_tokens: 10,
                    completion_tokens: 20,
                    total_tokens: 30,
                },
            },
            timestamp: chrono::Utc::now(),
            parent_session_key: None,
        };
        let json = serde_json::to_string(&ev).unwrap();
        assert!(json.contains("\"prompt_tokens\":10"));
        assert!(json.contains("\"completion_tokens\":20"));
        assert!(json.contains("\"total_tokens\":30"));
    }
}
