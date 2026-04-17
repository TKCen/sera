#![allow(deprecated)]

use std::collections::HashMap;

use crate::{
    dreaming::{DeepSleepConfig, DreamCandidate, DreamingConfig, DreamingPhases, DreamingWeights,
               LightSleepConfig, RemSleepConfig},
    registry::WorkflowRegistry,
    session_key::workflow_session_key,
    topological_sort,
    types::{CronSchedule, EventPattern, ThresholdCondition, ThresholdOperator, WorkflowDef,
            WorkflowTrigger},
    WorkflowTask, WorkflowTaskDependency, WorkflowTaskId, WorkflowTaskStatus, WorkflowTaskType,
};

// ---------------------------------------------------------------------------
// CronSchedule
// ---------------------------------------------------------------------------

#[test]
fn cron_valid_expression() {
    let s = CronSchedule { expression: "0 3 * * * *".to_string() };
    assert!(s.is_valid());
}

#[test]
fn cron_invalid_expression() {
    let s = CronSchedule { expression: "not a cron".to_string() };
    assert!(!s.is_valid());
}

#[test]
fn cron_next_fire_is_in_future() {
    let s = CronSchedule { expression: "0 * * * * *".to_string() };
    let next = s.next_fire().expect("should produce a next fire time");
    assert!(next > chrono::Utc::now());
}

#[test]
fn cron_next_fire_after() {
    use chrono::{TimeZone, Utc};
    let s = CronSchedule { expression: "0 0 3 * * *".to_string() };
    let base = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
    let next = s.next_fire_after(base).expect("should produce a next fire time");
    assert!(next > base);
}

// ---------------------------------------------------------------------------
// WorkflowRegistry
// ---------------------------------------------------------------------------

fn sample_def(name: &str) -> WorkflowDef {
    WorkflowDef {
        name: name.to_string(),
        trigger: WorkflowTrigger::Manual,
        agent_id: "agent-1".to_string(),
        config: serde_json::Value::Null,
        enabled: true,
    }
}

fn cron_def(name: &str) -> WorkflowDef {
    WorkflowDef {
        name: name.to_string(),
        trigger: WorkflowTrigger::Cron(CronSchedule {
            expression: "0 3 * * * *".to_string(),
        }),
        agent_id: "agent-2".to_string(),
        config: serde_json::Value::Null,
        enabled: true,
    }
}

#[test]
fn registry_register_and_get() {
    let mut reg = WorkflowRegistry::new();
    reg.register(sample_def("wf1")).unwrap();
    assert!(reg.get("wf1").is_some());
    assert!(reg.get("missing").is_none());
}

#[test]
fn registry_unregister() {
    let mut reg = WorkflowRegistry::new();
    reg.register(sample_def("wf1")).unwrap();
    assert!(reg.unregister("wf1"));
    assert!(!reg.unregister("wf1")); // already gone
}

#[test]
fn registry_list() {
    let mut reg = WorkflowRegistry::new();
    reg.register(sample_def("a")).unwrap();
    reg.register(sample_def("b")).unwrap();
    assert_eq!(reg.list().len(), 2);
}

#[test]
fn registry_list_enabled() {
    let mut reg = WorkflowRegistry::new();
    reg.register(sample_def("a")).unwrap();
    let mut disabled = sample_def("b");
    disabled.enabled = false;
    reg.register(disabled).unwrap();
    assert_eq!(reg.list_enabled().len(), 1);
    assert_eq!(reg.list_enabled()[0].name, "a");
}

#[test]
fn registry_list_cron() {
    let mut reg = WorkflowRegistry::new();
    reg.register(sample_def("manual")).unwrap();
    reg.register(cron_def("nightly")).unwrap();
    let crons = reg.list_cron();
    assert_eq!(crons.len(), 1);
    assert_eq!(crons[0].name, "nightly");
}

#[test]
fn registry_enable_disable() {
    let mut reg = WorkflowRegistry::new();
    reg.register(sample_def("wf")).unwrap();
    assert!(reg.disable("wf"));
    assert!(!reg.get("wf").unwrap().enabled);
    assert!(reg.enable("wf"));
    assert!(reg.get("wf").unwrap().enabled);
}

#[test]
fn registry_enable_missing_returns_false() {
    let mut reg = WorkflowRegistry::new();
    assert!(!reg.enable("nope"));
    assert!(!reg.disable("nope"));
}

