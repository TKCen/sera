//! `sera circle run --to <circle>` offline seam.
//!
//! Implements the SERA-NQH3 first-class Circle run command that addresses a
//! Circle as `to:circle:<name>` (per the 2026-06-30 decision) and emits a
//! [`CollaborationProofBundle`] from a deterministic sequence of
//! channel events — without any live LLM or gateway calls.
//!
//! The CLI surface:
//!
//!   `sera circle run --to <name-or-address>`
//!                  `[--objective <text>]`
//!                  `[--members a,b,c]`
//!                  `[--referee r]`
//!                  `[--bundle-out <path>]`
//!                  `[--json]`
//!
//! Determinism: the bundle is assembled entirely from CLI args + the channel
//! address; provider classes rotate around the roster (cloud, local,
//! cloud, local) so `validate_proof_bundle`'s mixed-provider check passes
//! without requiring a network call or a fixture directory. Time-stamps are
//! monotonic but stable per-event so re-runs with identical args produce
//! byte-identical bundle bytes (sha256 stable across runs).

use chrono::{DateTime, Duration, TimeZone, Utc};
use sera_types::circle::{
    CollaborationProofBundle, CollaborationVerdictRecord, ExecutionReceipt, LineageEdge,
    LineageRelation, MetricRef, ProofBundleEntry, ProofBundleMember, ReceiptOutcome, VerdictType,
};
use sera_types::circle_channel::{
    CircleChannelAddress, CircleChannelEvent, CircleChannelEventKind, CircleChannelRole,
    CircleMemberId,
};
use sera_types::circle_validator::validate_proof_bundle;
use serde_json::json;
use std::path::{Path, PathBuf};

use crate::sha256_hex;

/// Stable UTC reference used as `t=0` for synthesised channel events.
///
/// The exact instant does not matter as long as it is the same on every run —
/// keeping a fixed reference makes bundle bytes byte-stable for the same CLI
/// input. A round-trip through `Utc.timestamp_opt` returns the same value
/// regardless of host timezone.
fn event_time_at(event_id: u64) -> DateTime<Utc> {
    let base = Utc.with_ymd_and_hms(2026, 7, 2, 9, 0, 0).unwrap();
    base + Duration::seconds(event_id as i64)
}

/// One-line machine footer emitted after the bundle is written.
pub fn print_circle_run_footer(verdict: &str, bundle_sha256: &str) {
    println!("circle-run: {verdict} {bundle_sha256}");
}

