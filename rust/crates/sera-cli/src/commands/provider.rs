//! `sera provider` — manage LLM provider configurations.
//!
//! Subcommands: `list`, `add`, `remove`, `select`, `configure`.

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use comfy_table::Table;
use serde_json::json;

use sera_commands::{
    Command, CommandArgSchema, CommandArgs, CommandCategory, CommandContext, CommandDescription,
    CommandError, CommandResult,
};
use sera_config::ConfigRoot;
use sera_config::provider_registry::{self, AuthStrategy, ProviderEntry};
use sera_config::provider_wizard::{
    DialoguerPrompter, KeyringSecretStore, Prompter, ProviderConfig, SecretStore,
    handler_for, list_configured, read_manifest, remove_manifest, write_manifest,
};

use crate::config::CliConfig;

// ── Shared helpers ──────────────────────────────────────────────────────────

fn map_err<E: std::fmt::Display>(e: E) -> CommandError {
    CommandError::Execution(e.to_string())
}

fn config_root_from_args(args: &CommandArgs) -> ConfigRoot {
    args.get("config-root")
        .map(|s| ConfigRoot::new(s.to_string()))
        .unwrap_or_else(ConfigRoot::from_env)
}

fn auth_strategy_label(s: AuthStrategy) -> &'static str {
    match s {
        AuthStrategy::ApiKey => "api-key",
        AuthStrategy::OAuthDevice => "oauth-device",
        AuthStrategy::OAuthExternal => "oauth-external",
        AuthStrategy::CustomEndpoint => "custom-endpoint",
    }
}

// ── ProviderListCommand ─────────────────────────────────────────────────────

pub struct ProviderListCommand;

#[async_trait]
impl Command for ProviderListCommand {
    fn name(&self) -> &str {
        "provider:list"
    }

    fn describe(&self) -> CommandDescription {
        CommandDescription {
            summary: "List built-in providers and configured provider instances".into(),
            help: "Shows the registry of supported providers (their auth strategy and \
                   default base URL) and any configured provider instances under \
                   the SERA config root."
                .into(),
            category: CommandCategory::System,
        }
    }

    fn argument_schema(&self) -> CommandArgSchema {
        CommandArgSchema(clap::Command::new("list").about("List providers"))
    }

    async fn execute(
        &self,
        args: CommandArgs,
        _ctx: &CommandContext,
    ) -> Result<CommandResult, CommandError> {
        let root = config_root_from_args(&args);
        let providers_dir = root.providers_dir();

        let mut registry_table = Table::new();
        registry_table.set_header(vec!["id", "display name", "strategy", "default base URL"]);
        for entry in provider_registry::registry() {
            registry_table.add_row(vec![
                entry.id.to_string(),
                entry.display_name.to_string(),
                auth_strategy_label(entry.auth_strategy).to_string(),
                entry.default_base_url.unwrap_or("").to_string(),
            ]);
        }

        let configured = list_configured(&providers_dir).map_err(map_err)?;
        let mut configured_table = Table::new();
        configured_table.set_header(vec!["name", "provider", "base URL", "default model", "secret"]);
        for name in &configured {
            match read_manifest(&providers_dir, name) {
                Ok(m) => {
                    configured_table.add_row(vec![
                        name.clone(),
                        m.spec.provider_id,
                        m.spec.base_url,
                        m.spec.default_model.unwrap_or_default(),
                        if m.spec.api_key_ref.is_some() { "keyring" } else { "" }
                            .to_string(),
                    ]);
                }
                Err(e) => {
                    configured_table.add_row(vec![
                        name.clone(),
                        format!("<unreadable: {e}>"),
                        String::new(),
                        String::new(),
                        String::new(),
                    ]);
                }
            }
        }

        println!("Built-in providers:");
        println!("{registry_table}");
        println!();
        println!("Configured providers ({}):", configured.len());
        if configured.is_empty() {
            println!("  (none — run `sera provider add` to create one)");
        } else {
            println!("{configured_table}");
        }

        Ok(CommandResult::ok(json!({
            "registry": provider_registry::registry()
                .iter()
                .map(|e| json!({
                    "id": e.id,
                    "display_name": e.display_name,
                    "strategy": auth_strategy_label(e.auth_strategy),
                    "default_base_url": e.default_base_url,
                }))
                .collect::<Vec<_>>(),
            "configured": configured,
        })))
    }
}

