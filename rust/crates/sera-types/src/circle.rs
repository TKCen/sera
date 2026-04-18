//! Circle coordination types — SPEC-circles §3f.
//!
//! This module defines the *data* types for Circle coordination that must be
//! shared across crates: termination predicates and the shared
//! [`BlackboardEntry`] / [`BlackboardRetention`] types used by the
//! [`sera-workflow`] Coordinator. The runtime data structure (`CircleBlackboard`)
//! lives in `sera-workflow::coordination` — this crate holds only the serde-
//! friendly public types.
//!
//! Bead: `sera-8d1.3` (GH#146).

use std::num::NonZeroUsize;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Default `max_messages` floor when a Circle does not declare a termination
/// condition. See [`TerminationCondition::default`].
pub const DEFAULT_TERMINATION_MAX_MESSAGES: u32 = 50;

/// Default `timeout` ceiling (30 minutes) when a Circle does not declare a
/// termination condition. See [`TerminationCondition::default`].
pub const DEFAULT_TERMINATION_TIMEOUT_SECS: u64 = 30 * 60;

/// Composable predicates that determine when a Circle session should stop.
///
/// The variants mirror SPEC-circles §3f. `And` / `Or` allow arbitrary
/// composition (e.g. `And(MaxMessages(100), Or(TextMention(...), Timeout(...)))`).
///
/// # Default
///
/// [`TerminationCondition::default`] returns
/// `Or(MaxMessages(50), Timeout(30min))` — a safety net so a Circle without
/// an explicit condition cannot run unbounded.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum TerminationCondition {
    /// Stop after this many messages have been appended to the blackboard.
    MaxMessages(u32),
    /// Stop when any blackboard payload contains this text.
    TextMention(String),
    /// Stop after this wall-clock duration has elapsed since the session start.
    Timeout(#[serde(with = "duration_secs")] Duration),
    /// Stop when a participant emits an in-band decision-to-stop signal.
    ///
    /// Surfaced on the blackboard via an entry whose `artifact_type` is
    /// [`AGENT_DECISION_ARTIFACT`].
    AgentDecision,
    /// Stop when the embedder signals externally (public API call).
    ExternalSignal,
    /// Logical AND — both sub-conditions must be satisfied.
    And(Box<TerminationCondition>, Box<TerminationCondition>),
    /// Logical OR — either sub-condition satisfies.
    Or(Box<TerminationCondition>, Box<TerminationCondition>),
}

impl Default for TerminationCondition {
    fn default() -> Self {
        TerminationCondition::Or(
            Box::new(TerminationCondition::MaxMessages(
                DEFAULT_TERMINATION_MAX_MESSAGES,
            )),
            Box::new(TerminationCondition::Timeout(Duration::from_secs(
                DEFAULT_TERMINATION_TIMEOUT_SECS,
            ))),
        )
    }
}

/// Blackboard `artifact_type` recognised by [`TerminationCondition::AgentDecision`].
///
/// A participant requesting a circle stop should append a [`BlackboardEntry`]
/// whose `artifact_type` equals this constant.
pub const AGENT_DECISION_ARTIFACT: &str = "agent_decision_stop";

/// Append-only entry on a Circle blackboard.
///
/// Entries are ordered by insertion and later filtered by
/// [`BlackboardRetention`] on append.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BlackboardEntry {
    /// Participant (agent / sub-circle / human) that produced this entry.
    pub participant_id: String,
    /// Wall-clock timestamp at append-time.
    pub timestamp: DateTime<Utc>,
    /// Short discriminator for consumers — e.g. `"message"`, `"tool_call"`,
    /// [`AGENT_DECISION_ARTIFACT`].
    pub artifact_type: String,
    /// Free-form payload; typed JSON so termination predicates can inspect
    /// (e.g. [`TerminationCondition::TextMention`] reads stringified payloads).
    pub payload: serde_json::Value,
}

