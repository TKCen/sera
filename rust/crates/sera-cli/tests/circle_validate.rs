use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

fn write_replay_fixture(dir: &Path) {
    std::fs::write(
        dir.join("summary.json"),
        r#"{
  "run_id": "circle-replay-fixture-run",
  "circle_id": "sera-nqh3-replay-fixture",
  "objective": "Replay captured MiniMax and local Gemma role fixtures into a SERA Circle proof bundle.",
  "success_metric": {
    "kind": "inline",
    "description": "Replay fixture produces a structurally valid mixed-provider CollaborationProofBundle."
  },
  "roster": [],
  "roles": [
    {"role_id":"specifier_minimax","provider_cli":"minimax","model":"MiniMax-M3","session_id":"sess_spec","returncode":0,"duration_ms":101,"role":"cloud specifier"},
    {"role_id":"builder_local_gemma","provider_cli":"custom","model":"gemma4-local","session_id":"sess_build","returncode":0,"duration_ms":102,"role":"local builder"},
    {"role_id":"critic_minimax","provider_cli":"minimax","model":"MiniMax-M3","session_id":"sess_critic","returncode":0,"duration_ms":103,"role":"cloud critic"},
    {"role_id":"referee_local_gemma","provider_cli":"custom","model":"gemma4-local","session_id":"sess_ref","returncode":0,"duration_ms":104,"role":"local referee/integrator"}
  ],
  "budget_snapshot": {"max_iterations":4,"current_usage":410},
  "verdict_type": {"kind":"revision_required","feedback":"Fixture replay preserved referee revision request."}
}
"#,
    )
    .unwrap();

    let roles = [
        (
            "specifier_minimax",
            "minimax",
            "MiniMax-M3",
            "sess_spec",
            "2026-06-29T18:00:01Z",
            "Specification: build the smallest deterministic replay seam.",
        ),
        (
            "builder_local_gemma",
            "custom",
            "gemma4-local",
            "sess_build",
            "2026-06-29T18:00:02Z",
            "Builder: assemble a proof bundle from replay fixtures without provider calls.",
        ),
        (
            "critic_minimax",
            "minimax",
            "MiniMax-M3",
            "sess_critic",
            "2026-06-29T18:00:03Z",
            "Critic: require receipt-anchored provider evidence and subprocess tests.",
        ),
        (
            "referee_local_gemma",
            "custom",
            "gemma4-local",
            "sess_ref",
            "2026-06-29T18:00:04Z",
            "REVISION_REQUIRED: first-class replay works; live run remains a future slice.",
        ),
    ];

    for (role_id, provider_cli, model, session_id, ended_at, answer_cleaned) in roles {
        std::fs::write(
            dir.join(format!("{role_id}.json")),
            format!(
                r#"{{
  "role_id": "{role_id}",
  "provider_cli": "{provider_cli}",
  "model": "{model}",
  "started_at": "2026-06-29T18:00:00Z",
  "ended_at": "{ended_at}",
  "duration_ms": 100,
  "returncode": 0,
  "session_id": "{session_id}",
  "answer_cleaned": "{answer_cleaned}",
  "answer_had_harness_warnings": false
}}
"#
            ),
        )
        .unwrap();
    }
}

#[test]
fn circle_validate_cli_passes_valid_mixed_provider_bundle() {
    let output = Command::new(env!("CARGO_BIN_EXE_sera"))
        .args([
            "circle",
            "validate",
            "--bundle",
            fixture("circle_valid_mixed_provider.json")
                .to_str()
                .unwrap(),
        ])
        .output()
        .expect("run sera circle validate");

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Circle proof bundle PASS"),
        "stdout={stdout}"
    );
    assert!(stdout.contains("circle-validate: PASS "), "stdout={stdout}");
}

#[test]
fn circle_validate_cli_fails_receipt_provider_mismatch() {
    let output = Command::new(env!("CARGO_BIN_EXE_sera"))
        .args([
            "circle",
            "validate",
            "--bundle",
            fixture("circle_receipt_provider_mismatch.json")
                .to_str()
                .unwrap(),
        ])
        .output()
        .expect("run sera circle validate");

    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stdout.contains("circle-validate: FAIL "), "stdout={stdout}");
    assert!(
        stderr.contains("ReceiptProviderEvidenceMismatch"),
        "stderr={stderr}"
    );
}

#[test]
fn circle_validate_cli_bypasses_corrupt_config() {
    let bad_config = std::env::temp_dir().join(format!(
        "sera-bad-config-{}-{}.toml",
        std::process::id(),
        "circle-validate"
    ));
    std::fs::write(&bad_config, "endpoint = [this is invalid toml").unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_sera"))
        .args([
            "--config",
            bad_config.to_str().unwrap(),
            "circle",
            "validate",
            "--bundle",
            fixture("circle_valid_mixed_provider.json")
                .to_str()
                .unwrap(),
        ])
        .output()
        .expect("run sera circle validate with corrupt config");
    let _ = std::fs::remove_file(&bad_config);

    assert!(
        output.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("circle-validate: PASS "), "stdout={stdout}");
}

