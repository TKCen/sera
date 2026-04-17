//! Integration test: multi-event audit chain integrity.
//!
//! Verifies that a 5-entry chain validates cleanly, and that mutating a single
//! entry causes `verify_chain` to return `ChainBroken` at the correct index.

use async_trait::async_trait;
use sera_telemetry::audit::{AuditBackend, AuditEntry, AuditError};
use std::sync::Mutex;

// ---------------------------------------------------------------------------
// Local MemBackend (mirrors the one in src/audit.rs but in test scope)
// ---------------------------------------------------------------------------

struct MemBackend {
    entries: Mutex<Vec<AuditEntry>>,
}

impl MemBackend {
    fn new() -> Self {
        Self {
            entries: Mutex::new(Vec::new()),
        }
    }

    fn mutate_payload(&self, index: usize, new_payload: serde_json::Value) {
        let mut entries = self.entries.lock().unwrap();
        entries[index].payload = new_payload;
        // this_hash is intentionally NOT updated — that's the tamper we detect.
    }
}

#[async_trait]
impl AuditBackend for MemBackend {
    async fn append(&self, entry: AuditEntry) -> Result<AuditEntry, AuditError> {
        self.entries.lock().unwrap().push(entry.clone());
        Ok(entry)
    }

    async fn verify_chain(&self) -> Result<usize, AuditError> {
        let entries = self.entries.lock().unwrap();
        let mut prev: [u8; 32] = [0u8; 32];
        for (i, entry) in entries.iter().enumerate() {
            let expected =
                AuditEntry::compute_hash(entry.ocsf_class_uid, &entry.payload, &prev);
            if expected != entry.this_hash {
                return Err(AuditError::ChainBroken { index: i });
            }
            prev = entry.this_hash;
        }
        Ok(entries.len())
    }
}

fn make_entry(class_uid: u32, seq: u32, prev: [u8; 32]) -> AuditEntry {
    let payload = serde_json::json!({ "seq": seq });
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
// Test 1: 5-entry clean chain validates completely
// ---------------------------------------------------------------------------

#[tokio::test]
async fn five_entry_chain_validates_clean() {
    let backend = MemBackend::new();
    let mut prev = [0u8; 32];
    for seq in 0..5u32 {
        let entry = make_entry(2004, seq, prev);
        prev = entry.this_hash;
        backend.append(entry).await.unwrap();
    }
    let count = backend.verify_chain().await.expect("chain should be valid");
    assert_eq!(count, 5);
}

// ---------------------------------------------------------------------------
// Test 2: mutating entry #2 payload breaks chain at index 2
// ---------------------------------------------------------------------------

#[tokio::test]
async fn mutation_at_index_2_breaks_chain_at_index_2() {
    let backend = MemBackend::new();
    let mut prev = [0u8; 32];
    for seq in 0..5u32 {
        let entry = make_entry(2004, seq, prev);
        prev = entry.this_hash;
        backend.append(entry).await.unwrap();
    }

    // Tamper entry at index 2 — change its payload without recomputing this_hash.
    backend.mutate_payload(2, serde_json::json!({ "seq": 999, "tampered": true }));

    let result = backend.verify_chain().await;
    assert!(
        matches!(result, Err(AuditError::ChainBroken { index: 2 })),
        "expected ChainBroken at index 2, got {result:?}"
    );
}
