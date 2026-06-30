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
fn circle_replay_cli_ignores_colocated_metadata_when_summary_is_complete() {
    let temp = tempfile::tempdir().unwrap();
    let fixture_dir = temp.path().join("fixtures");
    std::fs::create_dir(&fixture_dir).unwrap();
    write_replay_fixture(&fixture_dir);

    let summary_path = fixture_dir.join("summary.json");
    let mut summary: Value =
        serde_json::from_slice(&std::fs::read(&summary_path).unwrap()).unwrap();
    summary.as_object_mut().unwrap().remove("budget_snapshot");
    std::fs::write(&summary_path, serde_json::to_vec_pretty(&summary).unwrap()).unwrap();
    std::fs::write(fixture_dir.join("proof_bundle.json"), b"not valid json").unwrap();

    let bundle_out = temp.path().join("proof_bundle.json");
    let output = Command::new(env!("CARGO_BIN_EXE_sera"))
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
        .expect("run sera circle replay with stale colocated proof bundle");

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

    for path in [
        fixture_dir.join("builder_local_gemma.json"),
        fixture_dir.join("referee_local_gemma.json"),
    ] {
        let mut fixture: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        fixture["provider"] = json!("lm-studio");
        std::fs::write(&path, serde_json::to_vec_pretty(&fixture).unwrap()).unwrap();
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
fn circle_replay_cli_does_not_infer_local_from_open_source_model_name() {
    let temp = tempfile::tempdir().unwrap();
    let fixture_dir = temp.path().join("fixtures");
    std::fs::create_dir(&fixture_dir).unwrap();
    write_replay_fixture(&fixture_dir);

    for path in [
        fixture_dir.join("builder_local_gemma.json"),
        fixture_dir.join("referee_local_gemma.json"),
    ] {
        let mut fixture: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        fixture["provider_cli"] = json!("openrouter");
        fixture["model"] = json!("mistral-large-cloud");
        std::fs::write(&path, serde_json::to_vec_pretty(&fixture).unwrap()).unwrap();
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
        .expect("run sera circle replay with cloud open-source model names");

    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stdout.contains("circle-replay: FAIL "), "stdout={stdout}");
    assert!(
        stderr.contains("MissingMixedProviderEvidence"),
        "stderr={stderr}"
    );

    let bundle: Value = serde_json::from_slice(&std::fs::read(&bundle_out).unwrap()).unwrap();
    let provider = bundle["execution_receipts"][1]["parameters"]["provider"]
        .as_str()
        .unwrap();
    assert_eq!(provider, "openrouter");
}

#[test]
fn circle_replay_cli_uses_summary_provider_evidence() {
    let temp = tempfile::tempdir().unwrap();
    let fixture_dir = temp.path().join("fixtures");
    std::fs::create_dir(&fixture_dir).unwrap();
    write_replay_fixture(&fixture_dir);

    let summary_path = fixture_dir.join("summary.json");
    let mut summary: Value =
        serde_json::from_slice(&std::fs::read(&summary_path).unwrap()).unwrap();
    let roles = summary["roles"].as_array_mut().unwrap();
    for index in [1usize, 3usize] {
        roles[index].as_object_mut().unwrap().remove("provider_cli");
        roles[index]["provider"] = json!("lm-studio");
    }
    std::fs::write(&summary_path, serde_json::to_vec_pretty(&summary).unwrap()).unwrap();

    for path in [
        fixture_dir.join("builder_local_gemma.json"),
        fixture_dir.join("referee_local_gemma.json"),
    ] {
        let mut fixture: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        fixture.as_object_mut().unwrap().remove("provider_cli");
        std::fs::write(&path, serde_json::to_vec_pretty(&fixture).unwrap()).unwrap();
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
        .expect("run sera circle replay with summary provider evidence");

    assert!(
        output.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let bundle: Value = serde_json::from_slice(&std::fs::read(&bundle_out).unwrap()).unwrap();
    let provider = bundle["execution_receipts"][1]["parameters"]["provider"]
        .as_str()
        .unwrap();
    assert_eq!(provider, "local-lmstudio");
}

#[test]
fn circle_replay_cli_rejects_missing_provider_evidence() {
    let temp = tempfile::tempdir().unwrap();
    let fixture_dir = temp.path().join("fixtures");
    std::fs::create_dir(&fixture_dir).unwrap();
    write_replay_fixture(&fixture_dir);

    let summary_path = fixture_dir.join("summary.json");
    let mut summary: Value =
        serde_json::from_slice(&std::fs::read(&summary_path).unwrap()).unwrap();
    let roles = summary["roles"].as_array_mut().unwrap();
    roles[0]["provider_cli"] = json!("   ");
    roles[2]["provider_cli"] = json!("custom");
    roles[2]["model"] = json!("gemma4-local");
    std::fs::write(&summary_path, serde_json::to_vec_pretty(&summary).unwrap()).unwrap();

    let specifier_path = fixture_dir.join("specifier_minimax.json");
    let mut specifier: Value =
        serde_json::from_slice(&std::fs::read(&specifier_path).unwrap()).unwrap();
    specifier["provider_cli"] = json!("\t ");
    std::fs::write(
        &specifier_path,
        serde_json::to_vec_pretty(&specifier).unwrap(),
    )
    .unwrap();

    let critic_path = fixture_dir.join("critic_minimax.json");
    let mut critic: Value = serde_json::from_slice(&std::fs::read(&critic_path).unwrap()).unwrap();
    critic["provider_cli"] = json!("custom");
    critic["model"] = json!("gemma4-local");
    std::fs::write(&critic_path, serde_json::to_vec_pretty(&critic).unwrap()).unwrap();

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
        .expect("run sera circle replay with missing provider evidence");

    assert_eq!(output.status.code(), Some(3));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stdout.contains("circle-replay: USAGE_ERROR unknown"),
        "stdout={stdout}"
    );
    assert!(
        stderr.contains("provider or provider_cli evidence"),
        "stderr={stderr}"
    );
}

#[test]
fn circle_replay_cli_rejects_conflicting_provider_evidence() {
    let temp = tempfile::tempdir().unwrap();
    let fixture_dir = temp.path().join("fixtures");
    std::fs::create_dir(&fixture_dir).unwrap();
    write_replay_fixture(&fixture_dir);

    let specifier_path = fixture_dir.join("specifier_minimax.json");
    let mut specifier: Value =
        serde_json::from_slice(&std::fs::read(&specifier_path).unwrap()).unwrap();
    specifier["provider"] = json!("local-lmstudio");
    specifier["provider_cli"] = json!("minimax");
    std::fs::write(
        &specifier_path,
        serde_json::to_vec_pretty(&specifier).unwrap(),
    )
    .unwrap();

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
        .expect("run sera circle replay with conflicting provider evidence");

    assert_eq!(output.status.code(), Some(3));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stdout.contains("circle-replay: USAGE_ERROR unknown"),
        "stdout={stdout}"
    );
    assert!(
        stderr.contains("conflicting provider evidence"),
        "stderr={stderr}"
    );
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

#[test]
fn circle_run_cli_captures_replays_validates_and_preserves_peer_challenges() {
    let temp = tempfile::tempdir().unwrap();
    let spec = temp.path().join("circle-run-spec.json");
    let capture_dir = temp.path().join("capture");
    let bundle_out = temp.path().join("out").join("proof_bundle.json");
    std::fs::write(
        &spec,
        r#"{
  "run_id": "circle-run-fixture-run",
  "circle_id": "sera-nqh3-run-fixture",
  "objective": "Run a configured Circle roster through capture, replay, validate, prove.",
  "success_metric": {
    "kind": "inline",
    "description": "first-class run writes a valid proof bundle"
  },
  "roles": [
    {"role_id":"specifier_minimax","provider_cli":"minimax","model":"MiniMax-M3","session_id":"sess_spec","returncode":0,"duration_ms":101,"role":"cloud specifier","ended_at":"2026-06-30T08:00:01Z","answer_cleaned":"Specification: preserve configured run receipts."},
    {"role_id":"builder_local_gemma","provider_cli":"custom","model":"gemma4-local","session_id":"sess_build","returncode":0,"duration_ms":102,"role":"local builder","ended_at":"2026-06-30T08:00:02Z","answer_cleaned":"Builder: materialize role fixtures from run spec."},
    {"role_id":"critic_minimax","provider_cli":"minimax","model":"MiniMax-M3","session_id":"sess_critic","returncode":0,"duration_ms":103,"role":"cloud critic","ended_at":"2026-06-30T08:00:03Z","answer_cleaned":"Critic: demand structured peer challenge evidence."},
    {"role_id":"referee_local_gemma","provider_cli":"custom","model":"gemma4-local","session_id":"sess_ref","returncode":0,"duration_ms":104,"role":"local referee/rule process","ended_at":"2026-06-30T08:00:04Z","answer_cleaned":"APPROVED: run preserved peer challenges and validation receipts."}
  ],
  "peer_challenges": [
    {
      "challenge_id": "challenge_critic_001",
      "challenger": "critic_minimax",
      "target_entry_id": 2,
      "claim": "The builder's fixture materialization is enough proof by itself.",
      "challenge": "The run must also validate the generated proof bundle and preserve dissent.",
      "evidence": ["circle-run stdout footer", "generated proof_bundle.json"],
      "severity": "high",
      "response_by": "referee_local_gemma",
      "disposition": "resolved"
    }
  ],
  "budget_snapshot": {"max_iterations":4,"current_usage":4},
  "verdict_type": {"kind":"approved"}
}
"#,
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_sera"))
        .args([
            "circle",
            "run",
            "--spec",
            spec.to_str().unwrap(),
            "--capture-dir",
            capture_dir.to_str().unwrap(),
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
    assert!(stdout.contains("circle-run: PASS "), "stdout={stdout}");
    assert!(stdout.contains("\"peer_challenges\":1"), "stdout={stdout}");
    assert!(capture_dir.join("summary.json").exists());
    assert!(capture_dir.join("critic_minimax.json").exists());
    assert!(bundle_out.exists(), "bundle was not written");

    let bundle: Value = serde_json::from_slice(&std::fs::read(&bundle_out).unwrap()).unwrap();
    assert_eq!(bundle["peer_challenges"].as_array().unwrap().len(), 1);
    assert_eq!(
        bundle["peer_challenges"][0]["disposition"]
            .as_str()
            .unwrap(),
        "resolved"
    );
}

#[test]
fn circle_run_cli_rejects_unsafe_role_id_path_component() {
    let temp = tempfile::tempdir().unwrap();
    let spec = temp.path().join("bad-role-id.json");
    let capture_dir = temp.path().join("capture");
    let bundle_out = temp.path().join("proof_bundle.json");
    std::fs::write(
        &spec,
        r#"{
  "run_id": "unsafe-role-run",
  "circle_id": "unsafe-role-circle",
  "objective": "Reject unsafe role ids.",
  "success_metric": {"kind":"inline","description":"unsafe role ids fail before file writes"},
  "roles": [
    {"role_id":"../escape","provider_cli":"minimax","model":"MiniMax-M3","ended_at":"2026-06-30T08:00:01Z","answer_cleaned":"This must not be written outside capture_dir."}
  ],
  "verdict_type": {"kind":"approved"}
}
"#,
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_sera"))
        .args([
            "circle",
            "run",
            "--spec",
            spec.to_str().unwrap(),
            "--capture-dir",
            capture_dir.to_str().unwrap(),
            "--bundle-out",
            bundle_out.to_str().unwrap(),
        ])
        .output()
        .expect("run sera circle run with unsafe role id");

    assert_eq!(output.status.code(), Some(3));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stdout.contains("circle-run: USAGE_ERROR unknown"),
        "stdout={stdout}"
    );
    assert!(stderr.contains("unsafe role_id"), "stderr={stderr}");
    assert!(
        !temp.path().join("escape.json").exists(),
        "unsafe role id wrote outside capture_dir"
    );
}

#[test]
fn circle_run_cli_rejects_reserved_role_id_basename() {
    let temp = tempfile::tempdir().unwrap();
    let spec = temp.path().join("reserved-role-id.json");
    let capture_dir = temp.path().join("capture");
    let bundle_out = temp.path().join("proof_bundle.json");
    std::fs::write(
        &spec,
        r#"{
  "run_id": "reserved-role-run",
  "circle_id": "reserved-role-circle",
  "objective": "Reject reserved fixture basenames.",
  "success_metric": {"kind":"inline","description":"reserved role ids fail before fixture corruption"},
  "roles": [
    {"role_id":"Summary","provider_cli":"minimax","model":"MiniMax-M3","ended_at":"2026-06-30T08:00:01Z","answer_cleaned":"This must not collide with summary.json."}
  ],
  "verdict_type": {"kind":"approved"}
}
"#,
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_sera"))
        .args([
            "circle",
            "run",
            "--spec",
            spec.to_str().unwrap(),
            "--capture-dir",
            capture_dir.to_str().unwrap(),
            "--bundle-out",
            bundle_out.to_str().unwrap(),
        ])
        .output()
        .expect("run sera circle run with reserved role id");

    assert_eq!(output.status.code(), Some(3));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stdout.contains("circle-run: USAGE_ERROR unknown"),
        "stdout={stdout}"
    );
    assert!(stderr.contains("reserved role_id"), "stderr={stderr}");
}

#[test]
fn circle_run_cli_rejects_duplicate_role_ids() {
    let temp = tempfile::tempdir().unwrap();
    let spec = temp.path().join("duplicate-role-id.json");
    let capture_dir = temp.path().join("capture");
    let bundle_out = temp.path().join("proof_bundle.json");
    std::fs::write(
        &spec,
        r#"{
  "run_id": "duplicate-role-run",
  "circle_id": "duplicate-role-circle",
  "objective": "Reject duplicate role ids.",
  "success_metric": {"kind":"inline","description":"duplicate role ids fail before overwrite"},
  "roles": [
    {"role_id":"critic","provider_cli":"minimax","model":"MiniMax-M3","ended_at":"2026-06-30T08:00:01Z","answer_cleaned":"first"},
    {"role_id":"Critic","provider_cli":"custom","model":"gemma4-local","ended_at":"2026-06-30T08:00:02Z","answer_cleaned":"second"}
  ],
  "verdict_type": {"kind":"approved"}
}
"#,
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_sera"))
        .args([
            "circle",
            "run",
            "--spec",
            spec.to_str().unwrap(),
            "--capture-dir",
            capture_dir.to_str().unwrap(),
            "--bundle-out",
            bundle_out.to_str().unwrap(),
        ])
        .output()
        .expect("run sera circle run with duplicate role id");

    assert_eq!(output.status.code(), Some(3));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stdout.contains("circle-run: USAGE_ERROR unknown"),
        "stdout={stdout}"
    );
    assert!(stderr.contains("duplicate role_id"), "stderr={stderr}");
}

#[test]
fn circle_run_cli_does_not_overwrite_reused_capture_dir_on_late_spec_error() {
    let temp = tempfile::tempdir().unwrap();
    let spec = temp.path().join("late-error-role-id.json");
    let capture_dir = temp.path().join("capture");
    std::fs::create_dir(&capture_dir).unwrap();
    let old_role = capture_dir.join("critic.json");
    std::fs::write(&old_role, r#"{"role_id":"critic","answer_cleaned":"old"}"#).unwrap();
    std::fs::write(
        capture_dir.join("summary.json"),
        r#"{"roles":[{"role_id":"critic"}]}"#,
    )
    .unwrap();
    let bundle_out = temp.path().join("proof_bundle.json");
    std::fs::write(
        &spec,
        r#"{
  "run_id": "late-error-role-run",
  "circle_id": "late-error-role-circle",
  "objective": "Do not partially overwrite reused capture dirs on invalid specs.",
  "success_metric": {"kind":"inline","description":"late duplicate should not write first fixture"},
  "roles": [
    {"role_id":"critic","provider_cli":"minimax","model":"MiniMax-M3","ended_at":"2026-06-30T08:00:01Z","answer_cleaned":"new content must not be written"},
    {"role_id":"Critic","provider_cli":"custom","model":"gemma4-local","ended_at":"2026-06-30T08:00:02Z","answer_cleaned":"duplicate fails after first fixture was built in memory"}
  ],
  "verdict_type": {"kind":"approved"}
}
"#,
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_sera"))
        .args([
            "circle",
            "run",
            "--spec",
            spec.to_str().unwrap(),
            "--capture-dir",
            capture_dir.to_str().unwrap(),
            "--bundle-out",
            bundle_out.to_str().unwrap(),
        ])
        .output()
        .expect("run sera circle run with late spec error over reused capture dir");

    assert_eq!(output.status.code(), Some(3));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stdout.contains("circle-run: USAGE_ERROR unknown"),
        "stdout={stdout}"
    );
    assert!(stderr.contains("duplicate role_id"), "stderr={stderr}");
    let role_contents = std::fs::read_to_string(&old_role).unwrap();
    assert_eq!(
        role_contents, r#"{"role_id":"critic","answer_cleaned":"old"}"#,
        "invalid spec partially overwrote reused capture fixture"
    );
}

#[test]
fn circle_run_cli_ignores_explicit_artifact_replay_metadata() {
    let temp = tempfile::tempdir().unwrap();
    let spec = temp.path().join("artifact-no-verdict-run-spec.json");
    let capture_dir = temp.path().join("capture");
    std::fs::create_dir(&capture_dir).unwrap();
    let bundle_out = temp.path().join("proof_bundle.json");
    std::fs::write(
        capture_dir.join("stale-proof.json"),
        r#"{
  "run_id": "artifact-stale-run",
  "circle_id": "artifact-stale-circle",
  "objective": "stale artifact metadata must not be reused",
  "success_metric": {"kind":"inline","description":"stale artifact"},
  "verdict": {"reviewer":"stale","timestamp":"2026-06-30T08:00:00Z","verdict_type":{"kind":"approved"},"rationale":"stale artifact"}
}
"#,
    )
    .unwrap();
    std::fs::write(
        &spec,
        r#"{
  "run_id": "circle-run-no-artifact-verdict",
  "circle_id": "sera-nqh3-no-artifact-verdict",
  "objective": "Run must not borrow explicit artifact metadata.",
  "success_metric": {"kind":"inline","description":"artifact metadata should be stripped"},
  "artifact": "stale-proof.json",
  "roles": [
    {"role_id":"specifier_minimax","provider_cli":"minimax","model":"MiniMax-M3","session_id":"sess_spec","returncode":0,"duration_ms":101,"role":"cloud specifier","ended_at":"2026-06-30T08:00:01Z","answer_cleaned":"Specification: no verdict supplied."},
    {"role_id":"builder_local_gemma","provider_cli":"custom","model":"gemma4-local","session_id":"sess_build","returncode":0,"duration_ms":102,"role":"local builder","ended_at":"2026-06-30T08:00:02Z","answer_cleaned":"Builder: valid local provider evidence."},
    {"role_id":"critic_minimax","provider_cli":"minimax","model":"MiniMax-M3","session_id":"sess_critic","returncode":0,"duration_ms":103,"role":"cloud critic","ended_at":"2026-06-30T08:00:03Z","answer_cleaned":"Critic: verdict is intentionally absent."},
    {"role_id":"referee_local_gemma","provider_cli":"custom","model":"gemma4-local","session_id":"sess_ref","returncode":0,"duration_ms":104,"role":"local referee/rule process","ended_at":"2026-06-30T08:00:04Z","answer_cleaned":"Referee: no verdict_type field supplied."}
  ]
}
"#,
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_sera"))
        .args([
            "circle",
            "run",
            "--spec",
            spec.to_str().unwrap(),
            "--capture-dir",
            capture_dir.to_str().unwrap(),
            "--bundle-out",
            bundle_out.to_str().unwrap(),
            "--json",
        ])
        .output()
        .expect("run sera circle run with stale explicit artifact metadata");

    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("circle-run: FAIL "), "stdout={stdout}");
    assert!(stdout.contains("MissingVerdict"), "stdout={stdout}");
    let written_summary: serde_json::Value =
        serde_json::from_slice(&std::fs::read(capture_dir.join("summary.json")).unwrap()).unwrap();
    assert!(
        written_summary.get("artifact").is_none(),
        "run summary should strip replay artifact metadata: {written_summary}"
    );
}

