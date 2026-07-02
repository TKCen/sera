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
        Some("8".to_string()),
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
        Some("8".to_string()),
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

/// Codex review thread (rust/crates/sera-cli/src/circle_closeout.rs
/// nested `--bundle-out`/`--report-out` paths): the operator seam must
/// create missing parent directories for both the proof bundle and the
/// report file so callers don't have to mkdir before every
/// `--bundle-out nested/path.json`. Without the `ensure_parent_dir`
/// guard the file write would fail with `No such file or directory`.
#[test]
fn closeout_creates_parent_directories_for_bundle_and_report() {
    let tmp_root = std::env::temp_dir().join(format!(
        "sera-circle-closeout-nested-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0),
    ));
    let bundle_out = tmp_root.join("nested").join("deep").join("bundle.json");
    let report_out = tmp_root.join("nested").join("deep").join("report.json");

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
        Some("8".to_string()),
        true,
    );
    assert_eq!(exit_code, 0, "nested-path closeout must succeed");
    assert!(
        bundle_out.exists(),
        "nested bundle path must be created by the CLI, got {}",
        bundle_out.display(),
    );
    assert!(
        report_out.exists(),
        "nested report path must be created by the CLI, got {}",
        report_out.display(),
    );

    // Clean up the temp tree so the test does not leak.
    let _ = std::fs::remove_dir_all(&tmp_root);
}

/// Codex review thread (rust/crates/sera-cli/src/main.rs line 218 —
/// "Preserve closeout footer for missing option values"): when a known
/// closeout option is supplied without its value (e.g.
/// `sera circle closeout --to --member alice ...`), clap must not
/// reject the invocation outright; the handler-level
/// `require_some(...)` validation is what should fail, and the
/// `circle-closeout:` machine footer must still be emitted so log
/// parsers keep one parsing vocabulary. This test invokes the
/// `sera` binary directly (the existing parser-test pattern in
/// `circle_closeout.rs`) to prove the footer is preserved.
#[test]
fn closeout_binary_emits_machine_footer_when_required_option_missing_value() {
    let output = Command::new(env!("CARGO_BIN_EXE_sera"))
        .args([
            "circle",
            "closeout",
            "--to", // missing value: clap should accept the flag (default_missing_value=""),
            "--member",
            "alice",
            "--role",
            "lead",
            "--summary",
            "open the run",
            "--referee",
            "ref",
            "--verdict",
            "approved",
            "--rationale",
            "ok",
        ])
        .output()
        .expect("run sera circle closeout");
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    // The required-value footer must appear on stdout — the parser
    // must not have rejected the invocation outright before the
    // handler validation path runs.
    assert!(
        stdout.lines().any(|line| line.starts_with("circle-closeout: "))
            || stderr.lines().any(|line| line.starts_with("circle-closeout: ")),
        "missing-option-value path must still emit the `circle-closeout:` footer; \
         stdout={stdout:?} stderr={stderr:?}",
    );
    // `--to` was provided without a value, so the handler-level
    // `require_some("--to", "")` rejects it as USAGE_ERROR (exit 3).
    assert_eq!(
        output.status.code(),
        Some(3),
        "missing --to value must surface as USAGE_ERROR exit code 3; \
         stdout={stdout:?} stderr={stderr:?}",
    );
}

