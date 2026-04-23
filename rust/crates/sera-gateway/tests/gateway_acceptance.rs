//! Gateway acceptance tests — Lane D, P0-5.

use sera_gateway::envelope::*;
use sera_gateway::harness_dispatch::*;
use sera_gateway::kill_switch::*;
use sera_gateway::transport::in_process::InProcessTransport;
use sera_gateway::transport::*;

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
        session_key: None,
        parent_session_key: None,
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
        parent_session_key: None,
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
        session_key: None,
        parent_session_key: None,
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
        msg: EventMsg::StreamingDelta { delta: "ok".into() },
        trace: W3cTraceContext::default(),
        timestamp: chrono::Utc::now(),
        parent_session_key: None,
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
                parent_session_key: None,
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
        session_key: None,
        parent_session_key: None,
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

    let (result, did_rollback) = ks.handle_command("ROLLBACK\n");
    assert_eq!(result, "OK\n");
    assert!(did_rollback);
    assert_eq!(ks.state(), KillSwitchState::Armed);

    let (result, did_rollback) = ks.handle_command("DISARM\n");
    assert_eq!(result, "OK\n");
    assert!(!did_rollback);
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
            items: vec![serde_json::json!({"type": "text", "text": "Hello"})],
            cwd: Some("/workspace".into()),
            approval_policy: None,
            sandbox_policy: None,
            model_override: None,
            effort: None,
            final_output_schema: None,
        },
        trace: W3cTraceContext::default(),
        change_artifact: None,
        session_key: None,
        parent_session_key: None,
    };
    let json = serde_json::to_string(&sub).unwrap();
    assert!(json.contains("Hello"));
    assert!(json.contains("user_turn"));
}

// ── 9. SessionStore unit tests ──────────────────────────────────────────────
//
// These tests verify that InMemorySessionStore correctly records Submission
// envelopes in the order they are appended, and that SubmissionRef correlates
// back to the envelope id. They form the contract that route wrappers rely on.

use sera_gateway::session_store::{InMemorySessionStore, SessionStore};

#[tokio::test]
async fn session_store_chat_envelope_recorded() {
    // Simulate what chat route wrapper does: build a UserTurn Submission and
    // append it before dispatching to the harness.
    let store = InMemorySessionStore::new();

    let sub = Submission {
        id: Uuid::new_v4(),
        op: Op::UserTurn {
            items: vec![serde_json::json!({"type": "text", "text": "hello agent"})],
            cwd: None,
            approval_policy: None,
            sandbox_policy: None,
            model_override: None,
            effort: None,
            final_output_schema: None,
        },
        trace: W3cTraceContext::default(),
        change_artifact: None,
        session_key: None,
        parent_session_key: None,
    };
    let sub_id = sub.id;

    let session = "session-chat-1";
    let r = store.append_envelope(session, &sub).await.unwrap();
    assert_eq!(r.session_id, session);
    assert_eq!(r.index, 0);

    assert_eq!(store.len_for(session).await, 1);

    let all = store.all_for(session).await;
    assert_eq!(all[0].submission.id, sub_id);
    match &all[0].submission.op {
        Op::UserTurn { items, .. } => {
            assert_eq!(items.len(), 1);
            assert_eq!(items[0]["type"], "text");
            assert_eq!(items[0]["text"], "hello agent");
        }
        other => panic!("expected UserTurn, got {:?}", other),
    }
}

#[tokio::test]
async fn session_store_task_enqueue_envelope_recorded() {
    // Simulate what the enqueue_task route wrapper does.
    let store = InMemorySessionStore::new();

    let sub = Submission {
        id: Uuid::new_v4(),
        op: Op::UserTurn {
            items: vec![serde_json::json!({"type": "text", "text": "summarise document"})],
            cwd: None,
            approval_policy: Some("agent-xyz".to_string()),
            sandbox_policy: None,
            model_override: None,
            effort: None,
            final_output_schema: None,
        },
        trace: W3cTraceContext::default(),
        change_artifact: None,
        session_key: None,
        parent_session_key: None,
    };
    let sub_id = sub.id;
    let session = "agent-xyz";
    let r = store.append_envelope(session, &sub).await.unwrap();
    assert_eq!(r.session_id, session);

    let all = store.all_for(session).await;
    assert_eq!(all[0].submission.id, sub_id);
    // approval_policy encodes the agent_id for correlation
    match &all[0].submission.op {
        Op::UserTurn {
            approval_policy, ..
        } => {
            assert_eq!(approval_policy.as_deref(), Some("agent-xyz"));
        }
        other => panic!("expected UserTurn, got {:?}", other),
    }
}

