//! Edge-case tests for Emitter: deep nesting, trace propagation through
//! multi-level chains, clone independence, and EventMeta serde.

use sera_telemetry::emitter::{Emitter, EventMeta};

// ---------------------------------------------------------------------------
// Deep nesting
// ---------------------------------------------------------------------------

#[test]
fn deep_child_chain_builds_correct_namespace() {
    let e = Emitter::root()
        .child("a")
        .child("b")
        .child("c")
        .child("d");
    assert_eq!(e.namespace(), "sera.a.b.c.d");
}

#[test]
fn child_of_traced_emitter_inherits_trace_at_all_depths() {
    let root = Emitter::root().with_trace("00-trace123-01");
    let level1 = root.child("worker");
    let level2 = level1.child("sandbox");
    let level3 = level2.child("tool");

    assert_eq!(level1.trace(), Some("00-trace123-01"));
    assert_eq!(level2.trace(), Some("00-trace123-01"));
    assert_eq!(level3.trace(), Some("00-trace123-01"));
}

// ---------------------------------------------------------------------------
// with_trace replaces trace on the same node without changing namespace
// ---------------------------------------------------------------------------

#[test]
fn with_trace_on_child_overrides_parent_trace() {
    let root = Emitter::root().with_trace("00-parent-01");
    let child = root.child("lane");
    // Override trace on the child itself
    let child_retraced = child.with_trace("00-child-01");

    assert_eq!(child.namespace(), "sera.lane");
    assert_eq!(child_retraced.namespace(), "sera.lane");
    assert_eq!(child_retraced.trace(), Some("00-child-01"));
}

#[test]
fn with_trace_does_not_mutate_original() {
    let root = Emitter::root();
    let traced = root.with_trace("00-xyz-01");
    // Original root should still have no trace
    assert_eq!(root.trace(), None);
    assert_eq!(traced.trace(), Some("00-xyz-01"));
}

// ---------------------------------------------------------------------------
// Clone independence
// ---------------------------------------------------------------------------

#[test]
fn cloned_emitter_shares_namespace_and_trace() {
    let e = Emitter::root().with_trace("00-abc-01").child("events");
    let cloned = e.clone();
    assert_eq!(cloned.namespace(), e.namespace());
    assert_eq!(cloned.trace(), e.trace());
}

// ---------------------------------------------------------------------------
// EventMeta fields
// ---------------------------------------------------------------------------

#[test]
fn event_meta_without_trace_has_none_trace() {
    let e = Emitter::root().child("queue");
    let meta = e.event_meta("enqueued", "TaskEnqueued");
    assert!(meta.trace.is_none());
    assert_eq!(meta.data_type, "TaskEnqueued");
}

#[test]
fn event_meta_created_at_is_recent() {
    use time::OffsetDateTime;
    let before = OffsetDateTime::now_utc();
    let meta = Emitter::root().event_meta("ping", "Ping");
    let after = OffsetDateTime::now_utc();
    assert!(meta.created_at >= before);
    assert!(meta.created_at <= after);
}

#[test]
fn event_meta_id_is_nonzero_uuid() {
    let meta = Emitter::root().event_meta("start", "Start");
    // A new UUIDv4 should never be the nil UUID.
    assert_ne!(meta.id, uuid::Uuid::nil());
}

#[test]
fn event_meta_json_roundtrip() {
    let e = Emitter::root().with_trace("00-rtrip-01").child("serde");
    let meta = e.event_meta("test_event", "TestPayload");

    let json = serde_json::to_string(&meta).expect("serialize EventMeta");
    let restored: EventMeta = serde_json::from_str(&json).expect("deserialize EventMeta");

    assert_eq!(restored.id, meta.id);
    assert_eq!(restored.name, meta.name);
    assert_eq!(restored.path, meta.path);
    assert_eq!(restored.data_type, meta.data_type);
    assert_eq!(restored.trace, meta.trace);
}

// ---------------------------------------------------------------------------
// Empty segment edge case
// ---------------------------------------------------------------------------

#[test]
fn child_with_empty_segment_still_appends_dot() {
    // Not a recommended usage, but must not panic.
    let e = Emitter::root().child("");
    assert_eq!(e.namespace(), "sera.");
}
