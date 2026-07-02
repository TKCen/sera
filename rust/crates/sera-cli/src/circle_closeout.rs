//! `sera circle closeout --to <circle> ...` operator seam.
//!
//! This is the small in-process operator-facing command that ties
//! `sera_runtime::circle_ingress::CircleIngress::accept` together with
//! `sera_runtime::circle_ingress::closeout_into_proof_bundle` and
//! returns a structured, audit-ready
//! `sera_runtime::circle_ingress::OperatorCloseoutReport` to the
//! caller.
//!
//! The CLI surface mirrors the offline `sera circle run` command's
//! argument style so operators get one consistent shape across both
//! resident and offline seams. The command is deliberately narrow:
//!
//!   - it does NOT call any live gateway,
//!   - it does NOT spawn a scheduler or external connector,
//!   - it does NOT persist the bundle or report anywhere except the
//!     optional `--bundle-out <PATH>` and the optional `--report-out
//!     <PATH>` paths supplied by the caller.
//!
//! See `docs/public/decisions/2026-06-30-circle-team-channel-topology.md`
//! §9 (Task E) for the canonical scope and the explicit guardrails that
//! separate this resident operator surface from the future external
//! connector / full-swarm work.

use std::path::{Path, PathBuf};

use serde_json::json;
use sera_runtime::circle_ingress::{
    address_circle, CircleIngress, CircleIngressCloseout, CircleIngressCloseoutBundle,
    OperatorCloseoutReport, OPERATOR_REPORT_DEFAULT_AUDIT_LIMIT,
};
use sera_types::circle::VerdictType;
use sera_types::circle_channel::CircleChannelRole;
use sera_types::circle_ingress::CircleIngressRequest;

/// Public entry point invoked from `main.rs` once the offline dispatch
/// guard matches `Commands::Circle { CircleCommand::Closeout { .. } }`.
///
/// Returns the process exit code — `0` on success, `1` when the closeout
/// bundle fails validation, `3` on usage / IO errors. The machine footer
/// (`circle-closeout: <verdict> <bundle_sha256>`) is always emitted, even
/// on failure, so downstream log parsers do not need a separate success
/// channel.
#[allow(clippy::too_many_arguments)]
pub fn run_circle_closeout(
    to: Option<String>,
    member: Option<String>,
    role: Option<String>,
    summary: Option<String>,
    parent_session_key: Option<String>,
    agent_id: Option<String>,
    referee: Option<String>,
    verdict: Option<String>,
    rationale: Option<String>,
    objective: Option<String>,
    bundle_out: Option<PathBuf>,
    report_out: Option<PathBuf>,
    audit_limit: Option<usize>,
    json_out: bool,
) -> i32 {
    let to = match require_some("--to", to) {
        Ok(v) => v,
        Err(code) => return code,
    };
    let member = match require_some("--member", member) {
        Ok(v) => v,
        Err(code) => return code,
    };
    let role_str = match require_some("--role", role) {
        Ok(v) => v,
        Err(code) => return code,
    };
    let summary = match require_some("--summary", summary) {
        Ok(v) => v,
        Err(code) => return code,
    };
    let referee = match require_some("--referee", referee) {
        Ok(v) => v,
        Err(code) => return code,
    };
    let verdict_text = match require_some("--verdict", verdict) {
        Ok(v) => v,
        Err(code) => return code,
    };
    let rationale = match require_some("--rationale", rationale) {
        Ok(v) => v,
        Err(code) => return code,
    };

    let role = match parse_role(&role_str) {
        Ok(r) => r,
        Err(err) => {
            eprintln!("invalid --role {role_str:?}: {err}");
            print_circle_closeout_footer("USAGE_ERROR", "unknown");
            return 3;
        }
    };

    let mut request = CircleIngressRequest::new(&to, &member, role, &summary);
    if let Some(parent) = parent_session_key
        && !parent.trim().is_empty()
    {
        request = request.with_parent_session_key(parent);
    }
    if let Some(agent) = agent_id
        && !agent.trim().is_empty()
    {
        request = request.with_agent_id(agent);
    }

    let mut closeout = CircleIngressCloseout::new(referee, verdict_text, rationale);
    if let Some(obj) = objective
        && !obj.trim().is_empty()
    {
        closeout = closeout.with_objective(obj);
    }

    let audit_limit = audit_limit.unwrap_or(OPERATOR_REPORT_DEFAULT_AUDIT_LIMIT);
    let funnel = CircleIngress::new();
    let report = match address_circle(&funnel, &request, &closeout, Some(audit_limit)) {
        Ok(r) => r,
        Err(err) => {
            eprintln!("sera circle closeout failed: {err}");
            print_circle_closeout_footer("USAGE_ERROR", "unknown");
            return 3;
        }
    };

    // Validation is the operator's truthful verdict — never coerce it.
    let validation_ok = report.validation.is_ok();
    let verdict_label = verdict_label_for(&report.verdict_type);

    if let Some(bundle_path) = bundle_out.as_deref()
        && let Err(err) = write_bundle(bundle_path, &report)
    {
        eprintln!("failed to write bundle {}: {err}", bundle_path.display());
        print_circle_closeout_footer("USAGE_ERROR", &report.bundle_sha256);
        return 3;
    }
    if let Some(report_path) = report_out.as_deref()
        && let Err(err) = write_report(report_path, &report)
    {
        eprintln!("failed to write report {}: {err}", report_path.display());
        print_circle_closeout_footer("USAGE_ERROR", &report.bundle_sha256);
        return 3;
    }

    if json_out {
        let summary_json = json!({
            "verdict": if validation_ok { "PASS" } else { "FAIL" },
            "circle_id": report.circle_id,
            "address": report.address,
            "member_id": report.member_id,
            "role": format!("{:?}", report.role),
            "session_key": report.session_key,
            "parent_session_key": report.parent_session_key,
            "request_id": report.request_id.to_string(),
            "run_id": report.run_id,
            "verdict_reviewer": report.verdict_reviewer,
            "verdict_type": format!("{:?}", report.verdict_type),
            "verdict_rationale": report.verdict_rationale,
            "entry_count": report.entry_count,
            "receipt_count": report.receipt_count,
            "lineage_edge_count": report.lineage_edge_count,
            "bundle_sha256": report.bundle_sha256,
            "activity_writes": report.activity_writes,
            "validation": match &report.validation {
                Ok(()) => json!(null),
                Err(errors) => json!(errors),
            },
            "audit_tail_len": report.audit_tail.len(),
            "bundle_out": bundle_out.as_ref().map(|p| p.display().to_string()),
            "report_out": report_out.as_ref().map(|p| p.display().to_string()),
        });
        println!("{}", summary_json);
    } else {
        println!(
            "Circle closeout {verdict_label}: circle={} member={} role={:?} \
             session_key={} parent_session_key={} request_id={} run_id={} \
             reviewer={} entries={} receipts={} lineage_edges={} \
             bundle_sha256={} activity_writes={}",
            report.circle_id,
            report.member_id,
            report.role,
            report.session_key,
            report.parent_session_key.as_deref().unwrap_or("<root>"),
            report.request_id,
            report.run_id,
            report.verdict_reviewer,
            report.entry_count,
            report.receipt_count,
            report.lineage_edge_count,
            report.bundle_sha256,
            report.activity_writes,
        );
        println!(
            "verdict: {:?} (rationale: {})",
            report.verdict_type,
            report.verdict_rationale,
        );
        if !report.audit_tail.is_empty() {
            println!("audit_tail ({} entries):", report.audit_tail.len());
            for entry in &report.audit_tail {
                println!(
                    "  agent={} circle={} summary={:?}",
                    entry.agent_id, entry.circle_id, entry.summary,
                );
            }
        }
    }

    let exit_code = if validation_ok { 0 } else { 1 };
    print_circle_closeout_footer(verdict_label, &report.bundle_sha256);
    exit_code
}

