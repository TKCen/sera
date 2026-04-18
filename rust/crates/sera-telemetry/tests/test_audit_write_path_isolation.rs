//! Verify audit_append returns NotInitialised when no backend is set.
//!
//! NOTE: This test must run in its own process (separate integration test binary)
//! because the OnceCell global is set in other test binaries.  Cargo integration
//! tests in `tests/` each get their own process, so isolation is guaranteed.

use sera_telemetry::audit::{AuditEntry, AuditError, audit_append};

#[tokio::test]
async fn audit_append_without_backend_returns_not_initialised() {
    let payload = serde_json::json!({"event": "test"});
    let prev_hash = [0u8; 32];
    let this_hash = AuditEntry::compute_hash(2004, &payload, &prev_hash);

    let entry = AuditEntry {
        ocsf_class_uid: 2004,
        payload,
        prev_hash,
        this_hash,
        signature: None,
    };

    let result = audit_append(entry).await;
    assert!(
        matches!(result, Err(AuditError::NotInitialised)),
        "expected NotInitialised, got {result:?}",
    );
}
