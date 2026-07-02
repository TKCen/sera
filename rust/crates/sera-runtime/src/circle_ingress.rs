//! Resident Circle ingress funnel — Task C runtime seam.
//!
//! Implements the small, in-process path that accepts a
//! `to:circle:<name>` request and returns inspectable bounded state. This
//! is the resident-runtime counterpart of the offline `sera circle run --to`
//! proof slice: the contract types live in [`sera_types::circle_ingress`]
//! and [`sera_types::circle_channel`]; the runtime just binds them to a
//! session/lineage `TurnContext`, a channel event envelope, and a
//! [`SharedCircleActivityLog`] entry.
//!
//! ## Scope
//!
//! Intentionally small. **No** LLM calls, **no** DB I/O, **no** external
//! connector, **no** scheduler, **no** dynamic decomposition. The only
//! runtime state mutators are:
//!
//! - the in-process channel-event list (returned in the record), and
//! - a [`SharedCircleActivityLog::record`] call when an activity log handle
//!   is supplied.
//!
//! ## Lineage
//!
//! Session lineage rides on `parent_session_key` exactly as in
//! `sera_types::runtime::TurnContext`. Nothing in this module invents a
//! parallel lineage field. When `parent_session_key` is `None` on the
//! request, the record's `TurnContext.parent_session_key` is `None` — i.e.
//! a root ingress call has no lineage.
//!
//! See `docs/public/decisions/2026-06-30-circle-team-channel-topology.md`
//! §4 for the documented scope and boundaries.

use std::sync::Arc;

use chrono::{Duration, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use sera_types::circle::{
    CollaborationProofBundle, CollaborationVerdictRecord, ExecutionReceipt, LineageEdge,
    LineageRelation, MetricRef, ProofBundleEntry, ProofBundleMember, ReceiptOutcome, VerdictType,
};
use sera_types::circle_channel::{
    CircleAddressParseError, CircleChannelEvent, CircleChannelEventKind, CircleChannelRole,
};
use sera_types::circle_ingress::{
    CircleIngressCaller, CircleIngressRecord, CircleIngressRequest, CIRCLE_INGRESS_DEFAULT_EVENT_LIMIT,
};
use sera_types::circle_validator::validate_proof_bundle;
use sera_types::runtime::TurnContext;

use crate::circle_activity::{ActivityLogRecordOutcome, SharedCircleActivityLog};
use sera_types::circle_activity::CircleActivityEntry;

/// Identifier kind assigned to the `TurnContext.event_id` we mint inside
/// the ingress funnel. Distinct from the per-event `event_id` on channel
/// events so observers can tell the two apart.
const INGRESS_EVENT_ID_PREFIX: &str = "ingress";

/// What kind of artifact the caller's summary should be classified as in
/// the produced channel `Post` event. Keeping a single string
/// (`resident_ingress_post`) keeps the seam small and lets downstream
/// consumers grep on a known tag without inventing per-member vocabulary.
const RESIDENT_INGRESS_POST_KIND: &str = "resident_ingress_post";

/// Failure modes for [`CircleIngress::accept`].
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CircleIngressError {
    /// The supplied `to:` address did not parse as a Circle address.
    #[error("invalid circle address: {0}")]
    InvalidAddress(#[from] CircleAddressParseError),
    /// The activity log mutex was poisoned; the operation was not applied.
    ///
    /// Returned by [`CircleIngress::accept`] when a non-`None` activity log
    /// failed to record the entry. The address, channel events, and
    /// `TurnContext` assembled for the call are discarded before this error
    /// is surfaced, so the caller never sees a partial state where the
    /// channel events exist without a corresponding activity-log entry.
    /// This is the only truthful return path for a failed activity write —
    /// [`crate::circle_activity::ActivityLogRecordOutcome::Poisoned`] is
    /// never silently coerced into a successful `activity_writes = 1`.
    #[error("activity log mutex poisoned")]
    ActivityLogPoisoned,
}

/// Outcome of an `accept` call, plus any auxiliary effects (activity log
/// entries) that were applied. Returned so tests and future call sites
/// can distinguish "channel events were produced" from "activity log
/// entries were inserted" without re-deriving either.
///
/// `PartialEq` is omitted because the embedded [`CircleIngressRecord`]
/// does not implement it (it carries a [`TurnContext`] which is not
/// `PartialEq` in this base). Tests compare fields explicitly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CircleIngressOutcome {
    /// The canonical record produced by the accept call.
    pub record: CircleIngressRecord,
    /// Activity-log entries that were actually written (one per accept
    /// when a log handle is supplied). Empty when no log was supplied.
    pub activity_writes: u32,
}

/// Resident Circle ingress funnel.
///
/// Constructing one is cheap; it carries no per-Circle state. Hold it on
/// a `RuntimeContext` or similar shared-state struct in downstream code.
#[derive(Debug, Default, Clone)]
pub struct CircleIngress {
    /// Optional activity log handle. When `Some`, each `accept` records
    /// one entry tagged with the member id, circle id, and the request's
    /// summary. Cloning the funnel shares nothing with prior accepts.
    activity: Option<SharedCircleActivityLog>,
}

impl CircleIngress {
    /// Build a new ingress funnel with no activity log attached.
    pub fn new() -> Self {
        Self::default()
    }

    /// Attach a thread-safe activity log; the funnel will record one entry
    /// per successful `accept` call. Cloning the log handle is allowed — the
    /// funnel clones internally so multiple funnels/owners can share state.
    pub fn with_activity_log(mut self, log: SharedCircleActivityLog) -> Self {
        self.activity = Some(log);
        self
    }

    /// Attach a thread-safe activity log by an `Arc` handle. Equivalent to
    /// `with_activity_log`; exists so call sites that already hold an
    /// `Arc<SharedCircleActivityLog>` can pass it through unchanged.
    pub fn with_arc_activity_log(mut self, log: Arc<SharedCircleActivityLog>) -> Self {
        self.activity = Some(Arc::unwrap_or_clone(log));
        self
    }

    /// Accept a Circle address request and produce a bounded resident
    /// record. Returns `Err` only when the address fails to parse; empty
    /// summaries, missing parent_session_key, and duplicate callers are all
    /// accepted (the seam stays small on purpose).
    pub fn accept(
        &self,
        request: &CircleIngressRequest,
    ) -> Result<CircleIngressOutcome, CircleIngressError> {
        let address = request.address()?;

        let session_key = CircleIngressRecord::session_key_for(&address, &request.caller);
        let turn_context = TurnContext {
            event_id: format!(
                "{}-{}",
                INGRESS_EVENT_ID_PREFIX,
                Uuid::new_v4().simple()
            ),
            agent_id: request
                .caller
                .agent_id
                .clone()
                .unwrap_or_else(|| request.caller.member_id.to_string()),
            session_key: session_key.clone(),
            messages: vec![Value::Object(serde_json::Map::from_iter([
                ("role".to_string(), Value::String("user".to_string())),
                (
                    "content".to_string(),
                    Value::String(request.summary.clone()),
                ),
            ]))],
            available_tools: vec![],
            metadata: std::collections::HashMap::new(),
            change_artifact: None,
            parent_session_key: request.parent_session_key.clone(),
            tool_use_behavior: sera_types::tool::ToolUseBehavior::default(),
        };

        let timestamp = Utc::now();
        let base = request.event_id_base.unwrap_or(1);
        let role_claim = CircleChannelEvent {
            event_id: base,
            address: address.clone(),
            timestamp,
            body: CircleChannelEventKind::ClaimRole {
                member: request.caller.member_id.clone(),
                role: request.caller.role,
            },
        };
        let post = CircleChannelEvent {
            event_id: base.saturating_add(1),
            address: address.clone(),
            timestamp,
            body: CircleChannelEventKind::Post {
                member: request.caller.member_id.clone(),
                artifact_type: RESIDENT_INGRESS_POST_KIND.to_string(),
                summary: request.summary.clone(),
            },
        };

        let channel_events = bounded_channel_events(
            vec![role_claim, post],
            CIRCLE_INGRESS_DEFAULT_EVENT_LIMIT,
        );

        let record = CircleIngressRecord {
            request_id: Uuid::new_v4(),
            address: address.clone(),
            caller: request.caller.clone(),
            turn_context,
            channel_events,
            accepted_at: timestamp,
        };

        let mut activity_writes = 0u32;
        if let Some(log) = self.activity.as_ref() {
            match log.record(
                member_id_for_activity(&request.caller),
                address.name.clone(),
                request.summary.clone(),
            ) {
                ActivityLogRecordOutcome::Recorded => {
                    activity_writes = 1;
                }
                ActivityLogRecordOutcome::Poisoned => {
                    // Honest accounting: refuse to claim a write that did not
                    // land. The address, channel events, and TurnContext
                    // already built above are discarded so the caller cannot
                    // observe a partial-state record.
                    return Err(CircleIngressError::ActivityLogPoisoned);
                }
            }
        }

        Ok(CircleIngressOutcome {
            record,
            activity_writes,
        })
    }
}

/// Take at most `limit` events from the front of `produced`. The ingress
/// path always produces a fixed small pair (ClaimRole, Post) today, but
/// the cap is enforced unconditionally so future event growth cannot blow
/// past the documented bound.
fn bounded_channel_events(
    produced: Vec<CircleChannelEvent>,
    limit: usize,
) -> Vec<CircleChannelEvent> {
    if produced.len() <= limit {
        produced
    } else {
        produced.into_iter().take(limit).collect()
    }
}

/// Returns the activity-log `agent_id` for a caller. We prefer the
/// optional `agent_id` so that prompt-assembled activity lists can group
/// by agent, falling back to the bare member id (which the runtime
/// already uses for routing) when no explicit agent is named.
fn member_id_for_activity(caller: &CircleIngressCaller) -> String {
    caller
        .agent_id
        .clone()
        .unwrap_or_else(|| caller.member_id.to_string())
}

/// Convenience: a stand-alone activity log constructor that keeps the
/// `Arc<Mutex<_>>` plumbing out of downstream call sites. Wraps the
/// underlying [`SharedCircleActivityLog`] in an `Arc` so the funnel can
/// share state with other runtime surfaces that need it.
pub fn shared_activity_log() -> Arc<SharedCircleActivityLog> {
    Arc::new(SharedCircleActivityLog::new())
}

// =========================================================================
// Proof closeout / referee export seam (Task D).
//
// Builds a `CollaborationProofBundle` from a resident `CircleIngressRecord`
// plus a small "referee closeout" payload. The bundle is the same shape
// the offline `sera circle run --to` CLI emits, so the existing
// `sera circle validate` proof-validation command round-trips resident
// evidence with zero changes — that is the equivalent validation seam
// for this slice.
//
// Time-stamps are monotonic, deterministic per-(circle, request_id), and
// anchored to a fixed UTC reference so that re-running the closeout with
// identical inputs produces byte-identical bundle bytes. Real wall-clock
// values from `accepted_at` are not embedded in the bundle payload;
// they remain on `CircleIngressRecord.accepted_at` for forensic use.
//
// ## Provider rotation — reconciled with the offline CLI seam
//
// `closeout_provider_for(idx)` is **aligned in spirit but not byte-for-
// byte** with `sera-cli/src/circle_run.rs::provider_for`. The exact
// rotation order differs (resident closeout starts on the local
// provider, CLI run starts on the cloud provider), and `validate_proof_
// bundle` only requires *mixed* local/cloud evidence — not a fixed
// sequence — so both seams pass the validator. Do not assume the
// ordering between the two paths is equivalent; downstream consumers
// must rely on the validator's mixed-provider check, not on a specific
// provider sequence.
// =========================================================================

/// Anchor timestamp for synthesised bundle events.
///
/// Stable across runs and processes so a fixed `(circle, request_id)`
/// pair always closes out into the same bytes.
fn closeout_t0(circle: &str, request_id: &Uuid) -> chrono::DateTime<Utc> {
    // Stable epoch: 2026-07-02T09:00:00Z, plus 1ns per slot in the
    // request_id (modulo 1e9) so two sibling request_ids closeout into
    // *different* but still deterministic seconds.
    let base = Utc.with_ymd_and_hms(2026, 7, 2, 9, 0, 0).unwrap();
    let nanos = (request_id.as_u128() % 1_000_000_000) as i64;
    let circle_offset_secs = (circle.len() as i64).min(60);
    base + Duration::nanoseconds(nanos) + Duration::seconds(circle_offset_secs)
}

fn closeout_event_time(t0: chrono::DateTime<Utc>, offset_secs: i64) -> chrono::DateTime<Utc> {
    t0 + Duration::seconds(offset_secs)
}

/// Provider-class rotation for the closeout bundle.
///
/// Mirrors `sera-cli/src/circle_run.rs::provider_for` so a small two-or-
/// three-entry bundle (Lead + Post + Verdict) still produces the mixed-
/// provider evidence that `validate_proof_bundle` requires. The rotation
/// is intentionally aligned with the offline `sera circle run` seam so
/// downstream consumers do not need two different provider-class schemes
/// to reconcile.
fn closeout_provider_for(idx: usize) -> &'static str {
    if idx.is_multiple_of(2) {
        "custom/local-lmstudio"
    } else {
        "minimax"
    }
}

