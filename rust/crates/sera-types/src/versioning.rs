use serde::{Deserialize, Serialize};

/// Build identity for every event context (SPEC-versioning §4.6).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildIdentity {
    pub version: String,
    pub commit: [u8; 20],
    pub build_time: chrono::DateTime<chrono::Utc>,
    pub signer_fingerprint: [u8; 32],
    pub constitution_hash: [u8; 32],
}
