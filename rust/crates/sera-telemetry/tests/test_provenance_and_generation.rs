//! Tests for provenance types (RunEvidence, CostRecord, LaneCommitProvenance)
//! and generation types (GenerationMarker, BuildIdentity, GenerationLabel).

use sera_telemetry::generation::{BuildIdentity, GenerationLabel, GenerationMarker};
use sera_telemetry::provenance::{CostRecord, LaneCommitProvenance, RunEvidence};
use time::OffsetDateTime;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helper
// ---------------------------------------------------------------------------

fn zero_cost() -> CostRecord {
    CostRecord {
        model: "none".to_string(),
        input_tokens: 0,
        output_tokens: 0,
        cache_tokens: 0,
        cost_micro_usd: 0,
    }
}

fn make_evidence(outcome: &str) -> RunEvidence {
    RunEvidence {
        run_id: Uuid::new_v4(),
        tools_exposed: vec!["bash".to_string(), "read_file".to_string()],
        tools_called: vec!["bash".to_string()],
        approvals: vec![],
        memory_writes: vec!["slot_a".to_string()],
        model_calls: vec![CostRecord {
            model: "claude-3-5-sonnet".to_string(),
            input_tokens: 500,
            output_tokens: 100,
            cache_tokens: 20,
            cost_micro_usd: 840,
        }],
        total_cost: CostRecord {
            model: "total".to_string(),
            input_tokens: 500,
            output_tokens: 100,
            cache_tokens: 20,
            cost_micro_usd: 840,
        },
        outcome: outcome.to_string(),
    }
}

// ---------------------------------------------------------------------------
// CostRecord
// ---------------------------------------------------------------------------

#[test]
fn cost_record_zero_values_roundtrip() {
    let cr = zero_cost();
    let json = serde_json::to_string(&cr).expect("serialize");
    let restored: CostRecord = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(restored.input_tokens, 0);
    assert_eq!(restored.output_tokens, 0);
    assert_eq!(restored.cost_micro_usd, 0);
    assert_eq!(restored.model, "none");
}

#[test]
fn cost_record_max_values_roundtrip() {
    let cr = CostRecord {
        model: "expensive-model".to_string(),
        input_tokens: u64::MAX,
        output_tokens: u64::MAX,
        cache_tokens: u64::MAX,
        cost_micro_usd: u64::MAX,
    };
    let json = serde_json::to_string(&cr).expect("serialize");
    let restored: CostRecord = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(restored.input_tokens, u64::MAX);
    assert_eq!(restored.cost_micro_usd, u64::MAX);
}

// ---------------------------------------------------------------------------
// LaneCommitProvenance
// ---------------------------------------------------------------------------

#[test]
fn lane_commit_provenance_populated_roundtrip() {
    let prov = LaneCommitProvenance {
        git_commit: Some("abc123def456".to_string()),
        branch: Some("feat/my-lane".to_string()),
        worktree: Some("/tmp/worktrees/lane-1".to_string()),
        canonical_commit: Some("deadbeef".to_string()),
        superseded_by: None,
        lineage: vec!["aaa".to_string(), "bbb".to_string(), "ccc".to_string()],
    };

    let json = serde_json::to_string(&prov).expect("serialize");
    let restored: LaneCommitProvenance = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(restored.git_commit.as_deref(), Some("abc123def456"));
    assert_eq!(restored.branch.as_deref(), Some("feat/my-lane"));
    assert_eq!(restored.lineage.len(), 3);
    assert_eq!(restored.lineage[2], "ccc");
    assert!(restored.superseded_by.is_none());
}

#[test]
fn lane_commit_provenance_with_superseded_by() {
    let prov = LaneCommitProvenance {
        superseded_by: Some("newcommit".to_string()),
        ..Default::default()
    };
    assert_eq!(prov.superseded_by.as_deref(), Some("newcommit"));
    assert!(prov.git_commit.is_none());
}

#[test]
fn lane_commit_provenance_lineage_ordering_preserved() {
    let ancestors = vec!["first".to_string(), "second".to_string(), "third".to_string()];
    let prov = LaneCommitProvenance {
        lineage: ancestors.clone(),
        ..Default::default()
    };
    assert_eq!(prov.lineage, ancestors);
}

// ---------------------------------------------------------------------------
// RunEvidence
// ---------------------------------------------------------------------------

