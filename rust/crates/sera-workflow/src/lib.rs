//! `sera-workflow` — workflow engine for SERA.
//!
//! # Overview
//!
//! Implements the SERA workflow engine (SPEC-workflow-engine):
//!
//! - [`types`] — core types: [`WorkflowDef`], [`WorkflowTrigger`], etc.
//! - [`schedule`] — cron schedule validation and next-fire computation.
//! - [`registry`] — [`WorkflowRegistry`] for registering and querying workflows.
//! - [`session_key`] — canonical session key construction.
//! - [`dreaming`] — built-in dreaming workflow configuration and scoring.
//! - [`error`] — [`WorkflowError`] variants.

pub mod dreaming;
pub mod error;
pub mod registry;
pub mod schedule;
pub mod session_key;
pub mod types;

// Convenient re-exports.
pub use dreaming::{
    DeepSleepConfig, DreamCandidate, DreamingConfig, DreamingPhases, DreamingWeights,
    LightSleepConfig, RemSleepConfig,
};
pub use error::WorkflowError;
pub use registry::WorkflowRegistry;
pub use session_key::workflow_session_key;
pub use types::{
    CronSchedule, EventPattern, ThresholdCondition, ThresholdOperator, WorkflowDef,
    WorkflowTrigger,
};

#[cfg(test)]
mod tests;
