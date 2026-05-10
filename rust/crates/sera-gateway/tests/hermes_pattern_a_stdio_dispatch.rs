//! sera-plcv M1 AC2 (Pattern A): end-to-end probe that a Hermes-shaped Stdio
//! harness's aliased tool emission routes back through gateway-owned dispatch.
//!
//! ## What this proves
//!
//! M1 AC2 reads:
//!
//! > Hermes ACP server spawns as a Stdio child of the gateway via sera-5mbr;
//! > its tool catalog registers and aliases via sera-w2dh; a probe call
//! > confirms tool dispatch returns through the gateway.
//!
//! Both component seams already exist:
//!
//! * `tests/transport_stdio_roundtrip.rs` (sera-5mbr) — exercises
//!   [`AppServerTransport::Stdio`] end-to-end with `TurnStarted` /
//!   `StreamingDelta` / `TurnCompleted` frames.
//! * `tests/byoh_registration_handshake.rs` (sera-w2dh) — exercises
//!   [`ByohCatalogRegistry::register_harness`] +
//!   [`ByohCatalogRegistry::prepare_outbound_call`] in-process.
//!
//! Neither one composes them. This test wires both into the AC2-shaped flow:
//! a bash one-shot stand-in for the Hermes ACP server emits a `ToolCallBegin`
//! NDJSON frame whose `tool` is the harness-internal name `Read`. The
//! gateway-side `ByohCatalogRegistry` is pre-seeded with the canonical
//! `read_file` tool that aliases `Read`. The test asserts the alias is rerouted
//! through gateway-owned dispatch (canonical `read_file`) and the harness's
//! internal implementation is never invoked.
//!
//! ## What this deliberately does not prove
//!
//! * The gateway runtime does not yet perform this routing automatically when
//!   it consumes a `ToolCallBegin` frame from a Stdio harness — the
//!   reconciliation registry is a module seam without an envelope op or
//!   wired-in transport interceptor. That wiring is the next slice after this
//!   probe; this test pins the contract the wiring must honour.
//! * The harness's catalog is registered in-process, not over the wire. There
//!   is no `Op::Register` variant for BYOH catalog handshake yet (the existing
//!   `RegisterOp::ExternalAgent` is Pattern B per `sera-types/src/envelope.rs`
//!   line 121). Carrying the catalog inside the Stdio NDJSON channel is a
//!   follow-up.
//!
//! Together these two limitations are exactly what M1 AC2 still needs after
//! this probe. See the OMC report for the bead update text.

#![cfg(feature = "stdio")]

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};
use tokio_stream::StreamExt;
use uuid::Uuid;

use sera_gateway::byoh_catalog::{GatewayToolDecl, HarnessCatalog, HarnessToolDecl};
use sera_gateway::byoh_registration::{ByohCatalogRegistry, OutboundCallDecision};
use sera_gateway::envelope::{EventMsg, Op, Submission};
use sera_gateway::transport::AppServerTransport;

/// Bash one-shot that consumes a single NDJSON submission and replies with
/// the AC2 Pattern A probe sequence: a `ToolCallBegin` whose `tool` is the
/// harness-internal alias `Read`, followed by `TurnCompleted`.
///
/// The `submission_id` field on the events is fixed; this layer does not
/// require correlation (sera-5mbr) and the AC2 invariant is about tool
/// routing, not envelope correlation.
const HERMES_STDIO_PROBE_SCRIPT: &str = r#"
read line
echo '{"id":"00000000-0000-0000-0000-000000000010","submission_id":"00000000-0000-0000-0000-000000000000","msg":{"type":"tool_call_begin","turn_id":"00000000-0000-0000-0000-000000000020","call_id":"call-1","tool":"Read","arguments":{"path":"/etc/passwd"}},"timestamp":"2024-01-01T00:00:00Z"}'
echo '{"id":"00000000-0000-0000-0000-000000000011","submission_id":"00000000-0000-0000-0000-000000000000","msg":{"type":"turn_completed","turn_id":"00000000-0000-0000-0000-000000000020","tokens":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}},"timestamp":"2024-01-01T00:00:00Z"}'
"#;

fn user_turn_submission() -> Submission {
    Submission {
        id: Uuid::new_v4(),
        op: Op::UserTurn {
            items: vec![json!({"role":"user","content":"read /etc/passwd"})],
            cwd: None,
            approval_policy: None,
            sandbox_policy: None,
            model_override: None,
            effort: None,
            final_output_schema: None,
        },
        trace: Default::default(),
        change_artifact: None,
        session_key: Some("hermes-pattern-a-session".to_string()),
        parent_session_key: None,
        parent_task_id: None,
    }
}

/// Records each gateway-side canonical dispatch — the only legitimate
/// destination for a tool call when the harness's emitted name collides
/// with a gateway canonical alias.
#[derive(Default)]
struct GatewayDispatchRecorder {
    calls: Mutex<Vec<(String, Value)>>,
    count: AtomicUsize,
}

impl GatewayDispatchRecorder {
    fn dispatch(&self, canonical: &str, args: Value) {
        self.calls
            .lock()
            .unwrap()
            .push((canonical.to_string(), args));
        self.count.fetch_add(1, Ordering::SeqCst);
    }
    fn count(&self) -> usize {
        self.count.load(Ordering::SeqCst)
    }
    fn calls(&self) -> Vec<(String, Value)> {
        self.calls.lock().unwrap().clone()
    }
}

/// Stand-in for the harness's own internal tool dispatcher. Any invocation
/// recorded here would prove a capability-laundering regression: the harness
/// emitted `Read`, the registry said "forward as canonical `read_file`", but
/// the harness ran its own `Read` instead.
#[derive(Default)]
struct HarnessInternalRecorder {
    count: AtomicUsize,
}

