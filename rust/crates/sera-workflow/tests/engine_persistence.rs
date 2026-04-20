//! Integration tests for the rusqlite-backed [`WorkflowEngine`].
//!
//! Covers:
//! (a) submit → claim → complete round-trip persisted on disk,
//! (b) orphan recovery after a simulated worker crash (claim without
//!     completing; heartbeat cutoff fires).

use std::sync::Arc;

use chrono::{Duration, TimeZone, Utc};
use tempfile::TempDir;

use sera_workflow::engine::{SqliteWorkflowBackend, WorkflowEngine};
use sera_workflow::task::{
    WorkflowTask, WorkflowTaskInput, WorkflowTaskStatus, WorkflowTaskType,
};

fn make_task(title: &str, priority: u8) -> WorkflowTask {
    WorkflowTask::new(WorkflowTaskInput {
        title: title.to_string(),
        description: "integration-test task".to_string(),
        acceptance_criteria: vec!["ac".to_string()],
        status: WorkflowTaskStatus::Open,
        priority,
        task_type: WorkflowTaskType::Chore,
        source_formula: None,
        source_location: None,
        created_at: Utc.with_ymd_and_hms(2025, 6, 1, 12, 0, 0).unwrap(),
    })
}

#[tokio::test]
async fn submit_claim_complete_round_trip_on_disk() {
    let tmp = TempDir::new().expect("tempdir");
    let db_path = tmp.path().join("workflow.sqlite");

    // -- Writer lane: submit + claim + complete -----------------------------
    let id = {
        let backend =
            Arc::new(SqliteWorkflowBackend::open(&db_path).expect("open sqlite"));
        let engine = WorkflowEngine::new(backend);

        let id = engine.submit_task(make_task("top", 0)).await.unwrap();
        // second lower-priority task to make sure claim picks priority 0 first
        engine.submit_task(make_task("bot", 3)).await.unwrap();

        let token = engine
            .claim_next_ready("worker-A", Utc::now())
            .await
            .unwrap()
            .expect("ready task exists");
        assert_eq!(token.task_id, id, "priority=0 wins the claim");
        assert_eq!(token.agent_id, "worker-A");

        engine
            .mark_complete(&id, serde_json::json!({ "ok": true }))
            .await
            .unwrap();

        id
    };

    // -- Reader lane: fresh handle on the same file sees the persisted row --
    let backend =
        Arc::new(SqliteWorkflowBackend::open(&db_path).expect("reopen sqlite"));
    let engine = WorkflowEngine::new(backend);

    let loaded = engine.load(&id).await.unwrap();
    assert_eq!(loaded.id, id);
    assert_eq!(loaded.status, WorkflowTaskStatus::Closed);
}

#[tokio::test]
async fn orphan_recovery_after_simulated_crash() {
    let tmp = TempDir::new().expect("tempdir");
    let db_path = tmp.path().join("workflow.sqlite");
    let backend =
        Arc::new(SqliteWorkflowBackend::open(&db_path).expect("open sqlite"));
    let engine = WorkflowEngine::new(backend);

    let claim_time = Utc.with_ymd_and_hms(2025, 6, 1, 12, 0, 0).unwrap();
    let id = engine.submit_task(make_task("orphan-me", 1)).await.unwrap();

    // Claim — simulating a worker that starts the work, then crashes before
    // calling mark_complete / mark_failed.
    let token = engine
        .claim_next_ready("worker-A", claim_time)
        .await
        .unwrap()
        .expect("ready");
    assert_eq!(token.task_id, id);

    // Status is Hooked; a second claim should find nothing ready.
    let second = engine
        .claim_next_ready("worker-B", claim_time + Duration::seconds(1))
        .await
        .unwrap();
    assert!(
        second.is_none(),
        "Hooked task is not re-claimable until the heartbeat cutoff fires"
    );

    let loaded = engine.load(&id).await.unwrap();
    assert_eq!(loaded.status, WorkflowTaskStatus::Hooked);

    // Cutoff BEFORE the claim time — nothing orphaned yet.
    let too_early = claim_time - Duration::seconds(60);
    let recovered = engine.recover_orphans(too_early).await.unwrap();
    assert!(
        recovered.is_empty(),
        "cutoff predates the claim — nothing reaped"
    );

    // Cutoff AFTER the claim — reaper flips it back to Open.
    let late_cutoff = claim_time + Duration::seconds(300);
    let recovered = engine.recover_orphans(late_cutoff).await.unwrap();
    assert_eq!(recovered, vec![id], "exactly the crashed task is recovered");

    let reloaded = engine.load(&id).await.unwrap();
    assert_eq!(reloaded.status, WorkflowTaskStatus::Open);
    assert!(
        reloaded.assignee.is_none(),
        "assignee cleared when claim is reaped"
    );

    // After recovery, a fresh worker can claim it again.
    let retry = engine
        .claim_next_ready("worker-B", late_cutoff + Duration::seconds(1))
        .await
        .unwrap()
        .expect("ready after reap");
    assert_eq!(retry.task_id, id);
    assert_eq!(retry.agent_id, "worker-B");
}
