//! `sera init` — first-run bootstrap wizard.
//!
//! Three modes:
//! - interactive (default): 3 questions + provider setup, writes both an
//!   `Instance` manifest and a provider manifest
//! - `--from-file <path>`: ingest a prepared manifest set non-interactively
//! - `--non-interactive`: defaults-only, local-mode bootstrap (CI)
//!
//! All three converge on the same on-disk shape:
//! `<config-root>/config.yaml` (kind: Instance) and, when applicable,
//! `<config-root>/providers/<name>.yaml` (kind: Provider). Provider manifests
//! reuse the same writer as `sera provider add` (`provider_wizard::write_manifest`).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::json;

use sera_commands::{
    Command, CommandArgSchema, CommandArgs, CommandCategory, CommandContext, CommandDescription,
    CommandError, CommandResult,
};
use sera_config::ConfigRoot;
use sera_config::manifest_loader::parse_manifests;
use sera_config::provider_registry::{self, ProviderEntry};
use sera_config::provider_wizard::{
    DialoguerPrompter, KeyringSecretStore, KeyringRef, Prompter, ProviderConfig, SecretStore,
    handler_for, write_manifest,
};
use sera_types::config_manifest::{
    CONFIG_VERSION, RawManifest, ResourceMetadata,
};

/// Keyring service name used for the gateway admin token.
const GATEWAY_KEYRING_SERVICE: &str = "sera-gateway";

fn map_err<E: std::fmt::Display>(e: E) -> CommandError {
    CommandError::Execution(e.to_string())
}

fn config_root_from_args(args: &CommandArgs) -> ConfigRoot {
    args.get("config-root")
        .map(|s| ConfigRoot::new(s.to_string()))
        .unwrap_or_else(ConfigRoot::from_env)
}

fn default_data_dir() -> String {
    // The brief specifies `$XDG_DATA_HOME/sera` via `dirs::data_dir()`.
    dirs::data_dir()
        .map(|d| d.join("sera").display().to_string())
        .unwrap_or_else(|| "/tmp/sera-data".to_string())
}

fn config_yaml_path(root: &ConfigRoot) -> PathBuf {
    root.path().join("config.yaml")
}

fn registry_ids() -> Vec<&'static str> {
    provider_registry::registry().iter().map(|e| e.id).collect()
}

/// Build a `RawManifest` for the top-level Instance and serialise it to YAML.
fn instance_manifest_yaml(spec: serde_json::Value) -> Result<String, CommandError> {
    let manifest = RawManifest {
        api_version: "sera.dev/v1".to_string(),
        kind: "Instance".to_string(),
        metadata: ResourceMetadata {
            name: "default".to_string(),
            labels: Default::default(),
            annotations: Default::default(),
            change_artifact: None,
            shadow: false,
        },
        config_version: CONFIG_VERSION,
        spec,
    };
    serde_yaml::to_string(&manifest).map_err(map_err)
}

/// Write `<root>/config.yaml` from the supplied spec value.
fn write_instance_manifest(
    root: &ConfigRoot,
    spec: serde_json::Value,
) -> Result<PathBuf, CommandError> {
    std::fs::create_dir_all(root.path()).map_err(map_err)?;
    let yaml = instance_manifest_yaml(spec)?;
    let path = config_yaml_path(root);
    std::fs::write(&path, yaml).map_err(map_err)?;
    Ok(path)
}

/// Write `<root>/providers/<name>.yaml` via the L.1 provider wizard helper.
fn write_provider_manifest(
    root: &ConfigRoot,
    name: &str,
    cfg: &ProviderConfig,
) -> Result<PathBuf, CommandError> {
    write_manifest(&root.providers_dir(), name, cfg).map_err(map_err)
}

/// Ask yes/no via the prompter; default is `"no"`.
fn prompt_yes(prompter: &dyn Prompter, label: &str) -> Result<bool, CommandError> {
    let raw = prompter
        .prompt_text(label, Some("no"))
        .map_err(map_err)?;
    let v = raw.trim().to_ascii_lowercase();
    Ok(v == "y" || v == "yes")
}

// ── InitCommand ─────────────────────────────────────────────────────────────