#[test]
fn circle_validate_cli_usage_error_footer_when_bundle_is_omitted() {
    let output = Command::new(env!("CARGO_BIN_EXE_sera"))
        .args(["circle", "validate"])
        .output()
        .expect("run sera circle validate without bundle");

    assert_eq!(output.status.code(), Some(3));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stdout.contains("circle-validate: USAGE_ERROR unknown"),
        "stdout={stdout}"
    );
    assert!(
        stderr.contains("missing required --bundle"),
        "stderr={stderr}"
    );
}

#[test]
fn circle_validate_cli_usage_error_footer_when_bundle_value_is_omitted() {
    let output = Command::new(env!("CARGO_BIN_EXE_sera"))
        .args(["circle", "validate", "--bundle"])
        .output()
        .expect("run sera circle validate with value-less bundle flag");

    assert_eq!(output.status.code(), Some(3));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stdout.contains("circle-validate: USAGE_ERROR unknown"),
        "stdout={stdout}"
    );
    assert!(
        stderr.contains("missing required --bundle"),
        "stderr={stderr}"
    );
}

#[test]
fn circle_validate_cli_usage_error_for_missing_bundle_file() {
    let missing = fixture("does-not-exist.json");
    let output = Command::new(env!("CARGO_BIN_EXE_sera"))
        .args(["circle", "validate", "--bundle", missing.to_str().unwrap()])
        .output()
        .expect("run sera circle validate");

    assert_eq!(output.status.code(), Some(3));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stdout.contains("circle-validate: USAGE_ERROR unknown"),
        "stdout={stdout}"
    );
    assert!(
        stderr.contains("failed to read Circle proof bundle"),
        "stderr={stderr}"
    );
}

#[test]
fn circle_replay_cli_generates_valid_bundle_from_fixture_dir() {
    let temp = tempfile::tempdir().unwrap();
    let fixture_dir = temp.path().join("fixtures");
    std::fs::create_dir(&fixture_dir).unwrap();
    write_replay_fixture(&fixture_dir);
    let bundle_out = temp.path().join("out").join("proof_bundle.json");

    let replay = Command::new(env!("CARGO_BIN_EXE_sera"))
        .args([
            "circle",
            "replay",
            "--fixture-dir",
            fixture_dir.to_str().unwrap(),
            "--bundle-out",
            bundle_out.to_str().unwrap(),
            "--json",
        ])
        .output()
        .expect("run sera circle replay");

    assert!(
        replay.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&replay.stdout),
        String::from_utf8_lossy(&replay.stderr)
    );
    let replay_stdout = String::from_utf8_lossy(&replay.stdout);
    assert!(
        replay_stdout.contains("circle-replay: PASS "),
        "stdout={replay_stdout}"
    );
    assert!(bundle_out.exists(), "bundle was not written");

    let validate = Command::new(env!("CARGO_BIN_EXE_sera"))
        .args([
            "circle",
            "validate",
            "--bundle",
            bundle_out.to_str().unwrap(),
            "--json",
        ])
        .output()
        .expect("validate replayed bundle");
    assert!(
        validate.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&validate.stdout),
        String::from_utf8_lossy(&validate.stderr)
    );
    let validate_stdout = String::from_utf8_lossy(&validate.stdout);
    assert!(
        validate_stdout.contains("circle-validate: PASS "),
        "stdout={validate_stdout}"
    );
}

#[test]
fn circle_replay_cli_resolves_relative_artifact_metadata_under_fixture_dir() {
    let temp = tempfile::tempdir().unwrap();
    let fixture_dir = temp.path().join("fixtures");
    std::fs::create_dir(&fixture_dir).unwrap();
    write_replay_fixture(&fixture_dir);

    let metadata_dir = fixture_dir.join("metadata");
    std::fs::create_dir(&metadata_dir).unwrap();
    std::fs::write(
        metadata_dir.join("proof_bundle.json"),
        serde_json::to_vec_pretty(&json!({
            "run_id": "relative-artifact-run",
            "circle_id": "relative-artifact-circle",
            "objective": "Metadata came from a relative artifact path under fixture_dir.",
            "success_metric": {
                "kind": "inline",
                "description": "relative artifact metadata resolved"
            }
        }))
        .unwrap(),
    )
    .unwrap();

    let summary_path = fixture_dir.join("summary.json");
    let mut summary: Value =
        serde_json::from_slice(&std::fs::read(&summary_path).unwrap()).unwrap();
    for field in [
        "run_id",
        "circle_id",
        "objective",
        "success_metric",
        "budget_snapshot",
    ] {
        summary.as_object_mut().unwrap().remove(field);
    }
    summary["artifact"] = json!("metadata/proof_bundle.json");
    std::fs::write(&summary_path, serde_json::to_vec_pretty(&summary).unwrap()).unwrap();

    let bundle_out = temp.path().join("out").join("proof_bundle.json");
    let output = Command::new(env!("CARGO_BIN_EXE_sera"))
        .current_dir(temp.path())
        .args([
            "circle",
            "replay",
            "--fixture-dir",
            fixture_dir.to_str().unwrap(),
            "--bundle-out",
            bundle_out.to_str().unwrap(),
            "--json",
        ])
        .output()
        .expect("run sera circle replay with relative artifact metadata");

    assert!(
        output.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("relative-artifact-run"), "stdout={stdout}");
    assert!(stdout.contains("circle-replay: PASS "), "stdout={stdout}");
}

