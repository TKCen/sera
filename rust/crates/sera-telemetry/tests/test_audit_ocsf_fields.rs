//! Verify AuditEntry has the expected OCSF fields.

use sera_telemetry::audit::AuditEntry;

#[test]
fn audit_entry_has_ocsf_class_uid() {
    let payload = serde_json::json!({"message": "test event"});
    let prev_hash = [0u8; 32];
    let this_hash = AuditEntry::compute_hash(2004, &payload, &prev_hash);

    let entry = AuditEntry {
        ocsf_class_uid: 2004,
        payload: payload.clone(),
        prev_hash,
        this_hash,
        signature: None,
    };

    assert_eq!(entry.ocsf_class_uid, 2004);
    assert_eq!(entry.prev_hash, [0u8; 32]);
    assert_ne!(entry.this_hash, [0u8; 32]);
    assert_eq!(entry.payload, payload);
    assert!(entry.signature.is_none());
}

#[test]
fn class_2004_is_detection_finding() {
    // OCSF class 2004 = Detection Finding
    let payload = serde_json::json!({"finding": "constitutional_violation"});
    let prev = [1u8; 32];
    let hash = AuditEntry::compute_hash(2004, &payload, &prev);

    let entry = AuditEntry {
        ocsf_class_uid: 2004,
        payload,
        prev_hash: prev,
        this_hash: hash,
        signature: None,
    };

    assert_eq!(entry.ocsf_class_uid, 2004);
    assert_ne!(entry.this_hash, [0u8; 32]);
}

#[test]
fn hash_changes_with_prev_hash() {
    let payload = serde_json::json!({"x": 1});
    let prev_a = [0u8; 32];
    let prev_b = [1u8; 32];

    let hash_a = AuditEntry::compute_hash(2004, &payload, &prev_a);
    let hash_b = AuditEntry::compute_hash(2004, &payload, &prev_b);

    assert_ne!(hash_a, hash_b);
}