#[test]
fn circle_run_cli_ignores_stale_colocated_replay_metadata() {
    let temp = tempfile::tempdir().unwrap();
    let spec = temp.path().join("no-verdict-run-spec.json");
    let capture_dir = temp.path().join("capture");
    std::fs::create_dir(&capture_dir).unwrap();
    let bundle_out = capture_dir.join("proof_bundle.json");
    std::fs::write(
        &bundle_out,
        r#"{
  "run_id": "stale-run",
  "circle_id": "stale-circle",
  "objective": "stale metadata must not be reused",
  "success_metric": {"kind":"inline","description":"stale"},
  "verdict": {"reviewer":"stale","timestamp":"2026-06-30T08:00:00Z","verdict_type":{"kind":"approved"},"rationale":"stale"}
}
"#,
    )
    .unwrap();
    std::fs::write(
        &spec,
        r#"{
  "run_id": "circle-run-no-verdict",
  "circle_id": "sera-nqh3-no-verdict",
  "objective": "Run must not borrow stale verdict metadata.",
  "success_metric": {"kind":"inline","description":"missing verdict should fail"},
  "roles": [
    {"role_id":"specifier_minimax","provider_cli":"minimax","model":"MiniMax-M3","session_id":"sess_spec","returncode":0,"duration_ms":101,"role":"cloud specifier","ended_at":"2026-06-30T08:00:01Z","answer_cleaned":"Specification: no verdict supplied."},
    {"role_id":"builder_local_gemma","provider_cli":"custom","model":"gemma4-local","session_id":"sess_build","returncode":0,"duration_ms":102,"role":"local builder","ended_at":"2026-06-30T08:00:02Z","answer_cleaned":"Builder: valid local provider evidence."},
    {"role_id":"critic_minimax","provider_cli":"minimax","model":"MiniMax-M3","session_id":"sess_critic","returncode":0,"duration_ms":103,"role":"cloud critic","ended_at":"2026-06-30T08:00:03Z","answer_cleaned":"Critic: verdict is intentionally absent."},
    {"role_id":"referee_local_gemma","provider_cli":"custom","model":"gemma4-local","session_id":"sess_ref","returncode":0,"duration_ms":104,"role":"local referee/rule process","ended_at":"2026-06-30T08:00:04Z","answer_cleaned":"Referee: no verdict_type field supplied."}
  ]
}
"#,
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_sera"))
        .args([
            "circle",
            "run",
            "--spec",
            spec.to_str().unwrap(),
            "--capture-dir",
            capture_dir.to_str().unwrap(),
            "--bundle-out",
            bundle_out.to_str().unwrap(),
            "--json",
        ])
        .output()
        .expect("run sera circle run without verdict_type over stale capture dir");

    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("circle-run: FAIL "), "stdout={stdout}");
    assert!(stdout.contains("MissingVerdict"), "stdout={stdout}");
}

#[test]
fn circle_run_cli_usage_error_footer_when_spec_is_omitted() {
    let output = Command::new(env!("CARGO_BIN_EXE_sera"))
        .args(["circle", "run"])
        .output()
        .expect("run sera circle run without spec");

    assert_eq!(output.status.code(), Some(3));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stdout.contains("circle-run: USAGE_ERROR unknown"),
        "stdout={stdout}"
    );
    assert!(
        stderr.contains("missing required --spec"),
        "stderr={stderr}"
    );
}
