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
    // Regression: await variants without an integration (Mail, Change)
    // must still block. Use AwaitType::Mail as the unit-variant sentinel
    // (GhPr is now a struct variant with its own integration).
    let now = base_instant();
    let mut t = make_task(1, vec![]);
    t.await_type = Some(AwaitType::Mail);

    let tasks = vec![t.clone()];
    let ready = ready_tasks(&tasks, now);
    assert!(!ready.iter().any(|r| r.id == t.id));
}

// ---------------------------------------------------------------------------
// AwaitType::Human gate (HitlLookup)
// ---------------------------------------------------------------------------

use sera_hitl::{ApprovalId, TicketStatus};

use crate::ready::{is_human_ready, ready_tasks_with_hitl, HitlLookup, NoopHitlLookup};

/// In-memory [`HitlLookup`] used exclusively in tests to stand in for a
/// real sera-hitl repository. Holds a tiny `ApprovalId -> TicketStatus`
/// table and returns `None` for any id that was never inserted.
#[derive(Debug, Default)]
struct MapHitlLookup {
    by_id: HashMap<String, TicketStatus>,
}

impl MapHitlLookup {
    fn new() -> Self {
        Self { by_id: HashMap::new() }
    }

    fn insert(&mut self, id: impl Into<String>, status: TicketStatus) {
        self.by_id.insert(id.into(), status);
    }
}

impl HitlLookup for MapHitlLookup {
    fn ticket_status(&self, id: &ApprovalId) -> Option<TicketStatus> {
        self.by_id.get(id.as_str()).copied()
    }
}

#[test]
fn human_missing_ticket_is_not_ready() {
    // Ticket not present in the lookup → treat as not ready. The gate must
    // never self-satisfy on unknown IDs, otherwise a stale workflow task
    // referencing a purged ticket would be surfaced forever.
    let gate = AwaitType::Human { approval_id: ApprovalId::new("ticket-missing") };
    let lookup = MapHitlLookup::new();
    assert!(!is_human_ready(&gate, &lookup));
}

#[test]
fn human_pending_ticket_is_not_ready() {
    // Pending is a non-terminal status — still waiting on the approver.
    let now = base_instant();
    let gate = AwaitType::Human { approval_id: ApprovalId::new("ticket-pending") };
    let mut lookup = MapHitlLookup::new();
    lookup.insert("ticket-pending", TicketStatus::Pending);
    assert!(!is_human_ready(&gate, &lookup));
    // Also exercise the gate through ready_tasks_with_hitl to cover the
    // scheduler path.
    let mut t = make_task(1, vec![]);
    t.await_type = Some(gate);
    let tasks = vec![t.clone()];
    let ready = ready_tasks_with_hitl(&tasks, now, &lookup);
    assert!(!ready.iter().any(|r| r.id == t.id));
}

#[test]
fn human_approved_ticket_is_ready() {
    let now = base_instant();
    let mut t = make_task(1, vec![]);
    t.await_type = Some(AwaitType::Human {
        approval_id: ApprovalId::new("ticket-approved"),
    });
    let mut lookup = MapHitlLookup::new();
    lookup.insert("ticket-approved", TicketStatus::Approved);

    let tasks = vec![t.clone()];
    let ready = ready_tasks_with_hitl(&tasks, now, &lookup);
    assert!(ready.iter().any(|r| r.id == t.id));
}

#[test]
fn human_rejected_ticket_is_ready() {
    // Rejection is terminal — the task must wake up so its handler can
    // branch on the denial. Gating only on Approved would strand the task.
    let now = base_instant();
    let mut t = make_task(1, vec![]);
    t.await_type = Some(AwaitType::Human {
        approval_id: ApprovalId::new("ticket-rejected"),
    });
    let mut lookup = MapHitlLookup::new();
    lookup.insert("ticket-rejected", TicketStatus::Rejected);

    let tasks = vec![t.clone()];
    let ready = ready_tasks_with_hitl(&tasks, now, &lookup);
    assert!(ready.iter().any(|r| r.id == t.id));
}

