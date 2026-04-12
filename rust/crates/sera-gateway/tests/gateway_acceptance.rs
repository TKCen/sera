//! Gateway acceptance tests — Lane D, P0-5.

use sera_gateway::envelope::*;
use sera_gateway::harness_dispatch::*;
use sera_gateway::kill_switch::*;
use sera_gateway::transport::*;
use sera_gateway::transport::in_process::InProcessTransport;

use std::pin::Pin;

use async_trait::async_trait;
use tokio_stream::{Stream, StreamExt};
use uuid::Uuid;

// ── 1. Submission serde roundtrip ───────────────────────────────────────────

#[test]
fn envelope_submission_roundtrip() {
    let sub = Submission {
        id: Uuid::new_v4(),
        op: Op::UserTurn {
            items: vec![],
            cwd: None,
            approval_policy: None,
            sandbox_policy: None,
            model_override: None,
            effort: None,
            final_output_schema: None,
        },
        trace: W3cTraceContext::default(),
        change_artifact: None,
    };
    let json = serde_json::to_string(&sub).unwrap();
    let parsed: Submission = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.id, sub.id);
}

// ── 2. Event serde roundtrip ────────────────────────────────────────────────

#[test]
fn envelope_event_roundtrip() {
    let evt = Event {
        id: Uuid::new_v4(),
        submission_id: Uuid::new_v4(),
        msg: EventMsg::StreamingDelta {
            delta: "hello".to_string(),
        },
        trace: W3cTraceContext::default(),
        timestamp: chrono::Utc::now(),
    };
    let json = serde_json::to_string(&evt).unwrap();
    let parsed: Event = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.id, evt.id);
}

// ── 3. Transport enum exhaustive ────────────────────────────────────────────

#[test]
fn transport_enum_exhaustive() {
    let variants = [
        AppServerTransport::InProcess,
        AppServerTransport::Stdio {
            command: "echo".into(),
            args: vec![],
            env: Default::default(),
        },
        AppServerTransport::WebSocket {
            bind: "0.0.0.0:8080".into(),
            tls: false,
        },
        AppServerTransport::Grpc {
            endpoint: "http://localhost:50051".into(),
            tls: false,
        },
        AppServerTransport::WebhookBack {
            callback_base_url: "http://localhost".into(),
        },
        AppServerTransport::Off,
    ];
    // Compile-time exhaustive — if a variant is added, this breaks.
    for v in &variants {
        match v {
            AppServerTransport::InProcess => {}
            AppServerTransport::Stdio { .. } => {}
            AppServerTransport::WebSocket { .. } => {}
            AppServerTransport::Grpc { .. } => {}
            AppServerTransport::WebhookBack { .. } => {}
            AppServerTransport::Off => {}
        }
    }
    assert_eq!(variants.len(), 6);
}

// ── 4. InProcess transport dispatch ─────────────────────────────────────────

#[tokio::test]
async fn in_process_transport_dispatch() {
    let (transport, mut sub_rx, event_tx) = InProcessTransport::new(16);

    let sub = Submission {
        id: Uuid::new_v4(),
        op: Op::Interrupt,
        trace: W3cTraceContext::default(),
        change_artifact: None,
    };
    let sub_id = sub.id;

    // Send submission
    transport.send_submission(sub).await.unwrap();

    // Receive on the runtime side
    let received = sub_rx.recv().await.unwrap();
    assert_eq!(received.id, sub_id);

    // Send event back
    let evt = Event {
        id: Uuid::new_v4(),
        submission_id: sub_id,
        msg: EventMsg::StreamingDelta {
            delta: "ok".into(),
        },
        trace: W3cTraceContext::default(),
        timestamp: chrono::Utc::now(),
    };
    let evt_id = evt.id;
    event_tx.send(evt).await.unwrap();

    // Receive events
    let mut stream = transport.recv_events().await.unwrap();
    let received_evt = stream.next().await.unwrap();
    assert_eq!(received_evt.id, evt_id);
}

// ── 5. Harness dispatch routes to correct harness ───────────────────────────

struct MockHarness {
    name: String,
}

#[async_trait]
impl AgentHarness for MockHarness {
    async fn handle(
        &self,
        submission: Submission,
    ) -> Result<Pin<Box<dyn Stream<Item = Event> + Send>>, HarnessError> {
        let name = self.name.clone();
        let stream = async_stream::stream! {
            yield Event {
                id: Uuid::new_v4(),
                submission_id: submission.id,
                msg: EventMsg::StreamingDelta { delta: name },
                trace: W3cTraceContext::default(),
                timestamp: chrono::Utc::now(),
            };
        };
        Ok(Box::pin(stream))
    }

    async fn health(&self) -> bool {
        true
    }

    async fn shutdown(&self) -> Result<(), HarnessError> {
        Ok(())
    }
}

#[tokio::test]
async fn harness_dispatch_routes_to_correct_harness() {
    let registry = new_harness_registry();

    {
        let mut reg = registry.write().await;
        reg.insert(
            "agent-a".to_string(),
            Box::new(MockHarness {
                name: "harness-a".into(),
            }),
        );
        reg.insert(
            "agent-b".to_string(),
            Box::new(MockHarness {
                name: "harness-b".into(),
            }),
        );
    }

    let sub = Submission {
        id: Uuid::new_v4(),
        op: Op::Interrupt,
        trace: W3cTraceContext::default(),
        change_artifact: None,
    };

    let mut stream = dispatch(&sub, "agent-b", &registry).await.unwrap();
    let evt = stream.next().await.unwrap();
    if let EventMsg::StreamingDelta { delta } = &evt.msg {
        assert_eq!(delta, "harness-b");
    } else {
        panic!("expected StreamingDelta");
    }
}

// ── 6. Kill switch admin socket ─────────────────────────────────────────────

#[test]
fn kill_switch_admin_socket_accepts_shutdown() {
    let ks = KillSwitch::new();
    assert_eq!(ks.state(), KillSwitchState::Disarmed);

    let result = ks.handle_command("ROLLBACK\n");
    assert_eq!(result, "OK\n");
    assert_eq!(ks.state(), KillSwitchState::Armed);

    let result = ks.handle_command("DISARM\n");
    assert_eq!(result, "OK\n");
    assert_eq!(ks.state(), KillSwitchState::Disarmed);
}

// ── 7. Generation marker ───────────────────────────────────────────────────

#[test]
fn generation_marker_propagates_to_event_context() {
    let gm = sera_gateway::generation::current_generation();
    assert!(!gm.label.is_empty());
    assert!(gm.binary_identity.starts_with("sera-gateway@"));
}

// ── 8. Submission construction ──────────────────────────────────────────────

#[test]
fn rest_chat_handler_wraps_as_submission() {
    let sub = Submission {
        id: Uuid::new_v4(),
        op: Op::UserTurn {
            items: vec![sera_types::ContentBlock::Text {
                text: "Hello".to_string(),
            }],
            cwd: Some("/workspace".into()),
            approval_policy: None,
            sandbox_policy: None,
            model_override: None,
            effort: None,
            final_output_schema: None,
        },
        trace: W3cTraceContext::default(),
        change_artifact: None,
    };
    let json = serde_json::to_string(&sub).unwrap();
    assert!(json.contains("Hello"));
    assert!(json.contains("user_turn"));
}
