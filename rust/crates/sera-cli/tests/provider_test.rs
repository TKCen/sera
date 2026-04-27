//! Integration tests for `sera provider` commands.
//!
//! Uses [`MockPrompter`] and [`MockSecretStore`] from `sera-config` so the
//! tests never touch the real terminal or OS keychain.

use std::sync::Arc;

use sera_cli::commands::provider::{
    ProviderAddCommand, ProviderListCommand, ProviderRemoveCommand,
};
use sera_commands::{Command, CommandArgs, CommandContext};
use sera_config::provider_wizard::test_support::{MockPrompter, MockSecretStore};

fn args(pairs: &[(&str, &str)]) -> CommandArgs {
    let mut a = CommandArgs::new();
    for (k, v) in pairs {
        a.insert(*k, *v);
    }
    a
}

#[tokio::test]
async fn list_includes_registry_and_no_configured_initially() {
    let dir = tempfile::tempdir().unwrap();
    let cmd = ProviderListCommand;
    let result = cmd
        .execute(
            args(&[("config-root", dir.path().to_str().unwrap())]),
            &CommandContext::new(),
        )
        .await
        .unwrap();
    let payload = result.data;
    let registry = payload.get("registry").unwrap().as_array().unwrap();
    assert!(!registry.is_empty(), "registry should never be empty");
    let configured = payload.get("configured").unwrap().as_array().unwrap();
    assert!(configured.is_empty(), "no configured providers expected");
}

#[tokio::test]
async fn add_then_list_shows_configured_provider() {
    let dir = tempfile::tempdir().unwrap();

    // SAFETY: avoid env-leakage interfering with the wizard
    let saved = std::env::var("OPENROUTER_API_KEY").ok();
    unsafe { std::env::remove_var("OPENROUTER_API_KEY") };

    let prompter = Arc::new(MockPrompter::new(
        vec![""], // base_url -> default
        vec![Some("CANARY-DO-NOT-LEAK")],
    ));
    let secrets = Arc::new(MockSecretStore::default());

    let add = ProviderAddCommand::with_dependencies(prompter, secrets.clone());
    let result = add
        .execute(
            args(&[
                ("config-root", dir.path().to_str().unwrap()),
                ("id", "openrouter"),
                ("name", "openrouter-prod"),
            ]),
            &CommandContext::new(),
        )
        .await
        .unwrap();

    if let Some(v) = saved {
        unsafe { std::env::set_var("OPENROUTER_API_KEY", v) };
    }

    assert_eq!(result.exit_code, 0);

    // The YAML must NOT contain the secret canary value.
    let yaml_path = dir.path().join("providers").join("openrouter-prod.yaml");
    let raw = std::fs::read_to_string(&yaml_path).unwrap();
    assert!(
        !raw.contains("CANARY-DO-NOT-LEAK"),
        "secret value leaked into manifest:\n{raw}"
    );
    assert!(raw.contains("kind: Provider"));
    assert!(raw.contains("openrouter-prod"));

    // Secret must have landed in the mock store.
    assert!(!secrets.snapshot().is_empty());

    // List now reports it.
    let list = ProviderListCommand;
    let res = list
        .execute(
            args(&[("config-root", dir.path().to_str().unwrap())]),
            &CommandContext::new(),
        )
        .await
        .unwrap();
    let payload = res.data;
    let configured = payload.get("configured").unwrap().as_array().unwrap();
    assert_eq!(configured.len(), 1);
    assert_eq!(configured[0].as_str(), Some("openrouter-prod"));
}

#[tokio::test]
async fn remove_deletes_yaml_and_keychain_entry() {
    let dir = tempfile::tempdir().unwrap();

    let saved = std::env::var("OPENROUTER_API_KEY").ok();
    unsafe { std::env::remove_var("OPENROUTER_API_KEY") };

    let prompter = Arc::new(MockPrompter::new(vec![""], vec![Some("k")]));
    let secrets = Arc::new(MockSecretStore::default());

    let add = ProviderAddCommand::with_dependencies(prompter, secrets.clone());
    add.execute(
        args(&[
            ("config-root", dir.path().to_str().unwrap()),
            ("id", "openrouter"),
            ("name", "to-delete"),
        ]),
        &CommandContext::new(),
    )
    .await
    .unwrap();

    if let Some(v) = saved {
        unsafe { std::env::set_var("OPENROUTER_API_KEY", v) };
    }

    assert!(!secrets.snapshot().is_empty());

    let rm = ProviderRemoveCommand::with_secrets(secrets.clone());
    rm.execute(
        args(&[
            ("config-root", dir.path().to_str().unwrap()),
            ("name", "to-delete"),
        ]),
        &CommandContext::new(),
    )
    .await
    .unwrap();

    let yaml_path = dir.path().join("providers").join("to-delete.yaml");
    assert!(!yaml_path.exists());
    assert!(secrets.snapshot().is_empty(), "keychain entry must be cleaned up");
}

#[tokio::test]
async fn add_with_unknown_id_errors() {
    let dir = tempfile::tempdir().unwrap();
    let prompter = Arc::new(MockPrompter::new(vec![], vec![]));
    let secrets = Arc::new(MockSecretStore::default());
    let add = ProviderAddCommand::with_dependencies(prompter, secrets);
    let err = add
        .execute(
            args(&[
                ("config-root", dir.path().to_str().unwrap()),
                ("id", "no-such-provider"),
            ]),
            &CommandContext::new(),
        )
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("unknown provider id"), "got: {err}");
}