#[test]
fn circle_replay_cli_treats_lm_studio_provider_as_local() {
    let temp = tempfile::tempdir().unwrap();
    let fixture_dir = temp.path().join("fixtures");
    std::fs::create_dir(&fixture_dir).unwrap();
    write_replay_fixture(&fixture_dir);

    for path in [
        fixture_dir.join("summary.json"),
        fixture_dir.join("builder_local_gemma.json"),
        fixture_dir.join("referee_local_gemma.json"),
    ] {
        let text = std::fs::read_to_string(&path).unwrap();
        std::fs::write(
            &path,
            text.replace(
                "\"provider_cli\":\"custom\"",
                "\"provider_cli\":\"lm-studio\"",
            )
            .replace(
                "\"provider_cli\": \"custom\"",
                "\"provider_cli\": \"lm-studio\"",
            ),
        )
        .unwrap();
    }

    let bundle_out = temp.path().join("proof_bundle.json");
    let output = Command::new(env!("CARGO_BIN_EXE_sera"))
        .args([
            "circle",
            "replay",
            "--fixture-dir",
            fixture_dir.to_str().unwrap(),
            "--bundle-out",
            bundle_out.to_str().unwrap(),
        ])
        .output()
        .expect("run sera circle replay with lm-studio fixtures");

    assert!(
        output.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("circle-replay: PASS "), "stdout={stdout}");

    let bundle: Value = serde_json::from_slice(&std::fs::read(&bundle_out).unwrap()).unwrap();
    let provider = bundle["execution_receipts"][1]["parameters"]["provider"]
        .as_str()
        .unwrap();
    assert_eq!(provider, "local-lmstudio");
}

#[test]
fn circle_replay_cli_bypasses_corrupt_config() {
    let temp = tempfile::tempdir().unwrap();
    let fixture_dir = temp.path().join("fixtures");
    std::fs::create_dir(&fixture_dir).unwrap();
    write_replay_fixture(&fixture_dir);
    let bundle_out = temp.path().join("proof_bundle.json");
    let bad_config = temp.path().join("bad.toml");
    std::fs::write(&bad_config, "endpoint = [this is invalid toml").unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_sera"))
        .args([
            "--config",
            bad_config.to_str().unwrap(),
            "circle",
            "replay",
            "--fixture-dir",
            fixture_dir.to_str().unwrap(),
            "--bundle-out",
            bundle_out.to_str().unwrap(),
        ])
        .output()
        .expect("run sera circle replay with corrupt config");

    assert!(
        output.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("circle-replay: PASS "), "stdout={stdout}");
}

#[test]
fn circle_replay_cli_usage_error_footer_when_fixture_dir_is_omitted() {
    let output = Command::new(env!("CARGO_BIN_EXE_sera"))
        .args(["circle", "replay"])
        .output()
        .expect("run sera circle replay without fixture dir");

    assert_eq!(output.status.code(), Some(3));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stdout.contains("circle-replay: USAGE_ERROR unknown"),
        "stdout={stdout}"
    );
    assert!(
        stderr.contains("missing required --fixture-dir"),
        "stderr={stderr}"
    );
}

#[test]
fn circle_replay_cli_usage_error_footer_when_bundle_out_is_omitted() {
    let temp = tempfile::tempdir().unwrap();
    let fixture_dir = temp.path().join("fixtures");
    std::fs::create_dir(&fixture_dir).unwrap();
    write_replay_fixture(&fixture_dir);

    let output = Command::new(env!("CARGO_BIN_EXE_sera"))
        .args([
            "circle",
            "replay",
            "--fixture-dir",
            fixture_dir.to_str().unwrap(),
        ])
        .output()
        .expect("run sera circle replay without bundle out");

    assert_eq!(output.status.code(), Some(3));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stdout.contains("circle-replay: USAGE_ERROR unknown"),
        "stdout={stdout}"
    );
    assert!(
        stderr.contains("missing required --bundle-out"),
        "stderr={stderr}"
    );
}