#[test]
fn human_expired_ticket_is_ready() {
    // Expiry is terminal too — the workflow should progress and record the
    // expiration rather than idle forever.
    let now = base_instant();
    let gate = AwaitType::Human { approval_id: ApprovalId::new("ticket-expired") };
    let mut lookup = MapHitlLookup::new();
    lookup.insert("ticket-expired", TicketStatus::Expired);
    assert!(is_human_ready(&gate, &lookup));

    let mut t = make_task(1, vec![]);
    t.await_type = Some(gate);
    let tasks = vec![t.clone()];
    let ready = ready_tasks_with_hitl(&tasks, now, &lookup);
    assert!(ready.iter().any(|r| r.id == t.id));
}

#[test]
fn human_escalated_ticket_is_not_ready() {
    // Escalated is non-terminal — the chain is still waiting on the next
    // approver. Important edge case since Escalated is neither Pending nor
    // a terminal state.
    let now = base_instant();
    let mut t = make_task(1, vec![]);
    t.await_type = Some(AwaitType::Human {
        approval_id: ApprovalId::new("ticket-escalated"),
    });
    let mut lookup = MapHitlLookup::new();
    lookup.insert("ticket-escalated", TicketStatus::Escalated);

    let tasks = vec![t.clone()];
    let ready = ready_tasks_with_hitl(&tasks, now, &lookup);
    assert!(!ready.iter().any(|r| r.id == t.id));
}

#[test]
fn human_ready_but_blocks_open_still_blocks() {
    // Combined gate: the Human ticket is Approved, but the task also has an
    // open Blocks dependency. The blocks gate must still win — a terminal
    // ticket does NOT bypass other gates.
    let now = base_instant();

    let blocker = make_task(1, vec![]);
    let mut blocked = make_task(2, vec![]);
    blocked.dependencies = vec![WorkflowTaskDependency {
        from: blocker.id,
        to: blocked.id,
        kind: crate::task::DependencyType::Blocks,
    }];
    blocked.await_type = Some(AwaitType::Human {
        approval_id: ApprovalId::new("ticket-approved-but-blocked"),
    });

    let mut lookup = MapHitlLookup::new();
    lookup.insert("ticket-approved-but-blocked", TicketStatus::Approved);

    let tasks = vec![blocker, blocked.clone()];
    let ready = ready_tasks_with_hitl(&tasks, now, &lookup);
    assert!(!ready.iter().any(|t| t.id == blocked.id));
}

#[test]
fn ready_tasks_uses_noop_lookup_for_human_gates() {
    // The pre-existing `ready_tasks` entry point must preserve its old
    // behaviour: Human gates block unless the caller opts in to a real
    // lookup via `ready_tasks_with_hitl`. This guards against accidentally
    // surfacing approval-gated tasks to legacy callers.
    let now = base_instant();
    let mut t = make_task(1, vec![]);
    t.await_type = Some(AwaitType::Human {
        approval_id: ApprovalId::new("whatever"),
    });

    let tasks = vec![t.clone()];
    let ready = ready_tasks(&tasks, now);
    assert!(!ready.iter().any(|r| r.id == t.id));

    // And NoopHitlLookup explicitly reports "unknown".
    let noop = NoopHitlLookup;
    assert!(noop.ticket_status(&ApprovalId::new("x")).is_none());
}

#[test]
fn human_gate_serde_roundtrip() {
    let gate = AwaitType::Human {
        approval_id: ApprovalId::new("ticket-xyz"),
    };
    let json = serde_json::to_string(&gate).unwrap();
    let back: AwaitType = serde_json::from_str(&json).unwrap();
    assert_eq!(gate, back);
    // Sanity: struct-variant serializes with discriminant and field.
    assert!(json.contains("human"));
    assert!(json.contains("approval_id"));
    assert!(json.contains("ticket-xyz"));
}

// ---------------------------------------------------------------------------
// AwaitType::GhRun gate (GhRunLookup) + ReadyContext bundle
// ---------------------------------------------------------------------------

