//! Integration tests for `sera config` verbs (local-only YAML).
//!
//! These never touch the gateway — `SERA_CONFIG_ROOT` is set to a tempdir.

use std::path::Path;

use sera_cli::commands::config::{ConfigGetCommand, ConfigPathCommand, ConfigSetCommand};
use sera_commands::{Command, CommandArgs, CommandContext};

fn args(pairs: &[(&str, &str)]) -> CommandArgs {
    let mut a = CommandArgs::new();
    for (k, v) in pairs {
        a.insert(*k, *v);
    }
    a
}

fn write_manifest(dir: &Path, body: &str) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join("config.yaml"), body).unwrap();
}

#[tokio::test]
async fn config_path_prints_resolved_config_yaml() {
    let dir = tempfile::tempdir().unwrap();
    let result = ConfigPathCommand
        .execute(
            args(&[("config-root", dir.path().to_str().unwrap())]),
            &CommandContext::new(),
        )
        .await
        .unwrap();
    let expected = dir.path().join("config.yaml").display().to_string();
    assert_eq!(result.data, serde_json::json!({"path": expected}));
}

#[tokio::test]
async fn config_get_walks_dotted_path() {
    let dir = tempfile::tempdir().unwrap();
    write_manifest(
        dir.path(),
        "apiVersion: sera.dev/v1\nkind: Instance\nmetadata:\n  name: default\nspec:\n  gateway:\n    mode: local\n  data_dir: /var/lib/sera\n",
    );
    let result = ConfigGetCommand
        .execute(
            args(&[
                ("config-root", dir.path().to_str().unwrap()),
                ("key", "spec.gateway.mode"),
            ]),
            &CommandContext::new(),
        )
        .await
        .unwrap();
    assert_eq!(result.data, serde_json::json!("local"));
}

#[tokio::test]
async fn config_set_round_trips_through_get() {
    let dir = tempfile::tempdir().unwrap();
    write_manifest(
        dir.path(),
        "apiVersion: sera.dev/v1\nkind: Instance\nmetadata:\n  name: default\nspec:\n  gateway:\n    mode: local\n  data_dir: /tmp\n",
    );

    ConfigSetCommand
        .execute(
            args(&[
                ("config-root", dir.path().to_str().unwrap()),
                ("key", "spec.gateway.mode"),
                ("value", "remote"),
            ]),
            &CommandContext::new(),
        )
        .await
        .unwrap();

    let result = ConfigGetCommand
        .execute(
            args(&[
                ("config-root", dir.path().to_str().unwrap()),
                ("key", "spec.gateway.mode"),
            ]),
            &CommandContext::new(),
        )
        .await
        .unwrap();
    assert_eq!(result.data, serde_json::json!("remote"));
}

#[tokio::test]
async fn config_get_missing_key_errors() {
    let dir = tempfile::tempdir().unwrap();
    write_manifest(dir.path(), "spec:\n  gateway:\n    mode: local\n");
    let err = ConfigGetCommand
        .execute(
            args(&[
                ("config-root", dir.path().to_str().unwrap()),
                ("key", "spec.does.not.exist"),
            ]),
            &CommandContext::new(),
        )
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("not found"), "got: {err}");
}
