//! End-to-end integration test for the SQLite-backed WorkflowTaskStore
//! (sera-d2xh) — verifies a Timer task survives a "restart" via tempfile
//! reopen and resolves on the next scheduler tick, then stays resolved
//! across another reopen.

use std::sync::Arc;

use chrono::{Duration as ChronoDuration, Utc};

use sera_gateway::scheduler::tick;
use sera_gateway::workflow_store::{
    SchedulerTaskStatus, SqliteWorkflowTaskStore, WorkflowTaskRecord, WorkflowTaskStore,
};
use sera_mail::lookup::InMemoryMailLookup;
use sera_workflow::task::WorkflowTaskInput;
use sera_workflow::{AwaitType, WorkflowTask, WorkflowTaskStatus, WorkflowTaskType};

fn make_timer_task(secs_offset: i64) -> WorkflowTask {
    let now = Utc::now();
    let mut task = WorkflowTask::new(WorkflowTaskInput {
        title: "sqlite-persistence-test".into(),
        description: "Wave E — sera-d2xh integration".into(),
        acceptance_criteria: Vec::new(),
        status: WorkflowTaskStatus::Open,
        priority: 5,
        task_type: WorkflowTaskType::Meta,
        source_formula: None,
        source_location: None,
        created_at: now,
    });
    task.await_type = Some(AwaitType::Timer {
        not_before: now + ChronoDuration::seconds(secs_offset),
    });
    task
}

#[tokio::test]
async fn sqlite_store_round_trips_through_scheduler_tick() {
    let tmp = tempfile::TempDir::new().unwrap();
    let db_path = tmp.path().join("workflow.sqlite");

    // Create a Timer task with a deadline already in the past so the very
    // next tick must resolve it.
    let task = make_timer_task(-2);
    let task_id = task.id.to_string();

    // Phase 1 — first "process": insert into the persistent store.
    {
        let store = SqliteWorkflowTaskStore::open(&db_path).unwrap();
        store
            .insert(WorkflowTaskRecord {
                task,
                agent_id: "sera".into(),
                resume_token: "tok-persistent".into(),
                status: SchedulerTaskStatus::Pending,
                resolved_at: None,
            })
            .await;
    }

    // Phase 2 — "restart": reopen the same DB, run a single tick, confirm
    // the gate resolved.
    let store = Arc::new(SqliteWorkflowTaskStore::open(&db_path).unwrap());
    let resolved = tick(
        Arc::clone(&store) as Arc<dyn WorkflowTaskStore>,
        Arc::new(InMemoryMailLookup::new()),
        None,
        None,
        None,
    )
    .await;
    assert_eq!(resolved, 1, "elapsed timer must resolve on reopened store");

    let rec = store.get(&task_id).await.expect("record present");
    assert_eq!(rec.status, SchedulerTaskStatus::Resolved);
    assert!(rec.resolved_at.is_some());
    assert_eq!(rec.resume_token, "tok-persistent");
    drop(store);

    // Phase 3 — "second restart": resolved state must round-trip.
    let store = SqliteWorkflowTaskStore::open(&db_path).unwrap();
    let rec = store.get(&task_id).await.expect("record still present");
    assert_eq!(rec.status, SchedulerTaskStatus::Resolved);
    let pending = store.list_pending().await;
    assert!(pending.is_empty(), "resolved task must not appear in list_pending");
}

#[tokio::test]
async fn sqlite_store_blocks_future_timer_across_restart() {
    let tmp = tempfile::TempDir::new().unwrap();
    let db_path = tmp.path().join("workflow.sqlite");

    // Deadline 60s in the future — must stay pending across restart and tick.
    let task = make_timer_task(60);
    let task_id = task.id.to_string();

    {
        let store = SqliteWorkflowTaskStore::open(&db_path).unwrap();
        store
            .insert(WorkflowTaskRecord {
                task,
                agent_id: "sera".into(),
                resume_token: "tok-future".into(),
                status: SchedulerTaskStatus::Pending,
                resolved_at: None,
            })
            .await;
    }

    let store = Arc::new(SqliteWorkflowTaskStore::open(&db_path).unwrap());
    let resolved = tick(
        Arc::clone(&store) as Arc<dyn WorkflowTaskStore>,
        Arc::new(InMemoryMailLookup::new()),
        None,
        None,
        None,
    )
    .await;
    assert_eq!(resolved, 0, "future-deadline timer must not resolve");

    let rec = store.get(&task_id).await.unwrap();
    assert_eq!(rec.status, SchedulerTaskStatus::Pending);
    assert!(rec.resolved_at.is_none());
}
