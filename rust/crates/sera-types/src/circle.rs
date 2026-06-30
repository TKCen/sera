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
// Party mode types (sera-8d1.2 / GH#145)
// =========================================================================

/// Default number of rounds a Party mode run allows before synthesis.
pub const DEFAULT_PARTY_MAX_ROUNDS: u32 = 3;

/// Ordering for Party mode turn-taking.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PartyOrdering {
    /// Iterate members in declaration order each round.
    #[default]
    RoundRobin,
    /// Iterate members by importance (descending). Requires an importance
    /// hint per member; without hints falls back to [`PartyOrdering::RoundRobin`].
    ImportanceBased,
}

/// Configuration for a Party mode coordination run.
///
/// A Party mode session broadcasts the same prompt to all members, collects
/// their responses (with inter-member visibility via the blackboard), repeats
/// for `max_rounds` rounds, then feeds the transcript to the `synthesizer`
/// for a final synthesis turn.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartyConfig {
    /// Maximum number of discussion rounds before synthesis.
    #[serde(default = "default_party_max_rounds")]
    pub max_rounds: u32,
    /// Ordering for turn-taking within each round.
    #[serde(default)]
    pub ordering: PartyOrdering,
    /// Participant id of the agent that synthesizes the final output.
    pub synthesizer: String,
}

fn default_party_max_rounds() -> u32 {
    DEFAULT_PARTY_MAX_ROUNDS
}

impl PartyConfig {
    /// Build a config with the default `max_rounds` and `RoundRobin` ordering.
    pub fn new(synthesizer: impl Into<String>) -> Self {
        Self {
            max_rounds: DEFAULT_PARTY_MAX_ROUNDS,
            ordering: PartyOrdering::RoundRobin,
            synthesizer: synthesizer.into(),
        }
    }
}

/// A single response posted by a party member during a round.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PartyResponse {
    /// Participant id of the responder.
    pub participant_id: String,
    /// Free-form response text.
    pub text: String,
    /// Wall-clock timestamp when the response was posted.
    pub posted_at: DateTime<Utc>,
}

/// A complete round of party-mode prompts + responses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartyRound {
    /// 1-indexed round number.
    pub round_no: u32,
    /// Wall-clock timestamp when the round's prompt was broadcast.
    pub prompts_sent_at: DateTime<Utc>,
    /// Responses posted during this round, in arrival order.
    pub responses: Vec<PartyResponse>,
}

/// Structured outcome of a Party mode run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartyOutcome {
    /// All rounds run, in order.
    pub rounds: Vec<PartyRound>,
    /// Final synthesized output produced by the synthesizer participant.
    pub synthesis: String,
}

// =========================================================================
// Collaboration envelope types (sera-nqh3 / SPEC-circles §3j)
// =========================================================================

/// Reference to a success metric or evaluator.
///
/// May be an inline description or a pointer to an external evaluator id.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MetricRef {
    /// Human-readable inline description of what counts as success.
    Inline { description: String },
    /// Reference to a named evaluator (benchmark, checklist, policy id).
    Evaluator {
        evaluator_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },
}

/// Noise/significance threshold for tie decisions.
///
/// Results whose delta is below `min_delta` are considered a tie regardless of
/// absolute score difference.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TiePolicy {
    /// Minimum meaningful delta; results within this band are ties.
    pub min_delta: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// A rule that can invalidate a submitted result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum InvalidResultRule {
    /// Result exploited a metric loophole without achieving the actual goal.
    MetricLoophole { description: String },
    /// Result was produced using or revealing private/confidential data.
    PrivateDataLeakage,
    /// Result overfits the evaluation criterion in a way not generalisable.
    Overfitting { description: String },
    /// Run cannot be independently verified (no receipts, missing lineage).
    UnverifiableRun,
    /// Custom invalidation rule (operator-defined).
    Custom { description: String },
}

/// Default visibility of proposals, runs, reviews, and side-channel communication.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum VisibilityPolicy {
    /// All events are public to circle members by default.
    PublicToCircleByDefault,
    /// Events are private by default; must be explicitly disclosed.
    PrivateByDefaultWithDisclosure,
    /// Mixed: named event kinds are public; others are private.
    Mixed {
        public_kinds: Vec<String>,
        private_kinds: Vec<String>,
    },
}

