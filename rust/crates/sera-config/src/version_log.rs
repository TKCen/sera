//! ConfigVersionLog — append-only, SHA-256 hash-chained version history.

use sha2::{Digest, Sha256};
use serde::{Deserialize, Serialize};

/// Opaque identifier for a change artifact.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ChangeArtifactId(pub String);

/// A single entry in the version log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigVersionEntry {
    /// Monotonically increasing version number (1-based after first append).
    pub version: u64,
    /// The change artifact that produced this entry.
    pub change_artifact: ChangeArtifactId,
    /// Optional opaque signature bytes (hex-encoded).
    pub signature: Option<String>,
    /// Hash of the previous entry (all-zeros hex string for genesis).
    pub prev_hash: String,
    /// Hash of this entry's canonical payload.
    pub this_hash: String,
    /// The config payload snapshot at this version.
    pub payload: serde_json::Value,
}

/// Errors from version log operations.
#[derive(Debug, thiserror::Error)]
pub enum VersionLogError {
    #[error("hash chain is broken at version {0}")]
    ChainBroken(u64),
    #[error("version log is empty")]
    Empty,
}

/// Append-only, hash-chained log of config versions.
pub struct ConfigVersionLog {
    entries: Vec<ConfigVersionEntry>,
}

/// 64-char all-zeros hex string representing the genesis prev_hash.
const ZERO_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";

impl ConfigVersionLog {
    /// Create an empty version log.
    pub fn new() -> Self {
        Self { entries: Vec::new() }
    }

    /// The hash of the most recently appended entry, or the zero hash if empty.
    pub fn tail_hash(&self) -> &str {
        self.entries.last().map(|e| e.this_hash.as_str()).unwrap_or(ZERO_HASH)
    }

    /// Current version number (0 = empty log).
    pub fn version(&self) -> u64 {
        self.entries.last().map(|e| e.version).unwrap_or(0)
    }

    /// Append a new entry to the log.
    ///
    /// The `prev_hash` is taken from `tail_hash()` automatically.
    pub fn append(
        &mut self,
        change_artifact: ChangeArtifactId,
        signature: Option<String>,
        payload: serde_json::Value,
    ) -> &ConfigVersionEntry {
        let prev_hash = self.tail_hash().to_string();
        let version = self.version() + 1;

        let this_hash = compute_hash(version, &change_artifact, &prev_hash, &payload);

        let entry = ConfigVersionEntry {
            version,
            change_artifact,
            signature,
            prev_hash,
            this_hash,
            payload,
        };
        self.entries.push(entry);
        self.entries.last().unwrap()
    }

    /// Verify the entire hash chain is intact.
    pub fn verify_chain(&self) -> Result<(), VersionLogError> {
        let mut expected_prev = ZERO_HASH.to_string();
        for entry in &self.entries {
            if entry.prev_hash != expected_prev {
                return Err(VersionLogError::ChainBroken(entry.version));
            }
            let recomputed = compute_hash(
                entry.version,
                &entry.change_artifact,
                &entry.prev_hash,
                &entry.payload,
            );
            if recomputed != entry.this_hash {
                return Err(VersionLogError::ChainBroken(entry.version));
            }
            expected_prev = entry.this_hash.clone();
        }
        Ok(())
    }

    /// Read-only view of all entries in append order.
    pub fn entries(&self) -> &[ConfigVersionEntry] {
        &self.entries
    }
}

impl Default for ConfigVersionLog {
    fn default() -> Self {
        Self::new()
    }
}

/// Compute the SHA-256 hash for a log entry.
fn compute_hash(
    version: u64,
    change_artifact: &ChangeArtifactId,
    prev_hash: &str,
    payload: &serde_json::Value,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(version.to_le_bytes());
    hasher.update(change_artifact.0.as_bytes());
    hasher.update(prev_hash.as_bytes());
    // Canonical JSON serialisation for deterministic hashing.
    let payload_bytes = serde_json::to_vec(payload).unwrap_or_default();
    hasher.update(&payload_bytes);
    hex::encode(hasher.finalize())
}