/// Retention policy applied on every [`BlackboardEntry`] append.
///
/// When `max_entries` is set, the oldest entries are dropped until the count
/// fits. When `max_age` is set, entries older than the ceiling are dropped.
/// Both can be combined; either may be `None` for "unbounded".
///
/// A `compact_fn` custom hook is intentionally omitted here — attaching a
/// function pointer prevents serde round-trip and breaks the YAML surface
/// demanded by SPEC-circles §3f. Runtime callers that need custom compaction
/// should wrap the `CircleBlackboard` directly.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct BlackboardRetention {
    /// Drop oldest entries when count exceeds this bound. `None` = unbounded.
    pub max_entries: Option<NonZeroUsize>,
    /// Drop entries older than this age at append-time. `None` = unbounded.
    #[serde(default, with = "duration_secs_opt")]
    pub max_age: Option<Duration>,
}

impl BlackboardRetention {
    /// Construct a retention policy with both bounds.
    pub fn new(max_entries: Option<NonZeroUsize>, max_age: Option<Duration>) -> Self {
        Self {
            max_entries,
            max_age,
        }
    }

    /// Retention with only a max-entries bound.
    pub fn with_max_entries(max_entries: NonZeroUsize) -> Self {
        Self {
            max_entries: Some(max_entries),
            max_age: None,
        }
    }

    /// Retention with only a max-age bound.
    pub fn with_max_age(max_age: Duration) -> Self {
        Self {
            max_entries: None,
            max_age: Some(max_age),
        }
    }
}

// =========================================================================
// Constitution types (sera-8d1.4)
// =========================================================================

/// Reference to a circle's constitution document.
///
/// A constitution is a shared markdown context (tech stack, conventions,
/// constraints) injected as a system-prompt prefix for every circle member.
/// It does NOT count against agent memory budgets.
///
/// # YAML / JSON forms
///
/// ```yaml
/// constitution:
///   text: "# Conventions\n- Use Rust..."
/// ```
/// or
/// ```yaml
/// constitution:
///   file: "circles/engineering/constitution.md"
/// ```
///
/// Uses `#[serde(untagged)]` with named struct variants so both YAML and JSON
/// produce `{"text": "..."}` / `{"file": "..."}` rather than YAML tag syntax.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ConstitutionRef {
    /// Inline markdown text: `{ text: "..." }`.
    Inline { text: String },
    /// Path to a markdown file: `{ file: "path/to/doc.md" }`.
    File { file: std::path::PathBuf },
}

/// A Circle definition — a named coordination group of agents.
///
/// The `constitution` field, when present, is resolved at session start and
/// injected as a system-prompt prefix for all members. Missing files produce
/// a `tracing::warn` but do NOT fail the session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Circle {
    /// Stable identifier (e.g. UUID or slug).
    pub id: String,
    /// Human-readable name (unique within a deployment).
    pub name: String,
    /// Display name shown in UI.
    pub display_name: String,
    /// Optional description of the circle's purpose.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Optional constitution document injected into member system prompts.
    /// Excluded from agent memory budget accounting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub constitution: Option<ConstitutionRef>,
}

// =========================================================================
// serde adapters
// =========================================================================

mod duration_secs {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::Duration;

    pub fn serialize<S: Serializer>(d: &Duration, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_u64(d.as_secs())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Duration, D::Error> {
        let secs = u64::deserialize(d)?;
        Ok(Duration::from_secs(secs))
    }
}

mod duration_secs_opt {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::Duration;

