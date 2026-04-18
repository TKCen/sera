//! Tests for AuditEntry serde roundtrip, signature field, hash sensitivity,
//! and AuditError display variants.

use sera_telemetry::audit::{AuditEntry, AuditError};

fn make_entry(class_uid: u32, payload: serde_json::Value, prev: [u8; 32]) -> AuditEntry {
    let this_hash = AuditEntry::compute_hash(class_uid, &payload, &prev);
    AuditEntry {
        ocsf_class_uid: class_uid,
        payload,
        prev_hash: prev,
        this_hash,
        signature: None,
    }
}

// ---------------------------------------------------------------------------
// AuditEntry serde roundtrip
// ---------------------------------------------------------------------------

#[test]
fn audit_entry_json_roundtrip_no_signature() {
    let entry = make_entry(2004, serde_json::json!({"action": "lane_start", "seq": 1}), [0u8; 32]);
    let json = serde_json::to_string(&entry).expect("serialize");
    let restored: AuditEntry = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(restored.ocsf_class_uid, entry.ocsf_class_uid);
    assert_eq!(restored.prev_hash, entry.prev_hash);
    assert_eq!(restored.this_hash, entry.this_hash);
    assert_eq!(restored.payload, entry.payload);
    assert!(restored.signature.is_none());
}

#[test]
fn audit_entry_json_roundtrip_with_signature() {
    let mut entry = make_entry(2004, serde_json::json!({"action": "signed_event"}), [0u8; 32]);
    entry.signature = Some(vec![0xde, 0xad, 0xbe, 0xef]);

    let json = serde_json::to_string(&entry).expect("serialize");
    let restored: AuditEntry = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(restored.signature, Some(vec![0xde, 0xad, 0xbe, 0xef]));
    assert_eq!(restored.this_hash, entry.this_hash);
}

// ---------------------------------------------------------------------------
// Hash sensitivity tests
// ---------------------------------------------------------------------------

#[test]
fn hash_changes_when_payload_content_changes() {
    let prev = [0u8; 32];
    let h1 = AuditEntry::compute_hash(2004, &serde_json::json!({"k": "a"}), &prev);
    let h2 = AuditEntry::compute_hash(2004, &serde_json::json!({"k": "b"}), &prev);
    assert_ne!(h1, h2, "different payload values must produce different hashes");
}

#[test]
fn hash_changes_when_payload_key_changes() {
    let prev = [0u8; 32];
    let h1 = AuditEntry::compute_hash(2004, &serde_json::json!({"key_a": 1}), &prev);
    let h2 = AuditEntry::compute_hash(2004, &serde_json::json!({"key_b": 1}), &prev);
    assert_ne!(h1, h2, "different payload keys must produce different hashes");
}

#[test]
fn hash_output_is_32_bytes() {
    let h = AuditEntry::compute_hash(2004, &serde_json::json!({}), &[0u8; 32]);
    // The type is [u8; 32] — this confirms the size at compile time,
    // but we also assert it dynamically for documentation clarity.
    assert_eq!(h.len(), 32);
}

#[test]
fn hash_of_empty_payload_is_not_zero() {
    let h = AuditEntry::compute_hash(2004, &serde_json::json!({}), &[0u8; 32]);
    assert_ne!(h, [0u8; 32], "SHA-256 of any real input must not be all zeros");
}

// ---------------------------------------------------------------------------
// AuditError display
// ---------------------------------------------------------------------------

#[test]
fn audit_error_not_initialised_display() {
    let e = AuditError::NotInitialised;
    assert!(
        e.to_string().contains("not initialised"),
        "display: '{}'",
        e
    );
}

#[test]
fn audit_error_chain_broken_display() {
    let e = AuditError::ChainBroken { index: 7 };
    let s = e.to_string();
    assert!(s.contains("broken"), "display: '{s}'");
    assert!(s.contains("7"), "display should include index: '{s}'");
}

#[test]
fn audit_error_write_display() {
    let e = AuditError::Write {
        reason: "disk full".to_string(),
    };
    let s = e.to_string();
    assert!(s.contains("write failed"), "display: '{s}'");
    assert!(s.contains("disk full"), "display should include reason: '{s}'");
}

#[test]
fn audit_error_debug_is_implemented() {
    let e = AuditError::ChainBroken { index: 3 };
    let d = format!("{e:?}");
    assert!(!d.is_empty());
}

// ---------------------------------------------------------------------------
// AuditEntry clone
// ---------------------------------------------------------------------------

#[test]
fn audit_entry_clone_is_independent() {
    let entry = make_entry(2004, serde_json::json!({"x": 1}), [0u8; 32]);
    let mut cloned = entry.clone();
    cloned.ocsf_class_uid = 9999;
    // Original should be unaffected
    assert_eq!(entry.ocsf_class_uid, 2004);
    assert_eq!(cloned.ocsf_class_uid, 9999);
}
