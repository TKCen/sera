//! End-to-end integration test for `sera init --from-file`.
//!
//! Drives the [`InitCommand`] non-interactively, asserts files land under the
//! tempdir-scoped config root, and confirms the secret-canary never appears
//! in the persisted YAML.

use std::sync::Arc;

use sera_cli::commands::InitCommand;
use sera_commands::{Command, CommandArgs, CommandContext};
use sera_config::provider_wizard::test_support::{MockPrompter, MockSecretStore};

fn args(pairs: &[(&str, &str)]) -> CommandArgs {
    let mut a = CommandArgs::new();
    for (k, v) in pairs {
        a.insert(*k, *v);
    }
    a
}

const CANARY: &str = "CANARY-INIT-FROM-FILE-DO-NOT-LEAK";

#[tokio::test]
async fn from_file_writes_instance_and_provider_under_config_root() {
    let dir = tempfile::tempdir().unwrap();

    // Manifest references a secret via env var.  The bead spec says we must
    // error out when the env var is unset; we set it here so this happy-path
    // test succeeds, then below we assert the canary value never lands in
    // any persisted YAML.
    let env_var = "SERA_SECRET_PROVIDERS_TEST_API_KEY";
    let saved = std::env::var(env_var).ok();
    unsafe { std::env::set_var(env_var, CANARY) };

    let manifest = r#"
apiVersion: sera.dev/v1
kind: Instance
metadata:
  name: default
spec:
  gateway:
    mode: local
  data_dir: /var/lib/sera
---
apiVersion: sera.dev/v1
kind: Provider
metadata:
  name: test-provider
spec:
  kind: openai-compatible
  base_url: "http://localhost:9999/v1"
  api_key:
    secret: providers/test/api-key
"#;
    let manifest_path = dir.path().join("bootstrap.yaml");
    std::fs::write(&manifest_path, manifest).unwrap();

    let prompter = Arc::new(MockPrompter::new(vec![], vec![]));
    let secrets = Arc::new(MockSecretStore::default());
    let cmd = InitCommand::with_dependencies(prompter, secrets);

    let result = cmd
        .execute(
            args(&[
                ("config-root", dir.path().to_str().unwrap()),
                ("from-file", manifest_path.to_str().unwrap()),
            ]),
            &CommandContext::new(),
        )
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0);

    // Restore env state before assertions so a panic doesn't leak the var.
    match saved {
        Some(v) => unsafe { std::env::set_var(env_var, v) },
        None => unsafe { std::env::remove_var(env_var) },
    }

    // Files landed under the config root.
    let cfg_path = dir.path().join("config.yaml");
    let prov_path = dir.path().join("providers").join("test-provider.yaml");
    assert!(cfg_path.exists(), "config.yaml not written");
    assert!(prov_path.exists(), "provider manifest not written");

    // Canary check — the resolved secret VALUE must never appear on disk.
    // Only the secret REFERENCE (path) should land in YAML.
    let cfg_raw = std::fs::read_to_string(&cfg_path).unwrap();
    let prov_raw = std::fs::read_to_string(&prov_path).unwrap();
    assert!(
        !cfg_raw.contains(CANARY),
        "raw secret leaked into config.yaml:\n{cfg_raw}"
    );
    assert!(
        !prov_raw.contains(CANARY),
        "raw secret leaked into provider manifest:\n{prov_raw}"
    );

    // The provider manifest should still carry the secret REFERENCE.
    assert!(prov_raw.contains("providers/test/api-key"));
}

#[tokio::test]
async fn from_file_with_unresolved_secret_errors() {
    let dir = tempfile::tempdir().unwrap();

    // Make sure the env var is NOT set so resolution fails.
    let env_var = "SERA_SECRET_PROVIDERS_MISSING_API_KEY";
    let saved = std::env::var(env_var).ok();
    unsafe { std::env::remove_var(env_var) };

    let manifest = r#"
apiVersion: sera.dev/v1
kind: Instance
metadata:
  name: default
spec:
  gateway:
    mode: local
---
apiVersion: sera.dev/v1
kind: Provider
metadata:
  name: missing
spec:
  kind: openai-compatible
  base_url: "http://example/v1"
  api_key:
    secret: providers/missing/api-key
"#;
    let manifest_path = dir.path().join("bootstrap.yaml");
    std::fs::write(&manifest_path, manifest).unwrap();

    let prompter = Arc::new(MockPrompter::new(vec![], vec![]));
    let secrets = Arc::new(MockSecretStore::default());
    let cmd = InitCommand::with_dependencies(prompter, secrets);

    let err = cmd
        .execute(
            args(&[
                ("config-root", dir.path().to_str().unwrap()),
                ("from-file", manifest_path.to_str().unwrap()),
            ]),
            &CommandContext::new(),
        )
        .await
        .unwrap_err()
        .to_string();

    if let Some(v) = saved {
        unsafe { std::env::set_var(env_var, v) };
    }

    assert!(
        err.contains("references secret") && err.contains("no env var"),
        "expected unresolved-secret error, got: {err}"
    );
    // Nothing should have been written.
    assert!(!dir.path().join("config.yaml").exists());
}
