//! End-to-end CapabilityPolicy enforcement (sera-ifjl PR-A).
//!
//! Two single-layer test suites already exist:
//!
//! 1. `sera-gateway/tests/capability_enforcement.rs` exercises the
//!    registry-level `CapabilityRegistry::check()` path with fixture
//!    YAMLs but never reaches the runtime dispatcher.
//! 2. `sera-runtime/src/tools/dispatcher.rs#[cfg(test)]` exercises the
//!    dispatcher gate, but builds the registry with
//!    `CapabilityRegistry::from_parts` (in-memory). The manifest →
//!    `policy_ref` → YAML loader path is therefore not on the test trace.
//!
//! This file ties the two halves together. It loads the existing fixture
//! YAMLs the same way the gateway's `build_embedded_transport` does
//! (`CapabilityRegistry::load_and_bind` keyed by `AgentSpec.policy_ref`,
//! `bin/sera.rs:204-218`), attaches the resulting registry to a
//! `RegistryDispatcher` (the `with_capability_registry` seam at
//! `bin/sera.rs:273-274`), and asserts the full path:
//!
//! ```text
//!   manifest binding → fixture YAML load → dispatcher gate
//!                    → [sera-policy] denial / handler runs
//!                    → PolicyDenial shape consumed by enforce_tool_events
//! ```
//!
//! Audit emission lives in a private function in the binary
//! (`emit_policy_denial_audit`, `bin/sera.rs:3888`). Booting a full axum
//! server purely to observe one `audit_append` call would dwarf the
//! surgical PR-A scope, so the audit leg is asserted structurally: the
//! same `CapabilityRegistry::check()` call that `enforce_tool_events`
//! invokes at `bin/sera.rs:3869` returns a `PolicyDenial` whose four
//! fields populate the OCSF Policy Activity payload (class_uid=6003)
//! emitted at `bin/sera.rs:3897`. The shape of the audit entry itself is
//! already covered by `denial_emits_ocsf_policy_activity_audit_entry` in
//! `capability_enforcement.rs`.

use std::path::PathBuf;
use std::sync::Arc;

use sera_gateway::capability_enforcement::CapabilityRegistry;
use sera_runtime::tools::TraitToolRegistry;
use sera_runtime::tools::dispatcher::RegistryDispatcher;
use sera_runtime::turn::{ToolDispatcher, ToolError};
use sera_types::tool::ToolContext;

fn fixture_dir() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("tests");
    p.push("fixtures");
    p.push("capability-policies");
    p
}

/// Mirror `build_embedded_transport`'s manifest-binding step at
/// `bin/sera.rs:204-218`: take an `(agent_name, policy_ref)` pair and bind
/// it against the fixture policies dir. Fail-closed at the loader level is
/// already covered by `capability_enforcement.rs::missing_policy_file_…`.
fn load_for(agent: &str, policy_ref: &str) -> Arc<CapabilityRegistry> {
    let bindings = vec![(agent.to_string(), Some(policy_ref.to_string()))];
    let registry = CapabilityRegistry::load_and_bind(&fixture_dir(), bindings)
        .expect("fixture policies load");
    Arc::new(registry)
}

/// Reproduce the embedded-transport seam at `bin/sera.rs:273-274`: build a
/// dispatcher backed by built-in tools and attach the loaded registry to
/// `agent`.
fn dispatcher_for(agent: &str, registry: Arc<CapabilityRegistry>) -> RegistryDispatcher {
    RegistryDispatcher::new(Arc::new(TraitToolRegistry::with_builtins()))
        .with_capability_registry(registry, agent.to_string())
}

fn tool_call(id: &str, name: &str, args: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "type": "function",
        "function": { "name": name, "arguments": args },
    })
}

// ── Denial path: tier-1 agent attempts a tier-2-only tool ─────────────────