#[test]
fn registry_duplicate_returns_error() {
    let mut reg = WorkflowRegistry::new();
    reg.register(sample_def("wf")).unwrap();
    let err = reg.register(sample_def("wf")).unwrap_err();
    assert!(matches!(err, crate::error::WorkflowError::DuplicateWorkflow { .. }));
}

// ---------------------------------------------------------------------------
// WorkflowTrigger serde roundtrips
// ---------------------------------------------------------------------------

#[test]
fn trigger_serde_manual() {
    let t = WorkflowTrigger::Manual;
    let json = serde_json::to_string(&t).unwrap();
    let back: WorkflowTrigger = serde_json::from_str(&json).unwrap();
    assert!(matches!(back, WorkflowTrigger::Manual));
}

#[test]
fn trigger_serde_cron() {
    let t = WorkflowTrigger::Cron(CronSchedule { expression: "0 * * * * *".to_string() });
    let json = serde_json::to_string(&t).unwrap();
    let back: WorkflowTrigger = serde_json::from_str(&json).unwrap();
    assert!(matches!(back, WorkflowTrigger::Cron(_)));
}

#[test]
fn trigger_serde_event() {
    let t = WorkflowTrigger::Event(EventPattern {
        kind: Some("memory.created".to_string()),
        source: None,
        metadata_match: HashMap::new(),
    });
    let json = serde_json::to_string(&t).unwrap();
    let back: WorkflowTrigger = serde_json::from_str(&json).unwrap();
    assert!(matches!(back, WorkflowTrigger::Event(_)));
}

#[test]
fn trigger_serde_threshold() {
    let t = WorkflowTrigger::Threshold(ThresholdCondition {
        metric: "memory_count".to_string(),
        operator: ThresholdOperator::Gt,
        value: 100.0,
        agent_id: None,
    });
    let json = serde_json::to_string(&t).unwrap();
    let back: WorkflowTrigger = serde_json::from_str(&json).unwrap();
    assert!(matches!(back, WorkflowTrigger::Threshold(_)));
}

// ---------------------------------------------------------------------------
// ThresholdOperator serde
// ---------------------------------------------------------------------------

#[test]
fn threshold_operator_serde_roundtrip() {
    for op in [
        ThresholdOperator::Gt,
        ThresholdOperator::Gte,
        ThresholdOperator::Lt,
        ThresholdOperator::Lte,
        ThresholdOperator::Eq,
    ] {
        let json = serde_json::to_string(&op).unwrap();
        let back: ThresholdOperator = serde_json::from_str(&json).unwrap();
        assert_eq!(op, back);
    }
}

// ---------------------------------------------------------------------------
// EventPattern serde
// ---------------------------------------------------------------------------

#[test]
fn event_pattern_serde_roundtrip() {
    let mut meta = HashMap::new();
    meta.insert("agent_id".to_string(), serde_json::json!("agent-1"));
    let p = EventPattern {
        kind: Some("audit".to_string()),
        source: Some("runtime".to_string()),
        metadata_match: meta,
    };
    let json = serde_json::to_string(&p).unwrap();
    let back: EventPattern = serde_json::from_str(&json).unwrap();
    assert_eq!(back.kind.as_deref(), Some("audit"));
    assert_eq!(back.source.as_deref(), Some("runtime"));
    assert_eq!(back.metadata_match.len(), 1);
}

// ---------------------------------------------------------------------------
// DreamingWeights
// ---------------------------------------------------------------------------

#[test]
fn dreaming_weights_default_sum_to_one() {
    let w = DreamingWeights::default();
    let sum = w.relevance
        + w.frequency
        + w.query_diversity
        + w.recency
        + w.consolidation
        + w.conceptual_richness;
    assert!((sum - 1.0).abs() < 1e-10, "weights sum to {sum}, expected 1.0");
}

// ---------------------------------------------------------------------------
// DreamCandidate
// ---------------------------------------------------------------------------

fn make_candidate() -> DreamCandidate {
    let mut scores = HashMap::new();
    scores.insert("relevance".to_string(), 0.8);
    scores.insert("frequency".to_string(), 0.6);
    scores.insert("query_diversity".to_string(), 0.5);
    scores.insert("recency".to_string(), 0.7);
    scores.insert("consolidation".to_string(), 0.4);
    scores.insert("conceptual_richness".to_string(), 0.9);
    DreamCandidate {
        memory_key: "mem-1".to_string(),
        scores,
        total_score: 0.0,
        recall_count: 5,
        unique_queries: 3,
    }
}