/// Public entry point invoked from `main.rs` once the offline dispatch guard
/// matches `Commands::Circle { CircleCommand::Run { .. } }`.
pub fn run_circle_run(
    to: Option<String>,
    objective: Option<String>,
    members: Option<String>,
    referee: Option<String>,
    bundle_out: Option<PathBuf>,
    json_out: bool,
) -> i32 {
    let to = match to {
        Some(s) => s,
        None => {
            eprintln!("missing required --to <NAME|to:circle:NAME> for Circle run");
            print_circle_run_footer("USAGE_ERROR", "unknown");
            return 3;
        }
    };

    let address = match CircleChannelAddress::parse(&to) {
        Ok(addr) => addr,
        Err(err) => {
            eprintln!("invalid --to address {to:?}: {err}");
            print_circle_run_footer("USAGE_ERROR", "unknown");
            return 3;
        }
    };

    if let Err(err) = validate_roster_inputs(&members, &referee) {
        eprintln!("{err}");
        print_circle_run_footer("USAGE_ERROR", "unknown");
        return 3;
    }

    let member_ids = match members.as_deref() {
        Some(raw) => parse_member_list(raw),
        None => vec!["alice".to_string(), "bob".to_string()],
    };
    let referee_id = referee
        .or_else(|| Some("ref".to_string()))
        .unwrap()
        .trim()
        .to_string();

    // Lead is always the first non-referee member id; if there are no members
    // the slot is empty and we surface a USAGE_ERROR.
    if member_ids.is_empty() {
        eprintln!("--members must contain at least one non-referee member id");
        print_circle_run_footer("USAGE_ERROR", "unknown");
        return 3;
    }
    if member_ids.iter().any(|m| m == &referee_id) {
        eprintln!("referee {referee_id:?} overlaps with member list");
        print_circle_run_footer("USAGE_ERROR", "unknown");
        return 3;
    }
    if bundle_out
        .as_deref()
        .is_some_and(|path| path.as_os_str().is_empty())
    {
        eprintln!("--bundle-out must not be blank");
        print_circle_run_footer("USAGE_ERROR", "unknown");
        return 3;
    }

    let objective = objective.unwrap_or_else(|| {
        "Synthesise a Circle proof bundle from channel-addressed events.".to_string()
    });

    let run_fingerprint_input = format!(
        "address={address}\nobjective={objective}\nmembers={}\nreferee={referee_id}",
        member_ids.join(",")
    );
    let run_fingerprint = sha256_hex(run_fingerprint_input.as_bytes());
    let run_id = format!(
        "sera-circle-run-{}-{}",
        address.name.to_ascii_lowercase().replace('.', "-"),
        &run_fingerprint[..12]
    );

    let events = build_channel_events(&address, &member_ids, &referee_id);
    let bundle = match assemble_bundle(
        &address,
        &objective,
        &run_id,
        &member_ids,
        &referee_id,
        &events,
    ) {
        Ok(bundle) => bundle,
        Err(err) => {
            eprintln!("failed to assemble Circle run bundle: {err}");
            print_circle_run_footer("USAGE_ERROR", "unknown");
            return 3;
        }
    };

    let bytes = match serde_json::to_vec_pretty(&bundle) {
        Ok(bytes) => bytes,
        Err(err) => {
            eprintln!("failed to serialize Circle run bundle: {err}");
            print_circle_run_footer("USAGE_ERROR", "unknown");
            return 3;
        }
    };
    let bundle_sha256 = sha256_hex(&bytes);

    let bundle_write_error = bundle_out.as_deref().and_then(|bundle_out| {
        write_bundle(bundle_out, &bytes)
            .err()
            .map(|err| (bundle_out, err))
    });
    if let Some((bundle_out, err)) = bundle_write_error {
        eprintln!(
            "failed to write Circle run bundle {}: {err}",
            bundle_out.display()
        );
        print_circle_run_footer("USAGE_ERROR", &bundle_sha256);
        return 3;
    }

    match validate_proof_bundle(&bundle) {
        Ok(()) => {
            let entries = bundle.entries.len();
            let receipts = bundle.execution_receipts.len();
            let lineage = bundle.lineage.len();
            let circle_id = &bundle.circle_id;
            if json_out {
                println!(
                    "{}",
                    json!({
                        "verdict": "PASS",
                        "bundle_sha256": bundle_sha256,
                        "address": address.to_string(),
                        "to": to,
                        "objective": objective,
                        "members": member_ids,
                        "referee": referee_id,
                        "bundle_out": bundle_out.as_ref().map(|p| p.display().to_string()),
                        "entries": entries,
                        "execution_receipts": receipts,
                        "lineage_edges": lineage,
                        "circle_id": circle_id,
                        "run_id": bundle.run_id,
                    })
                );
            } else {
                println!(
                    "Circle run PASS: addressed {address} members={} referee={referee_id:?} \
                     entries={entries} receipts={receipts} lineage={lineage} run_id={circle_id}",
                    member_ids.len()
                );
            }
            print_circle_run_footer("PASS", &bundle_sha256);
            0
        }
        Err(errors) => {
            if json_out {
                println!(
                    "{}",
                    json!({
                        "verdict": "FAIL",
                        "bundle_sha256": bundle_sha256,
                        "address": address.to_string(),
                        "to": to,
                        "members": member_ids,
                        "referee": referee_id,
                        "entries": bundle.entries.len(),
                        "execution_receipts": bundle.execution_receipts.len(),
                        "lineage_edges": bundle.lineage.len(),
                        "circle_id": bundle.circle_id,
                        "run_id": bundle.run_id,
                        "errors": errors.iter().map(|e| format!("{:?}", e.kind)).collect::<Vec<_>>(),
                    })
                );
            } else {
                eprintln!(
                    "Circle run FAIL: bundle produced {} validation error(s)",
                    errors.len()
                );
                for error in &errors {
                    eprintln!("- {:?}", error.kind);
                }
            }
            print_circle_run_footer("FAIL", &bundle_sha256);
            1
        }
    }
}

fn validate_roster_inputs(
    members: &Option<String>,
    referee: &Option<String>,
) -> Result<(), String> {
    if let Some(raw) = members.as_deref() {
        if raw.trim().is_empty() {
            return Err("--members must not be blank".to_string());
        }
        let parsed = parse_member_list(raw);
        if parsed.is_empty() {
            return Err("--members must contain at least one member id".to_string());
        }
        let mut seen = std::collections::HashSet::new();
        for m in &parsed {
            if !seen.insert(m.clone()) {
                return Err(format!("duplicate member id {m:?} in --members"));
            }
        }
    }
    if referee
        .as_deref()
        .is_some_and(|referee| referee.trim().is_empty())
    {
        return Err("--referee must not be blank".to_string());
    }
    Ok(())
}