#[test]
fn run_evidence_success_roundtrip() {
    let ev = make_evidence("success");
    let json = serde_json::to_string(&ev).expect("serialize");
    let restored: RunEvidence = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(restored.outcome, "success");
    assert_eq!(restored.tools_exposed, ev.tools_exposed);
    assert_eq!(restored.tools_called, ev.tools_called);
    assert_eq!(restored.memory_writes, ev.memory_writes);
    assert_eq!(restored.model_calls.len(), 1);
    assert_eq!(restored.model_calls[0].model, "claude-3-5-sonnet");
}

#[test]
fn run_evidence_failure_outcome() {
    let ev = make_evidence("failure");
    assert_eq!(ev.outcome, "failure");
}

#[test]
fn run_evidence_abandoned_outcome() {
    let ev = make_evidence("abandoned");
    let json = serde_json::to_string(&ev).expect("serialize");
    let restored: RunEvidence = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(restored.outcome, "abandoned");
}

#[test]
fn run_evidence_empty_tool_lists() {
    let ev = RunEvidence {
        run_id: Uuid::new_v4(),
        tools_exposed: vec![],
        tools_called: vec![],
        approvals: vec![],
        memory_writes: vec![],
        model_calls: vec![],
        total_cost: zero_cost(),
        outcome: "success".to_string(),
    };
    assert!(ev.tools_exposed.is_empty());
    assert!(ev.tools_called.is_empty());
    assert!(ev.model_calls.is_empty());
}

// ---------------------------------------------------------------------------
// GenerationLabel
// ---------------------------------------------------------------------------

#[test]
fn generation_label_equality() {
    let a = GenerationLabel("mvs-0.1.0".to_string());
    let b = GenerationLabel("mvs-0.1.0".to_string());
    assert_eq!(a, b);
}

#[test]
fn generation_label_inequality() {
    let a = GenerationLabel("mvs-0.1.0".to_string());
    let b = GenerationLabel("mvs-0.2.0".to_string());
    assert_ne!(a, b);
}

#[test]
fn generation_label_serde_roundtrip() {
    let label = GenerationLabel("sera-2.0-beta".to_string());
    let json = serde_json::to_string(&label).expect("serialize");
    let restored: GenerationLabel = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(restored, label);
}

// ---------------------------------------------------------------------------
// BuildIdentity
// ---------------------------------------------------------------------------

#[test]
fn build_identity_fields_accessible() {
    let id = BuildIdentity {
        version: "1.2.3".to_string(),
        commit: "cafebabe".to_string(),
        build_time: OffsetDateTime::UNIX_EPOCH,
        signer_fingerprint: [0xab; 32],
        constitution_hash: [0xcd; 32],
    };
    assert_eq!(id.version, "1.2.3");
    assert_eq!(id.commit, "cafebabe");
    assert_eq!(id.signer_fingerprint, [0xab; 32]);
    assert_eq!(id.constitution_hash, [0xcd; 32]);
}

#[test]
fn build_identity_serde_roundtrip() {
    let id = BuildIdentity {
        version: "0.9.0".to_string(),
        commit: "deadbeef".to_string(),
        build_time: OffsetDateTime::UNIX_EPOCH,
        signer_fingerprint: [0x11; 32],
        constitution_hash: [0x22; 32],
    };
    let json = serde_json::to_string(&id).expect("serialize");
    let restored: BuildIdentity = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(restored.version, id.version);
    assert_eq!(restored.commit, id.commit);
    assert_eq!(restored.signer_fingerprint, id.signer_fingerprint);
    assert_eq!(restored.constitution_hash, id.constitution_hash);
}

// ---------------------------------------------------------------------------
// GenerationMarker
// ---------------------------------------------------------------------------

#[test]
fn generation_marker_started_at_preserved() {
    let marker = GenerationMarker {
        label: GenerationLabel("test-gen".to_string()),
        binary_identity: BuildIdentity {
            version: "1.0.0".to_string(),
            commit: "aabbccdd".to_string(),
            build_time: OffsetDateTime::UNIX_EPOCH,
            signer_fingerprint: [0u8; 32],
            constitution_hash: [0u8; 32],
        },
        started_at: OffsetDateTime::UNIX_EPOCH,
    };
    assert_eq!(marker.started_at, OffsetDateTime::UNIX_EPOCH);
    assert_eq!(marker.label.0, "test-gen");
}

#[test]
fn generation_marker_different_labels_are_not_equal() {
    let make = |label: &str| GenerationLabel(label.to_string());
    assert_ne!(make("v1"), make("v2"));
}
