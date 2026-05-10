//! sera-plcv M1 AC2 (Pattern A, production-path slice): a wire-level Stdio
//! probe asserting that the gateway-owned [`ByohInterceptingTransport`]
//! decorator automatically applies [`ByohCatalogRegistry::prepare_outbound_call`]
//! to a harness-emitted `ToolCallBegin`, rewriting the alias `Read` to the
//! canonical `read_file` *before* it reaches any consumer.
//!
//! ## What this proves over PR #1252
//!
//! `tests/hermes_pattern_a_stdio_dispatch.rs` (PR #1252) composed
//! [`AppServerTransport::Stdio`] (sera-5mbr) with [`ByohCatalogRegistry`]
//! (sera-w2dh) but did the rewrite inline in the test loop. That proves the
//! contract is *consultable*; it does not prove the contract is *enforced* on
//! the production transport path. A real consumer that forgot to call
//! `prepare_outbound_call` would silently emit the harness alias to gateway
//! dispatch — capability laundering.
//!
//! This test moves the dispatch decision into a transport-layer wrapper:
//!
//! 1. Spawn the same Hermes-shaped Stdio child as PR #1252 (one
//!    `ToolCallBegin` with `tool="Read"`, then `TurnCompleted`).
//! 2. Pre-register the harness catalog on the gateway (canonical registration
//!    seam — wire-level `Op::Register(RegisterOp::ByohCatalog)` is a public
//!    protocol change tracked as a follow-up).
//! 3. Wrap the live Stdio transport with [`ByohInterceptingTransport`].
//! 4. Drive `recv_events` from the *wrapped* transport.
//! 5. Assert the consumer observes `tool == "read_file"` — no test-side call
//!    to the registry, because the wrapper applies the decision.
//!
//! ## What this still does not prove
//!
//! - Wire-level catalog registration over the Stdio NDJSON channel. Today
//!   the registry is seeded out-of-band; an `Op::Register(RegisterOp::ByohCatalog)`
//!   variant or an equivalent first-frame catalog handshake is the next slice.
//! - Harness-side suppression of internal implementations. The interceptor
//!   guarantees the gateway sees the canonical name; the harness must still
//!   honour `suppressed_internals` locally.

#![cfg(feature = "stdio")]

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use tokio_stream::StreamExt;
use uuid::Uuid;

use sera_gateway::byoh_catalog::{GatewayToolDecl, HarnessCatalog, HarnessToolDecl};
use sera_gateway::byoh_dispatch_interceptor::ByohInterceptingTransport;
use sera_gateway::byoh_registration::ByohCatalogRegistry;
use sera_gateway::envelope::{EventMsg, Op, Submission};
use sera_gateway::transport::AppServerTransport;

/// Bash one-shot identical in shape to the PR #1252 probe: emit a
/// `ToolCallBegin` whose `tool` is the harness-internal alias `Read`, then
/// close the turn. The interceptor under test must rewrite this frame.
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

