//! sera-telemetry — SERA 2.0 telemetry primitives.
//!
//! Provides:
//! - OCSF v1.7.0 audit events with Merkle hash-chain backend (`audit`)
//! - Hierarchical event emitter namespace tree (`emitter`)
//! - Generation identity markers (`generation`)
//! - Lane failure classification (`lane_failure`)
//! - OpenTelemetry triad initialisation helpers (`otel`)
//! - Lane commit provenance and run evidence (`provenance`)

pub mod audit;
pub mod emitter;
pub mod generation;
pub mod lane_failure;
pub mod otel;
pub mod provenance;
pub mod provider_credentials;
pub mod sera_errors;

pub use audit::{AuditBackend, AuditEntry, AuditError, audit_append, set_audit_backend};
pub use emitter::{Emitter, EventMeta};
pub use generation::{BuildIdentity, GenerationLabel, GenerationMarker};
pub use lane_failure::LaneFailureClass;
pub use otel::{OtelInitError, init_otel};
pub use provenance::{CostRecord, LaneCommitProvenance, RunEvidence};
pub use provider_credentials::{
    record as record_credential_outcome, snapshot as credential_snapshot, CounterSnapshot,
    CredentialOutcome,
};