/// Pointer to the principal or policy surface able to issue rulings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RefereeRef {
    /// A specific principal (agent or human) identified by id.
    Principal { principal_id: String },
    /// A policy description (e.g. "majority of lead agents" or "operator approval").
    Policy { description: String },
}

/// Scarce-resource budget envelope for a collaboration run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuotaPolicy {
    /// Total token budget for the run (across all members).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_token_limit: Option<u64>,
    /// Per-member token budget.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub per_member_token_limit: Option<u64>,
    /// Maximum number of deliberation iterations before forced termination.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_iterations: Option<u32>,
    /// Maximum number of tool/probe calls for the run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tool_calls: Option<u32>,
}

/// Attribution and derivative-work rules for staged proposals and reused artifacts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CreditPolicy {
    /// Whether attribution to originating participant is required on accepted results.
    pub attribution_required: bool,
    /// Whether participants may build on or reference each other's proposals.
    pub derivative_work_allowed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Where compute-poor or spec-rich agents can stage candidates for agents with budget.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StagingPolicy {
    /// Whether a shared staging area is active for this run.
    pub enabled: bool,
    /// Optional identifier for the staging workspace.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub staging_area_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Required proof-bundle fields that every accepted result must provide.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CircleReceiptPolicy {
    /// Require at least one `ExecutionReceipt` per accepted result.
    pub require_run_evidence: bool,
    /// Require at least one `LineageEdge` linking the result to a prior entry.
    pub require_lineage: bool,
    /// Require a `CollaborationVerdictRecord` before a result is considered final.
    pub require_verdict: bool,
    /// Additional named fields that must be present in the proof bundle.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub additional_required_fields: Vec<String>,
}

/// Rules governing private side-channel communication among members.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AntiCollusionPolicy {
    /// Private side channels are fully disallowed.
    DisallowPrivateSideChannels,
    /// Private coordination is allowed but must be flagged as non-verifiable in the bundle.
    AllowButMarkNonVerifiable,
    /// Private coordination requires explicit referee approval before use.
    RequireRefereeApproval,
}

/// Common-goal collaboration envelope for a Circle (SPEC-circles §3j).
///
/// Presence of this envelope is what distinguishes a SERA Circle from generic
/// parallel prompting. Every field encodes one institutional constraint:
/// shared goal, measurement, authority, identity, receipts, lineage,
/// scarcity, staging, verification, and anti-collusion.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CircleCollaborationEnvelope {
    /// Shared objective visible to every member. Different from individual task titles.
    pub objective: String,

    /// How progress and success are measured.
    pub success_metric: MetricRef,

    /// Minimum meaningful delta / noise threshold. Below this, competing results are ties.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tie_policy: Option<TiePolicy>,

    /// Rules that invalidate a result (loophole, leakage, unverifiable run, etc.).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub invalid_result_policy: Vec<InvalidResultRule>,

    /// Default visibility for proposals, runs, reviews, and side-channel communication.
    pub visibility_policy: VisibilityPolicy,

    /// Principal or policy surface that can issue rulings when agents dispute validity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub referee: Option<RefereeRef>,

    /// Scarce-resource accounting: token budget, tool calls, wall-clock, external credits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quota_policy: Option<QuotaPolicy>,

    /// Attribution and derivative-work rules for staged proposals and reused artifacts.
    pub credit_policy: CreditPolicy,

    /// Where compute-poor/spec-rich agents can stage candidates for others with budget.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub staging_policy: Option<StagingPolicy>,

    /// Required proof-bundle fields for every accepted result.
    pub receipt_policy: CircleReceiptPolicy,

    /// Rules governing private side channels.
    pub anti_collusion_policy: AntiCollusionPolicy,
}

// =========================================================================
// Proof-bundle types (sera-nqh3 golden fixture)
// =========================================================================

/// Role of a participant in a collaboration proof bundle.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProofBundleMember {
    pub participant_id: String,
    pub role: String,
}

/// Snapshot of the resource budget at the end of a collaboration run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BudgetSnapshot {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_token_limit: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_iterations: Option<u32>,
    pub current_usage: u64,
}