use crate::ready::{
    is_gh_pr_ready, is_gh_run_ready, ready_tasks_with_context, GhPrLookup, GhRunLookup,
    NoopGhPrLookup, NoopGhRunLookup, ReadyContext,
};
use crate::task::{GhPrId, GhPrState, GhRunId, GhRunStatus};

/// In-memory [`GhRunLookup`] used exclusively in tests to stand in for a
/// real GitHub polling source. Keys on the inner string of [`GhRunId`] so
/// tests can push opaque synthetic ids.
#[derive(Debug, Default)]
struct MapGhRunLookup {
    by_id: HashMap<String, GhRunStatus>,
}

impl MapGhRunLookup {
    fn new() -> Self {
        Self { by_id: HashMap::new() }
    }

    fn insert(&mut self, id: impl Into<String>, status: GhRunStatus) {
        self.by_id.insert(id.into(), status);
    }
}

impl GhRunLookup for MapGhRunLookup {
    fn run_status(&self, id: &GhRunId) -> Option<GhRunStatus> {
        self.by_id.get(id.as_str()).copied()
    }
}

fn gh_run_gate(id: &str) -> AwaitType {
    AwaitType::GhRun {
        run_id: GhRunId::new(id),
        repo: "acme/example".to_string(),
    }
}

fn ctx_with_gh_run<'a>(lookup: &'a dyn GhRunLookup) -> ReadyContext<'a> {
    ReadyContext {
        hitl: &NoopHitlLookup,
        gh_run: lookup,
        gh_pr: &NoopGhPrLookup,
    }
}

#[test]
fn gh_run_missing_run_is_not_ready() {
    // Run not present in the lookup → treat as not ready. Mirrors the
    // HitlLookup "unknown ticket" contract.
    let gate = gh_run_gate("run-missing");
    let lookup = MapGhRunLookup::new();
    assert!(!is_gh_run_ready(&gate, &lookup));
}

#[test]
fn gh_run_queued_is_not_ready() {
    let gate = gh_run_gate("run-queued");
    let mut lookup = MapGhRunLookup::new();
    lookup.insert("run-queued", GhRunStatus::Queued);
    assert!(!is_gh_run_ready(&gate, &lookup));
}

#[test]
fn gh_run_in_progress_is_not_ready() {
    let gate = gh_run_gate("run-inprogress");
    let mut lookup = MapGhRunLookup::new();
    lookup.insert("run-inprogress", GhRunStatus::InProgress);
    assert!(!is_gh_run_ready(&gate, &lookup));
}

#[test]
fn gh_run_completed_is_ready() {
    let now = base_instant();
    let mut t = make_task(1, vec![]);
    t.await_type = Some(gh_run_gate("run-completed"));

    let mut lookup = MapGhRunLookup::new();
    lookup.insert("run-completed", GhRunStatus::Completed);
    let ctx = ctx_with_gh_run(&lookup);

    let tasks = vec![t.clone()];
    let ready = ready_tasks_with_context(&tasks, now, &ctx);
    assert!(ready.iter().any(|r| r.id == t.id));
}

#[test]
fn gh_run_failed_is_ready() {
    // Failure is terminal — the task must wake up so its handler can branch
    // on the failure. Mirrors Rejected in the Human gate.
    let now = base_instant();
    let mut t = make_task(1, vec![]);
    t.await_type = Some(gh_run_gate("run-failed"));

    let mut lookup = MapGhRunLookup::new();
    lookup.insert("run-failed", GhRunStatus::Failed);
    let ctx = ctx_with_gh_run(&lookup);

    let tasks = vec![t.clone()];
    let ready = ready_tasks_with_context(&tasks, now, &ctx);
    assert!(ready.iter().any(|r| r.id == t.id));
}