pub struct InitCommand {
    prompter: Arc<dyn Prompter>,
    secrets: Arc<dyn SecretStore>,
}

impl InitCommand {
    pub fn new() -> Self {
        Self {
            prompter: Arc::new(DialoguerPrompter),
            secrets: Arc::new(KeyringSecretStore),
        }
    }

    pub fn with_dependencies(prompter: Arc<dyn Prompter>, secrets: Arc<dyn SecretStore>) -> Self {
        Self { prompter, secrets }
    }
}

impl Default for InitCommand {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Command for InitCommand {
    fn name(&self) -> &str {
        "init"
    }

    fn describe(&self) -> CommandDescription {
        CommandDescription {
            summary: "First-run bootstrap wizard — produces a working SERA config".into(),
            help: "Runs an interactive wizard (gateway mode, data dir, provider) and \
                   writes <config-root>/config.yaml plus a provider manifest. Use \
                   --from-file <path> to ingest a prepared manifest set, or \
                   --non-interactive for a defaults-only local bootstrap."
                .into(),
            category: CommandCategory::System,
        }
    }

    fn argument_schema(&self) -> CommandArgSchema {
        CommandArgSchema(
            clap::Command::new("init")
                .about("Bootstrap wizard")
                .arg(
                    clap::Arg::new("from-file")
                        .long("from-file")
                        .value_name("PATH")
                        .help("Ingest a YAML manifest set non-interactively"),
                )
                .arg(
                    clap::Arg::new("non-interactive")
                        .long("non-interactive")
                        .action(clap::ArgAction::SetTrue)
                        .help("Write defaults-only local config (CI mode)"),
                )
                .arg(
                    clap::Arg::new("force")
                        .long("force")
                        .action(clap::ArgAction::SetTrue)
                        .help("Overwrite an existing config.yaml without prompting"),
                ),
        )
    }

