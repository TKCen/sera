//! Signal System — agent → agent liveness + completion messaging.
//!
//! See `docs/signal-system-design.md` for the design rationale.
//!
//! * [`Signal`] enumerates every kind of signal an agent can emit.
//! * [`SignalTarget`] controls how a signal is delivered.
//! * [`Dispatch`] is the request shape sent when one agent dispatches another.
//!
//! **Invariant:** `Signal::Blocked` and `Signal::Review` always route to the
//! human-in-the-loop queue regardless of `SignalTarget` — see
//! [`Signal::is_attention_required`].

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::capability::AgentCapability;

/// Every kind of signal an agent can emit during (or after) a task.
///
/// Terminal states (`Done`, `Failed`) carry an `artifact_id` so the receiving
/// agent can pull the full result on demand. Attention states (`Blocked`,
/// `Review`) cannot be silenced — see [`Signal::is_attention_required`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Signal {
    /// Task completed successfully. `summary` is a one-liner; `artifact_id`
    /// points at the stored result.
    Done {
        artifact_id: String,
        summary: String,
        duration_ms: u64,
    },
    /// Task failed. `retries` is the number of attempts already made.
    Failed {
        artifact_id: String,
        error: String,
        retries: u8,
    },
    /// Agent is blocked; a capability is missing or preconditions are unmet.
    /// Always routed to HITL.
    Blocked {
        reason: String,
        requires: Vec<AgentCapability>,
    },
    /// Human review requested. Always routed to HITL.
    Review {
        artifact_id: String,
        prompt: String,
    },
    /// Agent started working on a task.
    Started {
        task_id: String,
        description: String,
    },
    /// Periodic progress update.
    Progress {
        task_id: String,
        /// Percent complete in the range `0..=100`.
        pct: u8,
        note: String,
    },
    /// Work handed off from one agent to another — convoy pattern.
    Handoff {
        from_agent: String,
        to_agent: String,
        artifact_id: String,
    },
}

impl Signal {
    /// Stable tag string for persistence (`agent_signals.signal_type`).
    pub fn kind(&self) -> &'static str {
        match self {
            Signal::Done { .. } => "done",
            Signal::Failed { .. } => "failed",
            Signal::Blocked { .. } => "blocked",
            Signal::Review { .. } => "review",
            Signal::Started { .. } => "started",
            Signal::Progress { .. } => "progress",
            Signal::Handoff { .. } => "handoff",
        }
    }

    /// `Blocked` and `Review` signals cannot be silenced — they always reach
    /// a human reviewer regardless of [`SignalTarget`].
    pub fn is_attention_required(&self) -> bool {
        matches!(self, Signal::Blocked { .. } | Signal::Review { .. })
    }
}

/// How a signal should be delivered.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum SignalTarget {
    /// Push into the dispatching agent's active session context (inbox).
    #[default]
    MainSession,
    /// Store the artifact only — agent pulls on demand, no push.
    ArtifactOnly,
    /// Fire-and-forget — result stored only, no inbox row written.
    Silent,
}

impl SignalTarget {
    /// `true` iff a signal with this target should write an inbox row.
    /// `ArtifactOnly` / `Silent` skip the row per the design doc.
    pub fn writes_inbox(self) -> bool {
        matches!(self, SignalTarget::MainSession)
    }
}

/// Which signal kinds a dispatch actually wants transmitted.
/// Mirrors the `Signal` variants but is a thin tag type so the caller can
/// subscribe to (say) only `Done` and `Failed` without constructing payloads.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum SignalType {
    Done,
    Failed,
    Blocked,
    Review,
    Started,
    Progress,
    Handoff,
}

impl SignalType {
    pub fn matches(self, signal: &Signal) -> bool {
        matches!(
            (self, signal),
            (SignalType::Done, Signal::Done { .. })
                | (SignalType::Failed, Signal::Failed { .. })
                | (SignalType::Blocked, Signal::Blocked { .. })
                | (SignalType::Review, Signal::Review { .. })
                | (SignalType::Started, Signal::Started { .. })
                | (SignalType::Progress, Signal::Progress { .. })
                | (SignalType::Handoff, Signal::Handoff { .. })
        )
    }
}

/// Retry policy for a dispatched task.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct RetryPolicy {
    pub max_attempts: u8,
    /// Delay between attempts in milliseconds.
    pub backoff_ms: u64,
}

/// A task description — kept intentionally flat so callers in other crates
/// can widen it later without a breaking change.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Task {
    pub id: String,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload: Option<serde_json::Value>,
}

/// Dispatch call shape — one agent asks another to run `task` and routes the
/// resulting signals according to `deliver_to` / `signal_on`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dispatch {
    pub task: Task,
    #[serde(default)]
    pub deliver_to: SignalTarget,
    /// Which signal kinds to actually transmit. Empty = transmit every kind.
    #[serde(default)]
    pub signal_on: Vec<SignalType>,
    /// Optional timeout for the whole dispatch.
    #[serde(default, with = "duration_ms_opt", skip_serializing_if = "Option::is_none")]
    pub timeout: Option<Duration>,
    #[serde(default)]
    pub retry_policy: RetryPolicy,
}