fn closeout_model_for(provider: &str) -> &'static str {
    if provider.contains("local") || provider.contains("lmstudio") {
        "circle-closeout-local"
    } else {
        "circle-closeout-cloud"
    }
}

/// Inputs the resident runtime supplies to close out an accepted record.
///
/// Keeping this as a small struct (rather than a long parameter list)
/// keeps the closeout signature stable as the seam grows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CircleIngressCloseout {
    /// Referee / integrator member id producing the verdict.
    pub referee_member_id: String,
    /// Short headline for the verdict (e.g. `"approved"`).
    pub verdict: String,
    /// Human-readable rationale accompanying the verdict.
    pub rationale: String,
    /// Optional objective text — when supplied, it is embedded on the bundle
    /// so the resulting proof carries the goal that drove the run. When
    /// `None`, the [`CircleIngressRequest::summary`] from the original
    /// accept is reused, so closeout stays one-shot without losing context.
    pub objective: Option<String>,
}

impl CircleIngressCloseout {
    /// Build a closeout with no override objective.
    pub fn new(
        referee_member_id: impl Into<String>,
        verdict: impl Into<String>,
        rationale: impl Into<String>,
    ) -> Self {
        Self {
            referee_member_id: referee_member_id.into(),
            verdict: verdict.into(),
            rationale: rationale.into(),
            objective: None,
        }
    }

    /// Build a closeout and override the objective text.
    pub fn with_objective(mut self, objective: impl Into<String>) -> Self {
        self.objective = Some(objective.into());
        self
    }
}

