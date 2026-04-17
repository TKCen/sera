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
//! - [`task`] — [`WorkflowTask`], [`WorkflowTaskId`], and related task types.
//! - [`ready`] — [`ready_tasks`] algorithm and [`dependency_closure`].
//! - [`claim`] — atomic claim protocol: [`claim_task`], [`confirm_claim`], [`StaleClaimReaper`].
//! - [`termination`] — termination triad: [`check_termination`], [`TerminationConfig`].

pub mod claim;
pub mod coordination;
pub mod dreaming;
pub mod source_ingest;
pub mod error;
pub mod ready;
pub mod registry;
pub mod scc;
pub mod schedule;
pub mod session_key;
pub mod sleeptime;
pub mod task;
pub mod termination;
pub mod types;

// Convenient re-exports — legacy modules.
pub use dreaming::{
    DeepSleepConfig, DreamCandidate, DreamingConfig, DreamingPhases, DreamingWeights,
    LightSleepConfig, RemSleepConfig,
};
pub use error::WorkflowError;
#[allow(deprecated)]
pub use registry::WorkflowRegistry;
pub use session_key::workflow_session_key;
pub use types::{
    CronSchedule, EventPattern, ThresholdCondition, ThresholdOperator, WorkflowDef,
    WorkflowTrigger,
};

// Re-exports — new Phase 0 types.
pub use claim::{claim_task, confirm_claim, ClaimError, ClaimToken, StaleClaimReaper};
#[allow(deprecated)]
pub use ready::{
    dependency_closure, is_gh_run_ready, is_human_ready, is_timer_ready, ready_tasks,
    ready_tasks_with_context, ready_tasks_with_hitl, topological_sort, CyclicDependency,
    GhRunLookup, HitlLookup, NoopGhRunLookup, NoopHitlLookup, ReadyContext,
};
pub use task::{
    AwaitType, DependencyType, GhRunId, GhRunStatus, WorkflowSentinel, WorkflowTask,
    WorkflowTaskDependency, WorkflowTaskId, WorkflowTaskStatus, WorkflowTaskType,
};
pub use termination::{
    check_termination, TerminationConfig, TerminationReason, TerminationState,
    WorkflowTermination,
};

// Re-exports — Circle coordination (SPEC-circles).
pub use coordination::{
    AggregatedResult, AggregationError, AllComplete, CircleMemory, ConcurrencyPolicy,
    ConcurrencyScheduler, ConvergenceConfig, ConvergenceState, CoordResult, CoordTask,
    CoordinationError, CoordinationPolicy, Coordinator, Custom, ExecFn, FirstSuccess, Majority,
    Outcome, ParticipantId, ResultAggregator, WorkflowMemoryManager,
};
pub use scc::{cyclic_sccs, has_cycle, tarjan_scc, Scc};

// Re-exports — Sleeptime Memory Consolidation (SPEC-memory §2b / sera-40o).
pub use sleeptime::{
    ConsolidationError, ConsolidationPhase, ConsolidationReport, ConsolidationResult,
    IdleDetector, SleeptimeConfig, SleeptimeConsolidator,
};

#[cfg(test)]
mod tests;
