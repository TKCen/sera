//! sera-errors — shared error codes for SERA crates.
//!
//! Phase 0 scaffold. `SeraErrorCode` will be consumed by `QueueError` and
//! `SandboxError` in Phase 1 to provide a unified error taxonomy.

use serde::{Deserialize, Serialize};

/// Unified error code taxonomy for cross-crate error categorisation.
///
/// Phase 0: enum exists with initial variants. Phase 1 will extend as
/// crates adopt the shared taxonomy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum SeraErrorCode {
    /// An internal error with no specific classification.
    Internal,
    /// The requested resource was not found.
    NotFound,
    /// The caller is not authorised for the requested action.
    Unauthorized,
    /// A timeout occurred.
    Timeout,
    /// A configuration error.
    Configuration,
    /// A serialisation/deserialisation error.
    Serialization,
}