    async fn execute(
        &self,
        args: CommandArgs,
        _ctx: &CommandContext,
    ) -> Result<CommandResult, CommandError> {
        let root = config_root_from_args(&args);
        let force = args.get("force").is_some();
        let non_interactive = args.get("non-interactive").is_some();
        let from_file = args.get("from-file").map(PathBuf::from);

        if non_interactive && from_file.is_some() {
            return Err(CommandError::InvalidArgs(
                "--non-interactive and --from-file are mutually exclusive".into(),
            ));
        }

        // Idempotency check (skipped in non-interactive: that mode is for CI
        // and should be a clean overwrite by definition).
        if config_yaml_path(&root).exists() && !force && !non_interactive {
            // For --from-file we still ask, since the user may not realise
            // a prior run wrote config.yaml.
            let proceed = prompt_yes(
                self.prompter.as_ref(),
                &format!(
                    "{} already exists — overwrite?",
                    config_yaml_path(&root).display()
                ),
            )?;
            if !proceed {
                return Err(CommandError::Execution(
                    "aborted: config.yaml already exists (use --force to overwrite)".into(),
                ));
            }
        }

        if let Some(path) = from_file {
            from_file_bootstrap(&root, &path)
        } else if non_interactive {
            non_interactive_bootstrap(&root)
        } else {
            interactive_wizard(&root, self.prompter.as_ref(), self.secrets.as_ref())
        }
    }
}

// ── Mode implementations ────────────────────────────────────────────────────

pub fn interactive_wizard(
    root: &ConfigRoot,
    prompter: &dyn Prompter,
    secrets: &dyn SecretStore,
) -> Result<CommandResult, CommandError> {
    // Q1: gateway mode.
    let mode_raw = prompter
        .prompt_text(
            "Run gateway locally or connect to a remote one? (local/remote)",
            Some("local"),
        )
        .map_err(map_err)?;
    let mode = mode_raw.trim().to_ascii_lowercase();
    if mode != "local" && mode != "remote" {
        return Err(CommandError::InvalidArgs(format!(
            "expected 'local' or 'remote', got '{mode_raw}'"
        )));
    }

    // Q2 (remote only): endpoint + admin token.
    let mut spec = json!({
        "gateway": { "mode": mode },
        "data_dir": "",
    });

    if mode == "remote" {
        let endpoint = prompter
            .prompt_text("Gateway endpoint?", Some("http://localhost:8080"))
            .map_err(map_err)?;
        let token = prompter
            .prompt_secret("Admin token")
            .map_err(map_err)?
            .ok_or_else(|| {
                CommandError::InvalidArgs("admin token is required for remote mode".into())
            })?;

        let kref = KeyringRef {
            keyring_service: GATEWAY_KEYRING_SERVICE.to_string(),
            keyring_entry: "default".to_string(),
        };
        secrets
            .set(&kref.keyring_service, &kref.keyring_entry, &token)
            .map_err(map_err)?;

        spec["gateway"]["endpoint"] = json!(endpoint);
        spec["gateway"]["admin_token_ref"] = json!({
            "keyring_service": kref.keyring_service,
            "keyring_entry": kref.keyring_entry,
        });
    }

    // Q3: data dir.
    let data_dir = prompter
        .prompt_text("Data directory?", Some(&default_data_dir()))
        .map_err(map_err)?;
    spec["data_dir"] = json!(data_dir);

    let instance_path = write_instance_manifest(root, spec)?;

    // Provider setup: pick a registry id then run the matching handler.
    let ids = registry_ids();
    let label = format!("Pick a provider ({})", ids.join(", "));
    let provider_id = prompter
        .prompt_text(&label, ids.first().copied())
        .map_err(map_err)?;
    let entry: &ProviderEntry = provider_registry::lookup(provider_id.trim()).ok_or_else(|| {
        CommandError::InvalidArgs(format!("unknown provider id: {provider_id}"))
    })?;

    // Instance name for the provider — default to the registry id.
    let provider_name = prompter
        .prompt_text("Provider instance name", Some(entry.id))
        .map_err(map_err)?;

    let handler = handler_for(entry.auth_strategy);
    let cfg = handler
        .run(entry, &provider_name, prompter, secrets)
        .map_err(map_err)?;

    let provider_path = write_provider_manifest(root, &provider_name, &cfg)?;

    println!("Wrote {}", instance_path.display());
    println!("Wrote {}", provider_path.display());
    println!("Configured. Run `sera` to start.");

    Ok(CommandResult::ok(json!({
        "config_path": instance_path.display().to_string(),
        "provider_path": provider_path.display().to_string(),
        "provider_name": provider_name,
        "provider_id": cfg.provider_id,
    })))
}

pub fn from_file_bootstrap(
    root: &ConfigRoot,
    path: &Path,
) -> Result<CommandResult, CommandError> {
    let raw = std::fs::read_to_string(path).map_err(|e| {
        CommandError::Execution(format!(
            "failed to read manifest file '{}': {e}",
            path.display()
        ))
    })?;
    let set = parse_manifests(&raw).map_err(map_err)?;

    if set.instances.len() != 1 {
        return Err(CommandError::InvalidArgs(format!(
            "expected exactly one Instance manifest, found {}",
            set.instances.len()
        )));
    }

    // Validate every referenced secret env var resolves so we fail fast on
    // missing secrets rather than at first runtime call.
    for prov in &set.providers {
        if let Ok(spec) = serde_json::from_value::<sera_types::config_manifest::ProviderSpec>(
            prov.spec.clone(),
        ) && let Some(secret_ref) = &spec.api_key
        {
            let resolved = sera_config::manifest_loader::resolve_secret(&secret_ref.secret);
            if resolved.is_none() {
                return Err(CommandError::Execution(format!(
                    "provider '{}' references secret '{}' but no env var is set",
                    prov.metadata.name, secret_ref.secret
                )));
            }
        }
    }

    // Persist the Instance manifest verbatim to <root>/config.yaml.
    std::fs::create_dir_all(root.path()).map_err(map_err)?;
    let instance = &set.instances[0];
    let instance_yaml = serde_yaml::to_string(&RawManifest {
        api_version: instance.api_version.as_str(),
        kind: instance.kind.to_string(),
        metadata: ResourceMetadata {
            name: instance.metadata.name.clone(),
            labels: instance.metadata.labels.clone(),
            annotations: instance.metadata.annotations.clone(),
            change_artifact: instance.metadata.change_artifact,
            shadow: instance.metadata.shadow,
        },
        config_version: instance.config_version,
        spec: instance.spec.clone(),
    })
    .map_err(map_err)?;
    let instance_path = config_yaml_path(root);
    std::fs::write(&instance_path, instance_yaml).map_err(map_err)?;

    // Persist each Provider manifest verbatim under providers/.
    let providers_dir = root.providers_dir();
    std::fs::create_dir_all(&providers_dir).map_err(map_err)?;
    let mut written_providers = Vec::new();
    for prov in &set.providers {
        let yaml = serde_yaml::to_string(&RawManifest {
            api_version: prov.api_version.as_str(),
            kind: prov.kind.to_string(),
            metadata: ResourceMetadata {
                name: prov.metadata.name.clone(),
                labels: prov.metadata.labels.clone(),
                annotations: prov.metadata.annotations.clone(),
                change_artifact: prov.metadata.change_artifact,
                shadow: prov.metadata.shadow,
            },
            config_version: prov.config_version,
            spec: prov.spec.clone(),
        })
        .map_err(map_err)?;
        let p = providers_dir.join(format!("{}.yaml", prov.metadata.name));
        std::fs::write(&p, yaml).map_err(map_err)?;
        written_providers.push(p.display().to_string());
    }

    println!("Wrote {}", instance_path.display());
    for p in &written_providers {
        println!("Wrote {p}");
    }
    println!("Configured. Run `sera` to start.");

    Ok(CommandResult::ok(json!({
        "config_path": instance_path.display().to_string(),
        "provider_paths": written_providers,
    })))
}

pub fn non_interactive_bootstrap(root: &ConfigRoot) -> Result<CommandResult, CommandError> {
    let spec = json!({
        "gateway": { "mode": "local" },
        "data_dir": default_data_dir(),
    });
    let path = write_instance_manifest(root, spec)?;
    println!("Wrote {}", path.display());
    println!("Configured. Run `sera` to start.");
    Ok(CommandResult::ok(json!({
        "config_path": path.display().to_string(),
    })))
}

#[cfg(test)]
mod tests {
    //! Unit tests for the wizard.
    //!
    //! Uses [`MockPrompter`] / [`MockSecretStore`] from `sera-config` so the
    //! tests never touch a real terminal or OS keychain.

