//! Integration test for the BYOH registration handshake (bead sera-w2dh,
//! registration slice).
//!
//! This is the named acceptance test on the bead:
//!
//! > harness with internal `Read` + gateway `read_file` produces exactly one
//! > `read_file` invocation through gateway dispatch; harness internal `Read`
//! > is bypassed.
//!
//! It exercises the full slice end-to-end:
//!
//! 1. Build a [`ByohCatalogRegistry`] seeded with a gateway canonical
//!    `read_file` tool that lists `Read` as an alias.
//! 2. Register a harness whose own catalog includes an internal `Read` and a
//!    non-colliding `Bash` tool — i.e. the poison case.
//! 3. Drive the harness-side dispatch path against the registry as a real
//!    BYOH harness would: a model-emitted `tool_call("Read", …)` hits
//!    [`ByohCatalogRegistry::prepare_outbound_call`], the harness honours the
//!    [`OutboundCallDecision::SuppressInternalAndForward`] result, and a
//!    single canonical `read_file` invocation lands on the gateway.
//! 4. Assert: gateway dispatch counter == 1, harness-internal `Read` counter
//!    == 0.
//!
//! The test fixture stands in for the real SQ/EQ wiring (sera-sq/eq seam):
//! it is the same dispatch decision the production transport will make once
//! the public registration op lands. Keeping the test at the module seam
//! avoids dragging in `AppState`, the runtime child, or HTTP — a regression
//! in any of those would mask, not catch, the laundering bug this test
//! exists to prevent.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

use sera_gateway::byoh_catalog::{GatewayToolDecl, HarnessCatalog, HarnessToolDecl};
use sera_gateway::byoh_registration::{
    ByohCatalogRegistry, DispatchError, OutboundCallDecision,
};

/// Records each gateway-side canonical dispatch — the *only* legitimate
/// destination for tool calls under the reconciled catalog when a collision
/// exists.
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
/// recorded here would prove a capability-laundering regression.
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

/// Faithfully reproduces what a BYOH harness must do when its model emits a
/// tool call: ask the registry for a decision and honour it.
fn drive_harness_emitted_tool_call(
    registry: &ByohCatalogRegistry,
    harness_id: &str,
    name: &str,
    args: Value,
    gateway: &GatewayDispatchRecorder,
    internal: &HarnessInternalRecorder,
) -> Result<(), DispatchError> {
    match registry.prepare_outbound_call(harness_id, name)? {
        OutboundCallDecision::SuppressInternalAndForward { canonical } => {
            gateway.dispatch(&canonical, args);
        }
        OutboundCallDecision::PassthroughToInternal => {
            internal.dispatch(name, args);
        }
    }
    Ok(())
}

fn h(name: &str) -> HarnessToolDecl {
    HarnessToolDecl {
        name: name.to_string(),
        description: format!("harness {name}"),
        schema: json!({"type": "object"}),
    }
}

fn g(name: &str, aliases: &[&str]) -> GatewayToolDecl {
    GatewayToolDecl {
        name: name.to_string(),
        aliases: aliases.iter().map(|s| s.to_string()).collect(),
        description: format!("gateway {name}"),
        schema: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
    }
}

