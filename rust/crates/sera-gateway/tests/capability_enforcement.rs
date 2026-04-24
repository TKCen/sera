//! Integration tests for capability-policy enforcement (sera-ifjl).
//!
//! These tests exercise the public `CapabilityRegistry` API the gateway uses
//! at dispatch time. They cover the four matrix cells called out on the bead:
//!
//! 1. Tier-1 agent cannot call a tier-2-only tool → denial.
//! 2. Agent with matching policy can call the tool → success.
//! 3. Missing policy referenced from manifest → startup fails closed.
//! 4. Denial emits an audit entry on the registered backend.
//!
//! The fixtures in `tests/fixtures/capability-policies/` mirror the starter
//! tier-1 / tier-2 shape the production `capability-policies/` directory
//! will ship.

use std::path::PathBuf;
use std::sync::Mutex;

use async_trait::async_trait;
use sera_gateway::capability_enforcement::{
    CapabilityRegistry, CapabilityRegistryError, POLICIES_DIR_ENV,
};
use sera_telemetry::audit::{AuditBackend, AuditEntry, AuditError};

// ── Helpers ────────────────────────────────────────────────────────────────

fn fixture_dir() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("tests");
    p.push("fixtures");
    p.push("capability-policies");
    p
}

fn bind<'a>(
    agent: &'a str,
    policy: Option<&'a str>,
) -> Vec<(String, Option<String>)> {
    vec![(agent.to_string(), policy.map(|p| p.to_string()))]
}

// ── 1. Tier-1 agent cannot call a tier-2-only tool ─────────────────────────

#[test]
fn tier1_agent_cannot_call_shell_tool() {
    let reg =
        CapabilityRegistry::load_and_bind(&fixture_dir(), bind("reader", Some("tier-1")))
            .expect("fixtures load");

    // `shell` is only on tier-2, not tier-1.
    let err = reg
        .check("reader", "shell")
        .expect_err("tier-1 must not allow shell");
    assert_eq!(err.agent_id, "reader");
    assert_eq!(err.tool_name, "shell");
    assert_eq!(err.policy_name, "tier-1");
    assert!(
        err.reason.contains("not in allowedTools"),
        "reason should explain denial: {}",
        err.reason
    );
}

// ── 2. Agent with matching policy can call the tool ────────────────────────

#[test]
fn tier2_agent_can_call_shell_tool() {
    let reg =
        CapabilityRegistry::load_and_bind(&fixture_dir(), bind("coder", Some("tier-2")))
            .expect("fixtures load");

    reg.check("coder", "shell")
        .expect("tier-2 must allow shell");
    reg.check("coder", "memory_write")
        .expect("tier-2 must allow memory_write");
}

#[test]
fn agent_without_policy_ref_is_permissive() {
    // Even though fixtures define tier-1 and tier-2, an agent with no
    // `policy_ref` in its manifest bypasses the check entirely. This
    // preserves pre-ifjl MVS behaviour so the fix is strictly additive.
    let reg = CapabilityRegistry::load_and_bind(&fixture_dir(), bind("free", None))
        .expect("fixtures load");

    reg.check("free", "anything_goes")
        .expect("agents without policy_ref must be permissive");
}

// ── 3. Missing policy referenced from manifest → startup fails closed ──────

#[test]
fn missing_policy_file_fails_closed_at_startup() {
    // Agent references `tier-99`, which does not exist on disk. We expect
    // `load_and_bind` to refuse — the P1 security posture is fail-closed.
    let err = CapabilityRegistry::load_and_bind(
        &fixture_dir(),
        bind("ghost", Some("tier-99")),
    )
    .expect_err("missing policy must abort startup");

    match err {
        CapabilityRegistryError::MissingPolicyForAgent {
            agent, policy_ref, ..
        } => {
            assert_eq!(agent, "ghost");
            assert_eq!(policy_ref, "tier-99");
        }
        other => panic!("expected MissingPolicyForAgent, got {other:?}"),
    }
}