    use super::*;
    use sera_config::provider_wizard::test_support::{MockPrompter, MockSecretStore};

    fn args(pairs: &[(&str, &str)]) -> CommandArgs {
        let mut a = CommandArgs::new();
        for (k, v) in pairs {
            a.insert(*k, *v);
        }
        a
    }

    #[tokio::test]
    async fn interactive_local_mode_writes_instance_and_provider_manifests() {
        let dir = tempfile::tempdir().unwrap();

        // Avoid env-leakage interfering with the api-key handler.
        let saved = std::env::var("OPENROUTER_API_KEY").ok();
        unsafe { std::env::remove_var("OPENROUTER_API_KEY") };

        // Texts (in order):
        //   1. mode -> "local"
        //   2. data_dir -> "" (use default)
        //   3. provider id -> "openrouter"
        //   4. provider name -> "" (default openrouter)
        //   5. base URL -> "" (default openrouter base URL)
        // Secrets:
        //   1. api key -> CANARY
        let prompter = Arc::new(MockPrompter::new(
            vec!["local", "", "openrouter", "", ""],
            vec![Some("CANARY-DO-NOT-LEAK")],
        ));
        let secrets = Arc::new(MockSecretStore::default());

        let cmd = InitCommand::with_dependencies(prompter, secrets.clone());
        let result = cmd
            .execute(
                args(&[("config-root", dir.path().to_str().unwrap())]),
                &CommandContext::new(),
            )
            .await
            .unwrap();

        if let Some(v) = saved {
            unsafe { std::env::set_var("OPENROUTER_API_KEY", v) };
        }

        assert_eq!(result.exit_code, 0);

        let cfg_path = dir.path().join("config.yaml");
        assert!(cfg_path.exists(), "config.yaml must be written");
        let cfg_raw = std::fs::read_to_string(&cfg_path).unwrap();
        assert!(cfg_raw.contains("kind: Instance"));
        assert!(cfg_raw.contains("name: default"));
        assert!(cfg_raw.contains("mode: local"));
        // No remote-only fields.
        assert!(!cfg_raw.contains("admin_token_ref"));
        assert!(!cfg_raw.contains("endpoint"));

        let prov_path = dir.path().join("providers").join("openrouter.yaml");
        assert!(prov_path.exists(), "provider manifest must be written");
        let prov_raw = std::fs::read_to_string(&prov_path).unwrap();
        assert!(prov_raw.contains("kind: Provider"));
        assert!(!prov_raw.contains("CANARY-DO-NOT-LEAK"));

        // Secret landed in the mock store.
        assert!(!secrets.snapshot().is_empty());
    }