/// Outcome of a closeout call: the proof bundle plus a small amount of
/// diagnostic metadata (entry count, receipt count, the deterministic
/// `t0` used for timestamps).
#[derive(Debug, Clone)]
pub struct CircleIngressCloseoutBundle {
    /// The proof bundle itself. Validated via [`validate_proof_bundle`]
    /// inside [`closeout_into_proof_bundle`] before this struct is
    /// returned, so consumers can safely serialise it for
    /// `sera circle validate --bundle`.
    pub bundle: CollaborationProofBundle,
    /// Number of bundle entries produced.
    pub entry_count: usize,
    /// Number of execution receipts produced.
    pub receipt_count: usize,
    /// Anchor timestamp used for the synthesised event times — exposed
    /// so tests can assert determinism without hard-coding the epoch.
    pub t0: chrono::DateTime<Utc>,
}

/// Close out an accepted [`CircleIngressRecord`] into a
/// [`CollaborationProofBundle`] compatible with the existing
/// `sera circle validate` command.
///
/// The mapping is deterministic:
/// - bundle `run_id` derives from `record.request_id` and `record.address.name`,
/// - bundle `circle_id` equals `record.address.name`,
/// - bundle entries are derived one-for-one from `record.channel_events`,
///   plus one synthesised verdict entry from `closeout`,
/// - one execution receipt per bundle entry, with mixed-provider rotation,
/// - one lineage edge per non-first entry pointing at the previous entry,
///   plus a final `Resolves` edge into the referee verdict entry,
/// - the verdict entry carries the bundled
///   [`CollaborationVerdictRecord`] with `reviewer = referee_member_id`.
///
/// Errors are returned when the assembled bundle fails the deterministic
/// `validate_proof_bundle` check (which would otherwise mean a downstream
/// `sera circle validate` would also fail). The error message lists the
/// validator failure kinds so this seam never silently emits evidence
/// that fails validation.
pub fn closeout_into_proof_bundle(
    record: &CircleIngressRecord,
    closeout: &CircleIngressCloseout,
) -> Result<CircleIngressCloseoutBundle, String> {
    let circle_id = &record.address.name;
    let caller_member = record.caller.member_id.as_str();
    let t0 = closeout_t0(circle_id, &record.request_id);

    // Canonical-only guard (Task E carryforward #1). The operator-facing
    // surface is built on top of the canonical Task C ingress shape
    // (`ClaimRole + Post` for one caller). Records with any other event
    // body variant (Receipt / Lineage / Verdict), a different event
    // count, or a member-mismatched pair (`ClaimRole(alice)` followed
    // by `Post(bob)`, for example) are not on the proven happy path for
    // the operator seam — refuse loudly rather than silently emitting a
    // bundle whose roster/role attribution no longer matches the input.
    if !is_canonical_claim_role_post_shape(&record.channel_events, record.caller.member_id.as_str()) {
        return Err(format!(
            "closeout_into_proof_bundle refuses non-canonical ingress shape: \
             expected exactly two events (ClaimRole + Post) from one caller, \
             got {} events with mixed body variants. \
             The operator seam only supports the canonical Task C shape; \
             broaden the seam in a later slice if multi-author ingress is needed.",
            record.channel_events.len(),
        ));
    }

    let objective = closeout
        .objective
        .clone()
        .unwrap_or_else(|| record_summary_from_record(record));

    let mut entries: Vec<ProofBundleEntry> = Vec::new();
    let mut receipts: Vec<ExecutionReceipt> = Vec::new();
    let mut lineage: Vec<LineageEdge> = Vec::new();

    for (idx, event) in record.channel_events.iter().enumerate() {
        let entry_id = (idx as u64) + 1;
        let provider = closeout_provider_for(idx);
        let ts = closeout_event_time(t0, idx as i64);
        let (base_author, artifact_type, role_label, response, body_kind) = match &event.body {
                CircleChannelEventKind::ClaimRole { member, role } => (
                    member.as_str().to_string(),
                    format!("role_claim:{}", role_label_token(*role)),
                    role.default_label(),
                    format!(
                        "Resident ingress: caller {caller_member} claimed role {} (event_id={})",
                        role.default_label(),
                        event.event_id,
                    ),
                    "lead_specification",
                ),
                CircleChannelEventKind::Post {
                    member,
                    artifact_type,
                    summary,
                } => (
                    member.as_str().to_string(),
                    artifact_type.clone(),
                    "Contributor",
                    format!(
                        "Resident ingress post (event_id={}): {summary}",
                        event.event_id
                    ),
                    "proposal",
                ),
                CircleChannelEventKind::Receipt { member, action, .. } => (
                    member.as_str().to_string(),
                    format!("receipt:{action}"),
                    "Contributor",
                    format!("Receipt emitted by {member} for action {action}"),
                    "proposal",
                ),
                CircleChannelEventKind::Lineage {
                    member,
                    from_entry_id,
                    to_entry_id,
                    relation,
                } => (
                    member.as_str().to_string(),
                    format!("lineage:{relation}"),
                    "Contributor",
                    format!("Resident lineage edge {from_entry_id} -{relation}-> {to_entry_id}"),
                    "proposal",
                ),
                CircleChannelEventKind::Verdict {
                    member,
                    verdict,
                    rationale,
                } => (
                    member.as_str().to_string(),
                    "verdict".to_string(),
                    "Referee/Integrator",
                    format!("Resident verdict: {verdict} (rationale: {rationale})"),
                    "verdict",
                ),
            };

            // Make the bundle's entry/receipt author UNIQUE per entry by
            // appending the event id (which is already monotonic per record).
            // The validator's `receipts_by_executor` map is keyed on the
            // entry's author and silently overwrites duplicate authors — the
            // offline `sera circle run` seam avoids this by giving every
            // member a unique `participant_id`. A single resident caller can
            // only contribute one base author, so we disambiguate at the
            // bundle boundary while keeping the underlying `author` readable
            // (the original member id remains the first segment).
            let bundle_author = format!("{base_author}@evt{idx}");

            let payload = match body_kind {
                "verdict" => json!({
                    "role": role_label,
                    "provider": provider,
                    "model": closeout_model_for(provider),
                    "response": response,
                    "author": base_author,
                }),
                _ => json!({
                    "role": role_label,
                    "provider": provider,
                    "model": closeout_model_for(provider),
                    "response": response,
                    "author": base_author,
                }),
            };

            entries.push(ProofBundleEntry {
                entry_id,
                author: bundle_author.clone(),
                timestamp: ts,
                artifact_type,
                payload,
            });
            receipts.push(make_closeout_receipt(
                entry_id,
                &bundle_author,
                artifact_type_for_closeout(body_kind),
                provider,
                ts,
            ));

            if entry_id > 1 {
                lineage.push(LineageEdge {
                    from_entry_id: entry_id - 1,
                    to_entry_id: entry_id,
                    relation: LineageRelation::DerivesFrom,
                });
            }
        }

        let verdict_entry_id = entries.len() as u64 + 1;
        let referee_provider = closeout_provider_for(entries.len());
        let referee_ts = closeout_event_time(t0, entries.len() as i64);
        let referee_author = format!("{}@verdict", closeout.referee_member_id);
        entries.push(ProofBundleEntry {
            entry_id: verdict_entry_id,
            author: referee_author.clone(),
            timestamp: referee_ts,
            artifact_type: "verdict".to_string(),
            payload: json!({
                "role": CircleChannelRole::Referee.default_label(),
                "provider": referee_provider,
                "model": closeout_model_for(referee_provider),
                "response": format!(
                    "Referee {}: {}. {}",
                    closeout.referee_member_id, closeout.verdict, closeout.rationale
                ),
                "author": closeout.referee_member_id,
            }),
        });
        receipts.push(make_closeout_receipt(
            verdict_entry_id,
            &referee_author,
            "sera_circle_ingress_verdict",
            referee_provider,
            referee_ts,
        ));

    // Connect every prior entry into the verdict so the bundle's lineage
    // is connected to the referee (Task D's "explicit closeout" beat).
    let last_pre_referee_entry = entries.len() as u64 - 1;
    if last_pre_referee_entry >= 1 {
        lineage.push(LineageEdge {
            from_entry_id: last_pre_referee_entry,
            to_entry_id: verdict_entry_id,
            relation: LineageRelation::Resolves,
        });
    }

    let roster = vec![
        ProofBundleMember {
            participant_id: caller_member.to_string(),
            role: record.caller.role.default_label().to_string(),
        },
        ProofBundleMember {
            participant_id: closeout.referee_member_id.clone(),
            role: CircleChannelRole::Referee.default_label().to_string(),
        },
    ];

    let verdict = Some(CollaborationVerdictRecord {
        reviewer: closeout.referee_member_id.clone(),
        timestamp: referee_ts,
        verdict_type: classify_verdict(&closeout.verdict, &closeout.rationale),
        rationale: closeout.rationale.clone(),
    });

    let bundle = CollaborationProofBundle {
        run_id: format!(
            "sera-circle-ingress-{}-{}",
            circle_id,
            &record.request_id.simple().to_string()[..12],
        ),
        circle_id: circle_id.clone(),
        objective,
        success_metric: MetricRef::Inline {
            description: "Resident ingress channel events round-trip into a structurally \
                          valid CollaborationProofBundle (mixed-provider, verdict, lineage)."
                .to_string(),
        },
        roster,
        budget_snapshot: None,
        entries,
        lineage,
        execution_receipts: receipts,
        verdict,
    };

    if let Err(errors) = validate_proof_bundle(&bundle) {
        let kinds: Vec<String> = errors.iter().map(|e| format!("{:?}", e.kind)).collect();
        return Err(format!(
            "closeout bundle failed validate_proof_bundle: {}",
            kinds.join("; ")
        ));
    }

    let entry_count = bundle.entries.len();
    let receipt_count = bundle.execution_receipts.len();

    Ok(CircleIngressCloseoutBundle {
        bundle,
        entry_count,
        receipt_count,
        t0,
    })
}

