//! sera-5mbr: round-trip a UserTurn submission through `AppServerTransport::Stdio`.
//!
//! Verifies the wiring between the [`AppServerTransport::connect`] factory
//! and the [`StdioTransport`] implementation by spawning a minimal bash
//! child that emits canned `TurnStarted` / `StreamingDelta` /
//! `TurnCompleted` NDJSON events and asserting they decode into typed
//! `Event` frames.

#![cfg(feature = "stdio")]

use std::collections::HashMap;
use std::time::Duration;

use tokio_stream::StreamExt;
use uuid::Uuid;

use sera_gateway::envelope::{EventMsg, Op, Submission};
use sera_gateway::transport::AppServerTransport;

/// Bash one-shot that consumes a single NDJSON submission and replies with
/// the canonical three-event sequence the gateway expects on a successful
/// turn. The submission_id field on the events is fixed (the round-trip
/// test does not require correlation in this layer).
const REPLY_SCRIPT: &str = r#"
read line
echo '{"id":"00000000-0000-0000-0000-000000000001","submission_id":"00000000-0000-0000-0000-000000000000","msg":{"type":"turn_started","turn_id":"00000000-0000-0000-0000-000000000002"},"timestamp":"2024-01-01T00:00:00Z"}'
echo '{"id":"00000000-0000-0000-0000-000000000003","submission_id":"00000000-0000-0000-0000-000000000000","msg":{"type":"streaming_delta","delta":"hello"},"timestamp":"2024-01-01T00:00:00Z"}'
echo '{"id":"00000000-0000-0000-0000-000000000004","submission_id":"00000000-0000-0000-0000-000000000000","msg":{"type":"turn_completed","turn_id":"00000000-0000-0000-0000-000000000002","tokens":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}},"timestamp":"2024-01-01T00:00:00Z"}'
"#;

fn user_turn_submission() -> Submission {
    Submission {
        id: Uuid::new_v4(),
        op: Op::UserTurn {
            items: vec![serde_json::json!({"role":"user","content":"hi"})],
            cwd: None,
            approval_policy: None,
            sandbox_policy: None,
            model_override: None,
            effort: None,
            final_output_schema: None,
        },
        trace: Default::default(),
        change_artifact: None,
        session_key: Some("session-1".to_string()),
        parent_session_key: None,
        parent_task_id: None,
    }
}

#[tokio::test]
async fn stdio_factory_round_trips_user_turn_envelope() {
    let cfg = AppServerTransport::Stdio {
        command: "bash".to_string(),
        args: vec!["-c".to_string(), REPLY_SCRIPT.to_string()],
        env: HashMap::new(),
    };

    let transport = cfg
        .connect()
        .await
        .expect("Stdio variant should reach a real transport implementation");

    transport
        .send_submission(user_turn_submission())
        .await
        .expect("send_submission");

    let mut stream = transport.recv_events().await.expect("recv_events");

    let mut saw_turn_started = false;
    let mut accumulated = String::new();
    let mut saw_completed = false;
    let mut total_tokens = 0u64;

    let timeout = tokio::time::sleep(Duration::from_secs(5));
    tokio::pin!(timeout);

    loop {
        tokio::select! {
            _ = &mut timeout => panic!("timed out waiting for turn_completed"),
            evt = stream.next() => {
                let Some(evt) = evt else { break };
                match evt.msg {
                    EventMsg::TurnStarted { .. } => saw_turn_started = true,
                    EventMsg::StreamingDelta { delta } => accumulated.push_str(&delta),
                    EventMsg::TurnCompleted { tokens, .. } => {
                        total_tokens = tokens.total_tokens as u64;
                        saw_completed = true;
                        break;
                    }
                    _ => {}
                }
            }
        }
    }

    assert!(saw_turn_started, "expected TurnStarted event");
    assert_eq!(accumulated, "hello", "streamed text should accumulate");
    assert!(saw_completed, "expected TurnCompleted event");
    assert_eq!(total_tokens, 2, "TurnCompleted.tokens should decode");

    transport.close().await.expect("close");
}

#[tokio::test]
async fn stdio_factory_close_is_idempotent_on_exited_child() {
    // The reply script exits after emitting events; close() should still
    // succeed because Tokio's Child::kill is well-defined on a finished
    // child (sera-5mbr regression guard for cancellation paths).
    let cfg = AppServerTransport::Stdio {
        command: "bash".to_string(),
        args: vec!["-c".to_string(), REPLY_SCRIPT.to_string()],
        env: HashMap::new(),
    };

    let transport = cfg.connect().await.expect("spawn");
    transport
        .send_submission(user_turn_submission())
        .await
        .expect("send");

    let mut stream = transport.recv_events().await.expect("recv");
    while let Some(evt) = stream.next().await {
        if matches!(evt.msg, EventMsg::TurnCompleted { .. }) {
            break;
        }
    }
    drop(stream);

    transport.close().await.expect("first close");
    transport.close().await.expect("second close");
}

#[tokio::test]
async fn off_variant_returns_closed_error() {
    let cfg = AppServerTransport::Off;
    let result = cfg.connect().await;
    assert!(
        matches!(
            result.as_ref().map(|_| ()),
            Err(sera_gateway::transport::TransportError::Closed)
        ),
        "Off variant must surface as Closed, got {result:?}",
        result = result.as_ref().map(|_| "<transport>"),
    );
}

#[tokio::test]
async fn in_process_variant_cannot_be_reached_via_factory() {
    let cfg = AppServerTransport::InProcess;
    let result = cfg.connect().await;
    assert!(
        matches!(
            result.as_ref().map(|_| ()),
            Err(sera_gateway::transport::TransportError::ConnectionFailed(_))
        ),
        "InProcess must require explicit channel pairing, got {result:?}",
        result = result.as_ref().map(|_| "<transport>"),
    );
}