/// Codex review thread (rust/crates/sera-cli/src/main.rs line 260 —
/// "Parse audit-limit in the handler"): when `--audit-limit` is
/// supplied without a value, clap must accept the flag (it does — the
/// `default_missing_value = ""` is now consumed by the handler parser,
/// not clap's `usize` parser) and the handler must still emit the
/// `circle-closeout:` footer. We assert the binary path: `sera circle
/// closeout ... --audit-limit` (no value) MUST exit 3 with the footer
/// on stdout.
#[test]
fn closeout_binary_emits_machine_footer_when_audit_limit_missing_value() {
    let output = Command::new(env!("CARGO_BIN_EXE_sera"))
        .args([
            "circle",
            "closeout",
            "--to",
            "to:circle:sera-nqh3",
            "--member",
            "alice",
            "--role",
            "lead",
            "--summary",
            "open the run",
            "--referee",
            "ref",
            "--verdict",
            "approved",
            "--rationale",
            "ok",
            "--audit-limit", // missing value: clap should accept it, handler uses default
        ])
        .output()
        .expect("run sera circle closeout");
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    // The footer must be emitted on stdout — clap must not have
    // rejected the invocation before the handler ran.
    assert!(
        stdout.lines().any(|line| line.starts_with("circle-closeout: "))
            || stderr.lines().any(|line| line.starts_with("circle-closeout: ")),
        "missing-audit-limit-value path must still emit the `circle-closeout:` footer; \
         stdout={stdout:?} stderr={stderr:?}",
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "missing --audit-limit value must fall back to the operator-report default \
         and exit 0 (NOT a USAGE_ERROR); \
         stdout={stdout:?} stderr={stderr:?}",
    );
}

/// Codex review thread (rust/crates/sera-cli/src/main.rs line 260 —
/// "Parse audit-limit in the handler"): when `--audit-limit` is
/// supplied with a non-numeric value, the handler-level parser must
/// reject it as USAGE_ERROR (exit 3) and STILL emit the
/// `circle-closeout:` footer so log parsers keep one vocabulary.
#[test]
fn closeout_binary_emits_machine_footer_when_audit_limit_invalid_value() {
    let output = Command::new(env!("CARGO_BIN_EXE_sera"))
        .args([
            "circle",
            "closeout",
            "--to",
            "to:circle:sera-nqh3",
            "--member",
            "alice",
            "--role",
            "lead",
            "--summary",
            "open the run",
            "--referee",
            "ref",
            "--verdict",
            "approved",
            "--rationale",
            "ok",
            "--audit-limit",
            "not-a-number",
        ])
        .output()
        .expect("run sera circle closeout");
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    // The footer must be emitted on stdout — clap must not have
    // rejected the invocation before the handler ran.
    assert!(
        stdout.lines().any(|line| line.starts_with("circle-closeout: "))
            || stderr.lines().any(|line| line.starts_with("circle-closeout: ")),
        "invalid-audit-limit path must still emit the `circle-closeout:` footer; \
         stdout={stdout:?} stderr={stderr:?}",
    );
    assert_eq!(
        output.status.code(),
        Some(3),
        "invalid --audit-limit value must surface as USAGE_ERROR exit code 3; \
         stdout={stdout:?} stderr={stderr:?}",
    );
    // The handler error message should mention the bad value so
    // operators can debug it from the stderr log.
    assert!(
        stderr.contains("invalid --audit-limit"),
        "invalid --audit-limit path must explain the rejection on stderr; \
         stdout={stdout:?} stderr={stderr:?}",
    );
}

/// Codex review thread (rust/crates/sera-cli/src/circle_closeout.rs
/// line 167 — "Reject identical output artifact paths"): when the
/// operator points `--bundle-out` and `--report-out` at the same path,
/// the CLI must refuse BEFORE writing either artifact, emit the
/// `circle-closeout:` footer (carrying the computed bundle sha), and
/// exit with USAGE_ERROR (3). Critically, no file should appear on
/// disk at the offending path — the bug shape was bundle clobbered
/// by report while the footer still advertised the bundle sha, so the
/// "no file written" assertion is the load-bearing one for the
/// regression.
#[test]
fn closeout_rejects_identical_bundle_and_report_out_paths() {
    let shared = temp_path("shared");

    // Sanity: confirm the path does not exist before the call. If a
    // prior failed run left cruft behind, this test would silently
    // mask the regression.
    let _ = std::fs::remove_file(&shared);
    assert!(!shared.exists(), "shared path must not exist pre-call");

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
        Some("Resident closeout: same path for both artifacts".to_string()),
        Some(shared.clone()),
        Some(shared.clone()),
        Some("8".to_string()),
        false,
    );
    assert_eq!(
        exit_code, 3,
        "identical --bundle-out and --report-out must exit 3 (USAGE_ERROR)"
    );
    // The pre-write guard MUST prevent either artifact from being
    // written. The bug shape is a report overwriting a bundle, so the
    // assertion below is the load-bearing one for the regression.
    assert!(
        !shared.exists(),
        "no file must be written when --bundle-out and --report-out collide; \
         found {} unexpectedly",
        shared.display(),
    );
}