fn print_circle_closeout_footer(verdict: &str, bundle_sha256: &str) {
    println!("circle-closeout: {verdict} {bundle_sha256}");
}

fn require_some(name: &str, value: Option<String>) -> Result<String, i32> {
    match value {
        Some(v) if !v.trim().is_empty() => Ok(v),
        _ => {
            eprintln!("missing required {name} for Circle closeout");
            print_circle_closeout_footer("USAGE_ERROR", "unknown");
            Err(3)
        }
    }
}

fn parse_role(raw: &str) -> Result<CircleChannelRole, String> {
    let lc = raw.trim().to_ascii_lowercase();
    let role = match lc.as_str() {
        "lead" => CircleChannelRole::Lead,
        "worker" => CircleChannelRole::Worker,
        "critic" => CircleChannelRole::Critic,
        "referee" => CircleChannelRole::Referee,
        other => return Err(format!("unknown role {other:?}")),
    };
    Ok(role)
}

/// Map a [`VerdictType`] to the short machine-readable label used in the
/// `circle-closeout:` footer and the text summary. Mirrors the offline
/// `sera circle run` verifier labels so log parsers share a vocabulary.
fn verdict_label_for(verdict_type: &VerdictType) -> &'static str {
    match verdict_type {
        VerdictType::Approved => "PASS",
        VerdictType::Rejected { .. } => "FAIL",
        VerdictType::Tie { .. } => "TIE",
        VerdictType::Invalid { .. } => "INVALID",
        VerdictType::RevisionRequired { .. } => "REVISION_REQUIRED",
    }
}

fn write_bundle(
    path: &Path,
    report: &OperatorCloseoutReport,
) -> Result<(), String> {
    let bundle_out: CircleIngressCloseoutBundle = CircleIngressCloseoutBundle {
        bundle: report.bundle.clone(),
        entry_count: report.entry_count,
        receipt_count: report.receipt_count,
        t0: report.run_id_t0,
    };
    let bytes = serde_json::to_vec_pretty(&bundle_out.bundle)
        .map_err(|err| format!("serialize bundle: {err}"))?;
    ensure_parent_dir(path).map_err(|err| format!("create parent for bundle: {err}"))?;
    std::fs::write(path, &bytes).map_err(|err| format!("write bundle: {err}"))?;
    Ok(())
}

fn write_report(path: &Path, report: &OperatorCloseoutReport) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(report)
        .map_err(|err| format!("serialize report: {err}"))?;
    ensure_parent_dir(path).map_err(|err| format!("create parent for report: {err}"))?;
    std::fs::write(path, &bytes).map_err(|err| format!("write report: {err}"))?;
    Ok(())
}

/// Ensure the parent directory of `path` exists. Mirrors the behaviour
/// of the offline Circle artifact writers (`sera circle run --bundle-out`)
/// so `--bundle-out nested/path.json` and `--report-out nested/path.json`
/// just work — the operator doesn't have to mkdir beforehand. Returns
/// `Ok(())` for paths that have no parent (e.g. bare filenames), and
/// surfaces the underlying IO error when `create_dir_all` fails.
fn ensure_parent_dir(path: &Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }
    Ok(())
}