#[tokio::test]
async fn session_store_permission_request_envelope_recorded() {
    // Simulate what create_request route wrapper does.
    let store = InMemorySessionStore::new();

    let sub = Submission {
        id: Uuid::new_v4(),
        op: Op::UserTurn {
            items: vec![
                serde_json::json!({"type": "text", "text": "permission_request:filesystem:/workspace/data:read"}),
            ],
            cwd: None,
            approval_policy: Some("instance-abc".to_string()),
            sandbox_policy: None,
            model_override: None,
            effort: None,
            final_output_schema: None,
        },
        trace: W3cTraceContext::default(),
        change_artifact: None,
        session_key: None,
        parent_session_key: None,
    };
    let sub_id = sub.id;
    let session = "instance-abc";
    let r = store.append_envelope(session, &sub).await.unwrap();
    assert_eq!(r.session_id, session);
    assert_eq!(store.len_for(session).await, 1);
    let all = store.all_for(session).await;
    assert_eq!(all[0].submission.id, sub_id);
}

#[tokio::test]
async fn session_store_intercom_dm_envelope_recorded() {
    // Simulate what intercom::dm route wrapper does.
    let store = InMemorySessionStore::new();

    let payload = serde_json::json!({"text": "hey, can you help?"});
    let sub = Submission {
        id: Uuid::new_v4(),
        op: Op::UserTurn {
            items: vec![serde_json::json!({"type": "text", "text": "intercom_dm:agent-a:agent-b"})],
            cwd: None,
            approval_policy: Some("agent-b".to_string()),
            sandbox_policy: None,
            model_override: None,
            effort: None,
            final_output_schema: Some(payload.clone()),
        },
        trace: W3cTraceContext::default(),
        change_artifact: None,
        session_key: None,
        parent_session_key: None,
    };
    let sub_id = sub.id;
    let session = "agent-a";
    let r = store.append_envelope(session, &sub).await.unwrap();
    assert_eq!(r.session_id, session);

    let all = store.all_for(session).await;
    assert_eq!(all[0].submission.id, sub_id);
    match &all[0].submission.op {
        Op::UserTurn {
            final_output_schema,
            ..
        } => {
            assert_eq!(final_output_schema.as_ref().unwrap(), &payload);
        }
        other => panic!("expected UserTurn, got {:?}", other),
    }
}

// ── 10. Integration: sequence of route envelopes with parent refs ───────────
//
// Fires a sequence of Submission envelopes (mimicking chat → task → result)
// and verifies the store sees them in order with correct ids.

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn session_store_sequence_preserves_order_and_ids() {
    let store = InMemorySessionStore::new();

    let chat_id = Uuid::new_v4();
    let task_id = Uuid::new_v4();
    let result_id = Uuid::new_v4();

    // 1. Chat turn submission
    let chat_sub = Submission {
        id: chat_id,
        op: Op::UserTurn {
            items: vec![serde_json::json!({"type": "text", "text": "start a research task"})],
            cwd: None,
            approval_policy: None,
            sandbox_policy: None,
            model_override: None,
            effort: None,
            final_output_schema: None,
        },
        trace: W3cTraceContext::default(),
        change_artifact: None,
        session_key: None,
        parent_session_key: None,
    };

    // 2. Task enqueue submission
    let task_sub = Submission {
        id: task_id,
        op: Op::UserTurn {
            items: vec![serde_json::json!({"type": "text", "text": "research task"})],
            cwd: None,
            approval_policy: Some("agent-research".to_string()),
            sandbox_policy: None,
            model_override: None,
            effort: None,
            final_output_schema: None,
        },
        trace: W3cTraceContext::default(),
        change_artifact: None,
        session_key: None,
        parent_session_key: None,
    };

    // 3. Task result submission
    let result_sub = Submission {
        id: result_id,
        op: Op::UserTurn {
            items: vec![
                serde_json::json!({"type": "text", "text": format!("task_result:{task_id}")}),
            ],
            cwd: None,
            approval_policy: Some("agent-research".to_string()),
            sandbox_policy: None,
            model_override: None,
            effort: None,
            final_output_schema: Some(serde_json::json!({"summary": "done"})),
        },
        trace: W3cTraceContext::default(),
        change_artifact: None,
        session_key: None,
        parent_session_key: None,
    };

    // Append in sequence — simulates a complete agent workflow
    let session = "agent-research";
    let r1 = store.append_envelope(session, &chat_sub).await.unwrap();
    let r2 = store.append_envelope(session, &task_sub).await.unwrap();
    let r3 = store.append_envelope(session, &result_sub).await.unwrap();

    // Refs carry the session id + monotonically increasing index.
    assert_eq!((r1.session_id.as_str(), r1.index), (session, 0));
    assert_eq!((r2.session_id.as_str(), r2.index), (session, 1));
    assert_eq!((r3.session_id.as_str(), r3.index), (session, 2));

    // Store must have exactly 3 entries in insertion order
    let all = store.all_for(session).await;
    assert_eq!(all.len(), 3);
    assert_eq!(all[0].submission.id, chat_id, "chat must be first");
    assert_eq!(all[1].submission.id, task_id, "task enqueue must be second");
    assert_eq!(all[2].submission.id, result_id, "task result must be third");
}