// ── ProviderAddCommand ──────────────────────────────────────────────────────

pub struct ProviderAddCommand {
    prompter: Arc<dyn Prompter>,
    secrets: Arc<dyn SecretStore>,
}

impl ProviderAddCommand {
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

impl Default for ProviderAddCommand {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Command for ProviderAddCommand {
    fn name(&self) -> &str {
        "provider:add"
    }

    fn describe(&self) -> CommandDescription {
        CommandDescription {
            summary: "Add a new provider instance".into(),
            help: "Selects a provider from the registry, runs the wizard for its \
                   auth strategy, stores any secrets in the OS keychain, and writes \
                   a manifest to <config-root>/providers/<name>.yaml."
                .into(),
            category: CommandCategory::System,
        }
    }

    fn argument_schema(&self) -> CommandArgSchema {
        CommandArgSchema(
            clap::Command::new("add")
                .about("Add a provider instance")
                .arg(clap::Arg::new("id").help("Registry id (e.g. openrouter)"))
                .arg(
                    clap::Arg::new("name")
                        .long("name")
                        .help("Instance name (default: <id>)")
                        .value_name("NAME"),
                ),
        )
    }

    async fn execute(
        &self,
        args: CommandArgs,
        _ctx: &CommandContext,
    ) -> Result<CommandResult, CommandError> {
        let root = config_root_from_args(&args);
        let providers_dir = root.providers_dir();

        let id = args
            .get("id")
            .ok_or_else(|| {
                CommandError::InvalidArgs(format!(
                    "registry id required (use one of: {})",
                    provider_registry::registry()
                        .iter()
                        .map(|e| e.id)
                        .collect::<Vec<_>>()
                        .join(", ")
                ))
            })?
            .to_owned();

        let entry: &ProviderEntry = provider_registry::lookup(&id).ok_or_else(|| {
            CommandError::InvalidArgs(format!("unknown provider id: {id}"))
        })?;

        let name = args.get("name").map(str::to_owned).unwrap_or_else(|| id.clone());

        let handler = handler_for(entry.auth_strategy);
        let cfg = handler
            .run(entry, &name, self.prompter.as_ref(), self.secrets.as_ref())
            .map_err(map_err)?;

        let path = write_manifest(&providers_dir, &name, &cfg).map_err(map_err)?;
        println!("Wrote {}", path.display());

        Ok(CommandResult::ok(json!({
            "name": name,
            "provider_id": cfg.provider_id,
            "path": path.display().to_string(),
        })))
    }
}

// ── ProviderRemoveCommand ───────────────────────────────────────────────────

pub struct ProviderRemoveCommand {
    secrets: Arc<dyn SecretStore>,
}

impl ProviderRemoveCommand {
    pub fn new() -> Self {
        Self {
            secrets: Arc::new(KeyringSecretStore),
        }
    }

    pub fn with_secrets(secrets: Arc<dyn SecretStore>) -> Self {
        Self { secrets }
    }
}

impl Default for ProviderRemoveCommand {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Command for ProviderRemoveCommand {
    fn name(&self) -> &str {
        "provider:remove"
    }

    fn describe(&self) -> CommandDescription {
        CommandDescription {
            summary: "Remove a provider instance".into(),
            help: "Deletes the YAML manifest and any associated keychain entry."
                .into(),
            category: CommandCategory::System,
        }
    }

    fn argument_schema(&self) -> CommandArgSchema {
        CommandArgSchema(
            clap::Command::new("remove")
                .about("Remove a provider instance")
                .arg(clap::Arg::new("name").required(true).help("Instance name")),
        )
    }

    async fn execute(
        &self,
        args: CommandArgs,
        _ctx: &CommandContext,
    ) -> Result<CommandResult, CommandError> {
        let root = config_root_from_args(&args);
        let providers_dir = root.providers_dir();

        let name = args
            .get("name")
            .ok_or_else(|| CommandError::InvalidArgs("name required".into()))?
            .to_owned();

        // Best-effort: if the manifest references a keychain entry, delete it.
        if let Ok(m) = read_manifest(&providers_dir, &name)
            && let Some(kref) = &m.spec.api_key_ref
            && let Err(e) = self
                .secrets
                .delete(&kref.keyring_service, &kref.keyring_entry)
        {
            tracing::warn!("failed to delete keyring entry: {e}");
        }

        remove_manifest(&providers_dir, &name).map_err(map_err)?;
        println!("Removed provider '{name}'");
        Ok(CommandResult::ok(json!({ "name": name })))
    }
}

// ── ProviderSelectCommand ───────────────────────────────────────────────────

pub struct ProviderSelectCommand;

#[async_trait]
impl Command for ProviderSelectCommand {
    fn name(&self) -> &str {
        "provider:select"
    }

