//! Deterministic validation helpers for [`CollaborationProofBundle`].
//!
//! All checks are pure in-memory computations — no provider calls, no I/O,
//! no credentials required. Safe to run in CI.

use std::collections::{HashMap, HashSet};

use crate::circle::{CollaborationProofBundle, ProofBundleEntry};

// =========================================================================
// Error types
// =========================================================================

/// Category of a proof-bundle validation failure.
#[derive(Debug, Clone, PartialEq)]
pub enum ValidationErrorKind {
    /// The bundle has no entries at all.
    EmptyEntries,
    /// Two or more entries share the same `entry_id`.
    DuplicateEntryId { entry_id: u64 },
    /// A lineage edge references an `entry_id` not present in `entries`.
    BrokenLineageRef {
        from_entry_id: u64,
        to_entry_id: u64,
    },
    /// An entry has a `payload.response` field that is blank (empty or whitespace-only).
    BlankPayloadResponse { entry_id: u64 },
    /// A provider-attributed entry has no corresponding execution receipt from its author.
    MissingReceiptForEntry { entry_id: u64, author: String },
    /// A provider-attributed entry has a receipt, but the receipt does not prove
    /// the same provider class (local vs cloud/non-local), or has no provider label.
    ReceiptProviderEvidenceMismatch { entry_id: u64, author: String },
    /// The bundle has no verdict.
    MissingVerdict,
    /// The bundle lacks evidence of at least one local provider and one cloud/non-local provider.
    MissingMixedProviderEvidence,
    /// A peer challenge references an entry id not present in `entries`.
    BrokenPeerChallengeRef {
        challenge_id: String,
        target_entry_id: u64,
    },
    /// Two or more peer challenges share the same stable challenge id.
    DuplicatePeerChallengeId { challenge_id: String },
    /// A peer challenge actor is not present in the run roster/authors.
    UnknownPeerChallengeActor {
        challenge_id: String,
        field: String,
        actor: String,
    },
    /// A peer challenge is missing the concrete disputed claim, challenge, or challenger.
    BlankPeerChallengeField { challenge_id: String, field: String },
}

/// A single validation failure produced by [`validate_proof_bundle`].
#[derive(Debug, Clone, PartialEq)]
pub struct ValidationError {
    pub kind: ValidationErrorKind,
}

impl ValidationError {
    fn new(kind: ValidationErrorKind) -> Self {
        Self { kind }
    }
}

// =========================================================================
// Public API
// =========================================================================