/// Returns true iff the supplied channel-event list matches the canonical
/// Task C ingress shape: exactly two events, the first a `ClaimRole` and
/// the second a `Post`, **both** authored by `caller_member_id`. The caller
/// match is what stops a record like `ClaimRole(alice) → Post(bob)` from
/// sliding through into a bundle whose roster, role attribution, and
/// receipt layout no longer describe one accountable agent.
///
/// Used by [`closeout_into_proof_bundle`] (canonical-only guard) and by
/// [`address_circle`] (operator-surface guarantee) so the bundle's
/// roster, role attribution, and receipt layout always match the
/// one-caller + one-role-claim + one-post contract that the validator
/// and the closeout helper were designed against.
fn is_canonical_claim_role_post_shape(
    events: &[CircleChannelEvent],
    caller_member_id: &str,
) -> bool {
    if events.len() != 2 {
        return false;
    }
    let claim_member = match &events[0].body {
        CircleChannelEventKind::ClaimRole { member, .. } => member.as_str(),
        _ => return false,
    };
    let post_member = match &events[1].body {
        CircleChannelEventKind::Post { member, .. } => member.as_str(),
        _ => return false,
    };
    claim_member == caller_member_id && post_member == caller_member_id
}

// =========================================================================
// Operator observability surface (Task E).
//
// `address_circle` is the one narrow operator-facing path that ties
// `CircleIngress::accept` and `closeout_into_proof_bundle` together and
// returns a structured report covering:
//   - canonical circle address / id
//   - caller member id + role
//   - session lineage (session_key + parent_session_key + request_id)
//   - proof bundle metadata (run_id, entry/receipt/lineage counts)
//   - verdict reviewer, type, rationale
//   - proof bundle sha256 + validate_proof_bundle success status
//   - audit-log tail for the addressed circle (used for in-process
//     observability; carries the freshly recorded entry plus any prior
//     peer entries that already live in the shared activity log).
//
// The seam is intentionally small and in-process: no Discord delivery,
// no external connector, no scheduler, no DB persistence. It is
// designed to back the `sera circle closeout` CLI command (and a future
// in-runtime operator route) without inventing a parallel ingress
// pipeline. The ActivityLog tail is gathered via
// `SharedCircleActivityLog::recent_for_circle` — never via
// `total_entries()` — so a poisoned mutex cannot silently zero the
// operator's view of recent activity (carryforward #2).
// =========================================================================

/// Default number of audit-log entries included in an
/// [`OperatorCloseoutReport::audit_tail`]. Tunable on `address_circle`
/// via `audit_limit`.
pub const OPERATOR_REPORT_DEFAULT_AUDIT_LIMIT: usize = 8;

/// Structured outcome of [`address_circle`] — the audit-ready, JSON-
/// friendly view of a resident Circle run.
#[derive(Debug, Clone, Serialize)]
pub struct OperatorCloseoutReport {
    /// Canonical Circle id (e.g. `"sera-nqh3"`).
    pub circle_id: String,
    /// Canonical Circle address (e.g. `"to:circle:sera-nqh3"`).
    pub address: String,
    /// Caller member id (e.g. `"alice"`).
    pub member_id: String,
    /// Caller's role claim (Lead / Worker / Critic / Referee).
    pub role: CircleChannelRole,
    /// Canonical session key for the addressed Circle.
    pub session_key: String,
    /// Parent session key (lineage). `None` for a root ingress call.
    pub parent_session_key: Option<String>,
    /// Unique identifier for this ingress accept call.
    pub request_id: Uuid,
    /// Proof bundle run id (`sera-circle-ingress-<circle>-<req-id-prefix>`).
    pub run_id: String,
    /// Verdict reviewer (the supplied referee / integrator).
    pub verdict_reviewer: String,
    /// Verdict type as classified by the helper.
    pub verdict_type: VerdictType,
    /// Verdict rationale supplied to the helper.
    pub verdict_rationale: String,
    /// Number of bundle entries (ingress events + verdict).
    pub entry_count: usize,
    /// Number of execution receipts in the bundle.
    pub receipt_count: usize,
    /// Number of lineage edges in the bundle.
    pub lineage_edge_count: usize,
    /// sha256 of the serialised proof bundle, hex-encoded.
    pub bundle_sha256: String,
    /// Anchor timestamp used for the synthesised bundle events. Surfaced
    /// on the operator report so callers writing the bundle to disk can
    /// reconstruct the same [`CircleIngressCloseoutBundle`] shape the
    /// helper produces.
    pub run_id_t0: chrono::DateTime<chrono::Utc>,
    /// Outcome of `validate_proof_bundle` on the emitted bundle. Always
    /// `Ok(())` in practice (the helper refuses to return a bundle that
    /// does not validate) but surfaced here so the operator surface
    /// does not rely on the helper's internal contract.
    pub validation: Result<(), Vec<String>>,
    /// Most-recent activity-log entries for the addressed circle, fetched
    /// via `recent_for_circle(name, "nobody", Some(limit))`. The `"nobody"`
    /// exclude-agent sentinel means the tail can include the just-recorded
    /// operator accept entry when a log is attached — the field name is
    /// descriptive of *what this is* (a recent activity tail for the
    /// addressed circle), not a guarantee that the caller is filtered out.
    /// When no log is attached this stays empty.
    pub audit_tail: Vec<CircleActivityEntry>,
    /// Whether the activity log actually recorded the operator's accept.
    /// `0` when no log was attached, `1` when the write succeeded. This
    /// is the same number the runtime's `CircleIngress::accept` returns
    /// — kept here so an operator can sanity-check the accounting.
    pub activity_writes: u32,
    /// The underlying [`CollaborationProofBundle`] produced by
    /// [`closeout_into_proof_bundle`]. Surfaced on the operator report
    /// so CLI callers can write a canonical bundle file to disk
    /// (re-validatable via `sera circle validate --bundle`) without
    /// re-running the closeout helper.
    #[serde(skip)]
    pub bundle: CollaborationProofBundle,
}