/// **The poison-tool probe.** Bead sera-w2dh acceptance: a harness with
/// internal `Read` + gateway `read_file` (alias `Read`) routes the aliased
/// call back through gateway dispatch exactly once; the harness internal is
/// never invoked.
#[test]
fn registration_handshake_poison_probe_routes_aliased_read_to_gateway() {
    let registry = ByohCatalogRegistry::new(vec![g("read_file", &["Read"])]);

    let reconciled = registry
        .register_harness(HarnessCatalog {
            harness_id: "claude-code".into(),
            tools: vec![h("Read"), h("Bash")],
        })
        .expect("registration ok");

    // Handshake invariant 1: the registration response declares the
    // suppression. The harness must respect this when wiring its dispatcher.
    assert!(
        reconciled.is_suppressed_internal("Read"),
        "registration must mark harness 'Read' as suppressed"
    );
    assert_eq!(
        reconciled.name_map.get("Read"),
        Some(&"read_file".to_string()),
        "registration must map harness 'Read' -> canonical 'read_file'"
    );
    assert!(
        !reconciled.advertises("Read"),
        "harness 'Read' must NOT be advertised in effective_tools (bypass surface)"
    );

    // Handshake invariant 2: the gateway canonical is what the model sees.
    assert!(reconciled.advertises("read_file"));
    assert!(reconciled.advertises("Bash"));

    // Now drive the dispatch path. The model emits Read; harness consults the
    // registry. Internal Read MUST be bypassed; gateway dispatch MUST fire
    // exactly once.
    let gateway = Arc::new(GatewayDispatchRecorder::default());
    let internal = Arc::new(HarnessInternalRecorder::default());

    drive_harness_emitted_tool_call(
        &registry,
        "claude-code",
        "Read",
        json!({"path": "/etc/passwd"}),
        &gateway,
        &internal,
    )
    .expect("dispatch decision available");

    assert_eq!(
        gateway.count(),
        1,
        "gateway must receive exactly one canonical invocation, got {}",
        gateway.count()
    );
    assert_eq!(
        internal.count(),
        0,
        "harness internal Read must be bypassed; got {} invocations",
        internal.count()
    );
    let calls = gateway.calls();
    assert_eq!(calls[0].0, "read_file", "must dispatch as canonical name");
    assert_eq!(
        calls[0].1["path"], "/etc/passwd",
        "args must reach gateway untouched"
    );

    // Non-colliding harness tool stays local — sanity check that we did not
    // over-route.
    drive_harness_emitted_tool_call(
        &registry,
        "claude-code",
        "Bash",
        json!({"cmd": "ls"}),
        &gateway,
        &internal,
    )
    .unwrap();
    assert_eq!(
        gateway.count(),
        1,
        "non-colliding tool must not increment gateway dispatches"
    );
    assert_eq!(internal.count(), 1, "Bash routes to harness internal");
}

/// Repeated emissions of the same poison name accumulate: each one costs
/// exactly one gateway dispatch and zero internals. This is the regression
/// shape: "exactly one *per call*", not "exactly one *total ever*".
#[test]
fn repeated_aliased_calls_each_dispatch_once_through_gateway() {
    let registry = ByohCatalogRegistry::new(vec![g("read_file", &["Read"])]);
    registry
        .register_harness(HarnessCatalog {
            harness_id: "claude-code".into(),
            tools: vec![h("Read")],
        })
        .unwrap();

    let gateway = Arc::new(GatewayDispatchRecorder::default());
    let internal = Arc::new(HarnessInternalRecorder::default());

    for path in ["/a", "/b", "/c"] {
        drive_harness_emitted_tool_call(
            &registry,
            "claude-code",
            "Read",
            json!({"path": path}),
            &gateway,
            &internal,
        )
        .unwrap();
    }

    assert_eq!(gateway.count(), 3);
    assert_eq!(internal.count(), 0);
    for (canonical, _) in gateway.calls() {
        assert_eq!(canonical, "read_file");
    }
}

/// A harness that has not registered cannot dispatch through the gateway —
/// the registration handshake is mandatory. This guards against a path where
/// a misconfigured harness silently falls back to its own internal `Read`.
#[test]
fn unregistered_harness_dispatch_is_an_error_not_a_fallback() {
    let registry = ByohCatalogRegistry::new(vec![g("read_file", &["Read"])]);
    let gateway = Arc::new(GatewayDispatchRecorder::default());
    let internal = Arc::new(HarnessInternalRecorder::default());

    let err = drive_harness_emitted_tool_call(
        &registry,
        "phantom-harness",
        "Read",
        json!({"path": "/x"}),
        &gateway,
        &internal,
    )
    .expect_err("unregistered harness must error");
    assert_eq!(
        err,
        DispatchError::HarnessNotRegistered {
            harness_id: "phantom-harness".into()
        }
    );
    assert_eq!(gateway.count(), 0);
    assert_eq!(internal.count(), 0);
}
