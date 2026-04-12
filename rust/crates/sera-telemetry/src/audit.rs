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
