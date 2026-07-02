//! Offline integration tests for `sera circle run --to <circle>`.
//!
//! Bead: `sera-nqh3` — Common-goal Circle demo. These tests are pure
//! in-process: they spawn the compiled `sera` binary the same way the
//! upstream `circle_validate.rs` suite does, and assert on stdout / stderr
//! footers plus the bundle artifact. There is no LLM, no gateway, no
//! network — every run is deterministic down to the bundle sha256.
//!
//! Spec target: `docs/public/decisions/2026-06-30-circle-team-channel-topology.md`
//! makes Circle channel-addressed (`to:circle:<name>`). This is the
//! offline seam for that decision: the `circle run` command addresses a
//! Circle, addresses the CLI roster, and emits a `CollaborationProofBundle`
//! that `circle validate` round-trips.

use std::path::Path;
use std::process::Command;

use serde_json::Value;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_sera"))
}

fn assert_circle_run_footer(stdout: &str, expected_verdict: &str) {
    assert!(
        stdout.contains(&format!("circle-run: {expected_verdict} ")),
        "expected `circle-run: {expected_verdict} <sha256>` footer, got: {stdout}"
    );
}

// ---------------------------------------------------------------------
// Happy path: 1 leader + 2 members + 1 referee (default roster)
// ---------------------------------------------------------------------