#[tokio::test]
async fn tier1_denies_shell_pre_dispatch() {
    // bob's manifest binds `policyRef: tier-1`; tier-1 only allows
    // memory_read. A `shell` tool call must be denied by the dispatcher
    // *before* `TraitToolRegistry::execute` runs — i.e. the handler is
    // never invoked, so no shell process is spawned.
    let registry = load_for("bob", "tier-1");
    let dispatcher = dispatcher_for("bob", Arc::clone(&registry));

    let call = tool_call("call-deny-1", "shell", "{\"command\":\"echo nope\"}");
    let err = dispatcher
        .dispatch(&call, &ToolContext::default())
        .await
        .expect_err("tier-1 must deny shell");

    match err {
        ToolError::PolicyDenied(msg) => {
            assert!(
                msg.starts_with("[sera-policy] tool 'shell' denied"),
                "denial must use the [sera-policy] prefix consumed by \
                 enforce_tool_events; got: {msg}",
            );
            assert!(
                msg.contains("'tier-1'"),
                "denial must name the bound policy: {msg}",
            );
        }
        // The capability gate must fire before TraitToolRegistry::execute,
        // so `NotFound` here would mean the dispatcher reached the registry
        // lookup before the policy check — i.e. ordering regression.
        other => panic!(
            "tier-1 must produce PolicyDenied (handler must NOT run); got {other:?}",
        ),
    }
}

// ── Allow path: tier-2 agent runs a built-in normally ─────────────────────

#[tokio::test]
async fn tier2_allows_file_list_handler_runs() {
    // alice's manifest binds `policyRef: tier-2`; tier-2 includes
    // `file-list` (a built-in Read tool). The dispatcher must let the
    // call through and the handler must run, returning the canonical
    // tool-result envelope.
    let registry = load_for("alice", "tier-2");
    let dispatcher = dispatcher_for("alice", Arc::clone(&registry));

    let call = tool_call("call-allow-1", "file-list", "{\"path\":\"/tmp\"}");
    let result = dispatcher
        .dispatch(&call, &ToolContext::default())
        .await
        .expect("tier-2 must allow file-list");

    assert_eq!(result["tool_call_id"], "call-allow-1");
    assert_eq!(result["role"], "tool");
    assert!(
        result["content"].is_string(),
        "allowed handler must produce a content string, got {result}",
    );
}

// ── Audit seam: enforce_tool_events sees a usable PolicyDenial ────────────

#[test]
fn gateway_enforce_tool_events_seam_sees_ocsf_fields() {
    // `enforce_tool_events` (`bin/sera.rs:3862`) re-runs `registry.check`
    // against the gateway's own copy of the registry for every observed
    // `tool_call_begin` and feeds the `PolicyDenial` into
    // `emit_policy_denial_audit` (`bin/sera.rs:3888`). The OCSF Policy
    // Activity payload at line 3897+ reads:
    //
    //   actor.user.name      ← denial.agent_id
    //   resource.name        ← denial.tool_name
    //   policy.name          ← denial.policy_name
    //   status_detail        ← denial.reason
    //
    // Asserting these fields against the fixture-loaded registry — the
    // exact shape `enforce_tool_events` consumes — closes the manifest →
    // dispatcher → audit-emit loop without owning a full gateway HTTP
    // fixture in this PR. The OCSF payload's outer shape (class_uid=6003,
    // action_id="blocked", category_uid=6) is already covered by
    // `capability_enforcement.rs::denial_emits_ocsf_policy_activity_audit_entry`.
    let registry = load_for("bob", "tier-1");
    let denial = registry
        .check("bob", "shell")
        .expect_err("tier-1 must deny shell");

    assert_eq!(denial.agent_id, "bob");
    assert_eq!(denial.tool_name, "shell");
    assert_eq!(denial.policy_name, "tier-1");
    assert!(
        denial.reason.contains("not in allowedTools"),
        "reason must be human-readable for the OCSF status_detail field: {}",
        denial.reason,
    );
}