impl Dispatch {
    /// `true` iff `signal` should be transmitted given the dispatch's filter.
    /// Empty `signal_on` means "transmit everything".
    pub fn transmits(&self, signal: &Signal) -> bool {
        if self.signal_on.is_empty() {
            return true;
        }
        self.signal_on.iter().any(|t| t.matches(signal))
    }
}

mod duration_ms_opt {
    use super::*;

    pub fn serialize<S: serde::Serializer>(
        v: &Option<Duration>,
        s: S,
    ) -> Result<S::Ok, S::Error> {
        match v {
            Some(d) => s.serialize_some(&(d.as_millis() as u64)),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: serde::Deserializer<'de>>(
        d: D,
    ) -> Result<Option<Duration>, D::Error> {
        let v: Option<u64> = Option::deserialize(d)?;
        Ok(v.map(Duration::from_millis))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn done_serializes_with_type_tag() {
        let s = Signal::Done {
            artifact_id: "art-1".into(),
            summary: "ok".into(),
            duration_ms: 1234,
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"type\":\"done\""));
        assert!(json.contains("\"artifact_id\":\"art-1\""));
        let parsed: Signal = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, s);
    }

    #[test]
    fn kind_is_stable() {
        assert_eq!(
            Signal::Failed {
                artifact_id: "a".into(),
                error: "e".into(),
                retries: 0
            }
            .kind(),
            "failed"
        );
        assert_eq!(
            Signal::Blocked {
                reason: "r".into(),
                requires: vec![]
            }
            .kind(),
            "blocked"
        );
    }

    #[test]
    fn blocked_and_review_require_attention() {
        let blocked = Signal::Blocked {
            reason: "missing cap".into(),
            requires: vec![AgentCapability::MetaChange],
        };
        let review = Signal::Review {
            artifact_id: "a".into(),
            prompt: "check this".into(),
        };
        assert!(blocked.is_attention_required());
        assert!(review.is_attention_required());

        let done = Signal::Done {
            artifact_id: "a".into(),
            summary: "".into(),
            duration_ms: 0,
        };
        assert!(!done.is_attention_required());
    }

    #[test]
    fn signal_target_writes_inbox_only_for_main_session() {
        assert!(SignalTarget::MainSession.writes_inbox());
        assert!(!SignalTarget::ArtifactOnly.writes_inbox());
        assert!(!SignalTarget::Silent.writes_inbox());
    }

    #[test]
    fn default_target_is_main_session() {
        assert_eq!(SignalTarget::default(), SignalTarget::MainSession);
    }

    #[test]
    fn dispatch_transmits_empty_filter_means_all() {
        let d = Dispatch {
            task: Task {
                id: "t".into(),
                description: "d".into(),
                payload: None,
            },
            deliver_to: SignalTarget::MainSession,
            signal_on: vec![],
            timeout: None,
            retry_policy: RetryPolicy::default(),
        };
        assert!(d.transmits(&Signal::Started {
            task_id: "t".into(),
            description: "d".into()
        }));
        assert!(d.transmits(&Signal::Done {
            artifact_id: "a".into(),
            summary: "".into(),
            duration_ms: 0
        }));
    }

    #[test]
    fn dispatch_transmits_honors_filter() {
        let d = Dispatch {
            task: Task {
                id: "t".into(),
                description: "d".into(),
                payload: None,
            },
            deliver_to: SignalTarget::MainSession,
            signal_on: vec![SignalType::Done, SignalType::Failed],
            timeout: Some(Duration::from_millis(5000)),
            retry_policy: RetryPolicy {
                max_attempts: 3,
                backoff_ms: 100,
            },
        };
        assert!(d.transmits(&Signal::Done {
            artifact_id: "a".into(),
            summary: "".into(),
            duration_ms: 0
        }));
        assert!(!d.transmits(&Signal::Progress {
            task_id: "t".into(),
            pct: 50,
            note: "".into()
        }));
    }

    #[test]
    fn dispatch_roundtrips_with_timeout() {
        let d = Dispatch {
            task: Task {
                id: "t".into(),
                description: "d".into(),
                payload: Some(serde_json::json!({"k":"v"})),
            },
            deliver_to: SignalTarget::Silent,
            signal_on: vec![SignalType::Done],
            timeout: Some(Duration::from_secs(2)),
            retry_policy: RetryPolicy::default(),
        };
        let json = serde_json::to_string(&d).unwrap();
        let back: Dispatch = serde_json::from_str(&json).unwrap();
        assert_eq!(back.timeout, Some(Duration::from_millis(2000)));
        assert_eq!(back.deliver_to, SignalTarget::Silent);
        assert_eq!(back.signal_on, vec![SignalType::Done]);
    }

    #[test]
    fn signal_type_matches_corresponding_variant() {
        let progress = Signal::Progress {
            task_id: "t".into(),
            pct: 10,
            note: "x".into(),
        };
        assert!(SignalType::Progress.matches(&progress));
        assert!(!SignalType::Done.matches(&progress));
    }
}
