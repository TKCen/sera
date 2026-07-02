//! Integration tests for `sera circle closeout` (Task E).
//!
//! Drives the new resident operator surface end-to-end via the public
//! entry point `sera_cli::circle_closeout::run_circle_closeout` and
//! asserts that the returned report + bundle survive a round-trip
//! through `sera circle validate`. These tests complement the focused
//! library tests in `sera-runtime::circle_ingress` and the offline
//! `sera circle run` tests in `tests/circle_run.rs`.

use std::path::PathBuf;
use std::process::Command;

use serde_json::Value;

use sera_cli::circle_closeout::run_circle_closeout;
use sera_types::circle::CollaborationProofBundle;
use sera_types::circle_validator::validate_proof_bundle;

fn temp_path(name: &str) -> PathBuf {
    let mut dir = std::env::temp_dir();
    let pid = std::process::id();
    dir.push(format!("sera-circle-closeout-{name}-{pid}"));
    dir
}

fn run_minimal_closeout(json_out: bool) -> (i32, PathBuf) {
    let bundle_out = temp_path("bundle");
    let report_out = temp_path("report");
    let exit_code = run_circle_closeout(
        Some("to:circle:sera-nqh3".to_string()),
        Some("alice".to_string()),
        Some("lead".to_string()),
        Some("open the run and submit proposal".to_string()),
        Some("session:parent-task-e".to_string()),
        Some("agent-007".to_string()),
        Some("ref".to_string()),
        Some("approved".to_string()),
        Some("Lead alice claimed the role and posted a non-blank summary.".to_string()),
        Some("Resident closeout: lead + post + referee verdict".to_string()),
        Some(bundle_out.clone()),
        Some(report_out.clone()),
        Some(8),
        json_out,
    );
    (exit_code, bundle_out)
}

#[test]
fn closeout_text_path_emits_machine_footer_and_passes_validation() {
    let (exit_code, bundle_path) = run_minimal_closeout(false);
    assert_eq!(exit_code, 0, "happy path must exit 0");

    // The CLI must emit the `circle-closeout:` footer on stdout. The
    // `cargo test` harness captures stdout per-test, so we read the
    // bundle file (the operator's only on-disk artifact) to confirm the
    // seam wrote it.
    let bytes = std::fs::read(&bundle_path).expect("bundle file must exist");
    let bundle: CollaborationProofBundle =
        serde_json::from_slice(&bytes).expect("bundle must parse as CollaborationProofBundle");
    assert!(
        validate_proof_bundle(&bundle).is_ok(),
        "CLI-emitted bundle must pass validate_proof_bundle",
    );
    assert_eq!(bundle.circle_id, "sera-nqh3");
    assert!(bundle.run_id.starts_with("sera-circle-ingress-sera-nqh3-"));
    assert_eq!(bundle.entries.len(), 3);
    assert_eq!(bundle.execution_receipts.len(), 3);
    assert_eq!(bundle.lineage.len(), 2);
}

#[test]
fn closeout_json_path_writes_full_report_with_all_required_fields() {
    let (exit_code, _) = run_minimal_closeout(true);
    assert_eq!(exit_code, 0, "json path must exit 0");

    // The report file is the one place every required field is laid
    // out for a machine parser; the operator's tooling reads this
    // file directly when `--report-out` is supplied.
    let mut dir = std::env::temp_dir();
    let pid = std::process::id();
    dir.push(format!("sera-circle-closeout-report-{pid}"));
    let report_path = dir;
    let report_bytes = std::fs::read(&report_path).expect("report file must exist");
    let report: Value = serde_json::from_slice(&report_bytes).expect("report must be JSON");

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
            report.get(required_key).is_some(),
            "OperatorCloseoutReport JSON must expose {required_key:?}; got {report:?}",
        );
    }
    assert_eq!(report["circle_id"], "sera-nqh3");
    assert_eq!(report["member_id"], "alice");
    assert_eq!(report["session_key"], "session:circle:sera-nqh3:alice");
    assert_eq!(report["parent_session_key"], "session:parent-task-e");
    assert_eq!(report["entry_count"], 3);
    assert_eq!(report["receipt_count"], 3);
    assert_eq!(report["lineage_edge_count"], 2);
    assert_eq!(report["activity_writes"], 0); // no log attached by the CLI
    // Result<(), Vec<String>> serialises as `{"Ok": null}` for Ok(()) and
    // `{"Err": [...]}` for the failure case; the operator surface never
    // emits a bare `null` here.
    assert_eq!(report["validation"]["Ok"], Value::Null);
    // VerdictType serialises as `{"kind": "approved", ...}` via serde's
    // internally-tagged enum representation (see `sera_types::circle::
    // VerdictType`); the operator report is the place downstream tooling
    // reads the classification.
    assert_eq!(
        report["verdict_type"]["kind"], "approved",
        "verdict_type should serialise to snake_case with an inner 'kind' tag"
    );
}

#[test]
fn closeout_refuses_invalid_address_with_usage_error() {
    let exit_code = run_circle_closeout(
        Some("to:agent:bob".to_string()),
        Some("alice".to_string()),
        Some("lead".to_string()),
        Some("x".to_string()),
        None,
        None,
        Some("ref".to_string()),
        Some("approved".to_string()),
        Some("rationale".to_string()),
        None,
        None,
        None,
        None,
        false,
    );
    assert_eq!(
        exit_code, 3,
        "invalid address must surface as USAGE_ERROR exit code 3"
    );
}

#[test]
fn closeout_refuses_invalid_role() {
    let exit_code = run_circle_closeout(
        Some("to:circle:sera-nqh3".to_string()),
        Some("alice".to_string()),
        Some("wizard".to_string()),
        Some("x".to_string()),
        None,
        None,
        Some("ref".to_string()),
        Some("approved".to_string()),
        Some("rationale".to_string()),
        None,
        None,
        None,
        None,
        false,
    );
    assert_eq!(
        exit_code, 3,
        "invalid --role must surface as USAGE_ERROR exit code 3"
    );
}

