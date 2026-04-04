//! SERA Events — audit trail, lifecycle events, Centrifugo pub/sub types.
//!
//! Versioned event payloads for:
//! - Audit events (Merkle hash-chain)
//! - Agent lifecycle events
//! - Centrifugo publication payloads
//! - Job queue payloads
//! - Runtime task/result payloads

pub mod audit;
pub mod centrifugo;
pub mod channels;
pub mod error;

pub use audit::AuditHashChain;
pub use centrifugo::CentrifugoClient;
pub use channels::ChannelNamespace;
pub use error::{AuditVerifyError, CentrifugoError};
