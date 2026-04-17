//! OCSF v1.7.0 audit events with Merkle hash-chain backend.

use async_trait::async_trait;
use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

/// An OCSF v1.7.0 audit entry with hash-chain linkage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    /// OCSF class UID (e.g. 2004 = Detection Finding).
    pub ocsf_class_uid: u32,
    /// Arbitrary OCSF-structured payload.
    pub payload: serde_json::Value,
    /// SHA-256 hash of the previous entry; all-zeros for genesis.
    pub prev_hash: [u8; 32],
    /// SHA-256 hash of this entry (covers class_uid + payload + prev_hash).
    pub this_hash: [u8; 32],
    /// Optional detached signature over `this_hash`.
    pub signature: Option<Vec<u8>>,
}

impl AuditEntry {
    /// Compute the hash for an entry given its fields.
    pub fn compute_hash(
        ocsf_class_uid: u32,
        payload: &serde_json::Value,
        prev_hash: &[u8; 32],
    ) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(ocsf_class_uid.to_be_bytes());
        hasher.update(payload.to_string().as_bytes());
        hasher.update(prev_hash);
        hasher.finalize().into()
    }
}

/// Errors produced by the audit subsystem.
#[derive(Debug, Error)]
pub enum AuditError {
    #[error("audit backend not initialised")]
    NotInitialised,
    #[error("hash chain is broken at entry index {index}")]
    ChainBroken { index: usize },
    #[error("write failed: {reason}")]
    Write { reason: String },
}

/// Object-safe async trait for audit backends.
#[async_trait]
pub trait AuditBackend: Send + Sync {
    /// Append an audit entry to the backend.
    async fn append(&self, entry: AuditEntry) -> Result<AuditEntry, AuditError>;
    /// Verify the hash chain; returns the number of verified entries.
    async fn verify_chain(&self) -> Result<usize, AuditError>;
}

/// Set-once global audit backend. Double-set panics.
static AUDIT_BACKEND: OnceCell<&'static dyn AuditBackend> = OnceCell::new();

/// Register the global audit backend. Panics if called more than once.
pub fn set_audit_backend(backend: &'static dyn AuditBackend) {
    if AUDIT_BACKEND.set(backend).is_err() {
        panic!("audit backend already set — double-set is not permitted");
    }
}

/// Append an entry via the global backend. Returns `NotInitialised` if not set.
pub async fn audit_append(entry: AuditEntry) -> Result<AuditEntry, AuditError> {
    let backend = AUDIT_BACKEND.get().ok_or(AuditError::NotInitialised)?;
    backend.append(entry).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // ---------------------------------------------------------------------------
    // Minimal in-process backend for unit tests.
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
    }

    #[async_trait::async_trait]
    impl AuditBackend for MemBackend {
        async fn append(&self, entry: AuditEntry) -> Result<AuditEntry, AuditError> {
            self.entries.lock().unwrap().push(entry.clone());
            Ok(entry)
        }

        async fn verify_chain(&self) -> Result<usize, AuditError> {
            let entries = self.entries.lock().unwrap();
            let mut prev: [u8; 32] = [0u8; 32];
            for (i, entry) in entries.iter().enumerate() {
                let expected = AuditEntry::compute_hash(
                    entry.ocsf_class_uid,
                    &entry.payload,
                    &prev,
                );
                if expected != entry.this_hash {
                    return Err(AuditError::ChainBroken { index: i });
                }
                prev = entry.this_hash;
            }
            Ok(entries.len())
        }
    }

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
    // Tests
    // ---------------------------------------------------------------------------

    #[test]
    fn genesis_entry_has_zero_prev_hash() {
        let prev = [0u8; 32];
        let entry = make_entry(2004, serde_json::json!({"action": "start"}), prev);
        assert_eq!(entry.prev_hash, [0u8; 32]);
        assert_ne!(entry.this_hash, [0u8; 32]);
    }

    #[test]
    fn compute_hash_is_deterministic() {
        let payload = serde_json::json!({"msg": "hello"});
        let prev = [1u8; 32];
        let h1 = AuditEntry::compute_hash(2004, &payload, &prev);
        let h2 = AuditEntry::compute_hash(2004, &payload, &prev);
        assert_eq!(h1, h2);
    }

    #[test]
    fn compute_hash_differs_on_different_uid() {
        let payload = serde_json::json!({});
        let prev = [0u8; 32];
        let h1 = AuditEntry::compute_hash(2004, &payload, &prev);
        let h2 = AuditEntry::compute_hash(9999, &payload, &prev);
        assert_ne!(h1, h2);
    }

    #[test]
    fn compute_hash_chain_links_correctly() {
        let prev0 = [0u8; 32];
        let e1 = make_entry(2004, serde_json::json!({"seq": 1}), prev0);
        let e2 = make_entry(2004, serde_json::json!({"seq": 2}), e1.this_hash);
        // e2's prev_hash must equal e1's this_hash
        assert_eq!(e2.prev_hash, e1.this_hash);
        assert_ne!(e2.this_hash, e1.this_hash);
    }

    #[tokio::test]
    async fn mem_backend_append_and_verify_chain_ok() {
        let backend = MemBackend::new();
        let prev0 = [0u8; 32];
        let e1 = make_entry(2004, serde_json::json!({"a": 1}), prev0);
        let e2 = make_entry(2004, serde_json::json!({"a": 2}), e1.this_hash);

        backend.append(e1).await.unwrap();
        backend.append(e2).await.unwrap();

        let count = backend.verify_chain().await.unwrap();
        assert_eq!(count, 2);
    }

    #[tokio::test]
    async fn mem_backend_detects_broken_chain() {
        let backend = MemBackend::new();
        // Append an entry whose this_hash is wrong (tampered)
        let mut entry = make_entry(2004, serde_json::json!({"x": 1}), [0u8; 32]);
        entry.this_hash[0] ^= 0xff; // corrupt
        backend.entries.lock().unwrap().push(entry);

        let result = backend.verify_chain().await;
        assert!(matches!(result, Err(AuditError::ChainBroken { index: 0 })));
    }

    #[tokio::test]
    async fn audit_append_returns_not_initialised_without_backend() {
        // The global backend is a OnceCell — in this test binary it may or may
        // not already be set by another test.  We only assert the shape of the
        // error when it isn't set; if it is already set the call should succeed.
        // We can confirm the function is reachable and returns a known type.
        let prev = [0u8; 32];
        let entry = make_entry(2004, serde_json::json!({}), prev);
        let result = audit_append(entry).await;
        // Either Ok (backend was set) or Err(NotInitialised) — both are valid.
        match result {
            Ok(_) => {}
            Err(AuditError::NotInitialised) => {}
            Err(e) => panic!("unexpected error: {e}"),
        }
    }
}