impl HarnessInternalRecorder {
    fn dispatch(&self, _name: &str, _args: Value) {
        self.count.fetch_add(1, Ordering::SeqCst);
    }
    fn count(&self) -> usize {
        self.count.load(Ordering::SeqCst)
    }
}

fn h(name: &str) -> HarnessToolDecl {
    HarnessToolDecl {
        name: name.to_string(),
        description: format!("hermes-acp internal {name}"),
        schema: json!({"type": "object"}),
    }
}

fn g(name: &str, aliases: &[&str]) -> GatewayToolDecl {
    GatewayToolDecl {
        name: name.to_string(),
        aliases: aliases.iter().map(|s| s.to_string()).collect(),
        description: format!("gateway canonical {name}"),
        schema: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
    }
}

/// **The AC2 Pattern A probe.** Compose the Stdio transport (sera-5mbr) and
/// the BYOH catalog registry (sera-w2dh) into one flow:
///
/// 1. Spawn a stdio child standing in for the Hermes ACP server.
/// 2. Pre-register that harness's catalog on the gateway side (internal
///    `Read` + `Bash`); seed the gateway with canonical `read_file`
///    aliasing `Read`.
/// 3. Send a UserTurn down the wire.
/// 4. Read the harness's emitted `ToolCallBegin` from the Stdio stream.
/// 5. Hand the emitted tool name to the registry; act on the decision.
/// 6. Assert the call landed at the gateway as canonical `read_file` and
///    the harness internal was bypassed.
#[tokio::test]
async fn hermes_pattern_a_stdio_tool_dispatch_routes_through_gateway() {
    // (1) Spawn the Hermes-ACP-shaped child via the public Stdio transport.
    let cfg = AppServerTransport::Stdio {
        command: "bash".to_string(),
        args: vec!["-c".to_string(), HERMES_STDIO_PROBE_SCRIPT.to_string()],
        env: HashMap::new(),
    };
    let transport = cfg
        .connect()
        .await
        .expect("Stdio variant must reach a real transport (sera-5mbr)");

    // (2) Pre-register the harness catalog on the gateway. Production wiring
    //     of registration over the Stdio NDJSON channel is the next slice;
    //     here the registry is exercised in-process so the dispatch decision
    //     under test is the BYOH reconciler's, not a synthetic stub's.
    let registry = ByohCatalogRegistry::new(vec![g("read_file", &["Read"])]);
    let reconciled = registry
        .register_harness(HarnessCatalog {
            harness_id: "hermes-acp".into(),
            tools: vec![h("Read"), h("Bash")],
        })
        .expect("registration ok (sera-w2dh)");

    // Sanity: the reconciled catalog the harness would install locally has
    // suppressed its internal `Read` and mapped it to canonical `read_file`.
    assert!(
        reconciled.is_suppressed_internal("Read"),
        "registration must mark harness 'Read' as suppressed"
    );
    assert_eq!(
        reconciled.name_map.get("Read"),
        Some(&"read_file".to_string()),
        "registration must map harness 'Read' -> canonical 'read_file'"
    );

    // (3) Drive the wire.
    transport
        .send_submission(user_turn_submission())
        .await
        .expect("send_submission");

    let mut stream = transport.recv_events().await.expect("recv_events");

    // (4) Pull frames until we see the harness's tool call. Bound the wait so
    //     a regression in the Stdio reader does not hang CI.
    let gateway = Arc::new(GatewayDispatchRecorder::default());
    let internal = Arc::new(HarnessInternalRecorder::default());

    let timeout = tokio::time::sleep(Duration::from_secs(5));
    tokio::pin!(timeout);

    let mut saw_tool_call = false;
    let mut saw_completed = false;

    loop {
        tokio::select! {
            _ = &mut timeout => panic!("timed out waiting for tool_call_begin / turn_completed"),
            evt = stream.next() => {
                let Some(evt) = evt else { break };
                match evt.msg {
                    EventMsg::ToolCallBegin { tool, arguments, .. } => {
                        saw_tool_call = true;

                        // (5) The gateway-side adapter consults the registry
                        //     for the routing decision. Production wiring of
                        //     this step into the Stdio event consumer is the
                        //     next slice; this test pins the contract.
                        let decision = registry
                            .prepare_outbound_call("hermes-acp", &tool)
                            .expect("registered harness must produce a decision");

                        match decision {
                            OutboundCallDecision::SuppressInternalAndForward { canonical } => {
                                gateway.dispatch(&canonical, arguments);
                            }
                            OutboundCallDecision::PassthroughToInternal => {
                                internal.dispatch(&tool, arguments);
                            }
                        }
                    }
                    EventMsg::TurnCompleted { .. } => {
                        saw_completed = true;
                        break;
                    }
                    _ => {}
                }
            }
        }
    }

    assert!(saw_tool_call, "harness must emit a ToolCallBegin over Stdio");
    assert!(saw_completed, "harness must close the turn cleanly");

    // (6) AC2 Pattern A invariant: tool dispatch returns through the gateway.
    assert_eq!(
        gateway.count(),
        1,
        "gateway must receive exactly one canonical invocation, got {}",
        gateway.count()
    );
    assert_eq!(
        internal.count(),
        0,
        "harness internal Read must be bypassed; got {} invocations (capability-laundering regression)",
        internal.count()
    );
    let calls = gateway.calls();
    assert_eq!(
        calls[0].0, "read_file",
        "gateway dispatch must use canonical name, not harness alias"
    );
    assert_eq!(
        calls[0].1["path"], "/etc/passwd",
        "tool arguments must reach gateway dispatch untouched"
    );

    transport.close().await.expect("close transport");
}