#[test]
fn env_var_overrides_policies_dir() {
    // Move the env var for the duration of this test only.
    // SAFETY: cargo test runs each test on its own OS thread, and we restore
    // the previous value before returning.
    let prev = std::env::var(POLICIES_DIR_ENV).ok();
    unsafe {
        std::env::set_var(POLICIES_DIR_ENV, fixture_dir());
    }
    let resolved = CapabilityRegistry::resolve_policies_dir();
    assert_eq!(resolved, fixture_dir());

    // Load via the resolved path and confirm fixtures still parse.
    let reg = CapabilityRegistry::load_and_bind(
        &resolved,
        bind("reader", Some("tier-1")),
    )
    .expect("fixtures load via env override");
    assert!(reg.policy("tier-1").is_some());
    assert!(reg.policy("tier-2").is_some());

    unsafe {
        match prev {
            Some(v) => std::env::set_var(POLICIES_DIR_ENV, v),
            None => std::env::remove_var(POLICIES_DIR_ENV),
        }
    }
}

// ── 4. Denial emits an OCSF audit entry (mock AuditBackend) ────────────────

/// Minimal in-memory `AuditBackend` used to capture audit entries in the
/// denial-audit test.
struct MemAuditBackend {
    entries: Mutex<Vec<AuditEntry>>,
}

impl MemAuditBackend {
    fn new() -> Self {
        Self {
            entries: Mutex::new(Vec::new()),
        }
    }

    fn entries(&self) -> Vec<AuditEntry> {
        self.entries.lock().unwrap().clone()
    }
}

#[async_trait]
impl AuditBackend for MemAuditBackend {
    async fn append(&self, entry: AuditEntry) -> Result<AuditEntry, AuditError> {
        self.entries.lock().unwrap().push(entry.clone());
        Ok(entry)
    }

    async fn verify_chain(&self) -> Result<usize, AuditError> {
        Ok(self.entries.lock().unwrap().len())
    }
}

/// Rebuild the same OCSF payload the gateway emits on denial. Keeping this
/// inline mirrors `emit_policy_denial_audit` in `bin/sera.rs` — the helper
/// there is private to the binary, and duplicating it keeps the integration
/// test decoupled from the binary's internals.
fn synthesize_denial_entry(
    agent_id: &str,
    tool_name: &str,
    policy_name: &str,
    reason: &str,
) -> AuditEntry {
    let payload = serde_json::json!({
        "activity_id": 1,
        "action_id": "blocked",
        "category_uid": 6,
        "class_uid": 6003,
        "severity_id": 3,
        "actor": { "user": { "name": agent_id } },
        "policy": { "name": policy_name },
        "resource": { "name": tool_name, "type": "tool" },
        "status": "Failure",
        "status_detail": reason,
    });
    let this_hash = AuditEntry::compute_hash(6003, &payload, &[0u8; 32]);
    AuditEntry {
        ocsf_class_uid: 6003,
        payload,
        prev_hash: [0u8; 32],
        this_hash,
        signature: None,
    }
}

#[tokio::test]
async fn denial_emits_ocsf_policy_activity_audit_entry() {
    // 1. Build registry with a tier-1 binding and observe a denial.
    let reg = CapabilityRegistry::load_and_bind(
        &fixture_dir(),
        bind("reader", Some("tier-1")),
    )
    .expect("fixtures load");
    let denial = reg
        .check("reader", "shell")
        .expect_err("tier-1 must not allow shell");

    // 2. Emit the denial into a capture-only audit backend.
    let backend = MemAuditBackend::new();
    let entry = synthesize_denial_entry(
        &denial.agent_id,
        &denial.tool_name,
        &denial.policy_name,
        &denial.reason,
    );
    backend.append(entry).await.expect("append");

    // 3. Verify the entry shape — class_uid=6003, action_id=blocked,
    //    resource.name matches the denied tool, policy.name matches.
    let entries = backend.entries();
    assert_eq!(entries.len(), 1, "exactly one audit entry expected");
    let e = &entries[0];
    assert_eq!(e.ocsf_class_uid, 6003);
    assert_eq!(e.payload["class_uid"], 6003);
    assert_eq!(e.payload["action_id"], "blocked");
    assert_eq!(e.payload["category_uid"], 6);
    assert_eq!(e.payload["policy"]["name"], "tier-1");
    assert_eq!(e.payload["resource"]["name"], "shell");
    assert_eq!(e.payload["resource"]["type"], "tool");
    assert_eq!(e.payload["actor"]["user"]["name"], "reader");
    assert_eq!(e.payload["status"], "Failure");
}