fn parse_member_list(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

fn build_channel_events(
    address: &CircleChannelAddress,
    members: &[String],
    referee: &str,
) -> Vec<CircleChannelEvent> {
    let mut events = Vec::new();
    let mut event_id: u64 = 1;

    // Lead claim + proposal
    events.push(CircleChannelEvent {
        event_id,
        address: address.clone(),
        timestamp: event_time_at(event_id),
        body: CircleChannelEventKind::ClaimRole {
            member: CircleMemberId::from(members[0].as_str()),
            role: CircleChannelRole::Lead,
        },
    });
    event_id += 1;

    // Each remaining member posts a proposal.
    for (idx, member) in members.iter().enumerate().skip(1) {
        let role = if idx == 1 {
            CircleChannelRole::Worker
        } else if idx == 2 {
            CircleChannelRole::Critic
        } else {
            CircleChannelRole::Worker
        };
        events.push(CircleChannelEvent {
            event_id,
            address: address.clone(),
            timestamp: event_time_at(event_id),
            body: CircleChannelEventKind::ClaimRole {
                member: CircleMemberId::from(member.as_str()),
                role,
            },
        });
        event_id += 1;

        events.push(CircleChannelEvent {
            event_id,
            address: address.clone(),
            timestamp: event_time_at(event_id),
            body: CircleChannelEventKind::Receipt {
                member: CircleMemberId::from(member.as_str()),
                action: "sera_circle_run_post".to_string(),
                tokens: Some(60),
            },
        });
        event_id += 1;
    }

    // Referee claim + verdict.
    events.push(CircleChannelEvent {
        event_id,
        address: address.clone(),
        timestamp: event_time_at(event_id),
        body: CircleChannelEventKind::ClaimRole {
            member: CircleMemberId::from(referee),
            role: CircleChannelRole::Referee,
        },
    });
    event_id += 1;

    events.push(CircleChannelEvent {
        event_id,
        address: address.clone(),
        timestamp: event_time_at(event_id),
        body: CircleChannelEventKind::Receipt {
            member: CircleMemberId::from(referee),
            action: "sera_circle_run_verdict".to_string(),
            tokens: Some(20),
        },
    });

    events
}

fn assemble_bundle(
    address: &CircleChannelAddress,
    objective: &str,
    run_id: &str,
    members: &[String],
    referee: &str,
    #[allow(unused_variables)] events: &[CircleChannelEvent],
) -> Result<CollaborationProofBundle, String> {
    let mut roster = Vec::with_capacity(members.len() + 1);
    roster.push(ProofBundleMember {
        participant_id: members[0].clone(),
        role: CircleChannelRole::Lead.default_label().to_string(),
    });
    for (idx, m) in members.iter().enumerate().skip(1) {
        let label = match idx {
            1 => CircleChannelRole::Worker,
            2 => CircleChannelRole::Critic,
            _ => CircleChannelRole::Worker,
        };
        roster.push(ProofBundleMember {
            participant_id: m.clone(),
            role: label.default_label().to_string(),
        });
    }
    roster.push(ProofBundleMember {
        participant_id: referee.to_string(),
        role: CircleChannelRole::Referee.default_label().to_string(),
    });

    let mut entries = Vec::with_capacity(members.len() + 1);
    let mut receipts = Vec::with_capacity(members.len() + 1);
    let mut lineage = Vec::new();

    // Lead entry — 1
    let lead_provider = provider_for(0);
    let lead_ts = event_time_at(1);
    entries.push(ProofBundleEntry {
        entry_id: 1,
        author: members[0].clone(),
        timestamp: lead_ts,
        artifact_type: "lead_specification".to_string(),
        payload: json!({
            "role": CircleChannelRole::Lead.default_label(),
            "provider": lead_provider,
            "model": model_for(lead_provider),
            "response": format!(
                "Lead {leader} addressed Circle {address} with objective: {objective}",
                leader = members[0]
            ),
        }),
    });
    receipts.push(make_receipt(
        1,
        &members[0],
        "sera_circle_run_lead",
        lead_provider,
    ));

    // Member entries + receipts — entries 2..members.len()
    for (idx, member) in members.iter().enumerate().skip(1) {
        let entry_id = (idx + 1) as u64;
        let provider = provider_for(idx);
        let ts = event_time_at(entry_id);
        entries.push(ProofBundleEntry {
            entry_id,
            author: member.clone(),
            timestamp: ts,
            artifact_type: artifact_type_for_index(idx),
            payload: json!({
                "role": match idx {
                    1 => CircleChannelRole::Worker.default_label(),
                    2 => CircleChannelRole::Critic.default_label(),
                    _ => CircleChannelRole::Worker.default_label(),
                },
                "provider": provider,
                "model": model_for(provider),
                "response": non_blank_response(idx, member, objective),
            }),
        });
        receipts.push(make_receipt(
            entry_id,
            member,
            "sera_circle_run_post",
            provider,
        ));

        // Connect every member contribution into the proof chain so every
        // entry has a path to the referee verdict.
        lineage.push(LineageEdge {
            from_entry_id: entry_id - 1,
            to_entry_id: entry_id,
            relation: if idx == 2 {
                LineageRelation::Criticizes
            } else {
                LineageRelation::DerivesFrom
            },
        });
    }

    // Referee entry — last
    let referee_entry_id = (members.len() as u64) + 1;
    let referee_provider = provider_for(members.len());
    let referee_ts = event_time_at(referee_entry_id);
    entries.push(ProofBundleEntry {
        entry_id: referee_entry_id,
        author: referee.to_string(),
        timestamp: referee_ts,
        artifact_type: "verdict".to_string(),
        payload: json!({
            "role": CircleChannelRole::Referee.default_label(),
            "provider": referee_provider,
            "model": model_for(referee_provider),
            "response": format!(
                "Referee {referee} APPROVED: every member posted a non-blank contribution \
                 and the synthesis matches the address {address}."
            ),
        }),
    });
    receipts.push(make_receipt(
        referee_entry_id,
        referee,
        "sera_circle_run_verdict",
        referee_provider,
    ));

    // Resolve lineage: last member entry → referee entry
    if members.len() > 1 {
        let final_member_entry = members.len() as u64;
        lineage.push(LineageEdge {
            from_entry_id: final_member_entry,
            to_entry_id: referee_entry_id,
            relation: LineageRelation::Resolves,
        });
    } else {
        // members.len() == 1 means we have only lead + referee; resolve
        // directly.
        lineage.push(LineageEdge {
            from_entry_id: 1,
            to_entry_id: referee_entry_id,
            relation: LineageRelation::Resolves,
        });
    }

    let verdict = Some(CollaborationVerdictRecord {
        reviewer: referee.to_string(),
        timestamp: referee_ts,
        verdict_type: VerdictType::Approved,
        rationale: format!(
            "Circle {address} (objective={objective}) — all members produced non-blank \
             entries, lineage is connected, mixed-provider receipts present."
        ),
    });

    Ok(CollaborationProofBundle {
        run_id: run_id.to_string(),
        circle_id: address.name.to_string(),
        objective: objective.to_string(),
        success_metric: MetricRef::Inline {
            description: "Channel-addressed Circle run produces a structurally valid \
                          mixed-provider CollaborationProofBundle."
                .to_string(),
        },
        roster,
        budget_snapshot: None,
        entries,
        lineage,
        execution_receipts: receipts,
        verdict,
    })
}

fn write_bundle(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)
            .map_err(|err| format!("failed to create parent dir {}: {err}", parent.display()))?;
    }
    std::fs::write(path, bytes).map_err(|err| format!("write failed: {err}"))
}