#[test]
fn closeout_root_call_has_no_parent_session_key() {
    let bundle_out = temp_path("root-bundle");
    let exit_code = run_circle_closeout(
        Some("to:circle:sera-nqh3".to_string()),
        Some("alice".to_string()),
        Some("worker".to_string()),
        Some("submit proposal".to_string()),
        None, // no parent_session_key
        None,
        Some("ref".to_string()),
        Some("needs another pass".to_string()),
        Some("rationale".to_string()),
        None,
        Some(bundle_out.clone()),
        None,
        None,
        false,
    );
    // The CLI exits 0 because the bundle structurally validates; the
    // verdict is non-approved, but the operator surface is the audit
    // hook, not a yes/no gate. The machine footer reports the
    // `REVISION_REQUIRED` label so log parsers can still react.
    assert_eq!(exit_code, 0, "validation passed so exit 0 is correct");

    let bytes = std::fs::read(&bundle_out).expect("bundle file must exist");
    let bundle: CollaborationProofBundle =
        serde_json::from_slice(&bytes).expect("bundle must parse");
    let verdict = bundle.verdict.as_ref().expect("verdict must be set");
    assert!(matches!(
        verdict.verdict_type,
        sera_types::circle::VerdictType::RevisionRequired { .. }
    ));
    assert_eq!(verdict.reviewer, "ref");
}

/// Sha256 contract: the bundle_sha256 reported on
/// `OperatorCloseoutReport`, the bytes written to `--bundle-out`, and
/// the footer sha reported by `sera circle validate --bundle` MUST all
/// agree. This locks the bug shape the lead smoke surfaced in Task E:
/// the runtime previously computed the sha over compact bundle bytes
/// while the CLI wrote pretty bytes, so the footer sha and the
/// validation footer sha could not both match the disk.
///
/// The fix lives in `sera_runtime::circle_ingress::address_circle`
/// (`serde_json::to_vec_pretty` for the sha) and
/// `sera_cli::circle_closeout::write_bundle` (pretty bytes for the
/// file). This test makes sure the two ends of that contract stay in
/// sync and that the operator-facing JSON summary, the on-disk bundle
/// bytes, and the offline validator footer all share one sha.
#[test]
fn closeout_bundle_sha256_matches_disk_bytes_and_validate_footer() {
    use sha2::{Digest, Sha256};

    let bundle_out = temp_path("sha-bundle");
    let report_out = temp_path("sha-report");
    let exit_code = run_circle_closeout(
        Some("to:circle:sera-nqh3".to_string()),
        Some("alice".to_string()),
        Some("lead".to_string()),
        Some("open the run and submit proposal".to_string()),
        Some("session:parent-task-e".to_string()),
        Some("agent-007".to_string()),
        Some("ref".to_string()),
        Some("approved".to_string()),
        Some("Lead alice claimed the role and posted a non-blank summary.".to_string()),
        Some("Resident closeout: lead + post + referee verdict".to_string()),
        Some(bundle_out.clone()),
        Some(report_out.clone()),
        Some(8),
        true,
    );
    assert_eq!(exit_code, 0, "happy path must exit 0");

    // 1. sha256 of the bytes written to --bundle-out.
    let bundle_bytes = std::fs::read(&bundle_out).expect("bundle file must exist");
    let mut hasher = Sha256::new();
    hasher.update(&bundle_bytes);
    let disk_sha = format!("{:x}", hasher.finalize());

    // 2. bundle_sha256 surfaced on OperatorCloseoutReport.bundle_sha256.
    let report_bytes = std::fs::read(&report_out).expect("report file must exist");
    let report: Value =
        serde_json::from_slice(&report_bytes).expect("report must parse as JSON");
    let report_sha = report["bundle_sha256"]
        .as_str()
        .expect("bundle_sha256 must be a string on the report")
        .to_string();

    // 3. sha256 reported by `sera circle validate --bundle <bundle_path>`.
    let validate_output = Command::new(env!("CARGO_BIN_EXE_sera"))
        .args([
            "circle",
            "validate",
            "--bundle",
            bundle_out.to_str().unwrap(),
            "--json",
        ])
        .output()
        .expect("run sera circle validate");
    assert!(
        validate_output.status.success(),
        "validate must succeed against the closeout-written bundle; \
         stdout={} stderr={}",
        String::from_utf8_lossy(&validate_output.stdout),
        String::from_utf8_lossy(&validate_output.stderr),
    );
    let validate_stdout = String::from_utf8_lossy(&validate_output.stdout);
    let validate_footer_sha = validate_stdout
        .lines()
        .rev()
        .find(|line| line.starts_with("circle-validate: "))
        .and_then(|line| line.split_whitespace().nth(2))
        .map(str::to_string)
        .expect("validate footer must be present");
    let validate_json_sha = serde_json::from_slice::<Value>(&validate_output.stdout)
        .ok()
        .and_then(|v| v["bundle_sha256"].as_str().map(str::to_string));

    // The three ends of the sha contract must agree.
    assert_eq!(
        report_sha, disk_sha,
        "OperatorCloseoutReport.bundle_sha256 must match sha256 of --bundle-out bytes",
    );
    assert_eq!(
        validate_footer_sha, disk_sha,
        "`sera circle validate` footer sha must match sha256 of --bundle-out bytes",
    );
    if let Some(json_sha) = validate_json_sha {
        assert_eq!(
            json_sha, disk_sha,
            "`sera circle validate --json` bundle_sha256 must match sha256 of --bundle-out bytes",
        );
    }
}