/// Validate a [`CollaborationProofBundle`] deterministically.
///
/// All checks run to completion — errors are accumulated, not short-circuited
/// (except when the entry list is empty, which makes further checks meaningless).
///
/// Returns `Ok(())` when every check passes, `Err(errors)` otherwise.
pub fn validate_proof_bundle(
    bundle: &CollaborationProofBundle,
) -> Result<(), Vec<ValidationError>> {
    let mut errors: Vec<ValidationError> = Vec::new();

    if bundle.entries.is_empty() {
        errors.push(ValidationError::new(ValidationErrorKind::EmptyEntries));
        return Err(errors);
    }

    check_unique_entry_ids(bundle, &mut errors);

    let entry_id_set: HashSet<u64> = bundle.entries.iter().map(|e| e.entry_id).collect();
    let actor_set: HashSet<&str> = bundle
        .roster
        .iter()
        .map(|member| member.participant_id.as_str())
        .chain(bundle.entries.iter().map(|entry| entry.author.as_str()))
        .collect();

    check_lineage_refs(bundle, &entry_id_set, &mut errors);
    check_blank_responses(bundle, &mut errors);
    check_receipts_for_provider_entries(bundle, &mut errors);
    check_peer_challenges(bundle, &entry_id_set, &actor_set, &mut errors);
    check_verdict_present(bundle, &mut errors);
    check_mixed_provider_evidence(bundle, &mut errors);

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

// =========================================================================
// Check helpers
// =========================================================================

fn check_unique_entry_ids(bundle: &CollaborationProofBundle, errors: &mut Vec<ValidationError>) {
    let mut seen: HashSet<u64> = HashSet::new();
    for entry in &bundle.entries {
        if !seen.insert(entry.entry_id) {
            errors.push(ValidationError::new(
                ValidationErrorKind::DuplicateEntryId {
                    entry_id: entry.entry_id,
                },
            ));
        }
    }
}

fn check_lineage_refs(
    bundle: &CollaborationProofBundle,
    entry_id_set: &HashSet<u64>,
    errors: &mut Vec<ValidationError>,
) {
    for edge in &bundle.lineage {
        if !entry_id_set.contains(&edge.from_entry_id) || !entry_id_set.contains(&edge.to_entry_id)
        {
            errors.push(ValidationError::new(
                ValidationErrorKind::BrokenLineageRef {
                    from_entry_id: edge.from_entry_id,
                    to_entry_id: edge.to_entry_id,
                },
            ));
        }
    }
}

fn check_blank_responses(bundle: &CollaborationProofBundle, errors: &mut Vec<ValidationError>) {
    for entry in &bundle.entries {
        if is_blank_response(entry) {
            errors.push(ValidationError::new(
                ValidationErrorKind::BlankPayloadResponse {
                    entry_id: entry.entry_id,
                },
            ));
        }
    }
}

fn check_receipts_for_provider_entries(
    bundle: &CollaborationProofBundle,
    errors: &mut Vec<ValidationError>,
) {
    let receipts_by_executor: HashMap<&str, Option<&str>> = bundle
        .execution_receipts
        .iter()
        .map(|r| {
            (
                r.executor.as_str(),
                r.parameters.get("provider").and_then(|v| v.as_str()),
            )
        })
        .collect();
    for entry in &bundle.entries {
        if let Some(entry_provider) = entry.payload.get("provider").and_then(|v| v.as_str()) {
            match receipts_by_executor.get(entry.author.as_str()) {
                None => errors.push(ValidationError::new(
                    ValidationErrorKind::MissingReceiptForEntry {
                        entry_id: entry.entry_id,
                        author: entry.author.clone(),
                    },
                )),
                Some(Some(receipt_provider))
                    if provider_classes_match(entry_provider, receipt_provider) => {}
                Some(_) => errors.push(ValidationError::new(
                    ValidationErrorKind::ReceiptProviderEvidenceMismatch {
                        entry_id: entry.entry_id,
                        author: entry.author.clone(),
                    },
                )),
            }
        }
    }
}

fn check_peer_challenges(
    bundle: &CollaborationProofBundle,
    entry_id_set: &HashSet<u64>,
    actor_set: &HashSet<&str>,
    errors: &mut Vec<ValidationError>,
) {
    let mut seen_challenge_ids: HashSet<&str> = HashSet::new();
    for challenge in &bundle.peer_challenges {
        if !seen_challenge_ids.insert(challenge.challenge_id.as_str()) {
            errors.push(ValidationError::new(
                ValidationErrorKind::DuplicatePeerChallengeId {
                    challenge_id: challenge.challenge_id.clone(),
                },
            ));
        }
        if !entry_id_set.contains(&challenge.target_entry_id) {
            errors.push(ValidationError::new(
                ValidationErrorKind::BrokenPeerChallengeRef {
                    challenge_id: challenge.challenge_id.clone(),
                    target_entry_id: challenge.target_entry_id,
                },
            ));
        }
        if !challenge.challenger.trim().is_empty()
            && !actor_set.contains(challenge.challenger.as_str())
        {
            errors.push(ValidationError::new(
                ValidationErrorKind::UnknownPeerChallengeActor {
                    challenge_id: challenge.challenge_id.clone(),
                    field: "challenger".to_string(),
                    actor: challenge.challenger.clone(),
                },
            ));
        }
        if let Some(response_by) = challenge
            .response_by
            .as_deref()
            .filter(|response_by| !response_by.trim().is_empty())
            && !actor_set.contains(response_by)
        {
            errors.push(ValidationError::new(
                ValidationErrorKind::UnknownPeerChallengeActor {
                    challenge_id: challenge.challenge_id.clone(),
                    field: "response_by".to_string(),
                    actor: response_by.to_string(),
                },
            ));
        }
        for (field, value) in [
            ("challenge_id", challenge.challenge_id.as_str()),
            ("challenger", challenge.challenger.as_str()),
            ("claim", challenge.claim.as_str()),
            ("challenge", challenge.challenge.as_str()),
        ] {
            if value.trim().is_empty() {
                errors.push(ValidationError::new(
                    ValidationErrorKind::BlankPeerChallengeField {
                        challenge_id: challenge.challenge_id.clone(),
                        field: field.to_string(),
                    },
                ));
            }
        }
    }
}

fn check_verdict_present(bundle: &CollaborationProofBundle, errors: &mut Vec<ValidationError>) {
    if bundle.verdict.is_none() {
        errors.push(ValidationError::new(ValidationErrorKind::MissingVerdict));
    }
}

fn check_mixed_provider_evidence(
    bundle: &CollaborationProofBundle,
    errors: &mut Vec<ValidationError>,
) {
    let mut has_local = false;
    let mut has_cloud = false;

    // Provider evidence must be anchored in execution receipts. Entry payload
    // labels are useful blackboard metadata, but they are self-reported and can
    // be fabricated or go stale; receipts are the auditable call evidence.
    for receipt in &bundle.execution_receipts {
        if let Some(p) = receipt.parameters.get("provider").and_then(|v| v.as_str()) {
            if is_local_provider(p) {
                has_local = true;
            } else {
                has_cloud = true;
            }
        }
    }

    if !(has_local && has_cloud) {
        errors.push(ValidationError::new(
            ValidationErrorKind::MissingMixedProviderEvidence,
        ));
    }
}

// =========================================================================
// Provider classification
// =========================================================================

/// Returns `true` if `payload.response` is present and blank.
fn is_blank_response(entry: &ProofBundleEntry) -> bool {
    let provider_attributed = entry
        .payload
        .get("provider")
        .and_then(|v| v.as_str())
        .is_some();
    match entry.payload.get("response").and_then(|v| v.as_str()) {
        Some(s) => s.trim().is_empty(),
        None => provider_attributed,
    }
}

/// A provider label is "local" if it names a host-local runtime.
fn is_local_provider(provider: &str) -> bool {
    let lc = provider.to_ascii_lowercase();
    lc.contains("local") || lc.contains("ollama") || lc.contains("lmstudio")
}

fn provider_classes_match(entry_provider: &str, receipt_provider: &str) -> bool {
    is_local_provider(entry_provider) == is_local_provider(receipt_provider)
}

// =========================================================================
// Tests
// =========================================================================

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;
    use crate::circle::{
        BudgetSnapshot, CollaborationProofBundle, CollaborationVerdictRecord, ExecutionReceipt,
        LineageEdge, LineageRelation, MetricRef, PeerChallenge, PeerChallengeDisposition,
        PeerChallengeSeverity, ProofBundleEntry, ProofBundleMember, ReceiptOutcome, VerdictType,
    };

    // -----------------------------------------------------------------
    // Fixture helpers
    // -----------------------------------------------------------------

    /// Sanitized fixture based on the corrected 2026-06-29 mixed-provider run.
    /// Providers: MiniMax (cloud) + local-lmstudio (local).
    fn mixed_provider_fixture() -> CollaborationProofBundle {
        let ts = |s: &str| s.parse::<chrono::DateTime<Utc>>().unwrap();

        CollaborationProofBundle {
            run_id: "run_sera_nqh3_mixed_providers_20260629_001".into(),
            circle_id: "sera-nqh3-mixed-provider-circle".into(),
            objective: "Test whether a SERA Circle can combine heterogeneous providers \
                        — local Gemma and MiniMax — under one common-goal envelope."
                .into(),
            success_metric: MetricRef::Inline {
                description: "Proof bundle exists with at least one local and one MiniMax model \
                              call, receipts, lineage, and a referee verdict."
                    .into(),
            },
            roster: vec![
                ProofBundleMember {
                    participant_id: "specifier_minimax".into(),
                    role: "Specifier / MiniMax cloud synthesis".into(),
                },
                ProofBundleMember {
                    participant_id: "builder_local_gemma".into(),
                    role: "Builder / local Gemma implementation perspective".into(),
                },
                ProofBundleMember {
                    participant_id: "critic_minimax".into(),
                    role: "Critic / MiniMax adversarial review".into(),
                },
                ProofBundleMember {
                    participant_id: "referee_local_gemma".into(),
                    role: "Referee / local Gemma operator-visible integration".into(),
                },
            ],
            budget_snapshot: Some(BudgetSnapshot {
                total_token_limit: None,
                max_iterations: Some(4),
                current_usage: 4,
            }),
            entries: vec![
                ProofBundleEntry {
                    entry_id: 1,
                    author: "specifier_minimax".into(),
                    timestamp: ts("2026-06-29T15:04:10Z"),
                    artifact_type: "specification".into(),
                    payload: serde_json::json!({
                        "role": "Specifier / cloud synthesis",
                        "provider": "minimax",
                        "model": "MiniMax-M3",
                        "response": "MiniMax cloud specifier: 8-bullet mixed-provider Circle spec. \
                                     (truncated for fixture)"
                    }),
                },
                ProofBundleEntry {
                    entry_id: 2,
                    author: "builder_local_gemma".into(),
                    timestamp: ts("2026-06-29T15:05:10Z"),
                    artifact_type: "proposal".into(),
                    payload: serde_json::json!({
                        "role": "Builder / local implementation",
                        "provider": "local-lmstudio",
                        "model": "gemma4-26b-a4b-qat-uncensored",
                        "response": "GA scaffold proposal: 4 files, stdlib only. \
                                     (truncated for fixture)"
                    }),
                },
                ProofBundleEntry {
                    entry_id: 3,
                    author: "critic_minimax".into(),
                    timestamp: ts("2026-06-29T15:06:28Z"),
                    artifact_type: "critique".into(),
                    payload: serde_json::json!({
                        "role": "Critic / cloud adversary",
                        "provider": "minimax",
                        "model": "MiniMax-M3",
                        "response": "CRITIQUE — B1: MB emitter missing. B2: Referee slot \
                                     collapsed onto operator. (truncated for fixture)"
                    }),
                },
                ProofBundleEntry {
                    entry_id: 4,
                    author: "referee_local_gemma".into(),
                    timestamp: ts("2026-06-29T15:07:41Z"),
                    artifact_type: "verdict".into(),
                    payload: serde_json::json!({
                        "role": "Referee / local integrator",
                        "provider": "local-lmstudio",
                        "model": "gemma4-26b-a4b-qat-uncensored",
                        "response": "VERDICT: revision_required. Lineage: 2 of 4 slots \
                                     substantively present. (truncated for fixture)"
                    }),
                },
            ],
            lineage: vec![
                LineageEdge {
                    from_entry_id: 1,
                    to_entry_id: 2,
                    relation: LineageRelation::DerivesFrom,
                },
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
                LineageEdge {
                    from_entry_id: 2,
                    to_entry_id: 4,
                    relation: LineageRelation::DerivesFrom,
                },
            ],
            execution_receipts: vec![
                ExecutionReceipt {
                    receipt_id: "rcpt_001".into(),
                    executor: "specifier_minimax".into(),
                    timestamp: ts("2026-06-29T15:04:10Z"),
                    action: "hermes_chat".into(),
                    parameters: serde_json::json!({
                        "provider": "minimax",
                        "model": "MiniMax-M3",
                        "duration_ms": 37261,
                        "returncode": 0
                    }),
                    outcome: ReceiptOutcome::Success,
                    cost_tokens: None,
                },
                ExecutionReceipt {
                    receipt_id: "rcpt_002".into(),
                    executor: "builder_local_gemma".into(),
                    timestamp: ts("2026-06-29T15:05:10Z"),
                    action: "hermes_chat".into(),
                    parameters: serde_json::json!({
                        "provider": "custom/local-lmstudio",
                        "model": "gemma4-26b-a4b-qat-uncensored",
                        "duration_ms": 59534,
                        "returncode": 0
                    }),
                    outcome: ReceiptOutcome::Success,
                    cost_tokens: None,
                },
                ExecutionReceipt {
                    receipt_id: "rcpt_003".into(),
                    executor: "critic_minimax".into(),
                    timestamp: ts("2026-06-29T15:06:28Z"),
                    action: "hermes_chat".into(),
                    parameters: serde_json::json!({
                        "provider": "minimax",
                        "model": "MiniMax-M3",
                        "duration_ms": 78214,
                        "returncode": 0
                    }),
                    outcome: ReceiptOutcome::Success,
                    cost_tokens: None,
                },
                ExecutionReceipt {
                    receipt_id: "rcpt_004".into(),
                    executor: "referee_local_gemma".into(),
                    timestamp: ts("2026-06-29T15:07:41Z"),
                    action: "hermes_chat".into(),
                    parameters: serde_json::json!({
                        "provider": "custom/local-lmstudio",
                        "model": "gemma4-26b-a4b-qat-uncensored",
                        "duration_ms": 73439,
                        "returncode": 0
                    }),
                    outcome: ReceiptOutcome::Success,
                    cost_tokens: None,
                },
            ],
            peer_challenges: vec![],
            verdict: Some(CollaborationVerdictRecord {
                reviewer: "referee_local_gemma".into(),
                timestamp: ts("2026-06-29T15:07:41Z"),
                verdict_type: VerdictType::RevisionRequired {
                    feedback: "Scaffolded mixed-provider Circle run succeeded; \
                               first-class SERA runner/provider-roster support \
                               still needs a bounded implementation lane."
                        .into(),
                },
                rationale: "B1: MB emitter missing. B2: Referee slot not independent. \
                            Verdict: revision_required."
                    .into(),
            }),
        }
    }

    // -----------------------------------------------------------------
    // Positive test
    // -----------------------------------------------------------------

    #[test]
    fn valid_mixed_provider_bundle_passes() {
        let bundle = mixed_provider_fixture();
        assert!(
            validate_proof_bundle(&bundle).is_ok(),
            "expected Ok for the corrected mixed-provider fixture"
        );
    }

    // -----------------------------------------------------------------
    // Negative: empty entries
    // -----------------------------------------------------------------

    #[test]
    fn empty_entries_fails_with_empty_entries_error() {
        let mut bundle = mixed_provider_fixture();
        bundle.entries.clear();
        bundle.lineage.clear();
        bundle.execution_receipts.clear();
        let errors = validate_proof_bundle(&bundle).unwrap_err();
        assert!(
            errors
                .iter()
                .any(|e| matches!(e.kind, ValidationErrorKind::EmptyEntries)),
            "expected EmptyEntries error"
        );
    }

    // -----------------------------------------------------------------
    // Negative: blank downstream entries / empty blackboard content
    // -----------------------------------------------------------------

    #[test]
    fn blank_payload_response_fails() {
        let mut bundle = mixed_provider_fixture();
        // Builder entry gets a blank response — simulates failed propagation
        bundle.entries[1].payload = serde_json::json!({
            "role": "Builder / local implementation",
            "provider": "local-lmstudio",
            "model": "gemma4-26b-a4b-qat-uncensored",
            "response": "   "
        });
        let errors = validate_proof_bundle(&bundle).unwrap_err();
        assert!(
            errors.iter().any(|e| matches!(
                e.kind,
                ValidationErrorKind::BlankPayloadResponse { entry_id: 2 }
            )),
            "expected BlankPayloadResponse for entry_id=2, got: {errors:?}"
        );
    }

    #[test]
    fn empty_string_response_fails() {
        let mut bundle = mixed_provider_fixture();
        bundle.entries[2].payload = serde_json::json!({
            "provider": "minimax",
            "response": ""
        });
        let errors = validate_proof_bundle(&bundle).unwrap_err();
        assert!(
            errors.iter().any(|e| matches!(
                e.kind,
                ValidationErrorKind::BlankPayloadResponse { entry_id: 3 }
            )),
            "expected BlankPayloadResponse for entry_id=3"
        );
    }

    #[test]
    fn missing_response_field_for_provider_entry_fails() {
        let mut bundle = mixed_provider_fixture();
        bundle.entries[1].payload = serde_json::json!({
            "role": "Builder / local implementation",
            "provider": "local-lmstudio",
            "model": "gemma4-26b-a4b-qat-uncensored"
        });
        let errors = validate_proof_bundle(&bundle).unwrap_err();
        assert!(
            errors.iter().any(|e| matches!(
                e.kind,
                ValidationErrorKind::BlankPayloadResponse { entry_id: 2 }
            )),
            "expected BlankPayloadResponse when provider entry omits response"
        );
    }

    #[test]
    fn null_response_for_provider_entry_fails() {
        let mut bundle = mixed_provider_fixture();
        bundle.entries[2].payload = serde_json::json!({
            "provider": "minimax",
            "response": null
        });
        let errors = validate_proof_bundle(&bundle).unwrap_err();
        assert!(
            errors.iter().any(|e| matches!(
                e.kind,
                ValidationErrorKind::BlankPayloadResponse { entry_id: 3 }
            )),
            "expected BlankPayloadResponse when provider entry has null response"
        );
    }

    // -----------------------------------------------------------------
    // Negative: missing mixed-provider evidence
    // -----------------------------------------------------------------

    #[test]
    fn only_local_providers_fails_mixed_provider_check() {
        let mut bundle = mixed_provider_fixture();
        // Replace cloud provider entries with local-only
        for entry in &mut bundle.entries {
            if let Some(obj) = entry.payload.as_object_mut() {
                obj.insert(
                    "provider".into(),
                    serde_json::Value::String("local-lmstudio".into()),
                );
            }
        }
        for receipt in &mut bundle.execution_receipts {
            if let Some(obj) = receipt.parameters.as_object_mut() {
                obj.insert(
                    "provider".into(),
                    serde_json::Value::String("custom/local-lmstudio".into()),
                );
            }
        }
        let errors = validate_proof_bundle(&bundle).unwrap_err();
        assert!(
            errors
                .iter()
                .any(|e| matches!(e.kind, ValidationErrorKind::MissingMixedProviderEvidence)),
            "expected MissingMixedProviderEvidence when all providers are local"
        );
    }

    #[test]
    fn only_cloud_providers_fails_mixed_provider_check() {
        let mut bundle = mixed_provider_fixture();
        for entry in &mut bundle.entries {
            if let Some(obj) = entry.payload.as_object_mut() {
                obj.insert(
                    "provider".into(),
                    serde_json::Value::String("minimax".into()),
                );
            }
        }
        for receipt in &mut bundle.execution_receipts {
            if let Some(obj) = receipt.parameters.as_object_mut() {
                obj.insert(
                    "provider".into(),
                    serde_json::Value::String("minimax".into()),
                );
            }
        }
        let errors = validate_proof_bundle(&bundle).unwrap_err();
        assert!(
            errors
                .iter()
                .any(|e| matches!(e.kind, ValidationErrorKind::MissingMixedProviderEvidence)),
            "expected MissingMixedProviderEvidence when all providers are cloud"
        );
    }

    #[test]
    fn self_reported_entry_provider_labels_do_not_satisfy_mixed_provider_check() {
        let mut bundle = mixed_provider_fixture();
        // Keep entry payload labels mixed, but make every receipt local. The
        // validator must trust receipts, not self-reported blackboard labels.
        for receipt in &mut bundle.execution_receipts {
            if let Some(obj) = receipt.parameters.as_object_mut() {
                obj.insert(
                    "provider".into(),
                    serde_json::Value::String("custom/local-lmstudio".into()),
                );
            }
        }
        let errors = validate_proof_bundle(&bundle).unwrap_err();
        assert!(
            errors
                .iter()
                .any(|e| matches!(e.kind, ValidationErrorKind::MissingMixedProviderEvidence)),
            "expected MissingMixedProviderEvidence when only self-reported entries are mixed"
        );
        assert!(
            errors.iter().any(|e| matches!(
                e.kind,
                ValidationErrorKind::ReceiptProviderEvidenceMismatch { entry_id: 1, .. }
            )),
            "expected receipt/provider mismatch for MiniMax entry backed by local receipt"
        );
    }

    #[test]
    fn receipt_without_provider_fails_provider_evidence_check() {
        let mut bundle = mixed_provider_fixture();
        if let Some(obj) = bundle.execution_receipts[0].parameters.as_object_mut() {
            obj.remove("provider");
        }
        let errors = validate_proof_bundle(&bundle).unwrap_err();
        assert!(
            errors.iter().any(|e| matches!(
                e.kind,
                ValidationErrorKind::ReceiptProviderEvidenceMismatch { entry_id: 1, .. }
            )),
            "expected receipt/provider evidence mismatch when receipt omits provider"
        );
    }

    #[test]
    fn peer_challenge_to_nonexistent_entry_fails() {
        let mut bundle = mixed_provider_fixture();
        bundle.peer_challenges.push(PeerChallenge {
            challenge_id: "challenge_missing_target".into(),
            challenger: "critic_minimax".into(),
            target_entry_id: 999,
            claim: "Builder output is fully grounded.".into(),
            challenge: "Target entry is not present in the bundle.".into(),
            evidence: vec!["test fixture".into()],
            severity: PeerChallengeSeverity::Blocking,
            response_by: None,
            disposition: PeerChallengeDisposition::Open,
        });
        let errors = validate_proof_bundle(&bundle).unwrap_err();
        assert!(
            errors.iter().any(|e| matches!(
                e.kind,
                ValidationErrorKind::BrokenPeerChallengeRef {
                    target_entry_id: 999,
                    ..
                }
            )),
            "expected BrokenPeerChallengeRef for target_entry_id=999"
        );
    }

    #[test]
    fn duplicate_peer_challenge_id_fails() {
        let mut bundle = mixed_provider_fixture();
        bundle.peer_challenges.push(PeerChallenge {
            challenge_id: "challenge_duplicate".into(),
            challenger: "critic_minimax".into(),
            target_entry_id: 2,
            claim: "Builder output is fully grounded.".into(),
            challenge: "First challenge with this id.".into(),
            evidence: vec!["test fixture".into()],
            severity: PeerChallengeSeverity::High,
            response_by: Some("referee_local_gemma".into()),
            disposition: PeerChallengeDisposition::Open,
        });
        bundle.peer_challenges.push(PeerChallenge {
            challenge_id: "challenge_duplicate".into(),
            challenger: "specifier_minimax".into(),
            target_entry_id: 3,
            claim: "Critic output is fully grounded.".into(),
            challenge: "Second challenge with conflicting content but same id.".into(),
            evidence: vec!["test fixture".into()],
            severity: PeerChallengeSeverity::Medium,
            response_by: Some("referee_local_gemma".into()),
            disposition: PeerChallengeDisposition::Resolved,
        });
        let errors = validate_proof_bundle(&bundle).unwrap_err();
        assert!(
            errors.iter().any(|e| matches!(
                e.kind,
                ValidationErrorKind::DuplicatePeerChallengeId { ref challenge_id }
                    if challenge_id == "challenge_duplicate"
            )),
            "expected DuplicatePeerChallengeId for challenge_duplicate"
        );
    }

    #[test]
    fn peer_challenge_unknown_actor_fails() {
        let mut bundle = mixed_provider_fixture();
        bundle.peer_challenges.push(PeerChallenge {
            challenge_id: "challenge_unknown_actor".into(),
            challenger: "invented_critic".into(),
            target_entry_id: 2,
            claim: "Builder output is fully grounded.".into(),
            challenge: "Challenger must be a real run participant.".into(),
            evidence: vec!["test fixture".into()],
            severity: PeerChallengeSeverity::High,
            response_by: Some("invented_referee".into()),
            disposition: PeerChallengeDisposition::Open,
        });
        let errors = validate_proof_bundle(&bundle).unwrap_err();
        assert!(
            errors.iter().any(|e| matches!(
                e.kind,
                ValidationErrorKind::UnknownPeerChallengeActor { ref field, ref actor, .. }
                    if field == "challenger" && actor == "invented_critic"
            )),
            "expected UnknownPeerChallengeActor for challenger"
        );
        assert!(
            errors.iter().any(|e| matches!(
                e.kind,
                ValidationErrorKind::UnknownPeerChallengeActor { ref field, ref actor, .. }
                    if field == "response_by" && actor == "invented_referee"
            )),
            "expected UnknownPeerChallengeActor for response_by"
        );
    }

    #[test]
    fn blank_peer_challenge_claim_fails() {
        let mut bundle = mixed_provider_fixture();
        bundle.peer_challenges.push(PeerChallenge {
            challenge_id: "challenge_blank_claim".into(),
            challenger: "critic_minimax".into(),
            target_entry_id: 2,
            claim: " ".into(),
            challenge: "The disputed claim must be explicit.".into(),
            evidence: vec![],
            severity: PeerChallengeSeverity::High,
            response_by: Some("referee_local_gemma".into()),
            disposition: PeerChallengeDisposition::Open,
        });
        let errors = validate_proof_bundle(&bundle).unwrap_err();
        assert!(
            errors.iter().any(|e| matches!(
                e.kind,
                ValidationErrorKind::BlankPeerChallengeField { ref field, .. }
                    if field == "claim"
            )),
            "expected BlankPeerChallengeField for claim"
        );
    }

    // -----------------------------------------------------------------
    // Negative: broken lineage reference
    // -----------------------------------------------------------------

    #[test]
    fn lineage_edge_to_nonexistent_entry_fails() {
        let mut bundle = mixed_provider_fixture();
        bundle.lineage.push(LineageEdge {
            from_entry_id: 1,
            to_entry_id: 999, // does not exist
            relation: LineageRelation::DerivesFrom,
        });
        let errors = validate_proof_bundle(&bundle).unwrap_err();
        assert!(
            errors.iter().any(|e| matches!(
                e.kind,
                ValidationErrorKind::BrokenLineageRef {
                    from_entry_id: 1,
                    to_entry_id: 999
                }
            )),
            "expected BrokenLineageRef for to_entry_id=999"
        );
    }

    #[test]
    fn lineage_edge_from_nonexistent_entry_fails() {
        let mut bundle = mixed_provider_fixture();
        bundle.lineage.push(LineageEdge {
            from_entry_id: 888, // does not exist
            to_entry_id: 2,
            relation: LineageRelation::Criticizes,
        });
        let errors = validate_proof_bundle(&bundle).unwrap_err();
        assert!(
            errors.iter().any(|e| matches!(
                e.kind,
                ValidationErrorKind::BrokenLineageRef {
                    from_entry_id: 888,
                    to_entry_id: 2
                }
            )),
            "expected BrokenLineageRef for from_entry_id=888"
        );
    }

    // -----------------------------------------------------------------
    // Negative: missing verdict
    // -----------------------------------------------------------------

    #[test]
    fn missing_verdict_fails() {
        let mut bundle = mixed_provider_fixture();
        bundle.verdict = None;
        let errors = validate_proof_bundle(&bundle).unwrap_err();
        assert!(
            errors
                .iter()
                .any(|e| matches!(e.kind, ValidationErrorKind::MissingVerdict)),
            "expected MissingVerdict error"
        );
    }

    // -----------------------------------------------------------------
    // Negative: duplicate entry IDs
    // -----------------------------------------------------------------

    #[test]
    fn duplicate_entry_id_fails() {
        let mut bundle = mixed_provider_fixture();
        // Force entry 2 to have the same ID as entry 1
        bundle.entries[1].entry_id = 1;
        let errors = validate_proof_bundle(&bundle).unwrap_err();
        assert!(
            errors.iter().any(|e| matches!(
                e.kind,
                ValidationErrorKind::DuplicateEntryId { entry_id: 1 }
            )),
            "expected DuplicateEntryId for entry_id=1"
        );
    }

    // -----------------------------------------------------------------
    // Negative: missing receipt for provider-attributed entry
    // -----------------------------------------------------------------

    #[test]
    fn missing_receipt_for_provider_entry_fails() {
        let mut bundle = mixed_provider_fixture();
        // Remove the receipt for the critic (entry 3 / executor critic_minimax)
        bundle
            .execution_receipts
            .retain(|r| r.executor != "critic_minimax");
        let errors = validate_proof_bundle(&bundle).unwrap_err();
        assert!(
            errors.iter().any(|e| matches!(
                e.kind,
                ValidationErrorKind::MissingReceiptForEntry { entry_id: 3, .. }
            )),
            "expected MissingReceiptForEntry for entry_id=3"
        );
    }

    // -----------------------------------------------------------------
    // is_local_provider classification
    // -----------------------------------------------------------------

    #[test]
    fn local_provider_classification() {
        assert!(is_local_provider("local-lmstudio"));
        assert!(is_local_provider("custom/local-lmstudio"));
        assert!(is_local_provider("ollama"));
        assert!(is_local_provider("LOCAL-OLLAMA"));
        assert!(!is_local_provider("minimax"));
        assert!(!is_local_provider("openai"));
        assert!(!is_local_provider("anthropic"));
    }
}