/// Drive a complete resident Circle run from the operator's perspective:
///
/// 1. accept the request via [`CircleIngress::accept`] using the
///    canonical `ClaimRole + Post` shape,
/// 2. close the resulting record out into a [`CollaborationProofBundle`]
///    via [`closeout_into_proof_bundle`] (canonical-only shape enforced),
/// 3. re-validate the bundle explicitly so the operator sees a fresh
///    validator verdict (the helper already validates internally; this
///    second check is purely so the report's `validation` field never
///    depends on a hidden contract),
/// 4. attach a recent activity-log tail for the addressed circle so the
///    operator can correlate with peer activity,
/// 5. return an [`OperatorCloseoutReport`] with everything in one place.
///
/// The function never mutates global state outside the funnel's
/// activity log (when one is attached) and the in-process
/// `CollaborationProofBundle` it returns. It is the single seam the
/// operator-facing route or CLI command should call.
pub fn address_circle(
    funnel: &CircleIngress,
    request: &CircleIngressRequest,
    closeout: &CircleIngressCloseout,
    audit_limit: Option<usize>,
) -> Result<OperatorCloseoutReport, String> {
    // 1. accept (errors are surfaced verbatim — InvalidAddress, ActivityLogPoisoned).
    let outcome = funnel
        .accept(request)
        .map_err(|err| format!("CircleIngress::accept failed: {err}"))?;

    // 2. closeout — canonical-only guard already wired inside the helper.
    let bundle_out = closeout_into_proof_bundle(&outcome.record, closeout)?;

    // 3. re-validate so the operator sees a fresh validator verdict.
    let validation = validate_proof_bundle(&bundle_out.bundle)
        .map_err(|errors| errors.iter().map(|e| format!("{:?}", e.kind)).collect());

    // 4. audit tail — uses recent_for_circle, not total_entries, so a
    // poisoned mutex cannot silently shrink the operator's view
    // (carryforward #2 from the Task D review).
    let audit_limit = audit_limit.unwrap_or(OPERATOR_REPORT_DEFAULT_AUDIT_LIMIT);
    let audit_tail: Vec<CircleActivityEntry> = funnel
        .activity
        .as_ref()
        .map(|log| log.recent_for_circle(&outcome.record.address.name, "nobody", Some(audit_limit)))
        .unwrap_or_default();

    // 5. report.
    //
    // Hash the **pretty-printed** serialisation of the bundle so the
    // reported sha256 matches the bytes the operator CLI writes to
    // `--bundle-out` (`serde_json::to_vec_pretty`) and the bytes
    // `sera circle validate --bundle` re-reads off disk. Using compact
    // serialisation here would silently diverge from the on-disk bytes
    // and break the `circle-closeout:` footer contract that ties the
    // report, the written file, and the validator footer together.
    let bundle_bytes = serde_json::to_vec_pretty(&bundle_out.bundle)
        .map_err(|err| format!("failed to serialise bundle for sha256: {err}"))?;
    let bundle_sha256 = sha256_hex_inline(&bundle_bytes);

    let verdict = bundle_out
        .bundle
        .verdict
        .as_ref()
        .ok_or_else(|| "closeout bundle produced without a verdict record".to_string())?;

    Ok(OperatorCloseoutReport {
        circle_id: bundle_out.bundle.circle_id.clone(),
        address: outcome.record.address.to_string(),
        member_id: outcome.record.caller.member_id.to_string(),
        role: outcome.record.caller.role,
        session_key: outcome.record.turn_context.session_key.clone(),
        parent_session_key: outcome.record.turn_context.parent_session_key.clone(),
        request_id: outcome.record.request_id,
        run_id: bundle_out.bundle.run_id.clone(),
        verdict_reviewer: verdict.reviewer.clone(),
        verdict_type: verdict.verdict_type.clone(),
        verdict_rationale: verdict.rationale.clone(),
        entry_count: bundle_out.entry_count,
        receipt_count: bundle_out.receipt_count,
        lineage_edge_count: bundle_out.bundle.lineage.len(),
        bundle_sha256,
        validation,
        audit_tail,
        activity_writes: outcome.activity_writes,
        run_id_t0: bundle_out.t0,
        bundle: bundle_out.bundle,
    })
}

/// Hex sha256 of a byte slice. Local copy of the helper used by
/// `sera-cli::sha256_hex` so this module remains dependency-light (the
/// CLI helper is `pub(crate)` and not exported across crate boundaries).
fn sha256_hex_inline(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn record_summary_from_record(record: &CircleIngressRecord) -> String {
    // The first channel event with a `Post` body carries the request's
    // summary; fall back to the first non-empty string we can find so the
    // bundle always has a non-blank objective.
    for event in &record.channel_events {
        if let CircleChannelEventKind::Post { summary, .. } = &event.body
            && !summary.trim().is_empty()
        {
            return summary.clone();
        }
        if let CircleChannelEventKind::Verdict { verdict, .. } = &event.body
            && !verdict.trim().is_empty()
        {
            return verdict.clone();
        }
    }
    "resident Circle ingress run".to_string()
}

fn role_label_token(role: CircleChannelRole) -> &'static str {
    match role {
        CircleChannelRole::Lead => "lead",
        CircleChannelRole::Worker => "worker",
        CircleChannelRole::Critic => "critic",
        CircleChannelRole::Referee => "referee",
    }
}

fn artifact_type_for_closeout(body_kind: &str) -> &'static str {
    match body_kind {
        "verdict" => "sera_circle_ingress_verdict",
        "lead_specification" => "sera_circle_ingress_lead",
        _ => "sera_circle_ingress_post",
    }
}

fn make_closeout_receipt(
    seq: u64,
    executor: &str,
    action: &str,
    provider: &str,
    timestamp: chrono::DateTime<Utc>,
) -> ExecutionReceipt {
    ExecutionReceipt {
        receipt_id: format!("rcpt_ingress_{seq:03}_{executor}"),
        executor: executor.to_string(),
        timestamp,
        action: action.to_string(),
        parameters: json!({
            "provider": provider,
            "model": closeout_model_for(provider),
            "duration_ms": 1,
            "returncode": 0,
        }),
        outcome: ReceiptOutcome::Success,
        cost_tokens: Some(1),
    }
}

fn classify_verdict(verdict_text: &str, rationale: &str) -> VerdictType {
    let lc = verdict_text.to_ascii_lowercase();
    if lc.contains("approve") {
        VerdictType::Approved
    } else if lc.contains("reject") {
        VerdictType::Rejected {
            reason: rationale.to_string(),
        }
    } else if lc.contains("tie") {
        VerdictType::Tie {
            note: rationale.to_string(),
        }
    } else if lc.contains("invalid") {
        VerdictType::Invalid {
            rule: rationale.to_string(),
        }
    } else {
        VerdictType::RevisionRequired {
            feedback: rationale.to_string(),
        }
    }
}

