//! Hierarchical Emitter namespace tree (BeeAI pattern).
//!
//! An `Emitter` represents a dotted namespace path (e.g. `sera.agent.sandbox`)
//! and can attach W3C trace-context headers for distributed tracing.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

/// Inner state of an `Emitter`.
#[derive(Debug, Clone)]
struct EmitterInner {
    /// Dotted namespace path, e.g. `"sera"` or `"sera.agent"`.
    namespace: String,
    /// Optional W3C traceparent header.
    trace: Option<String>,
    /// Reference to the parent emitter (if any).
    parent: Option<Arc<EmitterInner>>,
}

/// A hierarchical emitter node in the SERA namespace tree.
///
/// Emitters are cheap to clone (Arc-backed) and thread-safe.
#[derive(Debug, Clone)]
pub struct Emitter {
    inner: Arc<EmitterInner>,
}

impl Emitter {
    /// Create the root emitter at the `"sera"` namespace.
    pub fn root() -> Self {
        Self {
            inner: Arc::new(EmitterInner {
                namespace: "sera".to_string(),
                trace: None,
                parent: None,
            }),
        }
    }

    /// Derive a child emitter by appending a dotted segment.
    ///
    /// `emitter.child("agent")` on `"sera"` produces `"sera.agent"`.
    pub fn child(&self, segment: &str) -> Self {
        let namespace = format!("{}.{}", self.inner.namespace, segment);
        Self {
            inner: Arc::new(EmitterInner {
                namespace,
                trace: self.inner.trace.clone(),
                parent: Some(Arc::clone(&self.inner)),
            }),
        }
    }

    /// Attach a W3C traceparent header to a new emitter derived from this one.
    pub fn with_trace(&self, traceparent: impl Into<String>) -> Self {
        Self {
            inner: Arc::new(EmitterInner {
                namespace: self.inner.namespace.clone(),
                trace: Some(traceparent.into()),
                parent: self.inner.parent.clone(),
            }),
        }
    }

    /// Return the full dotted namespace path.
    pub fn namespace(&self) -> &str {
        &self.inner.namespace
    }

    /// Return the attached W3C trace, if any.
    pub fn trace(&self) -> Option<&str> {
        self.inner.trace.as_deref()
    }

    /// Build an `EventMeta` for the given event name and data type.
    pub fn event_meta(&self, name: impl Into<String>, data_type: impl Into<String>) -> EventMeta {
        let name = name.into();
        let path = format!("{}.{}", self.inner.namespace, name);
        EventMeta {
            id: Uuid::new_v4(),
            name,
            path,
            created_at: OffsetDateTime::now_utc(),
            trace: self.inner.trace.clone(),
            data_type: data_type.into(),
        }
    }
}

/// Metadata attached to every emitted event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventMeta {
    /// Unique event instance ID.
    pub id: Uuid,
    /// Short event name (e.g. `"started"`).
    pub name: String,
    /// Fully-qualified event path (namespace + name).
    pub path: String,
    /// Wall-clock time at emission.
    pub created_at: OffsetDateTime,
    /// W3C traceparent header, if the emitter had one.
    pub trace: Option<String>,
    /// Logical data type tag for the accompanying payload.
    pub data_type: String,
}
