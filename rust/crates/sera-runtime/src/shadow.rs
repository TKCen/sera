//! Shadow session executor — runtime orchestrator for constitutional dry-runs.
//!
//! This module provides the runtime-side scaffold referenced by
//! `SPEC-self-evolution.md` §11 (shadow-session dry-run gate). The gateway's
//! `/api/evolve/evaluate` handler (see sera-meta / sera-gateway) describes
//! **what** should happen; this module defines **how** the run-time actually
//! evaluates it — spawn a parallel copy of the active session with a proposed
//! rule-set applied, feed it turn input, and return per-turn output so the
//! caller can diff it against the real session's output.
//!
//! This is **scaffolding**: the [`InMemoryShadowExecutor`] is a deterministic
//! mock that does not spawn an agent-runtime child process. Later waves wire
//! a real implementation into sera-gateway / agent-runtime.
//!
//! ## Scope
//!
//! - Public trait [`ShadowSessionExecutor`] with async `run()` method.
//! - Value types: [`ShadowTurnInput`], [`ShadowTurnOutput`], [`ShadowRuleset`],
//!   [`ShadowBudget`], [`ShadowError`].
//! - Reference implementation: [`InMemoryShadowExecutor`].
//! - Diff helper: [`diff`] + [`ShadowDiff`] + [`ShadowDelta`].
//!
//! No sera-gateway or sera-meta wiring lives here — the caller translates
//! between [`sera_types::evolution::ChangeArtifact`] payloads and
//! [`ShadowRuleset`].

use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;

// ── Value types ──────────────────────────────────────────────────────────────

/// One turn's worth of input fed into a shadow session.
///
/// The executor receives a `Vec<ShadowTurnInput>` and emits one
/// [`ShadowTurnOutput`] per turn. `context` carries arbitrary JSON that the
/// executor forwards to the shadow runtime — typically derived from the real
/// session's `TurnContext`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShadowTurnInput {
    pub user_message: String,
    pub context: serde_json::Value,
}

/// The per-turn output we compare between the real session and the shadow.
///
/// `tool_calls` is **normalised** — each entry is rendered as
/// `"{tool_name}:{sha256(arg_json)[..8]}"` so ordering differences can be
/// tolerated upstream without the caller needing to understand the full call
/// structure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShadowTurnOutput {
    pub assistant_text: String,
    pub tool_calls: Vec<String>,
    pub terminated: bool,
}

/// Rule-set override applied to the shadow session for the duration of the
/// dry-run. The payload is whatever sera-meta stores on a
/// [`sera_types::evolution::ChangeArtifact`]; the executor is responsible for
/// loading it into the shadow runtime's constitutional gate before the first
/// turn.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShadowRuleset {
    pub rules_json: serde_json::Value,
    pub description: String,
}

/// Resource bounds for one shadow run. The executor MUST fail with
/// [`ShadowError::BudgetExceeded`] if any bound is exceeded — partial output
/// is discarded.
#[derive(Debug, Clone, Copy)]
pub struct ShadowBudget {
    pub max_turns: u8,
    pub max_wall_time: Duration,
    pub max_tool_calls: u16,
}

impl Default for ShadowBudget {
    /// Conservative defaults: 3 turns, 30 s wall-clock, 20 tool calls.
    fn default() -> Self {
        Self {
            max_turns: 3,
            max_wall_time: Duration::from_secs(30),
            max_tool_calls: 20,
        }
    }
}

/// Errors emitted by a [`ShadowSessionExecutor::run`] call.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum ShadowError {
    #[error("shadow budget exceeded: {0}")]
    BudgetExceeded(String),
    #[error("shadow ruleset invalid: {0}")]
    RulesetInvalid(String),
    #[error("shadow executor failed: {0}")]
    ExecutorFailed(String),
}

// ── Executor trait ───────────────────────────────────────────────────────────

/// A runtime capable of executing one or more turns inside a shadow session
/// with the given [`ShadowRuleset`] applied.
///
/// Implementations MUST be deterministic for identical inputs when the
/// underlying runtime is deterministic — or at minimum structurally
/// deterministic (same tool calls, same termination) when the LLM is
/// nondeterministic, so the diff gate in SPEC-self-evolution §11.3 can apply.
#[async_trait]
pub trait ShadowSessionExecutor: Send + Sync {
    /// Run `turns` through a shadow session with `ruleset` applied.
    ///
    /// Returns one [`ShadowTurnOutput`] per input turn on success. On any
    /// budget or ruleset violation, returns [`ShadowError`] and no partial
    /// output.
    async fn run(
        &self,
        ruleset: &ShadowRuleset,
        turns: Vec<ShadowTurnInput>,
        budget: ShadowBudget,
    ) -> Result<Vec<ShadowTurnOutput>, ShadowError>;
}

// ── Reference implementation: InMemoryShadowExecutor ─────────────────────────

/// Deterministic in-memory shadow executor used for tests and for the first
/// gateway wiring of `/api/evolve/evaluate`.
///
/// Does **not** spawn a real agent-runtime child process — it runs a mock
/// turn-loop that:
///
/// - echoes each input message prefixed with the ruleset description,
/// - synthesises zero tool calls,
/// - never terminates (`terminated = false` on every turn), and
/// - rejects empty `rules_json` with [`ShadowError::RulesetInvalid`].
#[derive(Debug, Default, Clone)]
pub struct InMemoryShadowExecutor;