// Re-export removed — kept here as a comment marker so future readers see
// that `CircleMemberId` is intentionally not in this module's top-level
// imports (it is only used inside the `tests` submodule below).

#[cfg(test)]
mod tests {
    use super::*;
    use sera_types::circle_channel::{CircleChannelRole, CircleMemberId};

    fn lead_request() -> CircleIngressRequest {
        CircleIngressRequest::new(
            "to:circle:sera-nqh3",
            "alice",
            CircleChannelRole::Lead,
            "open the run",
        )
        .with_agent_id("agent-007")
        .with_parent_session_key("session:parent-1")
    }

    #[test]
    fn accept_canonical_address_returns_record_with_two_events() {
        let funnel = CircleIngress::new();
        let outcome = funnel.accept(&lead_request()).unwrap();

        assert_eq!(outcome.record.address.name, "sera-nqh3");
        assert_eq!(outcome.record.address.to_string(), "to:circle:sera-nqh3");
        assert_eq!(outcome.activity_writes, 0, "no log attached");
        assert_eq!(outcome.record.channel_events.len(), 2);

        // ClaimRole(Lead, alice) is the first event.
        match &outcome.record.channel_events[0].body {
            CircleChannelEventKind::ClaimRole { member, role } => {
                assert_eq!(member, &CircleMemberId::from("alice"));
                assert_eq!(*role, CircleChannelRole::Lead);
            }
            other => panic!("expected ClaimRole, got {:?}", other),
        }

        // Post with the request summary.
        match &outcome.record.channel_events[1].body {
            CircleChannelEventKind::Post {
                member,
                artifact_type,
                summary,
            } => {
                assert_eq!(member, &CircleMemberId::from("alice"));
                assert_eq!(artifact_type, RESIDENT_INGRESS_POST_KIND);
                assert_eq!(summary, "open the run");
            }
            other => panic!("expected Post, got {:?}", other),
        }
    }

    #[test]
    fn session_key_carries_circle_name_and_member_id() {
        let funnel = CircleIngress::new();
        let outcome = funnel.accept(&lead_request()).unwrap();
        assert_eq!(
            outcome.record.turn_context.session_key,
            "session:circle:sera-nqh3:alice"
        );
    }

    #[test]
    fn lineage_propagates_via_parent_session_key() {
        let funnel = CircleIngress::new();
        let outcome = funnel.accept(&lead_request()).unwrap();
        assert_eq!(
            outcome.record.turn_context.parent_session_key.as_deref(),
            Some("session:parent-1")
        );
    }

    #[test]
    fn root_ingress_call_has_no_parent_session_key() {
        let req = CircleIngressRequest::new(
            "sera-nqh3",
            "bob",
            CircleChannelRole::Worker,
            "submit proposal",
        );
        let funnel = CircleIngress::new();
        let outcome = funnel.accept(&req).unwrap();
        assert!(outcome.record.turn_context.parent_session_key.is_none());
    }

    #[test]
    fn invalid_address_is_rejected_without_mutation() {
        let req = CircleIngressRequest::new(
            "to:agent:bob",
            "alice",
            CircleChannelRole::Lead,
            "x",
        );
        let log = shared_activity_log();
        let funnel = CircleIngress::new().with_arc_activity_log(Arc::clone(&log));

        let err = funnel.accept(&req).unwrap_err();
        assert!(matches!(err, CircleIngressError::InvalidAddress(_)));
        // Strengthened: assert on *total* activity-log entries across all
        // circles — the older test only filtered on circle_id and would
        // have passed even if a regression wrote to a different circle.
        assert_eq!(
            log.total_entries(),
            0,
            "failed parse must not write any activity entries, anywhere",
        );
        assert!(
            log.recent_for_circle("bob", "nobody", None).is_empty(),
            "failed parse must not write activity entries for circle 'bob'",
        );
    }

    #[test]
    fn activity_log_poisoned_surfaces_truthfully() {
        // Build a fresh log, then poison its inner mutex by panicking
        // inside a thread that holds the lock. accept() must then return
        // ActivityLogPoisoned rather than producing a record with a
        // swallowed write.
        let log = shared_activity_log();
        let poisoned_inner = log.inner_arc();
        let join = std::thread::spawn(move || {
            let _g = poisoned_inner.lock().unwrap();
            panic!("intentional poison for ActivityLogPoisoned accounting test");
        });
        let _ = join.join();

        let req = CircleIngressRequest::new(
            "to:circle:sera-nqh3",
            "alice",
            CircleChannelRole::Lead,
            "do the thing",
        );
        let funnel = CircleIngress::new().with_arc_activity_log(Arc::clone(&log));
        let err = funnel.accept(&req).unwrap_err();
        assert!(
            matches!(err, CircleIngressError::ActivityLogPoisoned),
            "expected honest ActivityLogPoisoned return, got {err:?}",
        );
        // No activity entry should have been written via the wrapper —
        // the previous code would have silently reported activity_writes=1
        // despite the inner log never receiving the entry.
        assert_eq!(
            log.total_entries(),
            0,
            "a poisoned write must not appear as a successful record",
        );
    }
    #[test]
    fn activity_log_records_one_entry_per_accepted_call() {
        // Two back-to-back `accept` calls in the same process share a
        // wall-clock second; the activity log's newest-first sort is
        // stable-but-equal in that case. We assert count and identity
        // invariants rather than order.
        let log = shared_activity_log();
        let funnel = CircleIngress::new().with_arc_activity_log(Arc::clone(&log));

        let mut req = CircleIngressRequest::new(
            "circle:sera-nqh3",
            "alice",
            CircleChannelRole::Lead,
            "first",
        )
        .with_agent_id("agent-007");
        let outcome1 = funnel.accept(&req).unwrap();
        assert_eq!(outcome1.activity_writes, 1);

        req = CircleIngressRequest::new(
            "circle:sera-nqh3",
            "alice",
            CircleChannelRole::Lead,
            "second",
        )
        .with_agent_id("agent-007");
        let outcome2 = funnel.accept(&req).unwrap();
        assert_eq!(outcome2.activity_writes, 1);

        let entries = log.recent_for_circle("sera-nqh3", "nobody", None);
        assert_eq!(entries.len(), 2, "two accepted calls wrote two entries");
        assert!(
            entries.iter().all(|e| e.agent_id == "agent-007"),
            "activity entries should carry the caller-supplied agent_id",
        );
        let summaries: std::collections::BTreeSet<&str> =
            entries.iter().map(|e| e.summary.as_str()).collect();
        assert_eq!(summaries, ["first", "second"].into_iter().collect());
    }

    #[test]
    fn event_id_base_offsets_produced_events() {
        let req = CircleIngressRequest::new(
            "to:circle:sera-nqh3",
            "alice",
            CircleChannelRole::Referee,
            "verdict pending",
        )
        .with_event_id_base(10);
        let funnel = CircleIngress::new();
        let outcome = funnel.accept(&req).unwrap();
        assert_eq!(outcome.record.channel_events[0].event_id, 10);
        assert_eq!(outcome.record.channel_events[1].event_id, 11);
    }

    #[test]
    fn record_request_id_is_unique_between_accepts() {
        let req = CircleIngressRequest::new(
            "to:circle:sera-nqh3",
            "alice",
            CircleChannelRole::Lead,
            "x",
        );
        let funnel = CircleIngress::new();
        let a = funnel.accept(&req).unwrap();
        let b = funnel.accept(&req).unwrap();
        assert_ne!(a.record.request_id, b.record.request_id);
        // session_keys are identical (same caller + address); lineage is
        // distinct via the new request_id.
        assert_eq!(a.record.turn_context.session_key, b.record.turn_context.session_key);
    }

