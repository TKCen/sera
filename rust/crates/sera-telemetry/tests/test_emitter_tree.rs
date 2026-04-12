//! Verify Emitter namespace tree and LaneCommitProvenance default.

use sera_telemetry::emitter::Emitter;
use sera_telemetry::provenance::LaneCommitProvenance;

#[test]
fn root_namespace_is_sera() {
    let root = Emitter::root();
    assert_eq!(root.namespace(), "sera");
}

#[test]
fn child_appends_dotted_segment() {
    let root = Emitter::root();
    let child = root.child("agent");
    assert_eq!(child.namespace(), "sera.agent");

    let grandchild = child.child("sandbox");
    assert_eq!(grandchild.namespace(), "sera.agent.sandbox");
}

#[test]
fn event_meta_has_correct_path() {
    let emitter = Emitter::root().child("agent");
    let meta = emitter.event_meta("started", "AgentStarted");

    assert_eq!(meta.name, "started");
    assert_eq!(meta.path, "sera.agent.started");
    assert_eq!(meta.data_type, "AgentStarted");
    assert!(meta.trace.is_none());
}

#[test]
fn with_trace_attaches_traceparent() {
    let root = Emitter::root();
    let traced = root.with_trace("00-abc123-def456-01");
    assert_eq!(traced.trace(), Some("00-abc123-def456-01"));

    let meta = traced.event_meta("heartbeat", "Heartbeat");
    assert_eq!(meta.trace.as_deref(), Some("00-abc123-def456-01"));
}

#[test]
fn lane_commit_provenance_default() {
    let prov = LaneCommitProvenance::default();
    assert!(prov.git_commit.is_none());
    assert!(prov.branch.is_none());
    assert!(prov.worktree.is_none());
    assert!(prov.canonical_commit.is_none());
    assert!(prov.superseded_by.is_none());
    assert!(prov.lineage.is_empty());
}