    #[tokio::test]
    async fn interactive_remote_mode_persists_admin_token_via_secret_store_only_keyring_ref_in_yaml()
     {
        let dir = tempfile::tempdir().unwrap();

        let saved = std::env::var("OPENAI_API_KEY").ok();
        unsafe { std::env::remove_var("OPENAI_API_KEY") };

        // Texts:
        //   1. mode -> "remote"
        //   2. endpoint -> "" (default http://localhost:8080)
        //   3. data dir -> ""
        //   4. provider id -> "openai"
        //   5. provider name -> ""
        //   6. base URL -> ""
        // Secrets:
        //   1. admin token -> CANARY-ADMIN
        //   2. provider api key -> CANARY-PROVIDER
        let prompter = Arc::new(MockPrompter::new(
            vec!["remote", "", "", "openai", "", ""],
            vec![Some("CANARY-ADMIN-TOKEN"), Some("CANARY-PROVIDER-KEY")],
        ));
        let secrets = Arc::new(MockSecretStore::default());

        let cmd = InitCommand::with_dependencies(prompter, secrets.clone());
        cmd.execute(
            args(&[("config-root", dir.path().to_str().unwrap())]),
            &CommandContext::new(),
        )
        .await
        .unwrap();

        if let Some(v) = saved {
            unsafe { std::env::set_var("OPENAI_API_KEY", v) };
        }

        let cfg_raw = std::fs::read_to_string(dir.path().join("config.yaml")).unwrap();
        assert!(cfg_raw.contains("mode: remote"));
        assert!(cfg_raw.contains("endpoint"));
        assert!(cfg_raw.contains("admin_token_ref"));
        // The token VALUE never appears on disk.
        assert!(!cfg_raw.contains("CANARY-ADMIN-TOKEN"));
        // Confirm the canary lives in the secret store under the gateway service.
        let snap = secrets.snapshot();
        let stored_admin = snap
            .get(&(GATEWAY_KEYRING_SERVICE.to_string(), "default".to_string()))
            .map(String::as_str);
        assert_eq!(stored_admin, Some("CANARY-ADMIN-TOKEN"));
    }

    #[tokio::test]
    async fn from_file_bootstrap_round_trips_instance_plus_two_providers() {
        let dir = tempfile::tempdir().unwrap();

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
  name: lm-studio
spec:
  kind: openai-compatible
  base_url: "http://localhost:1234/v1"
---
apiVersion: sera.dev/v1
kind: Provider
metadata:
  name: ollama
spec:
  kind: openai-compatible
  base_url: "http://localhost:11434/v1"
"#;

        let manifest_path = dir.path().join("manifest.yaml");
        std::fs::write(&manifest_path, manifest).unwrap();

        // No prompter / secrets needed for from-file path; pass empties.
        let prompter = Arc::new(MockPrompter::new(vec![], vec![]));
        let secrets = Arc::new(MockSecretStore::default());

        let cmd = InitCommand::with_dependencies(prompter, secrets);
        cmd.execute(
            args(&[
                ("config-root", dir.path().to_str().unwrap()),
                ("from-file", manifest_path.to_str().unwrap()),
            ]),
            &CommandContext::new(),
        )
        .await
        .unwrap();

        assert!(dir.path().join("config.yaml").exists());
        assert!(dir.path().join("providers").join("lm-studio.yaml").exists());
        assert!(dir.path().join("providers").join("ollama.yaml").exists());

        // Instance file must round-trip through the parser.
        let cfg_raw = std::fs::read_to_string(dir.path().join("config.yaml")).unwrap();
        let reparsed = parse_manifests(&cfg_raw).unwrap();
        assert_eq!(reparsed.instances.len(), 1);
        assert_eq!(reparsed.instances[0].metadata.name, "default");
    }

