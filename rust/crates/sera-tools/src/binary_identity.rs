//! TOFU (Trust On First Use) binary identity verification via SHA-256.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use sha2::{Digest, Sha256};

/// A binary tracked by TOFU.
pub struct NetworkBinary {
    pub path: PathBuf,
    pub tofu_sha256: [u8; 32],
}

/// Errors from binary identity operations.
#[derive(Debug, thiserror::Error)]
pub enum BinaryIdentityError {
    #[error("hash mismatch: binary has been tampered")]
    HashMismatch,
    #[error("io error: {reason}")]
    IoError { reason: String },
}

/// Store that pins SHA-256 on first use and verifies on subsequent uses.
#[derive(Default)]
pub struct BinaryIdentity {
    store: RwLock<HashMap<PathBuf, [u8; 32]>>,
}

impl BinaryIdentity {
    pub fn new() -> Self {
        Self {
            store: RwLock::new(HashMap::new()),
        }
    }

    fn hash_file(path: &Path) -> Result<[u8; 32], BinaryIdentityError> {
        let bytes = std::fs::read(path).map_err(|e| BinaryIdentityError::IoError {
            reason: e.to_string(),
        })?;
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        Ok(hasher.finalize().into())
    }

    /// First call pins the SHA-256 of the binary at `path`.
    /// Subsequent calls verify it matches the pinned hash.
    pub fn verify_or_pin(&self, path: &Path) -> Result<(), BinaryIdentityError> {
        let hash = Self::hash_file(path)?;

        // Check under read lock first
        {
            let store = self.store.read().unwrap();
            if let Some(&pinned) = store.get(path) {
                if pinned == hash {
                    return Ok(());
                } else {
                    return Err(BinaryIdentityError::HashMismatch);
                }
            }
        }

        // Not found — pin under write lock
        let mut store = self.store.write().unwrap();
        // Double-check after acquiring write lock
        if let Some(&pinned) = store.get(path) {
            if pinned == hash {
                return Ok(());
            } else {
                return Err(BinaryIdentityError::HashMismatch);
            }
        }
        store.insert(path.to_path_buf(), hash);
        Ok(())
    }
}
