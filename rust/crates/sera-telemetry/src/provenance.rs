//! Provenance records — git lineage, run evidence, and cost accounting.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Git provenance for a committed lane output.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LaneCommitProvenance {
    /// The git commit SHA that represents this lane's output.
    pub git_commit: Option<String>,
    /// Branch name the commit lives on.
    pub branch: Option<String>,
    /// Worktree path used during the lane run.
    pub worktree: Option<String>,
    /// Canonical (merge-base) commit in the main branch lineage.
    pub canonical_commit: Option<String>,
    /// If this lane was superseded, the commit that supersedes it.
    pub superseded_by: Option<String>,
    /// Ordered list of ancestor commit SHAs (oldest first).
    pub lineage: Vec<String>,
}

/// Per-model-call cost record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostRecord {
    /// Model identifier (e.g. `"claude-3-5-sonnet"`).
    pub model: String,
    /// Number of input tokens consumed.
    pub input_tokens: u64,
    /// Number of output tokens generated.
    pub output_tokens: u64,
    /// Number of tokens served from the provider cache.
    pub cache_tokens: u64,
    /// Total cost in micro-USD (1 = $0.000001).
    pub cost_micro_usd: u64,
}

/// Evidence record for a single agent lane run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunEvidence {
    /// Unique identifier for this run.
    pub run_id: Uuid,
    /// Tool names that were exposed to the agent.
    pub tools_exposed: Vec<String>,
    /// Tool names that the agent actually called.
    pub tools_called: Vec<String>,
    /// Human approval events that occurred during the run.
    pub approvals: Vec<String>,
    /// Keys of memory slots written during the run.
    pub memory_writes: Vec<String>,
    /// Per-model cost breakdown.
    pub model_calls: Vec<CostRecord>,
    /// Aggregate cost across all model calls.
    pub total_cost: CostRecord,
    /// Terminal outcome string (e.g. `"success"`, `"failure"`, `"abandoned"`).
    pub outcome: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lane_commit_provenance_default_is_all_none() {
        let p = LaneCommitProvenance::default();
        assert!(p.git_commit.is_none());
        assert!(p.branch.is_none());
        assert!(p.worktree.is_none());
        assert!(p.canonical_commit.is_none());
        assert!(p.superseded_by.is_none());
        assert!(p.lineage.is_empty());
    }

    #[test]
    fn cost_record_serde_round_trips() {
        let record = CostRecord {
            model: "claude-3-5-sonnet".to_string(),
            input_tokens: 1000,
            output_tokens: 200,
            cache_tokens: 50,
            cost_micro_usd: 4200,
        };
        let json = serde_json::to_string(&record).unwrap();
        let decoded: CostRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.model, record.model);
        assert_eq!(decoded.input_tokens, record.input_tokens);
        assert_eq!(decoded.cost_micro_usd, record.cost_micro_usd);
    }

    #[test]
    fn run_evidence_has_unique_run_id() {
        let make = || RunEvidence {
            run_id: Uuid::new_v4(),
            tools_exposed: vec![],
            tools_called: vec![],
            approvals: vec![],
            memory_writes: vec![],
            model_calls: vec![],
            total_cost: CostRecord {
                model: "none".to_string(),
                input_tokens: 0,
                output_tokens: 0,
                cache_tokens: 0,
                cost_micro_usd: 0,
            },
            outcome: "success".to_string(),
        };
        let a = make();
        let b = make();
        assert_ne!(a.run_id, b.run_id);
    }
}