    #[tokio::test]
    async fn non_interactive_writes_minimal_local_config() {
        let dir = tempfile::tempdir().unwrap();
        let prompter = Arc::new(MockPrompter::new(vec![], vec![]));
        let secrets = Arc::new(MockSecretStore::default());
        let cmd = InitCommand::with_dependencies(prompter, secrets);
        cmd.execute(
            args(&[
                ("config-root", dir.path().to_str().unwrap()),
                ("non-interactive", "true"),
            ]),
            &CommandContext::new(),
        )
        .await
        .unwrap();

        let cfg_raw = std::fs::read_to_string(dir.path().join("config.yaml")).unwrap();
        assert!(cfg_raw.contains("kind: Instance"));
        assert!(cfg_raw.contains("mode: local"));
        // No provider written in non-interactive mode.
        assert!(!dir.path().join("providers").exists());
    }

    #[tokio::test]
    async fn non_interactive_with_from_file_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let prompter = Arc::new(MockPrompter::new(vec![], vec![]));
        let secrets = Arc::new(MockSecretStore::default());
        let cmd = InitCommand::with_dependencies(prompter, secrets);
        let err = cmd
            .execute(
                args(&[
                    ("config-root", dir.path().to_str().unwrap()),
                    ("non-interactive", "true"),
                    ("from-file", "/nonexistent/path"),
                ]),
                &CommandContext::new(),
            )
            .await
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("mutually exclusive"),
            "expected mutually-exclusive error, got: {err}"
        );
    }

    #[tokio::test]
    async fn existing_config_without_force_aborts() {
        let dir = tempfile::tempdir().unwrap();
        // Pre-populate config.yaml so the idempotency branch fires.
        std::fs::write(dir.path().join("config.yaml"), "existing").unwrap();

        // Prompter answers "no" to overwrite.
        let prompter = Arc::new(MockPrompter::new(vec!["no"], vec![]));
        let secrets = Arc::new(MockSecretStore::default());
        let cmd = InitCommand::with_dependencies(prompter, secrets);

        let err = cmd
            .execute(
                args(&[("config-root", dir.path().to_str().unwrap())]),
                &CommandContext::new(),
            )
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("aborted"), "expected abort, got: {err}");

        // Original file untouched.
        let raw = std::fs::read_to_string(dir.path().join("config.yaml")).unwrap();
        assert_eq!(raw, "existing");
    }

    #[tokio::test]
    async fn force_overwrites_existing_config() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("config.yaml"), "stale").unwrap();

        let prompter = Arc::new(MockPrompter::new(vec![], vec![]));
        let secrets = Arc::new(MockSecretStore::default());
        let cmd = InitCommand::with_dependencies(prompter, secrets);
        cmd.execute(
            args(&[
                ("config-root", dir.path().to_str().unwrap()),
                ("non-interactive", "true"),
                ("force", "true"),
            ]),
            &CommandContext::new(),
        )
        .await
        .unwrap();

        let raw = std::fs::read_to_string(dir.path().join("config.yaml")).unwrap();
        assert_ne!(raw, "stale");
        assert!(raw.contains("kind: Instance"));
    }

    #[test]
    fn instance_manifest_yaml_round_trips_via_parse_manifests() {
        use sera_types::config_manifest::ResourceKind;
        let yaml = instance_manifest_yaml(json!({
            "gateway": { "mode": "local" },
            "data_dir": "/tmp/sera-data",
        }))
        .unwrap();
        let set = parse_manifests(&yaml).unwrap();
        assert_eq!(set.instances.len(), 1);
        assert_eq!(set.instances[0].metadata.name, "default");
        assert_eq!(set.instances[0].kind, ResourceKind::Instance);
    }
}