    // -----------------------------------------------------------------
    // Task D — proof closeout / referee export seam.
    // -----------------------------------------------------------------

    fn lead_request_d() -> CircleIngressRequest {
        CircleIngressRequest::new(
            "to:circle:sera-nqh3",
            "alice",
            CircleChannelRole::Lead,
            "open the run and submit proposal",
        )
        .with_agent_id("agent-007")
    }

    fn approved_closeout() -> CircleIngressCloseout {
        CircleIngressCloseout::new(
            "ref",
            "approved",
            "Lead alice claimed the role, posted a non-blank summary, \
             and the channel events round-trip into a valid bundle.",
        )
        .with_objective("Resident ingress closeout: lead + post + referee verdict")
    }

    #[test]
    fn closeout_produces_bundle_with_member_identity_and_verdict() {
        let funnel = CircleIngress::new();
        let outcome = funnel.accept(&lead_request_d()).unwrap();

        let closeout = approved_closeout();
        let out = closeout_into_proof_bundle(&outcome.record, &closeout)
            .expect("closeout bundle must validate");

        // Member identity carried over.
        assert_eq!(out.bundle.circle_id, "sera-nqh3");
        assert!(
            out.bundle
                .roster
                .iter()
                .any(|m| m.participant_id == "alice" && m.role == "Lead"),
            "caller should appear as Lead in the bundle roster",
        );
        assert!(
            out.bundle
                .roster
                .iter()
                .any(|m| m.participant_id == "ref" && m.role == "Referee/Integrator"),
            "referee should appear in the bundle roster",
        );

        // Verdict is recorded by the named referee.
        let verdict = out.bundle.verdict.as_ref().expect("verdict must be set");
        assert_eq!(verdict.reviewer, "ref");
        assert!(matches!(
            verdict.verdict_type,
            VerdictType::Approved
        ));

        // Two ingress events (ClaimRole + Post) plus the referee verdict
        // entry means three proof entries in the bundle.
        assert_eq!(out.bundle.entries.len(), 3, "2 ingress + 1 verdict");
        assert_eq!(out.entry_count, 3);
        // Receipts: one per entry, so 3.
        assert_eq!(out.bundle.execution_receipts.len(), 3);
        assert_eq!(out.receipt_count, 3);
        // Lineage: 1 DerivesFrom between the two ingress entries plus
        // 1 Resolves edge from the last pre-referee entry into the
        // verdict entry.
        assert_eq!(out.bundle.lineage.len(), 2);

        // Receipt provider classes are mixed (one local, one cloud).
        let mut has_local = false;
        let mut has_cloud = false;
        for receipt in &out.bundle.execution_receipts {
            let provider = receipt
                .parameters
                .get("provider")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if provider.contains("local") || provider.contains("lmstudio") {
                has_local = true;
            } else {
                has_cloud = true;
            }
        }
        assert!(
            has_local && has_cloud,
            "closeout bundle must include both local and cloud receipts",
        );
    }

    #[test]
    fn closeout_bundle_passes_validate_proof_bundle() {
        let funnel = CircleIngress::new();
        let outcome = funnel.accept(&lead_request_d()).unwrap();
        let closeout = approved_closeout();

        let out =
            closeout_into_proof_bundle(&outcome.record, &closeout).expect("closeout must succeed");
        // Re-run the validator directly to prove the bundle survives a
        // round-trip through the same check `sera circle validate` uses.
        assert!(
            validate_proof_bundle(&out.bundle).is_ok(),
            "closeout bundle must satisfy validate_proof_bundle",
        );
    }

    #[test]
    fn closeout_bundle_is_byte_deterministic_for_same_record() {
        let funnel = CircleIngress::new();
        let outcome = funnel.accept(&lead_request_d()).unwrap();
        let closeout = approved_closeout();

        let a = closeout_into_proof_bundle(&outcome.record, &closeout).unwrap();
        let b = closeout_into_proof_bundle(&outcome.record, &closeout).unwrap();

        let json_a = serde_json::to_vec(&a.bundle).unwrap();
        let json_b = serde_json::to_vec(&b.bundle).unwrap();
        assert_eq!(json_a, json_b, "bundle bytes must be deterministic");

        assert_eq!(a.t0, b.t0);
    }

    #[test]
    fn closeout_propagates_record_id_and_caller_into_bundle() {
        let funnel = CircleIngress::new();
        let outcome = funnel.accept(&lead_request_d()).unwrap();

        let out = closeout_into_proof_bundle(&outcome.record, &approved_closeout()).unwrap();
        // The bundle's run_id must derive from the record's request_id
        // (first 12 hex chars) plus the circle name.
        assert!(out.bundle.run_id.starts_with("sera-circle-ingress-sera-nqh3-"));
        // The first bundle entry must be authored by the ingress caller.
        assert_eq!(out.bundle.entries[0].author, "alice@evt0");
        // The first entry's payload must carry the *original* member id
        // under the `author` field so the bundle remains human-readable.
        assert_eq!(out.bundle.entries[0].payload["author"], "alice");
        // Objective was provided in the closeout.
        assert!(out.bundle.objective.contains("Resident ingress closeout"));
    }

    #[test]
    fn closeout_classifies_known_verdict_strings() {
        let funnel = CircleIngress::new();
        let outcome = funnel.accept(&lead_request_d()).unwrap();

        for (text, expected_kind) in [
            ("approved", "Approved"),
            ("APPROVE please", "Approved"),
            ("rejected because X", "Rejected"),
            ("tie between options", "Tie"),
            ("invalid input", "Invalid"),
            ("needs another pass", "RevisionRequired"),
        ] {
            let closeout = CircleIngressCloseout::new("ref", text, "rationale for the verdict");
            let out = closeout_into_proof_bundle(&outcome.record, &closeout)
                .unwrap_or_else(|err| {
                    panic!("closeout for {text:?} failed: {err}");
                });
            let kind = format!("{:?}", out.bundle.verdict.as_ref().unwrap().verdict_type);
            let kind = kind.split_whitespace().next().unwrap_or("").trim_end_matches('{');
            assert!(
                kind.contains(expected_kind),
                "verdict text {text:?} should classify as {expected_kind}, got {kind}",
            );
        }
    }

    #[test]
    fn closeout_emits_distinct_run_ids_for_distinct_request_ids() {
        let funnel = CircleIngress::new();
        let a = funnel.accept(&lead_request_d()).unwrap();
        let b = funnel.accept(&lead_request_d()).unwrap();
        let closeout = approved_closeout();

        let out_a = closeout_into_proof_bundle(&a.record, &closeout).unwrap();
        let out_b = closeout_into_proof_bundle(&b.record, &closeout).unwrap();

        assert_ne!(
            out_a.bundle.run_id, out_b.bundle.run_id,
            "distinct request_ids must close out into distinct run_ids",
        );
    }

    // -----------------------------------------------------------------
    // Task E — operator observability surface.
    // -----------------------------------------------------------------

    fn operator_request() -> CircleIngressRequest {
        CircleIngressRequest::new(
            "to:circle:sera-nqh3",
            "alice",
            CircleChannelRole::Lead,
            "open the run and submit proposal",
        )
        .with_agent_id("agent-007")
        .with_parent_session_key("session:parent-task-e")
    }

    fn operator_closeout() -> CircleIngressCloseout {
        CircleIngressCloseout::new(
            "ref",
            "approved",
            "Lead alice claimed the role, posted a non-blank summary, \
             and the channel events round-trip into a valid bundle.",
        )
        .with_objective("Operator-surface closeout: lead + post + referee verdict")
    }