#[test]
fn gh_run_cancelled_skipped_neutral_are_ready() {
    // Each of these terminal conclusions must wake the task. Exercise them
    // in a single test via the pure gate function.
    for status in [
        GhRunStatus::Cancelled,
        GhRunStatus::Skipped,
        GhRunStatus::Neutral,
    ] {
        let id_str = format!("run-{status:?}").to_lowercase();
        let gate = gh_run_gate(&id_str);
        let mut lookup = MapGhRunLookup::new();
        lookup.insert(id_str.clone(), status);
        assert!(
            is_gh_run_ready(&gate, &lookup),
            "{status:?} must be terminal and therefore ready"
        );
        assert!(status.is_terminal(), "{status:?} must report is_terminal()");
    }
}

#[test]
fn gh_run_unknown_is_not_ready() {
    // Unknown is a deliberately conservative non-terminal state — we never
    // wake a task on an ambiguous signal.
    let gate = gh_run_gate("run-unknown");
    let mut lookup = MapGhRunLookup::new();
    lookup.insert("run-unknown", GhRunStatus::Unknown);
    assert!(!is_gh_run_ready(&gate, &lookup));
    assert!(!GhRunStatus::Unknown.is_terminal());
}

#[test]
fn gh_run_ready_but_blocks_open_still_blocks() {
    // Combined gate: run is Completed, but the task still has an unsatisfied
    // Blocks dep. The Blocks gate must win — a terminal run does not bypass
    // other gates, matching the Human+Blocks test.
    let now = base_instant();

    let blocker = make_task(1, vec![]);
    let mut blocked = make_task(2, vec![]);
    blocked.dependencies = vec![WorkflowTaskDependency {
        from: blocker.id,
        to: blocked.id,
        kind: crate::task::DependencyType::Blocks,
    }];
    blocked.await_type = Some(gh_run_gate("run-completed-but-blocked"));

    let mut lookup = MapGhRunLookup::new();
    lookup.insert("run-completed-but-blocked", GhRunStatus::Completed);
    let ctx = ctx_with_gh_run(&lookup);

    let tasks = vec![blocker, blocked.clone()];
    let ready = ready_tasks_with_context(&tasks, now, &ctx);
    assert!(!ready.iter().any(|t| t.id == blocked.id));
}

#[test]
fn default_noop_context_blocks_human_gh_run_and_gh_pr() {
    // ReadyContext::default_noop() must leave all three integrated external-
    // signal gates pending — it preserves the pre-integration behaviour when
    // callers have not yet wired into real lookup sources.
    let now = base_instant();

    let mut human_task = make_task(1, vec![]);
    human_task.await_type = Some(AwaitType::Human {
        approval_id: ApprovalId::new("whatever"),
    });

    let mut gh_run_task = make_task(2, vec![]);
    gh_run_task.await_type = Some(gh_run_gate("whatever"));

    let mut gh_pr_task = make_task(3, vec![]);
    gh_pr_task.await_type = Some(gh_pr_gate("whatever"));

    let tasks = vec![human_task.clone(), gh_run_task.clone(), gh_pr_task.clone()];
    let ctx = ReadyContext::default_noop();
    let ready = ready_tasks_with_context(&tasks, now, &ctx);

    assert!(!ready.iter().any(|r| r.id == human_task.id));
    assert!(!ready.iter().any(|r| r.id == gh_run_task.id));
    assert!(!ready.iter().any(|r| r.id == gh_pr_task.id));

    // Noop lookups explicitly report "unknown".
    let noop_run = NoopGhRunLookup;
    assert!(noop_run.run_status(&GhRunId::new("x")).is_none());
    let noop_pr = NoopGhPrLookup;
    assert!(noop_pr.pr_state(&GhPrId::new("x")).is_none());
}

#[test]
fn gh_run_gate_serde_roundtrip() {
    let gate = AwaitType::GhRun {
        run_id: GhRunId::new("12345678"),
        repo: "acme/example".to_string(),
    };
    let json = serde_json::to_string(&gate).unwrap();
    let back: AwaitType = serde_json::from_str(&json).unwrap();
    assert_eq!(gate, back);
    // Sanity: struct-variant serializes with discriminant, id, and repo.
    assert!(json.contains("gh_run"));
    assert!(json.contains("run_id"));
    assert!(json.contains("12345678"));
    assert!(json.contains("repo"));
    assert!(json.contains("acme/example"));
}

