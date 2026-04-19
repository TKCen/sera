//! SERA 2.0 Evaluation Harness.
//!
//! This crate is the scaffolding for measuring whether the SERA harness lifts
//! a local `qwen/qwen3.6-35b-a3b` model to parity with GPT-4-class frontier
//! models on agent-style tasks. See `docs/sera-eval-design.md` for the full
//! design, hypothesis, and success criteria.
//!
//! The crate is deliberately kept to **types + traits + SQLite schema** in
//! this initial PR. The real benchmark loaders, runner, and CLI subcommand
//! land in follow-up PRs. Keeping the stub small lets the design be reviewed
//! without being entangled with runner implementation details.
//!
//! ## Modules
//!
//! - [`task_def`] — [`TaskDef`], [`TaskResult`], [`MetricSet`], assertion kinds.
//! - [`suite`] — [`BenchmarkSuite`] trait; adapters implement it per suite.
//! - [`runner`] — [`EvalRunner`] skeleton and [`HarnessConfig`] enum.
//! - [`store`] — SQLite results store (`eval_runs`, `eval_task_results`, …).
//! - [`error`] — Crate-wide [`EvalError`].

pub mod error;
pub mod runner;
pub mod store;
pub mod suite;
pub mod task_def;

pub use error::EvalError;
pub use runner::{EvalRunner, HarnessConfig, RunHandle};
pub use store::{EvalStore, EVAL_SCHEMA_SQL};
pub use suite::{BenchmarkSuite, SuiteId};
pub use task_def::{
    Assertion, AssertionKind, MetricSet, TaskBudget, TaskDef, TaskInput, TaskResult,
    TaskSetup, Verdict,
};
