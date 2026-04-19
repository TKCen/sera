//! Runner skeleton — [`EvalRunner`] coordinates a single run over a suite.
//!
//! This PR intentionally ships only the **skeleton**: the config enum, the
//! handle, and the shape of the `run()` entry point. The real fan-out over
//! tasks (invoking `sera-models::ModelProvider`, enforcing `TaskBudget`,
//! persisting `TaskResult`s) lands in a follow-up PR once the design is
//! agreed upon.

use serde::{Deserialize, Serialize};

use crate::error::EvalError;
use crate::suite::{BenchmarkSuite, SuiteId};

/// Which slice of the SERA harness this run exercises. The isolation matrix
/// in `docs/sera-eval-design.md` §4 maps these variants to expected behaviour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HarnessConfig {
    /// Bare OpenAI-style chat/completions — no SERA components.
    Raw,
    /// Raw + ContextEngine (condensers, transcript compression).
    WithContext,
    /// Raw + skills autoloading.
    WithSkills,
    /// Raw + SQLite FTS5 memory (+ pgvector when enabled).
    WithMemory,
    /// Everything — context + skills + memory + hooks + HITL wiring.
    Full,
}

impl HarnessConfig {
    pub fn as_str(self) -> &'static str {
        match self {
            HarnessConfig::Raw => "raw",
            HarnessConfig::WithContext => "+context",
            HarnessConfig::WithSkills => "+skills",
            HarnessConfig::WithMemory => "+memory",
            HarnessConfig::Full => "+full",
        }
    }
}

/// Parameters for a single evaluation run.
#[derive(Debug, Clone)]
pub struct RunRequest {
    /// Which suite to run.
    pub suite: SuiteId,
    /// OpenAI-compat model identifier (`qwen/qwen3.6-35b-a3b`, `gpt-4o`, …).
    pub model: String,
    /// Which harness slice to exercise.
    pub harness: HarnessConfig,
    /// Optional task-id glob filter (`sera-internal-memory-*`).
    pub task_filter: Option<String>,
    /// How many samples to run per task. Majority vote decides the verdict.
    /// Default 1 — only bump for high-variance frontier comparisons.
    pub n_samples: u32,
}

impl RunRequest {
    pub fn new(suite: SuiteId, model: impl Into<String>, harness: HarnessConfig) -> Self {
        Self {
            suite,
            model: model.into(),
            harness,
            task_filter: None,
            n_samples: 1,
        }
    }
}

/// Handle returned from starting a run. The real runner will stream task
/// results via a channel; for now we only expose the id so the store can
/// key results by it in tests.
#[derive(Debug, Clone)]
pub struct RunHandle {
    pub run_id: String,
}

/// Runner skeleton. `run()` panics as `unimplemented` until the follow-up PR
/// wires `sera-models` + the harness profiles. Tests exercise the type shape
/// and config plumbing only.
pub struct EvalRunner {
    // Fields intentionally omitted — the concrete runner will hold a
    // `Box<dyn ModelProvider>`, an `EvalStore`, and harness scaffolding.
}

impl Default for EvalRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl EvalRunner {
    pub fn new() -> Self {
        Self {}
    }

    /// Entry point for a run. Unimplemented in the stub.
    pub async fn run(
        &self,
        _request: RunRequest,
        _suite: &dyn BenchmarkSuite,
    ) -> Result<RunHandle, EvalError> {
        unimplemented!(
            "EvalRunner::run lands in a follow-up PR; see docs/sera-eval-design.md §9.1"
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn harness_config_labels_match_design_doc() {
        // These labels are load-bearing: they are persisted in
        // `eval_runs.harness` and rendered into reports. Pinning them here
        // makes accidental renames break this test rather than silently
        // invalidating historical runs.
        assert_eq!(HarnessConfig::Raw.as_str(), "raw");
        assert_eq!(HarnessConfig::WithContext.as_str(), "+context");
        assert_eq!(HarnessConfig::WithSkills.as_str(), "+skills");
        assert_eq!(HarnessConfig::WithMemory.as_str(), "+memory");
        assert_eq!(HarnessConfig::Full.as_str(), "+full");
    }

    #[test]
    fn run_request_defaults() {
        let r = RunRequest::new(SuiteId::SeraInternal, "qwen/qwen3.6-35b-a3b", HarnessConfig::Full);
        assert_eq!(r.n_samples, 1);
        assert!(r.task_filter.is_none());
        assert_eq!(r.harness, HarnessConfig::Full);
    }
}
