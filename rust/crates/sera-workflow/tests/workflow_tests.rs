use chrono::{DateTime, TimeZone, Utc};
use sera_workflow::{
    claim::{claim_task, confirm_claim, ClaimError, StaleClaimReaper},
    ready::ready_tasks,
    task::{
        DependencyType, WorkflowTask, WorkflowTaskDependency, WorkflowTaskId,
        WorkflowTaskStatus, WorkflowTaskType,
    },
    termination::{check_termination, TerminationConfig, TerminationReason, TerminationState},
};
use sera_types::evolution::BlastRadius;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 6, 1, 12, 0, 0).unwrap()
}

fn make_task(title: &str, priority: u8) -> WorkflowTask {
    WorkflowTask::new(sera_workflow::task::WorkflowTaskInput {
        title: title.to_string(),
        description: "description".to_string(),
        acceptance_criteria: vec!["ac1".to_string()],
        status: WorkflowTaskStatus::Open,
        priority,
        task_type: WorkflowTaskType::Chore,
        source_formula: None,
        source_location: None,
        created_at: now(),
    })
}

// ---------------------------------------------------------------------------
// 1. WorkflowTaskId — content hash stability
// ---------------------------------------------------------------------------

#[test]
fn workflow_task_id_content_hash_stable() {
    let t1 = make_task("fix login", 1);
    let t2 = make_task("fix login", 1);
    assert_eq!(t1.id, t2.id, "same content should produce the same hash");
}

// ---------------------------------------------------------------------------
// 2. WorkflowTaskId — hex roundtrip
// ---------------------------------------------------------------------------

#[test]
fn workflow_task_id_hex_roundtrip() {
    let task = make_task("roundtrip", 0);
    let hex = task.id.to_string();
    let parsed: WorkflowTaskId = hex.parse().expect("should parse hex string");
    assert_eq!(task.id, parsed);
}

// ---------------------------------------------------------------------------
// 3. WorkflowTaskId — differs on title change
// ---------------------------------------------------------------------------

#[test]
fn workflow_task_id_differs_on_title_change() {
    let t1 = make_task("title A", 0);
    let t2 = make_task("title B", 0);
    assert_ne!(t1.id, t2.id, "different title should produce different hash");
}

// ---------------------------------------------------------------------------
// 4. ready_tasks — closed blocker unblocks task
// ---------------------------------------------------------------------------

#[test]
fn ready_tasks_no_open_blockers() {
    let mut blocker = make_task("blocker", 0);
    blocker.status = WorkflowTaskStatus::Closed;

    let mut blocked = make_task("blocked", 1);
    blocked.dependencies.push(WorkflowTaskDependency {
        from: blocker.id,
        to: blocked.id,
        kind: DependencyType::Blocks,
    });

    let tasks = vec![blocker, blocked];
    let ready = ready_tasks(&tasks, now());
    // Only the blocked task was Open; blocker is Closed so it is ready.
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].title, "blocked");
}

// ---------------------------------------------------------------------------
// 5. ready_tasks — ConditionalBlocks satisfied when blocker closed
// ---------------------------------------------------------------------------

#[test]
fn ready_tasks_conditional_blocks_satisfied_when_blocker_closed() {
    let mut blocker = make_task("conditional blocker", 0);
    blocker.status = WorkflowTaskStatus::Closed;

    let mut target = make_task("target", 1);
    target.dependencies.push(WorkflowTaskDependency {
        from: blocker.id,
        to: target.id,
        kind: DependencyType::ConditionalBlocks,
    });

    let tasks = vec![blocker, target];
    let ready = ready_tasks(&tasks, now());
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].title, "target");
}

// ---------------------------------------------------------------------------
// 6. ready_tasks — ConditionalBlocks not satisfied when blocker open
// ---------------------------------------------------------------------------

#[test]
fn ready_tasks_conditional_blocks_not_satisfied_when_blocker_open() {
    let blocker = make_task("open blocker", 0);

    let mut target = make_task("target", 1);
    target.dependencies.push(WorkflowTaskDependency {
        from: blocker.id,
        to: target.id,
        kind: DependencyType::ConditionalBlocks,
    });

    let tasks = vec![blocker, target];
    let ready = ready_tasks(&tasks, now());
    // blocker is Open so target is still blocked; only blocker should be ready.
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].title, "open blocker");
}

// ---------------------------------------------------------------------------
// 7. ready_tasks — defer_until in future → not ready
// ---------------------------------------------------------------------------

#[test]
fn ready_tasks_defer_until_future() {
    let mut task = make_task("deferred", 0);
    task.defer_until = Some(now() + chrono::Duration::hours(1));

    let tasks = vec![task];
    let ready = ready_tasks(&tasks, now());
    assert!(ready.is_empty(), "deferred task should not appear in ready list");
}

// ---------------------------------------------------------------------------
// 8. ready_tasks — sorted by priority ASC
// ---------------------------------------------------------------------------

#[test]
fn ready_tasks_sorted_by_priority() {
    let low = make_task("low priority", 10);
    let high = make_task("high priority", 1);
    let mid = make_task("mid priority", 5);

    let tasks = vec![low, high, mid];
    let ready = ready_tasks(&tasks, now());
    assert_eq!(ready.len(), 3);
    assert_eq!(ready[0].title, "high priority");
    assert_eq!(ready[1].title, "mid priority");
    assert_eq!(ready[2].title, "low priority");
}