/// Codex review thread (rust/crates/sera-cli/src/circle_closeout.rs
/// line 171 — fourth pass, "Normalize artifact paths before comparing
/// them"): a raw `Path::eq` equality check missed lexical aliases
/// such as `bundle.json` vs `./bundle.json` and `dir/../bundle.json`
/// vs `bundle.json`. The operator seam must therefore compare
/// normalized target paths and refuse the collision BEFORE writing
/// either artifact. This test covers the load-bearing alias shapes
/// without creating any files (so it does not mask the regression by
/// making the equal-looking paths trivially "identical" via the
/// filesystem).
#[test]
fn closeout_rejects_lexically_aliased_bundle_and_report_out_paths() {
    // Pick a parent directory we know exists (the system temp dir)
    // and a non-existing target inside it so `canonicalize` would
    // fail on either path. Lexical normalization must catch the
    // aliases anyway.
    let tmp_root = std::env::temp_dir().join(format!(
        "sera-circle-closeout-alias-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0),
    ));
    let bundle_path = tmp_root.join("bundle.json");
    let report_path = tmp_root.join(".").join("bundle.json");
    let report_path_dotdot = tmp_root.join("nested").join("..").join("bundle.json");

    // Sanity: none of the targets exist yet — the lexical alias
    // check must catch the collision WITHOUT touching the disk.
    let _ = std::fs::remove_dir_all(&tmp_root);
    assert!(!bundle_path.exists(), "bundle target must not exist pre-call");
    assert!(!report_path.exists(), "alias target must not exist pre-call");

    // Alias shape #1: `--bundle-out <tmp>/bundle.json` vs
    // `--report-out <tmp>/./bundle.json`.
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
        Some("Resident closeout: lexical alias #1 (./)".to_string()),
        Some(bundle_path.clone()),
        Some(report_path.clone()),
        Some("8".to_string()),
        false,
    );
    assert_eq!(
        exit_code, 3,
        "lexically aliased paths (./ prefix) must exit 3 (USAGE_ERROR); \
         bundle={} report={}",
        bundle_path.display(),
        report_path.display(),
    );
    assert!(
        !bundle_path.exists(),
        "no file must be written when alias #1 collides; found {} unexpectedly",
        bundle_path.display(),
    );
    assert!(
        !report_path.exists(),
        "no file must be written when alias #1 collides; found {} unexpectedly",
        report_path.display(),
    );

    // Alias shape #2: `--bundle-out <tmp>/bundle.json` vs
    // `--report-out <tmp>/nested/../bundle.json` (parent traversal
    // that resolves back to the same lexical target).
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
        Some("Resident closeout: lexical alias #2 (nested/..)".to_string()),
        Some(bundle_path.clone()),
        Some(report_path_dotdot.clone()),
        Some("8".to_string()),
        false,
    );
    assert_eq!(
        exit_code, 3,
        "lexically aliased paths (nested/..) must exit 3 (USAGE_ERROR); \
         bundle={} report={}",
        bundle_path.display(),
        report_path_dotdot.display(),
    );
    assert!(
        !bundle_path.exists(),
        "no file must be written when alias #2 collides; found {} unexpectedly",
        bundle_path.display(),
    );
    assert!(
        !report_path_dotdot.exists(),
        "no file must be written when alias #2 collides; found {} unexpectedly",
        report_path_dotdot.display(),
    );

    // Clean up the temp tree so the test does not leak.
    let _ = std::fs::remove_dir_all(&tmp_root);
}