/// A single entry in the collaboration proof bundle's blackboard transcript.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProofBundleEntry {
    pub entry_id: u64,
    pub author: String,
    pub timestamp: DateTime<Utc>,
    pub artifact_type: String,
    pub payload: serde_json::Value,
}

/// Semantic relationship between two proof-bundle entries.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LineageRelation {
    Criticizes,
    Resolves,
    DerivesFrom,
    Supersedes,
    Custom(String),
}

/// Directed edge in the proof-bundle lineage DAG.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LineageEdge {
    pub from_entry_id: u64,
    pub to_entry_id: u64,
    pub relation: LineageRelation,
}

/// Outcome of an execution receipt.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReceiptOutcome {
    Success,
    Failure { reason: String },
    Partial { note: String },
}

/// Auditable evidence of a single tool call or probe during a collaboration run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExecutionReceipt {
    pub receipt_id: String,
    pub executor: String,
    pub timestamp: DateTime<Utc>,
    pub action: String,
    pub parameters: serde_json::Value,
    pub outcome: ReceiptOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_tokens: Option<u64>,
}

/// Verdict type for a collaboration proof bundle.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum VerdictType {
    Approved,
    Rejected { reason: String },
    Tie { note: String },
    Invalid { rule: String },
    RevisionRequired { feedback: String },
}

/// Structured verdict record produced by a referee or reviewer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CollaborationVerdictRecord {
    pub reviewer: String,
    pub timestamp: DateTime<Utc>,
    pub verdict_type: VerdictType,
    pub rationale: String,
}

/// Severity of a peer challenge raised during Circle deliberation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PeerChallengeSeverity {
    Low,
    Medium,
    High,
    Blocking,
}

/// Current disposition of a peer challenge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PeerChallengeDisposition {
    Open,
    Accepted,
    Rejected,
    Resolved,
    Superseded,
}

fn default_peer_challenge_disposition() -> PeerChallengeDisposition {
    PeerChallengeDisposition::Open
}

/// Structured challenge object preserving dissent before final integration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PeerChallenge {
    /// Stable id for this challenge within the run.
    pub challenge_id: String,
    /// Participant that raised the challenge.
    pub challenger: String,
    /// Blackboard entry being challenged.
    pub target_entry_id: u64,
    /// Claim under dispute.
    pub claim: String,
    /// Concrete challenge or objection.
    pub challenge: String,
    /// Receipt ids, file paths, quotes, or other compact evidence pointers.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<String>,
    pub severity: PeerChallengeSeverity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_by: Option<String>,
    #[serde(default = "default_peer_challenge_disposition")]
    pub disposition: PeerChallengeDisposition,
}

