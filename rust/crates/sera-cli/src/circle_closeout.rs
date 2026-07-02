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
///
/// `audit_limit` is accepted as `Option<String>` (and parsed in this
/// handler) rather than `Option<usize>` so `--audit-limit` with no value
/// still reaches this function and emits the `circle-closeout:` footer
/// instead of being rejected by clap's `usize` parser before we ever
/// run. Invalid numeric values surface as `USAGE_ERROR` exit 3 with the
/// footer; missing/blank values fall through to the operator-report
/// default.
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
    audit_limit: Option<String>,
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

    // Parse --audit-limit in the handler (not via clap's `usize` parser)
    // so a missing value still reaches the footer path. `Some("")` and
    // `None` both fall back to the operator-report default; any other
    // string must parse as `usize` or we surface a usage error WITH the
    // footer — matching the other handler-level error paths above.
    let audit_limit: Option<usize> = match audit_limit.as_deref().map(str::trim) {
        None => None,
        Some("") => None,
        Some(raw) => match raw.parse::<usize>() {
            Ok(n) => Some(n),
            Err(err) => {
                eprintln!("invalid --audit-limit {raw:?}: {err}");
                print_circle_closeout_footer("USAGE_ERROR", "unknown");
                return 3;
            }
        },
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

    // Codex review thread (circle_closeout.rs line 167 — "Reject
    // identical output artifact paths"): if the operator points
    // `--bundle-out` and `--report-out` at the same path, the bundle
    // write and the report write would clobber each other on disk
    // while the footer still advertises the bundle sha. Detect the
    // collision BEFORE writing either file: surface `USAGE_ERROR`
    // with the footer (using the computed bundle sha when available)
    // and leave the path untouched.
    //
    // Codex review thread (circle_closeout.rs line 171 — fourth pass,
    // "Normalize artifact paths before comparing them"): a raw
    // `Path` equality check missed lexical aliases such as
    // `bundle.json` vs `./bundle.json` and `dir/../bundle.json` vs
    // `bundle.json`. We now compare the lexically-normalized target
    // paths (and only fall back to a canonical comparison when both
    // targets exist, so we never `touch` a file just to compare).
    // The normalised form collapses `.`, `..`, and redundant
    // separators; `canonicalize` further resolves symlinks for
    // existing files but is intentionally not used on non-existing
    // paths (it would `Err`).
    if let (Some(bundle_path), Some(report_path)) = (bundle_out.as_deref(), report_out.as_deref())
        && artifact_paths_collide(bundle_path, report_path)
    {
        eprintln!(
            "--bundle-out and --report-out must point to different paths; got {} for both",
            bundle_path.display(),
        );
        print_circle_closeout_footer("USAGE_ERROR", &report.bundle_sha256);
        return 3;
    }

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
        // Codex review thread (circle_closeout.rs line 176 — "Use the
        // closeout verdict in JSON output"): machine consumers reading
        // the compact JSON must see the same verdict label as the
        // text summary and the `circle-closeout:` footer (`PASS` /
        // `FAIL` / `TIE` / `INVALID` / `REVISION_REQUIRED`). The
        // `validation` field below still carries the structural
        // validation result (Ok / Err) so the two concerns stay
        // separable.
        let summary_json = json!({
            "verdict": verdict_label,
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

/// Decide whether two artifact target paths would write to the same
/// on-disk location, so the operator seam can refuse the collision
/// BEFORE writing either file.
///
/// Codex review thread (circle_closeout.rs line 340 — fifth pass,
/// "Reject relative/absolute artifact aliases"): a relative path
/// such as `bundle.json` and its absolute equivalent
/// `$PWD/bundle.json` did not compare equal under `lex_normalize`
/// when the target file did not yet exist — `canonicalize` errors
/// on non-existing paths and lexical normalization leaves a bare
/// relative path starting from `CurDir`, while an absolute path
/// starts from `RootDir`. We now anchor every relative-path input
/// against `std::env::current_dir()` BEFORE lexical normalization
/// so the two cases collapse to the same form. `current_dir`
/// failure (rare — propagated from the OS) falls through to the
/// existing lexical-only comparison so the function stays robust.
///
/// Codex review thread (circle_closeout.rs line 171 — fourth pass,
/// "Normalize artifact paths before comparing them"): we also
/// compare the lexically-normalized targets (collapsing `.`,
/// `..`, and redundant separators) and, when both targets already
/// exist on disk, fall through to `canonicalize` so symlink
/// aliases are caught too. `canonicalize` is intentionally NOT
/// called on non-existing paths because it would error out — we
/// don't want to create files just to compare them.
fn artifact_paths_collide(bundle_path: &Path, report_path: &Path) -> bool {
    let bundle_anchored = anchor_relative_path(bundle_path);
    let report_anchored = anchor_relative_path(report_path);
    let bundle_lex = lex_normalize(&bundle_anchored);
    let report_lex = lex_normalize(&report_anchored);
    if bundle_lex == report_lex {
        return true;
    }
    // Both targets exist -> resolve symlinks too. We deliberately do
    // not create files just to satisfy canonicalize.
    if let (Ok(bundle_canon), Ok(report_canon)) = (
        std::fs::canonicalize(&bundle_lex),
        std::fs::canonicalize(&report_lex),
    ) {
        return bundle_canon == report_canon;
    }
    false
}

/// Anchor a path that is NOT already absolute against
/// `std::env::current_dir()` so relative and absolute
/// representations of the same target (e.g. `bundle.json` vs
/// `$PWD/bundle.json`) collapse to the same normalized form
/// before lexical comparison. Absolute paths are returned
/// unchanged. If `current_dir` itself fails (very rare; only on
/// platforms where the OS cannot resolve the cwd), the original
/// path is returned unchanged so lexical comparison can still
/// attempt to detect obvious `.`/`..` aliases.
///
/// Codex review thread (circle_closeout.rs line 340 — fifth pass):
/// without this anchor step, a non-existing `bundle.json`
/// (relative) and a non-existing `$PWD/bundle.json` (absolute)
/// would compare UNEQUAL because `lex_normalize` cannot bridge a
/// `CurDir` root vs a `RootDir` root — they remain two distinct
/// lexical trees even after canonicalization refuses to run.
fn anchor_relative_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        return path.to_path_buf();
    }
    match std::env::current_dir() {
        Ok(cwd) => cwd.join(path),
        Err(_) => path.to_path_buf(),
    }
}

/// Lexically normalize a path without touching the filesystem.
/// Equivalent to `std::path::Path::components` collapsed: `.` is
/// dropped, `..` pops the previous non-`..` component, redundant
/// separators are removed. Works for both existing and non-existing
/// paths.
fn lex_normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                let popped = out.components().next_back();
                match popped {
                    Some(std::path::Component::Normal(_)) => {
                        out.pop();
                    }
                    Some(std::path::Component::RootDir) | None => {
                        // Cannot pop further; preserve the `..` so
                        // `../foo` and `foo` still compare unequal
                        // even though the lexical form is the same.
                        out.push("..");
                    }
                    _ => {
                        out.push("..");
                    }
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
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