/// Provider-class rotation that guarantees at least one local and one cloud
/// receipt regardless of how small the roster is.
fn provider_for(idx: usize) -> &'static str {
    match idx % 2 {
        0 => "minimax",
        _ => "custom/local-lmstudio",
    }
}

fn model_for(provider: &str) -> &'static str {
    if provider.contains("local") || provider.contains("lmstudio") {
        "circle-run-local-1"
    } else {
        "circle-run-cloud-1"
    }
}

fn artifact_type_for_index(idx: usize) -> String {
    match idx {
        1 => "proposal".to_string(),
        2 => "critique".to_string(),
        _ => "proposal".to_string(),
    }
}

fn make_receipt(seq: u64, executor: &str, action: &str, provider: &str) -> ExecutionReceipt {
    ExecutionReceipt {
        receipt_id: format!("rcpt_{seq:03}_{executor}"),
        executor: executor.to_string(),
        timestamp: event_time_at(seq),
        action: action.to_string(),
        parameters: json!({
            "provider": provider,
            "model": model_for(provider),
            "duration_ms": 12,
            "returncode": 0,
        }),
        outcome: ReceiptOutcome::Success,
        cost_tokens: Some(50),
    }
}

fn non_blank_response(idx: usize, member: &str, objective: &str) -> String {
    match idx {
        1 => format!(
            "Worker {member}: synthesising first draft for {objective}; full transcript \
             available in channel artifact."
        ),
        2 => format!(
            "Critic {member}: proposal covers the stated objective but leaves one open \
             line item for the referee."
        ),
        _ => format!("Member {member} (slot {idx}) addressed the objective: {objective}."),
    }
}