impl InMemoryShadowExecutor {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ShadowSessionExecutor for InMemoryShadowExecutor {
    async fn run(
        &self,
        ruleset: &ShadowRuleset,
        turns: Vec<ShadowTurnInput>,
        budget: ShadowBudget,
    ) -> Result<Vec<ShadowTurnOutput>, ShadowError> {
        // Ruleset validation: empty `rules_json` (null or empty object) is
        // rejected. We treat both as "no rules supplied".
        let empty = match &ruleset.rules_json {
            serde_json::Value::Null => true,
            serde_json::Value::Object(map) => map.is_empty(),
            _ => false,
        };
        if empty {
            return Err(ShadowError::RulesetInvalid(
                "rules_json is empty; expected at least one rule".to_string(),
            ));
        }

        // Budget: reject up-front if the requested turn count exceeds the
        // allowed max. This matches the SPEC contract that budget violations
        // short-circuit the run.
        if turns.len() > budget.max_turns as usize {
            return Err(ShadowError::BudgetExceeded(format!(
                "requested {} turns exceeds max_turns={}",
                turns.len(),
                budget.max_turns
            )));
        }

        let mut out = Vec::with_capacity(turns.len());
        for turn in turns {
            out.push(ShadowTurnOutput {
                assistant_text: format!(
                    "[shadow:{}] echo: {}",
                    ruleset.description, turn.user_message
                ),
                tool_calls: Vec::new(),
                terminated: false,
            });
        }
        Ok(out)
    }
}

// ── Diff helper ──────────────────────────────────────────────────────────────

/// Result of comparing one real turn's output against its shadow counterpart.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShadowDiff {
    pub matched: bool,
    pub deltas: Vec<ShadowDelta>,
}

/// A single field-level difference between real and shadow output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ShadowDelta {
    TextDiff {
        real: String,
        shadow: String,
    },
    ToolCallMismatch {
        real: Vec<String>,
        shadow: Vec<String>,
    },
    TerminationMismatch {
        real: bool,
        shadow: bool,
    },
}

/// Compare a real turn's output against the shadow's. Text comparison
/// normalises leading/trailing whitespace and collapses internal whitespace
/// runs to a single space — consistent with the "semantic equivalence" rule
/// in SPEC-self-evolution §11.3.
pub fn diff(real: &ShadowTurnOutput, shadow: &ShadowTurnOutput) -> ShadowDiff {
    let mut deltas = Vec::new();

    let real_norm = normalise_ws(&real.assistant_text);
    let shadow_norm = normalise_ws(&shadow.assistant_text);
    if real_norm != shadow_norm {
        deltas.push(ShadowDelta::TextDiff {
            real: real.assistant_text.clone(),
            shadow: shadow.assistant_text.clone(),
        });
    }

    if real.tool_calls != shadow.tool_calls {
        deltas.push(ShadowDelta::ToolCallMismatch {
            real: real.tool_calls.clone(),
            shadow: shadow.tool_calls.clone(),
        });
    }

    if real.terminated != shadow.terminated {
        deltas.push(ShadowDelta::TerminationMismatch {
            real: real.terminated,
            shadow: shadow.terminated,
        });
    }

    ShadowDiff {
        matched: deltas.is_empty(),
        deltas,
    }
}

/// Collapse whitespace runs and trim — used by [`diff`] for text comparison.
fn normalise_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

// ── Unit tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn ruleset() -> ShadowRuleset {
        ShadowRuleset {
            rules_json: serde_json::json!({ "rule-1": "no-op" }),
            description: "test".to_string(),
        }
    }

    #[test]
    fn normalise_ws_collapses_internal_spaces() {
        assert_eq!(normalise_ws("  a\t b\n\nc "), "a b c");
    }

    #[test]
    fn diff_identical_matches() {
        let a = ShadowTurnOutput {
            assistant_text: "hi".into(),
            tool_calls: vec!["tool:abc".into()],
            terminated: false,
        };
        let d = diff(&a, &a.clone());
        assert!(d.matched);
        assert!(d.deltas.is_empty());
    }

    #[test]
    fn diff_whitespace_differences_match() {
        let a = ShadowTurnOutput {
            assistant_text: "hello world".into(),
            tool_calls: vec![],
            terminated: false,
        };
        let b = ShadowTurnOutput {
            assistant_text: "  hello   world  ".into(),
            tool_calls: vec![],
            terminated: false,
        };
        assert!(diff(&a, &b).matched);
    }

    #[tokio::test]
    async fn default_budget_has_expected_values() {
        let b = ShadowBudget::default();
        assert_eq!(b.max_turns, 3);
        assert_eq!(b.max_wall_time, Duration::from_secs(30));
        assert_eq!(b.max_tool_calls, 20);
    }

    #[tokio::test]
    async fn in_memory_executor_echoes_turns() {
        let exec = InMemoryShadowExecutor::new();
        let out = exec
            .run(
                &ruleset(),
                vec![ShadowTurnInput {
                    user_message: "ping".to_string(),
                    context: serde_json::json!({}),
                }],
                ShadowBudget::default(),
            )
            .await
            .expect("run should succeed");
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].assistant_text, "[shadow:test] echo: ping");
        assert!(out[0].tool_calls.is_empty());
        assert!(!out[0].terminated);
    }
}