#[test]
fn circle_run_cli_addresses_circle_and_emits_valid_proof_bundle() {
    let temp = tempfile::tempdir().unwrap();
    let bundle_out = temp.path().join("out").join("proof_bundle.json");

    let output = bin()
        .args([
            "circle",
            "run",
            "--to",
            "sera-nqh3",
            "--members",
            "alice,bob,carol",
            "--referee",
            "ref",
            "--objective",
            "Prove the channel-addressed Circle seam",
            "--bundle-out",
            bundle_out.to_str().unwrap(),
            "--json",
        ])
        .output()
        .expect("run sera circle run");

    assert!(
        output.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_circle_run_footer(&stdout, "PASS");
    assert!(
        bundle_out.exists(),
        "bundle was not written to {}",
        bundle_out.display()
    );

    let body: Value = serde_json::from_slice(&std::fs::read(&bundle_out).expect("read bundle"))
        .expect("parse bundle JSON");
    assert_eq!(body["circle_id"], "sera-nqh3");
    assert_eq!(body["objective"], "Prove the channel-addressed Circle seam");
    assert_eq!(body["entries"].as_array().unwrap().len(), 4);
    assert_eq!(body["execution_receipts"].as_array().unwrap().len(), 4);
    assert_eq!(body["lineage"].as_array().unwrap().len(), 3);
    assert!(body["verdict"].is_object());

    // Round-trip through `sera circle validate` — proves the bundle
    // satisfies `validate_proof_bundle` (mixed-provider, verdict, etc.).
    let validate = bin()
        .args([
            "circle",
            "validate",
            "--bundle",
            bundle_out.to_str().unwrap(),
            "--json",
        ])
        .output()
        .expect("run sera circle validate");
    let validate_stdout = String::from_utf8_lossy(&validate.stdout);
    assert!(
        validate_stdout.contains("\"verdict\":\"PASS\""),
        "validate stdout did not contain PASS: {validate_stdout}"
    );
    assert!(
        validate_stdout.contains("circle-validate: PASS"),
        "expected `circle-validate: PASS` footer, got: {validate_stdout}"
    );
}

// ---------------------------------------------------------------------
// Determinism: identical args → byte-stable sha256
// ---------------------------------------------------------------------

#[test]
fn circle_run_cli_is_byte_deterministic_for_identical_args() {
    let temp_a = tempfile::tempdir().unwrap();
    let temp_b = tempfile::tempdir().unwrap();
    let out_a = temp_a.path().join("proof_bundle.json");
    let out_b = temp_b.path().join("proof_bundle.json");

    let run_once = |out: &Path| {
        let output = bin()
            .args([
                "circle",
                "run",
                "--to",
                "sera-nqh3",
                "--members",
                "alice,bob",
                "--referee",
                "ref",
                "--bundle-out",
                out.to_str().unwrap(),
            ])
            .output()
            .expect("run");
        assert!(output.status.success(), "{:?}", output);
        let bytes = std::fs::read(out).expect("read bundle");
        let sha = {
            use sha2::{Digest, Sha256};
            let mut hasher = Sha256::new();
            hasher.update(&bytes);
            format!("{:x}", hasher.finalize())
        };
        (bytes, sha)
    };

    let (bytes_a, sha_a) = run_once(&out_a);
    let (bytes_b, sha_b) = run_once(&out_b);

    assert_eq!(sha_a, sha_b, "identical args must produce stable sha256");
    assert_eq!(
        bytes_a, bytes_b,
        "identical args must produce identical bytes"
    );
}

// ---------------------------------------------------------------------
// Address parsing: accepts `to:circle:<name>`, `circle:<name>`, bare name
// ---------------------------------------------------------------------

#[test]
fn circle_run_cli_accepts_canonical_to_circle_address() {
    let temp = tempfile::tempdir().unwrap();
    let bundle_out = temp.path().join("b.json");
    let output = bin()
        .args([
            "circle",
            "run",
            "--to",
            "to:circle:hello",
            "--members",
            "a,b",
            "--referee",
            "r",
            "--bundle-out",
            bundle_out.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_circle_run_footer(&stdout, "PASS");
    assert!(
        stdout.contains("\"address\":\"to:circle:hello\""),
        "stdout={stdout}"
    );
}

#[test]
fn circle_run_cli_accepts_short_circle_address_and_bare_name() {
    for arg in ["circle:engineering", "engineering"] {
        let output = bin()
            .args([
                "circle",
                "run",
                "--to",
                arg,
                "--members",
                "a,b",
                "--referee",
                "r",
                "--json",
            ])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "address {arg} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("\"address\":\"to:circle:engineering\""),
            "address {arg} did not normalize: {stdout}"
        );
    }
}

// ---------------------------------------------------------------------
// Negative fixtures
// ---------------------------------------------------------------------

#[test]
fn circle_run_cli_missing_target_emits_usage_error_footer() {
    let output = bin()
        .args(["circle", "run", "--members", "a,b", "--referee", "r"])
        .output()
        .expect("run");
    assert_eq!(output.status.code(), Some(3));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_circle_run_footer(&stdout, "USAGE_ERROR");
    assert!(
        stdout.contains("circle-run: USAGE_ERROR unknown"),
        "stdout={stdout}"
    );
    assert!(stderr.contains("missing required --to"), "stderr={stderr}");
}

#[test]
fn circle_run_cli_invalid_target_path_separator_emits_usage_error_footer() {
    let output = bin()
        .args([
            "circle",
            "run",
            "--to",
            "../escape",
            "--members",
            "a,b",
            "--referee",
            "r",
        ])
        .output()
        .expect("run");
    assert_eq!(output.status.code(), Some(3));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_circle_run_footer(&stdout, "USAGE_ERROR");
    assert!(stderr.contains("invalid Circle name"), "stderr={stderr}");
}

#[test]
fn circle_run_cli_unsupported_kind_emits_usage_error_footer() {
    let output = bin()
        .args([
            "circle",
            "run",
            "--to",
            "to:agent:bob",
            "--members",
            "a,b",
            "--referee",
            "r",
        ])
        .output()
        .expect("run");
    assert_eq!(output.status.code(), Some(3));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_circle_run_footer(&stdout, "USAGE_ERROR");
    assert!(
        stderr.contains("unsupported channel kind"),
        "stderr={stderr}"
    );
}

#[test]
fn circle_run_cli_duplicate_member_id_emits_usage_error_footer() {
    let output = bin()
        .args([
            "circle",
            "run",
            "--to",
            "sera-nqh3",
            "--members",
            "a,a",
            "--referee",
            "r",
        ])
        .output()
        .expect("run");
    assert_eq!(output.status.code(), Some(3));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_circle_run_footer(&stdout, "USAGE_ERROR");
    assert!(stderr.contains("duplicate member id"), "stderr={stderr}");
}

#[test]
fn circle_run_cli_referee_overlap_emits_usage_error_footer() {
    let output = bin()
        .args([
            "circle",
            "run",
            "--to",
            "sera-nqh3",
            "--members",
            "alice,bob",
            "--referee",
            "alice",
        ])
        .output()
        .expect("run");
    assert_eq!(output.status.code(), Some(3));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_circle_run_footer(&stdout, "USAGE_ERROR");
    assert!(stderr.contains("referee"), "stderr={stderr}");
}

#[test]
fn circle_run_cli_blank_members_emits_usage_error_footer() {
    let output = bin()
        .args([
            "circle",
            "run",
            "--to",
            "sera-nqh3",
            "--members",
            "",
            "--referee",
            "r",
        ])
        .output()
        .expect("run");
    assert_eq!(output.status.code(), Some(3));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_circle_run_footer(&stdout, "USAGE_ERROR");
    assert!(stderr.contains("--members"), "stderr={stderr}");
}

// ---------------------------------------------------------------------
// Footer: every successful run emits `circle-run: PASS <sha256>` and that
// sha256 matches the bytes-on-disk for `--bundle-out`.
// ---------------------------------------------------------------------

#[test]
fn circle_run_cli_footer_sha256_matches_written_bundle_bytes() {
    let temp = tempfile::tempdir().unwrap();
    let bundle_out = temp.path().join("proof_bundle.json");

    let output = bin()
        .args([
            "circle",
            "run",
            "--to",
            "sera-nqh3",
            "--members",
            "alice,bob,carol",
            "--referee",
            "ref",
            "--bundle-out",
            bundle_out.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);

    let footer_sha = stdout
        .lines()
        .rev()
        .find(|line| line.starts_with("circle-run: "))
        .and_then(|line| line.split_whitespace().nth(2))
        .map(str::to_string)
        .expect("footer missing");

    let bytes = std::fs::read(&bundle_out).expect("read bundle");
    let disk_sha = {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        format!("{:x}", hasher.finalize())
    };
    assert_eq!(footer_sha, disk_sha, "footer sha must match disk bytes");
}

// ---------------------------------------------------------------------
// Sanity: roster reflects CLI args (1 lead + N members + 1 referee)
// ---------------------------------------------------------------------

#[test]
fn circle_run_cli_roster_reflects_cli_inputs() {
    let temp = tempfile::tempdir().unwrap();
    let bundle_out = temp.path().join("b.json");

    let output = bin()
        .args([
            "circle",
            "run",
            "--to",
            "sera-nqh3",
            "--members",
            "alpha,beta,gamma",
            "--referee",
            "judge",
            "--bundle-out",
            bundle_out.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(output.status.success());

    let bundle: Value = serde_json::from_slice(&std::fs::read(&bundle_out).unwrap()).unwrap();
    let roster = bundle["roster"].as_array().unwrap();
    let ids: Vec<&str> = roster
        .iter()
        .map(|m| m["participant_id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, vec!["alpha", "beta", "gamma", "judge"]);

    let roles: Vec<&str> = roster.iter().map(|m| m["role"].as_str().unwrap()).collect();
    assert_eq!(roles[0], "Lead");
    assert_eq!(roles[1], "Worker");
    assert_eq!(roles[2], "Critic");
    assert!(roles[3].contains("Referee"));
}