    fn describe(&self) -> CommandDescription {
        CommandDescription {
            summary: "Set a provider as the default in the CLI config".into(),
            help: "Updates ~/.sera/config.toml to reference the named provider \
                   as the CLI's default."
                .into(),
            category: CommandCategory::System,
        }
    }

    fn argument_schema(&self) -> CommandArgSchema {
        CommandArgSchema(
            clap::Command::new("select")
                .about("Set a provider as the default")
                .arg(clap::Arg::new("name").required(true).help("Instance name")),
        )
    }

    async fn execute(
        &self,
        args: CommandArgs,
        _ctx: &CommandContext,
    ) -> Result<CommandResult, CommandError> {
        let root = config_root_from_args(&args);
        let providers_dir = root.providers_dir();

        let name = args
            .get("name")
            .ok_or_else(|| CommandError::InvalidArgs("name required".into()))?
            .to_owned();

        // Verify the provider exists before we mutate config.
        read_manifest(&providers_dir, &name).map_err(|e| {
            CommandError::Execution(format!("provider '{name}' not configured: {e}"))
        })?;

        let cli_path = args
            .get("cli-config")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(CliConfig::default_path);
        let mut cli_cfg = CliConfig::load(&cli_path).map_err(map_err)?;
        cli_cfg.default_provider = Some(name.clone());
        cli_cfg.save(&cli_path).map_err(map_err)?;

        println!("Default provider set to '{name}' (saved to {})", cli_path.display());
        Ok(CommandResult::ok(json!({
            "default_provider": name,
            "cli_config_path": cli_path.display().to_string(),
        })))
    }
}

// ── ProviderConfigureCommand ────────────────────────────────────────────────

pub struct ProviderConfigureCommand {
    prompter: Arc<dyn Prompter>,
    secrets: Arc<dyn SecretStore>,
}

impl ProviderConfigureCommand {
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

impl Default for ProviderConfigureCommand {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Command for ProviderConfigureCommand {
    fn name(&self) -> &str {
        "provider:configure"
    }

    fn describe(&self) -> CommandDescription {
        CommandDescription {
            summary: "Re-run the wizard for an existing provider".into(),
            help: "Loads the existing manifest, runs the wizard for the provider's \
                   auth strategy, and overwrites the manifest in place."
                .into(),
            category: CommandCategory::System,
        }
    }

    fn argument_schema(&self) -> CommandArgSchema {
        CommandArgSchema(
            clap::Command::new("configure")
                .about("Re-run the wizard for an existing provider")
                .arg(clap::Arg::new("name").required(true).help("Instance name")),
        )
    }

    async fn execute(
        &self,
        args: CommandArgs,
        _ctx: &CommandContext,
    ) -> Result<CommandResult, CommandError> {
        let root = config_root_from_args(&args);
        let providers_dir = root.providers_dir();

        let name = args
            .get("name")
            .ok_or_else(|| CommandError::InvalidArgs("name required".into()))?
            .to_owned();

        let existing = read_manifest(&providers_dir, &name).map_err(map_err)?;
        let entry = provider_registry::lookup(&existing.spec.provider_id).ok_or_else(|| {
            CommandError::Execution(format!(
                "manifest references unknown provider id '{}'",
                existing.spec.provider_id
            ))
        })?;

        let handler = handler_for(entry.auth_strategy);
        let new_cfg: ProviderConfig = handler
            .run(entry, &name, self.prompter.as_ref(), self.secrets.as_ref())
            .map_err(map_err)?;

        let path = write_manifest(&providers_dir, &name, &new_cfg).map_err(map_err)?;
        println!("Updated {}", path.display());
        Ok(CommandResult::ok(json!({
            "name": name,
            "path": path.display().to_string(),
        })))
    }
}