    pub fn serialize<S: Serializer>(d: &Option<Duration>, s: S) -> Result<S::Ok, S::Error> {
        match d {
            Some(d) => s.serialize_some(&d.as_secs()),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Duration>, D::Error> {
        let opt: Option<u64> = Option::deserialize(d)?;
        Ok(opt.map(Duration::from_secs))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_or_of_max_messages_and_timeout() {
        match TerminationCondition::default() {
            TerminationCondition::Or(a, b) => {
                assert!(matches!(
                    *a,
                    TerminationCondition::MaxMessages(DEFAULT_TERMINATION_MAX_MESSAGES)
                ));
                assert!(matches!(*b, TerminationCondition::Timeout(d) if d.as_secs() == DEFAULT_TERMINATION_TIMEOUT_SECS));
            }
            other => panic!("unexpected default: {other:?}"),
        }
    }

    #[test]
    fn yaml_round_trip_simple() {
        let t = TerminationCondition::MaxMessages(42);
        let yaml = serde_yaml::to_string(&t).unwrap();
        let parsed: TerminationCondition = serde_yaml::from_str(&yaml).unwrap();
        assert!(matches!(parsed, TerminationCondition::MaxMessages(42)));
    }

    #[test]
    fn yaml_round_trip_composed() {
        let t = TerminationCondition::And(
            Box::new(TerminationCondition::MaxMessages(10)),
            Box::new(TerminationCondition::Or(
                Box::new(TerminationCondition::TextMention("STOP".into())),
                Box::new(TerminationCondition::Timeout(Duration::from_secs(5))),
            )),
        );
        let yaml = serde_yaml::to_string(&t).unwrap();
        let parsed: TerminationCondition = serde_yaml::from_str(&yaml).unwrap();
        // shape-check via re-serialize equality
        let yaml2 = serde_yaml::to_string(&parsed).unwrap();
        assert_eq!(yaml, yaml2);
    }

    #[test]
    fn retention_serde_round_trip() {
        let r = BlackboardRetention::new(
            NonZeroUsize::new(8),
            Some(Duration::from_secs(60)),
        );
        let yaml = serde_yaml::to_string(&r).unwrap();
        let parsed: BlackboardRetention = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(parsed, r);
    }

    // ── ConstitutionRef serde tests (sera-8d1.4) ─────────────────────────────

    #[test]
    fn constitution_ref_inline_yaml_round_trip() {
        let c = ConstitutionRef::Inline { text: "# Conventions\n- Use Rust\n".to_string() };
        let yaml = serde_yaml::to_string(&c).unwrap();
        assert!(yaml.contains("text:"), "expected 'text:' key, got: {yaml}");
        let parsed: ConstitutionRef = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(parsed, c);
    }

    #[test]
    fn constitution_ref_file_yaml_round_trip() {
        let c = ConstitutionRef::File { file: std::path::PathBuf::from("circles/eng/constitution.md") };
        let yaml = serde_yaml::to_string(&c).unwrap();
        assert!(yaml.contains("file:"), "expected 'file:' key, got: {yaml}");
        let parsed: ConstitutionRef = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(parsed, c);
    }

    #[test]
    fn constitution_ref_inline_json_round_trip() {
        let c = ConstitutionRef::Inline { text: "hello world".to_string() };
        let json = serde_json::to_string(&c).unwrap();
        assert!(json.contains(r#""text""#), "expected 'text' key, got: {json}");
        let parsed: ConstitutionRef = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, c);
    }

    #[test]
    fn constitution_ref_file_json_round_trip() {
        let c = ConstitutionRef::File { file: std::path::PathBuf::from("path/to/doc.md") };
        let json = serde_json::to_string(&c).unwrap();
        assert!(json.contains(r#""file""#), "expected 'file' key, got: {json}");
        let parsed: ConstitutionRef = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, c);
    }

    #[test]
    fn circle_with_constitution_yaml_round_trip() {
        let circle = Circle {
            id: "circle-1".to_string(),
            name: "engineering".to_string(),
            display_name: "Engineering Circle".to_string(),
            description: Some("Main eng team".to_string()),
            constitution: Some(ConstitutionRef::Inline { text: "# Stack\n- Rust".to_string() }),
        };
        let yaml = serde_yaml::to_string(&circle).unwrap();
        let parsed: Circle = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(parsed.name, "engineering");
        assert!(matches!(parsed.constitution, Some(ConstitutionRef::Inline { .. })));
    }

    #[test]
    fn circle_without_constitution_omits_field() {
        let circle = Circle {
            id: "c".to_string(),
            name: "ops".to_string(),
            display_name: "Ops".to_string(),
            description: None,
            constitution: None,
        };
        let json = serde_json::to_string(&circle).unwrap();
        assert!(!json.contains("constitution"), "field should be omitted: {json}");
    }
}