// ---------------------------------------------------------------------------
// 9. atomic_claim_transitions_to_hooked
// ---------------------------------------------------------------------------

#[test]
fn atomic_claim_transitions_to_hooked() {
    let task = make_task("claimable", 0);
    let task_id = task.id;
    let mut tasks = vec![task];

    let token = claim_task(&mut tasks, &task_id, "agent-1", now())
        .expect("claim should succeed");

    assert_eq!(tasks[0].status, WorkflowTaskStatus::Hooked);
    assert_eq!(tasks[0].assignee.as_deref(), Some("agent-1"));
    assert_eq!(token.task_id, task_id);
    assert_eq!(token.agent_id, "agent-1");
}

// ---------------------------------------------------------------------------
// 10. double_claim_returns_already_claimed
// ---------------------------------------------------------------------------

#[test]
fn double_claim_returns_already_claimed() {
    let task = make_task("double claim", 0);
    let task_id = task.id;
    let mut tasks = vec![task];

    claim_task(&mut tasks, &task_id, "agent-1", now()).unwrap();
    let err = claim_task(&mut tasks, &task_id, "agent-2", now())
        .expect_err("second claim should fail");

    assert!(
        matches!(err, ClaimError::AlreadyClaimed { .. }),
        "expected AlreadyClaimed, got {err:?}"
    );
}

// ---------------------------------------------------------------------------
// 11. workflow_task_status_hooked_to_inprogress_via_confirm
// ---------------------------------------------------------------------------

#[test]
fn workflow_task_status_hooked_to_inprogress_via_confirm() {
    let task = make_task("confirm me", 0);
    let task_id = task.id;
    let mut tasks = vec![task];

    let token = claim_task(&mut tasks, &task_id, "agent-1", now()).unwrap();
    confirm_claim(&mut tasks, &token).expect("confirm should succeed");

    assert_eq!(tasks[0].status, WorkflowTaskStatus::InProgress);
}

// ---------------------------------------------------------------------------
// 12. termination_triad_fires_on_each_condition
// ---------------------------------------------------------------------------

#[test]
fn termination_triad_fires_on_each_condition() {
    // N-round exceeded.
    let config = TerminationConfig { max_rounds: Some(5), max_cost_usd: None };
    let state = TerminationState { rounds_elapsed: 5, cost_usd_accumulated: 0.0, consecutive_idle_rounds: 0 };
    assert_eq!(check_termination(&config, &state), Some(TerminationReason::NRoundExceeded));

    // Idle — 3 consecutive idle rounds.
    let config2 = TerminationConfig { max_rounds: None, max_cost_usd: None };
    let state2 = TerminationState { rounds_elapsed: 1, cost_usd_accumulated: 0.0, consecutive_idle_rounds: 3 };
    assert_eq!(check_termination(&config2, &state2), Some(TerminationReason::Idle));

    // Budget exhausted.
    let config3 = TerminationConfig { max_rounds: None, max_cost_usd: Some(1.0) };
    let state3 = TerminationState { rounds_elapsed: 0, cost_usd_accumulated: 1.0, consecutive_idle_rounds: 0 };
    assert_eq!(check_termination(&config3, &state3), Some(TerminationReason::BudgetExhausted));

    // No condition met.
    let config4 = TerminationConfig { max_rounds: Some(10), max_cost_usd: Some(5.0) };
    let state4 = TerminationState { rounds_elapsed: 2, cost_usd_accumulated: 0.5, consecutive_idle_rounds: 1 };
    assert_eq!(check_termination(&config4, &state4), None);
}

// ---------------------------------------------------------------------------
// 13. meta_scope_blast_radius_serde_roundtrip
// ---------------------------------------------------------------------------

#[test]
fn meta_scope_blast_radius_serde_roundtrip() {
    let mut task = make_task("meta task", 0);
    task.meta_scope = Some(BlastRadius::AgentManifest);

    let json = serde_json::to_string(&task).expect("serialize");
    let back: WorkflowTask = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(back.meta_scope, Some(BlastRadius::AgentManifest));
    assert_eq!(back.title, "meta task");
}

// ---------------------------------------------------------------------------
// 14. stale_claim_reaper_resets_timed_out_hooked_tasks
// ---------------------------------------------------------------------------

#[test]
fn stale_claim_reaper_resets_timed_out_hooked_tasks() {
    let task = make_task("stale", 0);
    // Claim the task.
    let task_id = task.id;
    let mut tasks = vec![task.clone()];
    claim_task(&mut tasks, &task_id, "agent-1", now()).unwrap();

    // Manually store a hooked_at timestamp in metadata that is well in the past.
    let old_hooked_at = now() - chrono::Duration::minutes(10);
    tasks[0].metadata = serde_json::json!({
        "hooked_at": old_hooked_at.to_rfc3339()
    });

    let reaper = StaleClaimReaper::new(std::time::Duration::from_secs(60)); // 1 minute
    let reset = reaper.reap_stale(&mut tasks, now());

    assert_eq!(reset, 1);
    assert_eq!(tasks[0].status, WorkflowTaskStatus::Open);
    assert!(tasks[0].assignee.is_none());
}
