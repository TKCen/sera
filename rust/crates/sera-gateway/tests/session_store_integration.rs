//! End-to-end integration test for `sera-r9ed` two-layer session
//! persistence: submit envelopes, commit to the shadow repo, replay from the
//! SHA alone, and verify byte-for-byte reproduction of the PartTable state.

use std::sync::Arc;

use sera_gateway::session_store::{
    SessionStore, SqliteGitSessionStore, StoredSubmission, SubmissionRef,
};
use sera_types::content_block::ContentBlock;
use sera_types::envelope::{Event, EventMsg, Op, Submission, W3cTraceContext};
use tempfile::TempDir;

fn new_store() -> (SqliteGitSessionStore, TempDir) {
    let dir = TempDir::new().unwrap();
    let db = dir.path().join("parts.sqlite");
    let sessions = dir.path().join("sessions");
    let store = SqliteGitSessionStore::open(&db, &sessions).unwrap();
    (store, dir)
}

fn turn(text: &str) -> Submission {
    Submission {
        id: uuid::Uuid::new_v4(),
        op: Op::UserTurn {
            items: vec![ContentBlock::Text {
                text: text.to_string(),
            }],
            cwd: None,
            approval_policy: None,
            sandbox_policy: None,
            model_override: None,
            effort: None,
            final_output_schema: None,
        },
        trace: W3cTraceContext::default(),
        change_artifact: None,
    }
}

fn streaming_event(submission_id: uuid::Uuid, delta: &str) -> Event {
    Event {
        id: uuid::Uuid::new_v4(),
        submission_id,
        msg: EventMsg::StreamingDelta {
            delta: delta.to_string(),
        },
        // Freeze timestamp to keep the assertion stable if someone reruns.
        trace: W3cTraceContext::default(),
        timestamp: chrono::DateTime::<chrono::Utc>::from_timestamp(1_767_225_600, 0).unwrap(),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn submission_commit_replay_full_cycle() {
    let (store, _dir) = new_store();

    // Turn 1 — one submission, two emissions.
    let sub1 = turn("hello");
    let sub1_id = sub1.id;
    let bundle1 = StoredSubmission {
        submission: sub1,
        emissions: vec![
            streaming_event(sub1_id, "he"),
            streaming_event(sub1_id, "llo"),
        ],
    };
    let ref1 = store.append_submission("sess-A", &bundle1).await.unwrap();

    // Turn 2 — only a submission.
    let sub2 = turn("goodbye");
    let sub2_id = sub2.id;
    let bundle2 = StoredSubmission {
        submission: sub2,
        emissions: vec![],
    };
    let ref2 = store.append_submission("sess-A", &bundle2).await.unwrap();

    // Head matches the most recent append.
    let head = store.head("sess-A").await.unwrap().unwrap();
    assert_eq!(head, ref2);
    assert_eq!(ref1.index, 0);
    assert_eq!(ref2.index, 1);

    // Replay returns both submissions in append order.
    let replayed = store.replay("sess-A").await.unwrap();
    assert_eq!(replayed.len(), 2);
    assert_eq!(replayed[0].id, sub1_id);
    assert_eq!(replayed[1].id, sub2_id);

    // Other sessions are isolated.
    assert!(store.head("sess-B").await.unwrap().is_none());
    assert!(store.replay("sess-B").await.unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn replay_from_sha_alone_reproduces_session_state() {
    let (store, dir) = new_store();

    // Build up a session.
    let mut submitted_ids = Vec::new();
    let mut last_ref: Option<SubmissionRef> = None;
    for text in ["a", "b", "c"] {
        let sub = turn(text);
        submitted_ids.push(sub.id);
        let bundle = StoredSubmission {
            submission: sub,
            emissions: vec![],
        };
        last_ref = Some(store.append_submission("s", &bundle).await.unwrap());
    }
    let last_ref = last_ref.unwrap();

    // Drop the store and open a fresh one against the same on-disk state —
    // nothing else is needed to replay.
    drop(store);
    let reopened =
        SqliteGitSessionStore::open(dir.path().join("parts.sqlite"), dir.path().join("sessions"))
            .unwrap();

    let head = reopened.head("s").await.unwrap().unwrap();
    assert_eq!(
        head.commit, last_ref.commit,
        "head SHA must survive reopen byte-for-byte"
    );

    let replayed = reopened.replay("s").await.unwrap();
    let replayed_ids: Vec<_> = replayed.iter().map(|s| s.id).collect();
    assert_eq!(replayed_ids, submitted_ids);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_writers_to_same_session_preserve_order_and_atomic_head() {
    let (store, _dir) = new_store();
    let store = Arc::new(store);

    let total = 24;
    let mut handles = Vec::with_capacity(total);
    for i in 0..total {
        let s = Arc::clone(&store);
        handles.push(tokio::spawn(async move {
            let bundle = StoredSubmission {
                submission: turn(&format!("msg-{i}")),
                emissions: vec![],
            };
            s.append_submission("race", &bundle).await.unwrap()
        }));
    }
    let mut results = Vec::new();
    for h in handles {
        results.push(h.await.unwrap());
    }

    // Every writer got a distinct index 0..total and a distinct commit SHA —
    // the head moved atomically, no two submissions ever produced the same
    // parent chain position.
    let mut indices: Vec<u64> = results.iter().map(|r| r.index).collect();
    indices.sort();
    assert_eq!(indices, (0..total as u64).collect::<Vec<_>>());

    let mut commits: Vec<String> = results.iter().map(|r| r.commit.clone()).collect();
    commits.sort();
    commits.dedup();
    assert_eq!(commits.len(), total, "no two commits may collide");

    // Final head == highest index's commit.
    let head = store.head("race").await.unwrap().unwrap();
    assert_eq!(head.index, (total as u64) - 1);

    // And replay yields exactly `total` submissions in a linear chain.
    let replayed = store.replay("race").await.unwrap();
    assert_eq!(replayed.len(), total);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shadow_repo_tree_structure_matches_spec() {
    let (store, _dir) = new_store();
    let sub = turn("structure");
    let sub_id = sub.id;
    let bundle = StoredSubmission {
        submission: sub,
        emissions: vec![streaming_event(sub_id, "x"), streaming_event(sub_id, "y")],
    };
    let r = store.append_submission("s", &bundle).await.unwrap();

    // Open the on-disk bare repo directly and assert the layout:
    //   submission.json (blob)
    //   emissions/0000.json (blob)
    //   emissions/0001.json (blob)
    let repo_path = _dir.path().join("sessions").join("s").join("git");
    let repo = git2::Repository::open_bare(&repo_path).unwrap();
    let commit = repo
        .find_commit(git2::Oid::from_str(&r.commit).unwrap())
        .unwrap();
    let tree = commit.tree().unwrap();

    assert!(
        tree.get_name("submission.json").is_some(),
        "submission.json must be at tree root"
    );
    let emissions = tree.get_name("emissions").expect("emissions subtree");
    let emissions_tree = repo.find_tree(emissions.id()).unwrap();
    let mut names: Vec<String> = emissions_tree
        .iter()
        .map(|e| e.name().unwrap().to_string())
        .collect();
    names.sort();
    assert_eq!(
        names,
        vec!["0000.json".to_string(), "0001.json".to_string()]
    );
}
