use std::path::PathBuf;
use std::process::Command;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
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
            fixture("circle_valid_mixed_provider.json").to_str().unwrap(),
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