/// Complete proof bundle for a collaboration run.
///
/// Serialises to / deserialises from JSON as the golden fixture format for
/// `sera-nqh3` acceptance tests. Contains the full audit trail: objective,
/// roster, blackboard transcript, lineage DAG, execution receipts, and verdict.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CollaborationProofBundle {
    pub run_id: String,
    pub circle_id: String,
    pub objective: String,
    pub success_metric: MetricRef,
    pub roster: Vec<ProofBundleMember>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget_snapshot: Option<BudgetSnapshot>,
    pub entries: Vec<ProofBundleEntry>,
    pub lineage: Vec<LineageEdge>,
    pub execution_receipts: Vec<ExecutionReceipt>,
    /// Structured challenges raised by peers before referee integration.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub peer_challenges: Vec<PeerChallenge>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verdict: Option<CollaborationVerdictRecord>,
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
                assert!(
                    matches!(*b, TerminationCondition::Timeout(d) if d.as_secs() == DEFAULT_TERMINATION_TIMEOUT_SECS)
                );
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
        let r = BlackboardRetention::new(NonZeroUsize::new(8), Some(Duration::from_secs(60)));
        let yaml = serde_yaml::to_string(&r).unwrap();
        let parsed: BlackboardRetention = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(parsed, r);
    }

    // ── ConstitutionRef serde tests (sera-8d1.4) ─────────────────────────────

    #[test]
    fn constitution_ref_inline_yaml_round_trip() {
        let c = ConstitutionRef::Inline {
            text: "# Conventions\n- Use Rust\n".to_string(),
        };
        let yaml = serde_yaml::to_string(&c).unwrap();
        assert!(yaml.contains("text:"), "expected 'text:' key, got: {yaml}");
        let parsed: ConstitutionRef = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(parsed, c);
    }

    #[test]
    fn constitution_ref_file_yaml_round_trip() {
        let c = ConstitutionRef::File {
            file: std::path::PathBuf::from("circles/eng/constitution.md"),
        };
        let yaml = serde_yaml::to_string(&c).unwrap();
        assert!(yaml.contains("file:"), "expected 'file:' key, got: {yaml}");
        let parsed: ConstitutionRef = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(parsed, c);
    }

    #[test]
    fn constitution_ref_inline_json_round_trip() {
        let c = ConstitutionRef::Inline {
            text: "hello world".to_string(),
        };
        let json = serde_json::to_string(&c).unwrap();
        assert!(
            json.contains(r#""text""#),
            "expected 'text' key, got: {json}"
        );
        let parsed: ConstitutionRef = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, c);
    }

    #[test]
    fn constitution_ref_file_json_round_trip() {
        let c = ConstitutionRef::File {
            file: std::path::PathBuf::from("path/to/doc.md"),
        };
        let json = serde_json::to_string(&c).unwrap();
        assert!(
            json.contains(r#""file""#),
            "expected 'file' key, got: {json}"
        );
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
            constitution: Some(ConstitutionRef::Inline {
                text: "# Stack\n- Rust".to_string(),
            }),
        };
        let yaml = serde_yaml::to_string(&circle).unwrap();
        let parsed: Circle = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(parsed.name, "engineering");
        assert!(matches!(
            parsed.constitution,
            Some(ConstitutionRef::Inline { .. })
        ));
    }

    // ── Party mode serde tests (sera-8d1.2) ──────────────────────────────────

    #[test]
    fn party_config_yaml_round_trip_defaults() {
        let cfg = PartyConfig::new("lead");
        let yaml = serde_yaml::to_string(&cfg).unwrap();
        let parsed: PartyConfig = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(parsed.max_rounds, DEFAULT_PARTY_MAX_ROUNDS);
        assert_eq!(parsed.synthesizer, "lead");
        assert_eq!(parsed.ordering, PartyOrdering::RoundRobin);
    }

    #[test]
    fn party_config_yaml_explicit_values() {
        let yaml = "max_rounds: 5\nordering: importance_based\nsynthesizer: alice\n";
        let parsed: PartyConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(parsed.max_rounds, 5);
        assert_eq!(parsed.ordering, PartyOrdering::ImportanceBased);
        assert_eq!(parsed.synthesizer, "alice");
    }

    #[test]
    fn party_config_yaml_missing_optional_fields_uses_defaults() {
        let yaml = "synthesizer: bob\n";
        let parsed: PartyConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(parsed.max_rounds, DEFAULT_PARTY_MAX_ROUNDS);
        assert_eq!(parsed.ordering, PartyOrdering::RoundRobin);
        assert_eq!(parsed.synthesizer, "bob");
    }

    #[test]
    fn party_ordering_serde_round_trip() {
        for o in [PartyOrdering::RoundRobin, PartyOrdering::ImportanceBased] {
            let yaml = serde_yaml::to_string(&o).unwrap();
            let parsed: PartyOrdering = serde_yaml::from_str(&yaml).unwrap();
            assert_eq!(parsed, o);
        }
    }

    #[test]
    fn party_response_serde_round_trip() {
        let resp = PartyResponse {
            participant_id: "alice".to_string(),
            text: "hello world".to_string(),
            posted_at: Utc::now(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: PartyResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, resp);
    }

    #[test]
    fn party_round_yaml_round_trip() {
        let round = PartyRound {
            round_no: 2,
            prompts_sent_at: Utc::now(),
            responses: vec![
                PartyResponse {
                    participant_id: "alice".into(),
                    text: "a".into(),
                    posted_at: Utc::now(),
                },
                PartyResponse {
                    participant_id: "bob".into(),
                    text: "b".into(),
                    posted_at: Utc::now(),
                },
            ],
        };
        let yaml = serde_yaml::to_string(&round).unwrap();
        let parsed: PartyRound = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(parsed.round_no, 2);
        assert_eq!(parsed.responses.len(), 2);
        assert_eq!(parsed.responses[0].participant_id, "alice");
    }

    #[test]
    fn party_outcome_json_round_trip() {
        let outcome = PartyOutcome {
            rounds: vec![PartyRound {
                round_no: 1,
                prompts_sent_at: Utc::now(),
                responses: vec![],
            }],
            synthesis: "final answer".to_string(),
        };
        let json = serde_json::to_string(&outcome).unwrap();
        let parsed: PartyOutcome = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.rounds.len(), 1);
        assert_eq!(parsed.synthesis, "final answer");
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
        assert!(
            !json.contains("constitution"),
            "field should be omitted: {json}"
        );
    }

    // ── Collaboration envelope serde tests (sera-nqh3 / SPEC-circles §3j) ──

    fn minimal_envelope() -> CircleCollaborationEnvelope {
        CircleCollaborationEnvelope {
            objective: "Verify and deduplicate the Hermes-parity gap map".to_string(),
            success_metric: MetricRef::Inline {
                description: "Zero duplicated tasks, all gap items cited with source paths."
                    .to_string(),
            },
            tie_policy: None,
            invalid_result_policy: vec![],
            visibility_policy: VisibilityPolicy::PublicToCircleByDefault,
            referee: None,
            quota_policy: None,
            credit_policy: CreditPolicy {
                attribution_required: true,
                derivative_work_allowed: true,
                description: None,
            },
            staging_policy: None,
            receipt_policy: CircleReceiptPolicy {
                require_run_evidence: true,
                require_lineage: true,
                require_verdict: true,
                additional_required_fields: vec![],
            },
            anti_collusion_policy: AntiCollusionPolicy::DisallowPrivateSideChannels,
        }
    }

    #[test]
    fn collaboration_envelope_json_round_trip_minimal() {
        let env = minimal_envelope();
        let json = serde_json::to_string(&env).unwrap();
        let parsed: CircleCollaborationEnvelope = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, env);
    }

    #[test]
    fn collaboration_envelope_yaml_round_trip_full() {
        let env = CircleCollaborationEnvelope {
            objective: "Prove the collaboration envelope shape".to_string(),
            success_metric: MetricRef::Evaluator {
                evaluator_id: "metric_dedup_complete".to_string(),
                description: Some("Zero duplicated tasks".to_string()),
            },
            tie_policy: Some(TiePolicy {
                min_delta: 0.01,
                description: Some("Results within 1% are ties".to_string()),
            }),
            invalid_result_policy: vec![
                InvalidResultRule::MetricLoophole {
                    description: "Fabricated citations".to_string(),
                },
                InvalidResultRule::UnverifiableRun,
            ],
            visibility_policy: VisibilityPolicy::Mixed {
                public_kinds: vec!["proposal".to_string(), "objection".to_string()],
                private_kinds: vec!["draft".to_string()],
            },
            referee: Some(RefereeRef::Principal {
                principal_id: "referee".to_string(),
            }),
            quota_policy: Some(QuotaPolicy {
                total_token_limit: Some(100_000),
                per_member_token_limit: Some(20_000),
                max_iterations: Some(3),
                max_tool_calls: None,
            }),
            credit_policy: CreditPolicy {
                attribution_required: true,
                derivative_work_allowed: true,
                description: Some("SERA standard attribution".to_string()),
            },
            staging_policy: Some(StagingPolicy {
                enabled: true,
                staging_area_id: Some("sera-nqh3-staging".to_string()),
                description: None,
            }),
            receipt_policy: CircleReceiptPolicy {
                require_run_evidence: true,
                require_lineage: true,
                require_verdict: true,
                additional_required_fields: vec!["cited_paths".to_string()],
            },
            anti_collusion_policy: AntiCollusionPolicy::RequireRefereeApproval,
        };
        let yaml = serde_yaml::to_string(&env).unwrap();
        let parsed: CircleCollaborationEnvelope = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(parsed, env);
    }

    #[test]
    fn anti_collusion_policy_all_variants_round_trip() {
        for policy in [
            AntiCollusionPolicy::DisallowPrivateSideChannels,
            AntiCollusionPolicy::AllowButMarkNonVerifiable,
            AntiCollusionPolicy::RequireRefereeApproval,
        ] {
            let json = serde_json::to_string(&policy).unwrap();
            let parsed: AntiCollusionPolicy = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, policy);
        }
    }

    #[test]
    fn visibility_policy_variants_round_trip() {
        let policies = vec![
            VisibilityPolicy::PublicToCircleByDefault,
            VisibilityPolicy::PrivateByDefaultWithDisclosure,
            VisibilityPolicy::Mixed {
                public_kinds: vec!["proposal".to_string()],
                private_kinds: vec!["draft".to_string()],
            },
        ];
        for p in policies {
            let json = serde_json::to_string(&p).unwrap();
            let parsed: VisibilityPolicy = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, p);
        }
    }

    #[test]
    fn proof_bundle_golden_fixture_round_trip() {
        let bundle = CollaborationProofBundle {
            run_id: "run_sera_nqh3_001".to_string(),
            circle_id: "sera-nqh3-circle".to_string(),
            objective: "Verify and deduplicate the Hermes-parity gap map for sera-nqh3".to_string(),
            success_metric: MetricRef::Evaluator {
                evaluator_id: "metric_dedup_complete".to_string(),
                description: Some(
                    "Zero duplicated tasks, all gap items cited with source code file paths, \
                     and no outstanding critic objections."
                        .to_string(),
                ),
            },
            roster: vec![
                ProofBundleMember {
                    participant_id: "sera_lead".to_string(),
                    role: "Lead".to_string(),
                },
                ProofBundleMember {
                    participant_id: "specifier".to_string(),
                    role: "TaskSpecifier".to_string(),
                },
                ProofBundleMember {
                    participant_id: "builder".to_string(),
                    role: "Worker".to_string(),
                },
                ProofBundleMember {
                    participant_id: "critic".to_string(),
                    role: "Critic".to_string(),
                },
                ProofBundleMember {
                    participant_id: "referee".to_string(),
                    role: "Reviewer".to_string(),
                },
            ],
            budget_snapshot: Some(BudgetSnapshot {
                total_token_limit: Some(100_000),
                max_iterations: Some(3),
                current_usage: 42_050,
            }),
            entries: vec![
                ProofBundleEntry {
                    entry_id: 1,
                    author: "specifier".to_string(),
                    timestamp: "2026-06-29T16:00:00Z".parse().unwrap(),
                    artifact_type: "sharpened_specification".to_string(),
                    payload: serde_json::json!({
                        "requirements": [
                            "Cite exact file path in sera-types and sera-workflow for the circle gap",
                            "Ensure no overlapping tasks with the existing PartyMode feature"
                        ]
                    }),
                },
                ProofBundleEntry {
                    entry_id: 2,
                    author: "builder".to_string(),
                    timestamp: "2026-06-29T16:01:00Z".parse().unwrap(),
                    artifact_type: "proposal".to_string(),
                    payload: serde_json::json!({
                        "proposal_text": "First iteration of gap map. Identified lack of \
                                          CircleCollaborationEnvelope in \
                                          rust/crates/sera-types/src/circle.rs.",
                        "version": 1
                    }),
                },
                ProofBundleEntry {
                    entry_id: 3,
                    author: "critic".to_string(),
                    timestamp: "2026-06-29T16:02:00Z".parse().unwrap(),
                    artifact_type: "objection".to_string(),
                    payload: serde_json::json!({
                        "objection_text": "The builder's proposal misses citing the specific \
                                           ResultAggregator files in sera-workflow.",
                        "target_entry_id": 2
                    }),
                },
                ProofBundleEntry {
                    entry_id: 4,
                    author: "builder".to_string(),
                    timestamp: "2026-06-29T16:03:00Z".parse().unwrap(),
                    artifact_type: "proposal".to_string(),
                    payload: serde_json::json!({
                        "proposal_text": "Updated gap map: added citation to \
                                          rust/crates/sera-workflow/src/coordination.rs \
                                          for ResultAggregator.",
                        "version": 2
                    }),
                },
            ],
            lineage: vec![
                LineageEdge {
                    from_entry_id: 2,
                    to_entry_id: 3,
                    relation: LineageRelation::Criticizes,
                },
                LineageEdge {
                    from_entry_id: 3,
                    to_entry_id: 4,
                    relation: LineageRelation::Resolves,
                },
            ],
            execution_receipts: vec![ExecutionReceipt {
                receipt_id: "rcpt_grep_001".to_string(),
                executor: "builder".to_string(),
                timestamp: "2026-06-29T16:00:30Z".parse().unwrap(),
                action: "grep_search".to_string(),
                parameters: serde_json::json!({"query": "ResultAggregator"}),
                outcome: ReceiptOutcome::Success,
                cost_tokens: Some(120),
            }],
            peer_challenges: vec![PeerChallenge {
                challenge_id: "challenge_critic_001".to_string(),
                challenger: "critic".to_string(),
                target_entry_id: 2,
                claim: "The builder proposal cited every relevant Circle coordination file."
                    .to_string(),
                challenge: "The proposal missed ResultAggregator evidence in sera-workflow."
                    .to_string(),
                evidence: vec!["rust/crates/sera-workflow/src/coordination.rs".to_string()],
                severity: PeerChallengeSeverity::High,
                response_by: Some("builder".to_string()),
                disposition: PeerChallengeDisposition::Resolved,
            }],
            verdict: Some(CollaborationVerdictRecord {
                reviewer: "referee".to_string(),
                timestamp: "2026-06-29T16:04:00Z".parse().unwrap(),
                verdict_type: VerdictType::Approved,
                rationale: "Critic objections resolved, citations verified against the workspace."
                    .to_string(),
            }),
        };

        let json = serde_json::to_string_pretty(&bundle).unwrap();
        let parsed: CollaborationProofBundle = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, bundle);

        // Spot-check structure
        assert_eq!(parsed.roster.len(), 5);
        assert_eq!(parsed.entries.len(), 4);
        assert_eq!(parsed.lineage.len(), 2);
        assert_eq!(parsed.execution_receipts.len(), 1);
        assert!(matches!(
            parsed.verdict,
            Some(CollaborationVerdictRecord {
                verdict_type: VerdictType::Approved,
                ..
            })
        ));
    }

    #[test]
    fn proof_bundle_no_verdict_omits_field() {
        let bundle = CollaborationProofBundle {
            run_id: "r1".to_string(),
            circle_id: "c1".to_string(),
            objective: "test".to_string(),
            success_metric: MetricRef::Inline {
                description: "pass".to_string(),
            },
            roster: vec![],
            budget_snapshot: None,
            entries: vec![],
            lineage: vec![],
            execution_receipts: vec![],
            peer_challenges: vec![],
            verdict: None,
        };
        let json = serde_json::to_string(&bundle).unwrap();
        assert!(
            !json.contains("verdict"),
            "verdict should be absent: {json}"
        );
        assert!(
            !json.contains("budget_snapshot"),
            "budget_snapshot should be absent: {json}"
        );
    }

    #[test]
    fn verdict_type_rejected_round_trip() {
        let v = VerdictType::Rejected {
            reason: "fabricated citations".to_string(),
        };
        let json = serde_json::to_string(&v).unwrap();
        let parsed: VerdictType = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, v);
    }

    #[test]
    fn lineage_relation_custom_round_trip() {
        let r = LineageRelation::Custom("annotates".to_string());
        let json = serde_json::to_string(&r).unwrap();
        let parsed: LineageRelation = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, r);
    }

    #[test]
    fn invalid_result_rule_custom_round_trip() {
        let rule = InvalidResultRule::Custom {
            description: "no external oracles allowed".to_string(),
        };
        let json = serde_json::to_string(&rule).unwrap();
        assert!(
            json.contains(r#""kind":"custom""#),
            "expected kind tag, got: {json}"
        );
        assert!(
            json.contains(r#""description""#),
            "expected description field, got: {json}"
        );
        let parsed: InvalidResultRule = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, rule);

        let yaml = serde_yaml::to_string(&rule).unwrap();
        let parsed_yaml: InvalidResultRule = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(parsed_yaml, rule);
    }
}
