//! Generation marker — design-forward identity for a SERA binary generation.

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

/// Human-readable label for a SERA generation (e.g. `"mvs-0.1.0"`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GenerationLabel(pub String);

/// Cryptographic identity of a compiled SERA binary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildIdentity {
    /// Semantic version string.
    pub version: String,
    /// Short git commit SHA (hex).
    pub commit: String,
    /// UTC timestamp when this binary was built.
    pub build_time: OffsetDateTime,
    /// Fingerprint of the signing key (32 bytes).
    pub signer_fingerprint: [u8; 32],
    /// SHA-256 hash of the constitution file at build time.
    pub constitution_hash: [u8; 32],
}

/// Marks a running SERA process with its generation identity and start time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerationMarker {
    /// Human-readable generation label.
    pub label: GenerationLabel,
    /// Cryptographic identity of the running binary.
    pub binary_identity: BuildIdentity,
    /// UTC time at which this generation started.
    pub started_at: OffsetDateTime,
}