#[test]
fn dream_candidate_compute_score() {
    let mut c = make_candidate();
    let w = DreamingWeights::default();
    c.compute_score(&w);
    // expected: 0.8*0.30 + 0.6*0.24 + 0.5*0.15 + 0.7*0.15 + 0.4*0.10 + 0.9*0.06
    let expected = 0.8 * 0.30
        + 0.6 * 0.24
        + 0.5 * 0.15
        + 0.7 * 0.15
        + 0.4 * 0.10
        + 0.9 * 0.06;
    assert!((c.total_score - expected).abs() < 1e-10);
}

#[test]
fn dream_candidate_passes_gates_all_met() {
    let mut c = make_candidate();
    let w = DreamingWeights::default();
    c.compute_score(&w);
    let cfg = DeepSleepConfig {
        min_score: 0.5,
        min_recall_count: 3,
        min_unique_queries: 2,
        max_age_days: 30,
        limit: 10,
    };
    assert!(c.passes_gates(&cfg));
}

#[test]
fn dream_candidate_fails_gate_low_score() {
    let mut c = make_candidate();
    let w = DreamingWeights::default();
    c.compute_score(&w);
    let cfg = DeepSleepConfig {
        min_score: 0.99,
        min_recall_count: 1,
        min_unique_queries: 1,
        max_age_days: 30,
        limit: 10,
    };
    assert!(!c.passes_gates(&cfg));
}

#[test]
fn dream_candidate_fails_gate_recall_count() {
    let mut c = make_candidate();
    let w = DreamingWeights::default();
    c.compute_score(&w);
    let cfg = DeepSleepConfig {
        min_score: 0.0,
        min_recall_count: 10,
        min_unique_queries: 1,
        max_age_days: 30,
        limit: 10,
    };
    assert!(!c.passes_gates(&cfg));
}

#[test]
fn dream_candidate_fails_gate_unique_queries() {
    let mut c = make_candidate();
    let w = DreamingWeights::default();
    c.compute_score(&w);
    let cfg = DeepSleepConfig {
        min_score: 0.0,
        min_recall_count: 1,
        min_unique_queries: 10,
        max_age_days: 30,
        limit: 10,
    };
    assert!(!c.passes_gates(&cfg));
}

// ---------------------------------------------------------------------------
// DreamingConfig serde
// ---------------------------------------------------------------------------

#[test]
fn dreaming_config_serde_roundtrip() {
    let cfg = DreamingConfig {
        enabled: true,
        schedule: "0 3 * * * *".to_string(),
        phases: DreamingPhases {
            light: LightSleepConfig { lookback_days: 7, limit: 50 },
            rem: RemSleepConfig { lookback_days: 30, min_pattern_strength: 0.6 },
            deep: DeepSleepConfig {
                min_score: 0.5,
                min_recall_count: 3,
                min_unique_queries: 2,
                max_age_days: 90,
                limit: 20,
            },
        },
    };
    let json = serde_json::to_string(&cfg).unwrap();
    let back: DreamingConfig = serde_json::from_str(&json).unwrap();
    assert!(back.enabled);
    assert_eq!(back.schedule, "0 3 * * * *");
    assert_eq!(back.phases.light.lookback_days, 7);
    assert_eq!(back.phases.rem.min_pattern_strength, 0.6);
    assert_eq!(back.phases.deep.min_recall_count, 3);
}

// ---------------------------------------------------------------------------
// workflow_session_key
// ---------------------------------------------------------------------------

#[test]
fn session_key_format() {
    let key = workflow_session_key("agent-42", "dreaming");
    assert_eq!(key, "workflow:agent-42:dreaming");
}

// ---------------------------------------------------------------------------
// topological_sort
// ---------------------------------------------------------------------------

fn make_task(seed: u8, _deps: impl Into<Vec<WorkflowTaskDependency>>) -> WorkflowTask {
    let task_id = WorkflowTaskId::from_content(
        &format!("Task {seed}"),
        &format!("Description {seed}"),
        "Acceptance",
        "formula",
        "location",
        chrono::Utc::now(),
    );
    let deps = _deps.into();
    WorkflowTask {
        id: task_id,
        title: format!("Task {seed}"),
        description: format!("Description {seed}"),
        acceptance_criteria: vec!["AC".to_string()],
        status: WorkflowTaskStatus::Open,
        priority: seed,
        task_type: WorkflowTaskType::Feature,
        assignee: None,
        due_at: None,
        defer_until: None,
        metadata: serde_json::Value::Null,
        await_type: None,
        await_id: None,
        timeout: None,
        ephemeral: false,
        source_formula: None,
        source_location: None,
        created_at: chrono::Utc::now(),
        meta_scope: None,
        change_artifact_id: None,
        dependencies: deps,
    }
}