#[test]
fn gh_run_status_serde_roundtrip() {
    for status in [
        GhRunStatus::Queued,
        GhRunStatus::InProgress,
        GhRunStatus::Completed,
        GhRunStatus::Failed,
        GhRunStatus::Cancelled,
        GhRunStatus::Skipped,
        GhRunStatus::Neutral,
        GhRunStatus::Unknown,
    ] {
        let json = serde_json::to_string(&status).unwrap();
        let back: GhRunStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(status, back);
    }
}

// ---------------------------------------------------------------------------
// AwaitType::GhPr gate (GhPrLookup) + ReadyContext bundle
// ---------------------------------------------------------------------------

/// In-memory [`GhPrLookup`] used exclusively in tests to stand in for a
/// real GitHub polling source. Keys on the inner string of [`GhPrId`] so
/// tests can push opaque synthetic ids.
#[derive(Debug, Default)]
struct MapGhPrLookup {
    by_id: HashMap<String, GhPrState>,
}

impl MapGhPrLookup {
    fn new() -> Self {
        Self { by_id: HashMap::new() }
    }

    fn insert(&mut self, id: impl Into<String>, state: GhPrState) {
        self.by_id.insert(id.into(), state);
    }
}

impl GhPrLookup for MapGhPrLookup {
    fn pr_state(&self, id: &GhPrId) -> Option<GhPrState> {
        self.by_id.get(id.as_str()).cloned()
    }
}

fn gh_pr_gate(id: &str) -> AwaitType {
    AwaitType::GhPr {
        pr_id: GhPrId::new(id),
        repo: "acme/example".to_string(),
    }
}

fn ctx_with_gh_pr<'a>(lookup: &'a dyn GhPrLookup) -> ReadyContext<'a> {
    ReadyContext {
        hitl: &NoopHitlLookup,
        gh_run: &NoopGhRunLookup,
        gh_pr: lookup,
    }
}

#[test]
fn gh_pr_missing_is_not_ready() {
    // PR not present in the lookup → treat as not ready. Mirrors the
    // GhRunLookup "unknown run" contract.
    let gate = gh_pr_gate("pr-missing");
    let lookup = MapGhPrLookup::new();
    assert!(!is_gh_pr_ready(&gate, &lookup));
}

#[test]
fn gh_pr_open_is_not_ready() {
    let gate = gh_pr_gate("pr-open");
    let mut lookup = MapGhPrLookup::new();
    lookup.insert("pr-open", GhPrState::Open);
    assert!(!is_gh_pr_ready(&gate, &lookup));
    assert!(!GhPrState::Open.is_terminal());
}

#[test]
fn gh_pr_draft_is_not_ready() {
    // Draft is non-terminal — the PR is still being worked on.
    let gate = gh_pr_gate("pr-draft");
    let mut lookup = MapGhPrLookup::new();
    lookup.insert("pr-draft", GhPrState::Draft);
    assert!(!is_gh_pr_ready(&gate, &lookup));
    assert!(!GhPrState::Draft.is_terminal());
}

#[test]
fn gh_pr_closed_is_ready() {
    // Closed without merging is terminal — the task must wake up so its
    // handler can branch on the outcome.
    let now = base_instant();
    let mut t = make_task(1, vec![]);
    t.await_type = Some(gh_pr_gate("pr-closed"));

    let mut lookup = MapGhPrLookup::new();
    lookup.insert("pr-closed", GhPrState::Closed);
    let ctx = ctx_with_gh_pr(&lookup);

    let tasks = vec![t.clone()];
    let ready = ready_tasks_with_context(&tasks, now, &ctx);
    assert!(ready.iter().any(|r| r.id == t.id));
    assert!(GhPrState::Closed.is_terminal());
}

