//! [`BenchmarkSuite`] trait — the shared interface every benchmark adapter
//! (SWE-Bench Lite, TAU-bench, the SERA internal corpus) implements.
//!
//! The trait is deliberately synchronous and narrow. Adapters just enumerate
//! their tasks; the async runner is responsible for fanning tasks out to the
//! model and recording results. Keeping grading out of the trait means a new
//! suite only needs to describe how to load its YAML — grading rules are
//! encoded in the [`crate::task_def::Assertion`] list on each task.

use crate::error::EvalError;
use crate::task_def::TaskDef;

/// Canonical suite identifiers known to the crate. Extending this enum is a
/// semver decision — we explicitly want a closed set so misspelled suite
/// names fail loudly at parse time instead of silently running nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SuiteId {
    SeraInternal,
    SweBenchLite,
    TauBenchRetail,
    TauBenchAirline,
    /// Escape hatch for out-of-tree suites pointed at via a filesystem path.
    Custom(String),
}

impl SuiteId {
    pub fn as_str(&self) -> &str {
        match self {
            SuiteId::SeraInternal => "sera-internal",
            SuiteId::SweBenchLite => "swe-bench-lite",
            SuiteId::TauBenchRetail => "tau-bench-retail",
            SuiteId::TauBenchAirline => "tau-bench-airline",
            SuiteId::Custom(name) => name.as_str(),
        }
    }

    pub fn from_str(value: &str) -> Self {
        match value {
            "sera-internal" => SuiteId::SeraInternal,
            "swe-bench-lite" => SuiteId::SweBenchLite,
            "tau-bench-retail" => SuiteId::TauBenchRetail,
            "tau-bench-airline" => SuiteId::TauBenchAirline,
            other => SuiteId::Custom(other.to_string()),
        }
    }
}

/// A benchmark adapter. Loading is eager and synchronous — every known suite
/// is either a bundled directory of YAML or a cached download of a HF / git
/// artefact. Streaming is unnecessary at the scale we operate at (hundreds to
/// low thousands of tasks).
pub trait BenchmarkSuite {
    /// Which suite this adapter implements.
    fn id(&self) -> SuiteId;

    /// All tasks in the suite. Order is stable so task ids and row ids can
    /// be lined up run-over-run.
    fn tasks(&self) -> Result<Vec<TaskDef>, EvalError>;

    /// Optional filter for `--tasks <glob>` — default is a no-op pass-through.
    fn filter(&self, tasks: Vec<TaskDef>, glob: Option<&str>) -> Vec<TaskDef> {
        match glob {
            None => tasks,
            Some(pattern) => tasks
                .into_iter()
                .filter(|t| simple_glob_match(pattern, &t.id))
                .collect(),
        }
    }
}

/// Minimal `*` glob — no `**`, no character classes, just `?` and `*`.
/// Enough for task-id filtering on the CLI. If we outgrow this we switch to
/// the `globset` crate, but right now adding a dep for `*foo*` is overkill.
fn simple_glob_match(pattern: &str, value: &str) -> bool {
    fn inner(p: &[u8], v: &[u8]) -> bool {
        match (p.first(), v.first()) {
            (None, None) => true,
            (Some(b'*'), _) => {
                // Consume zero-or-more characters.
                inner(&p[1..], v) || (!v.is_empty() && inner(p, &v[1..]))
            }
            (Some(b'?'), Some(_)) => inner(&p[1..], &v[1..]),
            (Some(a), Some(b)) if a == b => inner(&p[1..], &v[1..]),
            _ => false,
        }
    }
    inner(pattern.as_bytes(), value.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suite_id_round_trip() {
        for id in [
            SuiteId::SeraInternal,
            SuiteId::SweBenchLite,
            SuiteId::TauBenchRetail,
            SuiteId::TauBenchAirline,
        ] {
            assert_eq!(SuiteId::from_str(id.as_str()), id);
        }
        assert_eq!(
            SuiteId::from_str("my-private-suite"),
            SuiteId::Custom("my-private-suite".into())
        );
    }

    #[test]
    fn glob_matches_prefix_suffix_and_contains() {
        assert!(simple_glob_match("sera-*", "sera-internal-0001"));
        assert!(simple_glob_match("*memory*", "sera-internal-memory-1"));
        assert!(simple_glob_match("sera-internal-000?", "sera-internal-0003"));
        assert!(!simple_glob_match("sera-*", "tau-bench-1"));
        assert!(!simple_glob_match("sera-internal-000?", "sera-internal-00010"));
    }
}