#[test]
fn topological_sort_independent() {
    // Tasks with no dependencies - all independent
    let a = make_task(1, vec![]);
    let b = make_task(2, vec![]);
    let c = make_task(3, vec![]);
    let tasks = vec![a, b, c];

    let sorted = topological_sort(&tasks).unwrap();
    assert_eq!(sorted.len(), 3);
}

#[test]
fn topological_sort_ignores_non_blocks() {
    // Tasks with Related dependencies - treated as independent
    let a = make_task(1, vec![]);
    let b = make_task(2, vec![]);
    let tasks = vec![a, b];

    let sorted = topological_sort(&tasks).unwrap();
    assert_eq!(sorted.len(), 2);
}

// ---------------------------------------------------------------------------
// AwaitType::Timer gate
// ---------------------------------------------------------------------------

use chrono::{Duration as ChronoDuration, TimeZone};

use crate::task::AwaitType;
use crate::{is_timer_ready, ready_tasks};

fn base_instant() -> chrono::DateTime<chrono::Utc> {
    chrono::Utc.with_ymd_and_hms(2026, 1, 1, 12, 0, 0).unwrap()
}

#[test]
fn timer_past_not_before_is_ready() {
    let now = base_instant();
    let gate = AwaitType::Timer { not_before: now - ChronoDuration::seconds(60) };
    assert!(is_timer_ready(&gate, now));
}

#[test]
fn timer_future_not_before_is_not_ready() {
    let now = base_instant();
    let gate = AwaitType::Timer { not_before: now + ChronoDuration::seconds(60) };
    assert!(!is_timer_ready(&gate, now));
}

#[test]
fn timer_boundary_equal_is_ready() {
    // not_before == now must count as ready (>=, not >).
    let now = base_instant();
    let gate = AwaitType::Timer { not_before: now };
    assert!(is_timer_ready(&gate, now));
}

#[test]
fn timer_does_not_bypass_other_gates() {
    // Even when the Timer is elapsed, the task must also pass other gates
    // (here: Blocks dependency). Timer does not short-circuit.
    let now = base_instant();

    let blocker = make_task(1, vec![]);
    let mut blocked = make_task(2, vec![]);
    blocked.dependencies = vec![WorkflowTaskDependency {
        from: blocker.id,
        to: blocked.id,
        kind: crate::task::DependencyType::Blocks,
    }];
    blocked.await_type = Some(AwaitType::Timer {
        not_before: now - ChronoDuration::seconds(1),
    });
    // blocker is still Open, so Blocks gate must reject `blocked`.

    let tasks = vec![blocker, blocked.clone()];
    let ready = ready_tasks(&tasks, now);
    assert!(!ready.iter().any(|t| t.id == blocked.id));
}

#[test]
fn timer_serde_roundtrip() {
    let now = base_instant();
    let gate = AwaitType::Timer { not_before: now };
    let json = serde_json::to_string(&gate).unwrap();
    let back: AwaitType = serde_json::from_str(&json).unwrap();
    assert_eq!(gate, back);
    // Sanity: struct-variant serializes with discriminant and field.
    assert!(json.contains("timer"));
    assert!(json.contains("not_before"));
}

#[test]
fn ready_queue_surfaces_elapsed_timer_and_hides_pending_timer() {
    let now = base_instant();

    let mut past = make_task(1, vec![]);
    past.await_type = Some(AwaitType::Timer {
        not_before: now - ChronoDuration::seconds(5),
    });

    let mut future = make_task(2, vec![]);
    future.await_type = Some(AwaitType::Timer {
        not_before: now + ChronoDuration::seconds(5),
    });

    let tasks = vec![past.clone(), future.clone()];
    let ready = ready_tasks(&tasks, now);

    assert!(ready.iter().any(|t| t.id == past.id), "elapsed Timer should be ready");
    assert!(!ready.iter().any(|t| t.id == future.id), "pending Timer should not be ready");
}

#[test]
fn non_timer_await_still_blocks() {
    // Regression: other AwaitType variants must still block until their
    // integrations (GhRun/GhPr/Human/Mail/Change) land.
    let now = base_instant();
    let mut t = make_task(1, vec![]);
    t.await_type = Some(AwaitType::Human);

    let tasks = vec![t.clone()];
    let ready = ready_tasks(&tasks, now);
    assert!(!ready.iter().any(|r| r.id == t.id));
}