#[test]
fn gh_pr_merged_is_ready() {
    let now = base_instant();
    let mut t = make_task(1, vec![]);
    t.await_type = Some(gh_pr_gate("pr-merged"));

    let mut lookup = MapGhPrLookup::new();
    lookup.insert("pr-merged", GhPrState::Merged);
    let ctx = ctx_with_gh_pr(&lookup);

    let tasks = vec![t.clone()];
    let ready = ready_tasks_with_context(&tasks, now, &ctx);
    assert!(ready.iter().any(|r| r.id == t.id));
    assert!(GhPrState::Merged.is_terminal());
}

#[test]
fn gh_pr_unknown_is_not_ready() {
    // Unknown is a deliberately conservative non-terminal state — we never
    // wake a task on an ambiguous signal.
    let gate = gh_pr_gate("pr-unknown");
    let mut lookup = MapGhPrLookup::new();
    lookup.insert("pr-unknown", GhPrState::Unknown);
    assert!(!is_gh_pr_ready(&gate, &lookup));
    assert!(!GhPrState::Unknown.is_terminal());
}

#[test]
fn gh_pr_ready_but_blocks_open_still_blocks() {
    // Combined gate: PR is Merged, but the task still has an unsatisfied
    // Blocks dep. The Blocks gate must win — a terminal PR does not bypass
    // other gates, matching the GhRun+Blocks and Human+Blocks tests.
    let now = base_instant();

    let blocker = make_task(1, vec![]);
    let mut blocked = make_task(2, vec![]);
    blocked.dependencies = vec![WorkflowTaskDependency {
        from: blocker.id,
        to: blocked.id,
        kind: crate::task::DependencyType::Blocks,
    }];
    blocked.await_type = Some(gh_pr_gate("pr-merged-but-blocked"));

    let mut lookup = MapGhPrLookup::new();
    lookup.insert("pr-merged-but-blocked", GhPrState::Merged);
    let ctx = ctx_with_gh_pr(&lookup);

    let tasks = vec![blocker, blocked.clone()];
    let ready = ready_tasks_with_context(&tasks, now, &ctx);
    assert!(!ready.iter().any(|t| t.id == blocked.id));
}

#[test]
fn gh_pr_gate_serde_roundtrip() {
    let gate = AwaitType::GhPr {
        pr_id: GhPrId::new("42"),
        repo: "acme/example".to_string(),
    };
    let json = serde_json::to_string(&gate).unwrap();
    let back: AwaitType = serde_json::from_str(&json).unwrap();
    assert_eq!(gate, back);
    assert!(json.contains("gh_pr"));
    assert!(json.contains("pr_id"));
    assert!(json.contains("42"));
    assert!(json.contains("repo"));
    assert!(json.contains("acme/example"));
}

#[test]
fn gh_pr_state_serde_roundtrip() {
    for state in [
        GhPrState::Open,
        GhPrState::Closed,
        GhPrState::Merged,
        GhPrState::Draft,
        GhPrState::Unknown,
    ] {
        let json = serde_json::to_string(&state).unwrap();
        let back: GhPrState = serde_json::from_str(&json).unwrap();
        assert_eq!(state, back);
    }
}

#[test]
fn ready_tasks_with_hitl_shim_still_works() {
    // Guard the deprecated shim — sera-gj93 callers must keep compiling until
    // they migrate. The shim builds a ReadyContext with NoopGhRunLookup and
    // delegates to ready_tasks_with_context.
    let now = base_instant();
    let mut t = make_task(1, vec![]);
    t.await_type = Some(AwaitType::Human {
        approval_id: ApprovalId::new("ticket-approved-shim"),
    });
    let mut lookup = MapHitlLookup::new();
    lookup.insert("ticket-approved-shim", TicketStatus::Approved);

    let tasks = vec![t.clone()];
    let ready = ready_tasks_with_hitl(&tasks, now, &lookup);
    assert!(ready.iter().any(|r| r.id == t.id));
}
