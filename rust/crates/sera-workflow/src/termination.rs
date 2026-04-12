use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Configuration thresholds that govern when a workflow run terminates.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerminationConfig {
    /// Stop after this many rounds regardless of other conditions.
    pub max_rounds: Option<u32>,
    /// Stop once cumulative cost exceeds this amount (USD).
    pub max_cost_usd: Option<f64>,
}

/// Running counters updated each round.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TerminationState {
    pub rounds_elapsed: u32,
    pub cost_usd_accumulated: f64,
    /// Number of consecutive rounds where no useful work was done.
    pub consecutive_idle_rounds: u32,
}

/// Why a workflow run stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminationReason {
    NRoundExceeded,
    Idle,
    BudgetExhausted,
    ExplicitStop,
}

/// Record of a completed (terminated) workflow run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowTermination {
    pub reason: TerminationReason,
    pub terminated_at: DateTime<Utc>,
}

/// Evaluate whether any termination condition is satisfied.
///
/// Returns `Some(reason)` on the first condition that fires, in priority order:
/// 1. Budget exhausted
/// 2. N-round limit exceeded
/// 3. Idle (3+ consecutive idle rounds)
///
/// Returns `None` if no condition is met.
pub fn check_termination(
    config: &TerminationConfig,
    state: &TerminationState,
) -> Option<TerminationReason> {
    if let Some(max_cost) = config.max_cost_usd
        && state.cost_usd_accumulated >= max_cost
    {
        return Some(TerminationReason::BudgetExhausted);
    }

    if let Some(max_rounds) = config.max_rounds
        && state.rounds_elapsed >= max_rounds
    {
        return Some(TerminationReason::NRoundExceeded);
    }

    if state.consecutive_idle_rounds >= 3 {
        return Some(TerminationReason::Idle);
    }

    None
}