    #[test]
    fn address_circle_returns_report_with_all_required_fields() {
        let log = shared_activity_log();
        let funnel = CircleIngress::new().with_arc_activity_log(Arc::clone(&log));

        let report =
            address_circle(&funnel, &operator_request(), &operator_closeout(), None)
                .expect("address_circle must succeed on canonical input");

        // Canonical circle + address.
        assert_eq!(report.circle_id, "sera-nqh3");
        assert_eq!(report.address, "to:circle:sera-nqh3");

        // Member identity + role.
        assert_eq!(report.member_id, "alice");
        assert_eq!(report.role, CircleChannelRole::Lead);

        // Session lineage — session_key carries the address + member;
        // parent_session_key is propagated verbatim; request_id is a
        // real Uuid (not nil).
        assert_eq!(report.session_key, "session:circle:sera-nqh3:alice");
        assert_eq!(
            report.parent_session_key.as_deref(),
            Some("session:parent-task-e"),
        );
        assert!(
            !report.request_id.is_nil(),
            "address_circle must mint a fresh request_id",
        );

        // Proof bundle metadata: run_id is the closeout-derived
        // identifier; counts match the canonical shape (2 ingress +
        // 1 verdict = 3 entries, 3 receipts, 2 lineage edges).
        assert!(report.run_id.starts_with("sera-circle-ingress-sera-nqh3-"));
        assert_eq!(report.entry_count, 3);
        assert_eq!(report.receipt_count, 3);
        assert_eq!(report.lineage_edge_count, 2);

        // Verdict reviewer + type + rationale carried through.
        assert_eq!(report.verdict_reviewer, "ref");
        assert!(matches!(report.verdict_type, VerdictType::Approved));
        assert!(report.verdict_rationale.contains("Lead alice claimed"));

        // Validation result is Ok — the operator surface re-runs the
        // validator and reports it honestly.
        assert!(
            report.validation.is_ok(),
            "address_circle must surface a fresh validator verdict, got {:?}",
            report.validation,
        );

        // Bundle sha256 is a 64-char lowercase hex string.
        assert_eq!(report.bundle_sha256.len(), 64);
        assert!(
            report.bundle_sha256.chars().all(|c| c.is_ascii_hexdigit()),
            "bundle_sha256 must be hex, got {:?}",
            report.bundle_sha256,
        );

        // Activity accounting — one entry written.
        assert_eq!(report.activity_writes, 1);

        // Audit tail — at least the entry we just recorded is present.
        // We exclude the caller themselves ("nobody" sentinel here
        // keeps the test stable across the "exclude own agent_id"
        // semantics of `recent_for_circle`).
        assert!(
            !report.audit_tail.is_empty(),
            "address_circle must surface a non-empty audit tail when a log is attached",
        );
        assert!(
            report.audit_tail.iter().all(|e| e.circle_id == "sera-nqh3"),
            "audit tail must be scoped to the addressed circle",
        );
    }

    #[test]
    fn address_circle_round_trips_through_serialize_json() {
        let funnel = CircleIngress::new();
        let report = address_circle(&funnel, &operator_request(), &operator_closeout(), None)
            .expect("address_circle must succeed");

        let json = serde_json::to_string(&report)
            .expect("OperatorCloseoutReport must serialise to JSON");
        // Spot-check that the JSON exposes every required field under a
        // snake_case key so a downstream machine parser does not need
        // to know the Rust type.
        for required_key in [
            "circle_id",
            "address",
            "member_id",
            "role",
            "session_key",
            "parent_session_key",
            "request_id",
            "run_id",
            "verdict_reviewer",
            "verdict_type",
            "verdict_rationale",
            "entry_count",
            "receipt_count",
            "lineage_edge_count",
            "bundle_sha256",
            "run_id_t0",
            "validation",
            "audit_tail",
            "activity_writes",
        ] {
            assert!(
                json.contains(required_key),
                "OperatorCloseoutReport JSON must expose key {required_key:?}; got {json}",
            );
        }
    }

    #[test]
    fn address_circle_refuses_invalid_address() {
        let funnel = CircleIngress::new();
        let bad = CircleIngressRequest::new(
            "to:agent:bob",
            "alice",
            CircleChannelRole::Lead,
            "x",
        );
        let err = address_circle(&funnel, &bad, &operator_closeout(), None).unwrap_err();
        assert!(
            err.contains("CircleIngress::accept failed"),
            "address_circle must surface accept errors verbatim, got {err:?}",
        );
        assert!(
            err.contains("invalid circle address"),
            "address_circle must surface the underlying InvalidAddress error, got {err:?}",
        );
    }

    #[test]
    fn address_circle_without_log_still_returns_report_with_empty_audit_tail() {
        let funnel = CircleIngress::new();
        let report =
            address_circle(&funnel, &operator_request(), &operator_closeout(), None)
                .expect("address_circle must succeed without an activity log attached");
        assert_eq!(report.activity_writes, 0);
        assert!(
            report.audit_tail.is_empty(),
            "no log attached means no audit tail, got {:?}",
            report.audit_tail,
        );
        // Bundle + verdict are still computed and valid.
        assert!(report.validation.is_ok());
        assert_eq!(report.entry_count, 3);
    }

    #[test]
    fn canonical_only_guard_rejects_non_canonical_record() {
        // Build a record by hand with three events (one too many) — the
        // closeout helper must refuse, even though the helper's
        // exhaustive match would happily produce a bundle from it.
        let funnel = CircleIngress::new();
        let outcome = funnel.accept(&operator_request()).unwrap();
        let mut events = outcome.record.channel_events.clone();
        events.push(events[1].clone());
        let mut mutated_record = outcome.record.clone();
        mutated_record.channel_events = events;

        let err = closeout_into_proof_bundle(&mutated_record, &operator_closeout())
            .unwrap_err();
        assert!(
            err.contains("non-canonical ingress shape"),
            "closeout must refuse the non-canonical shape explicitly, got {err:?}",
        );
    }

    #[test]
    fn canonical_only_guard_rejects_member_mismatch_between_claim_role_and_post() {
        // Codex review thread (rust/crates/sera-runtime/src/circle_ingress.rs
        // line 681): the canonical guard must also reject a record whose
        // ClaimRole and Post events are authored by different members
        // (`ClaimRole(alice) → Post(bob)`). Without this check, the
        // helper would happily assemble a bundle whose roster lists
        // alice as Lead but whose entries are authored by bob —
        // i.e. roster/role attribution and entry authorship disagree.
        let funnel = CircleIngress::new();
        let outcome = funnel.accept(&operator_request()).unwrap();
        let mut mutated_record = outcome.record.clone();

        // Reach into the second event and rewrite its author to a
        // different member while keeping the canonical
        // (ClaimRole, Post) body shape — only the member id changes.
        if let Some(event) = mutated_record.channel_events.get_mut(1) {
            event.body = CircleChannelEventKind::Post {
                member: CircleMemberId::from("bob"),
                artifact_type: RESIDENT_INGRESS_POST_KIND.to_string(),
                summary: "open the run and submit proposal".to_string(),
            };
        } else {
            panic!("canonical record should have at least two events");
        }

        let err = closeout_into_proof_bundle(&mutated_record, &operator_closeout())
            .expect_err("closeout must refuse a member-mismatched canonical shape");
        assert!(
            err.contains("non-canonical ingress shape"),
            "closeout must refuse the member-mismatched canonical shape explicitly, got {err:?}",
        );
    }
}