/// **The AC2 production-path probe.** When the live Stdio transport is
/// wrapped with [`ByohInterceptingTransport`], a harness emitting alias
/// `Read` lands on the consumer as canonical `read_file`. The consumer makes
/// no call to the registry — that is the whole point.
#[tokio::test]
async fn intercepting_transport_rewrites_aliased_tool_call_begin_on_wire() {
    // (1) Spawn the Hermes-ACP-shaped child via the public Stdio transport.
    let cfg = AppServerTransport::Stdio {
        command: "bash".to_string(),
        args: vec!["-c".to_string(), HERMES_STDIO_PROBE_SCRIPT.to_string()],
        env: HashMap::new(),
    };
    let inner = cfg
        .connect()
        .await
        .expect("Stdio variant must reach a real transport (sera-5mbr)");

    // (2) Pre-register the harness catalog. This is the canonical
    //     registration seam (sera-w2dh); a wire-level
    //     `Op::Register(RegisterOp::ByohCatalog)` is a follow-up.
    let registry = Arc::new(ByohCatalogRegistry::new(vec![g("read_file", &["Read"])]));
    let reconciled = registry
        .register_harness(HarnessCatalog {
            harness_id: "hermes-acp".into(),
            tools: vec![h("Read"), h("Bash")],
        })
        .expect("registration ok (sera-w2dh)");
    assert!(
        reconciled.is_suppressed_internal("Read"),
        "registration must mark harness 'Read' as suppressed"
    );
    assert_eq!(
        reconciled.name_map.get("Read"),
        Some(&"read_file".to_string()),
        "registration must map harness 'Read' -> canonical 'read_file'"
    );

    // (3) Compose: wrap the live Stdio transport with the interceptor.
    let transport = ByohInterceptingTransport::new(inner, Arc::clone(&registry), "hermes-acp");

    // (4) Drive the wire. The consumer makes no registry call.
    use sera_gateway::transport::Transport;
    transport
        .send_submission(user_turn_submission())
        .await
        .expect("send_submission");

    let mut stream = transport.recv_events().await.expect("recv_events");

    let timeout = tokio::time::sleep(Duration::from_secs(5));
    tokio::pin!(timeout);

    let mut observed_tool: Option<String> = None;
    let mut observed_args: Option<serde_json::Value> = None;
    let mut saw_completed = false;

    loop {
        tokio::select! {
            _ = &mut timeout => panic!("timed out waiting for tool_call_begin / turn_completed"),
            evt = stream.next() => {
                let Some(evt) = evt else { break };
                match evt.msg {
                    EventMsg::ToolCallBegin { tool, arguments, .. } => {
                        observed_tool = Some(tool);
                        observed_args = Some(arguments);
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

    // (5) AC2 production-path invariant: the alias has been rewritten by the
    //     transport layer before the consumer saw it.
    let observed_tool = observed_tool.expect("must observe a ToolCallBegin frame");
    let observed_args = observed_args.expect("must observe arguments");
    assert_eq!(
        observed_tool, "read_file",
        "interceptor must rewrite alias 'Read' to canonical 'read_file' on the wire; got '{observed_tool}'"
    );
    assert_eq!(
        observed_args["path"], "/etc/passwd",
        "tool arguments must reach the consumer untouched"
    );
    assert!(saw_completed, "harness must close the turn cleanly");

    transport.close().await.expect("close transport");
}

/// Negative control: with a non-colliding tool name, the interceptor must not
/// alter the frame. Proves the wrapper is selective, not a blanket rewriter.
#[tokio::test]
async fn intercepting_transport_does_not_rewrite_non_colliding_tool() {
    const NON_COLLIDING_PROBE: &str = r#"
read line
echo '{"id":"00000000-0000-0000-0000-000000000010","submission_id":"00000000-0000-0000-0000-000000000000","msg":{"type":"tool_call_begin","turn_id":"00000000-0000-0000-0000-000000000020","call_id":"call-2","tool":"Bash","arguments":{"cmd":"ls"}},"timestamp":"2024-01-01T00:00:00Z"}'
echo '{"id":"00000000-0000-0000-0000-000000000011","submission_id":"00000000-0000-0000-0000-000000000000","msg":{"type":"turn_completed","turn_id":"00000000-0000-0000-0000-000000000020","tokens":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}},"timestamp":"2024-01-01T00:00:00Z"}'
"#;

    let cfg = AppServerTransport::Stdio {
        command: "bash".to_string(),
        args: vec!["-c".to_string(), NON_COLLIDING_PROBE.to_string()],
        env: HashMap::new(),
    };
    let inner = cfg.connect().await.expect("spawn");

    let registry = Arc::new(ByohCatalogRegistry::new(vec![g("read_file", &["Read"])]));
    registry
        .register_harness(HarnessCatalog {
            harness_id: "hermes-acp".into(),
            tools: vec![h("Read"), h("Bash")],
        })
        .expect("registration ok");

    let transport = ByohInterceptingTransport::new(inner, Arc::clone(&registry), "hermes-acp");

    use sera_gateway::transport::Transport;
    transport
        .send_submission(user_turn_submission())
        .await
        .expect("send_submission");
    let mut stream = transport.recv_events().await.expect("recv_events");

    let timeout = tokio::time::sleep(Duration::from_secs(5));
    tokio::pin!(timeout);

    let mut observed_tool: Option<String> = None;
    let mut saw_completed = false;
    loop {
        tokio::select! {
            _ = &mut timeout => panic!("timed out"),
            evt = stream.next() => {
                let Some(evt) = evt else { break };
                match evt.msg {
                    EventMsg::ToolCallBegin { tool, .. } => observed_tool = Some(tool),
                    EventMsg::TurnCompleted { .. } => { saw_completed = true; break; }
                    _ => {}
                }
            }
        }
    }

    assert_eq!(
        observed_tool.as_deref(),
        Some("Bash"),
        "non-colliding tool must pass through unchanged"
    );
    assert!(saw_completed);
    transport.close().await.expect("close");
}