/// Codex review thread (rust/crates/sera-cli/src/circle_closeout.rs
/// line 176 — "Use the closeout verdict in JSON output"): for
/// non-approved verdicts the bundle still structurally validates, so
/// the previous implementation emitted `verdict: "PASS"` in the JSON
/// while the text summary and the `circle-closeout:` footer used
/// `verdict_label_for(...)` and reported `REVISION_REQUIRED` (or
/// `FAIL` / `TIE` / `INVALID`). Machine consumers reading the JSON
/// must see the same verdict label as the footer, while the
/// `validation` field still exposes the structural validation
/// status.
///
/// This drives the `sera` binary directly so we can capture the
/// compact stdout JSON (`--json` is printed, not file-written) and
/// the `circle-closeout:` footer that follows it.
#[test]
fn closeout_json_verdict_matches_footer_label_for_non_approved_verdict() {
    let tmp_root = std::env::temp_dir().join(format!(
        "sera-circle-closeout-json-verdict-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0),
    ));
    let bundle_out = tmp_root.join("bundle.json");
    let report_out = tmp_root.join("report.json");

    let output = Command::new(env!("CARGO_BIN_EXE_sera"))
        .args([
            "circle",
            "closeout",
            "--to",
            "to:circle:sera-nqh3",
            "--member",
            "alice",
            "--role",
            "lead",
            "--summary",
            "need another pass before approval",
            "--referee",
            "ref",
            // "needs another pass" maps to a non-approved verdict
            // (RevisionRequired) while the bundle still structurally
            // validates — exactly the shape the bug report flagged.
            "--verdict",
            "needs another pass",
            "--rationale",
            "Non-approved referee verdict for the JSON path regression.",
            "--objective",
            "Resident closeout: non-approved verdict",
            "--bundle-out",
            bundle_out.to_str().unwrap(),
            "--report-out",
            report_out.to_str().unwrap(),
            "--audit-limit",
            "8",
            "--json",
        ])
        .output()
        .expect("run sera circle closeout");
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    // Structural validation passes (CLI exits 0). Validation
    // (structural) and verdict (referee) are intentionally separate.
    assert_eq!(
        output.status.code(),
        Some(0),
        "structural validation pass must exit 0; stdout={stdout:?} stderr={stderr:?}"
    );

    // The compact JSON summary is the first non-empty stdout line;
    // the `circle-closeout:` footer is the last line that begins
    // with that prefix.
    let json_line = stdout
        .lines()
        .find(|line| line.trim_start().starts_with('{'))
        .expect("compact JSON summary must be on stdout");
    let footer_label = stdout
        .lines()
        .rev()
        .find(|line| line.starts_with("circle-closeout: "))
        .and_then(|line| line.split_whitespace().nth(1))
        .map(str::to_string)
        .expect("circle-closeout: footer must be present");

    let json: Value = serde_json::from_str(json_line).expect("compact JSON must parse");

    // JSON verdict must match the footer label.
    // REVISION_REQUIRED here, NOT PASS.
    assert_eq!(
        json["verdict"], "REVISION_REQUIRED",
        "JSON verdict must follow verdict_label_for and not be coerced to PASS; \
         got json={json:?}",
    );
    assert_eq!(
        footer_label, "REVISION_REQUIRED",
        "footer label must match the JSON verdict field"
    );
    assert_eq!(
        json["verdict_type"], "RevisionRequired { feedback: \"Non-approved referee verdict for the JSON path regression.\" }",
        "verdict_type Debug print must reflect the closeout verdict"
    );
    // Structural validation is still surfaced separately as `null`
    // in the compact JSON when the bundle validates cleanly.
    assert!(
        json["validation"].is_null(),
        "structural validation must remain in the validation field; got {}",
        json["validation"]
    );

    // Sanity: both artifacts written (the regression concerns
    // judgment, not write semantics), and the report file's
    // full-shaped validation uses the struct `{"Ok": null}` shape.
    assert!(bundle_out.exists(), "bundle must be written");
    assert!(report_out.exists(), "report must be written");
    let report_bytes = std::fs::read(&report_out).expect("report file must exist");
    let report: Value =
        serde_json::from_slice(&report_bytes).expect("report must parse as JSON");
    assert_eq!(
        report["validation"]["Ok"],
        Value::Null,
        "full OperatorCloseoutReport validation field must remain {{\"Ok\": null}}"
    );

    let _ = std::fs::remove_dir_all(&tmp_root);
}