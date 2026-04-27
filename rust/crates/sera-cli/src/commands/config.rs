//! `sera config` — local YAML get/set/edit/path against the L.2 Instance
//! manifest. These verbs never round-trip through the gateway.

use std::path::PathBuf;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::json;

use sera_commands::{
    Command, CommandArgSchema, CommandArgs, CommandCategory, CommandContext, CommandDescription,
    CommandError, CommandResult,
};
use sera_config::ConfigRoot;

fn map_err<E: std::fmt::Display>(e: E) -> CommandError {
    CommandError::Execution(e.to_string())
}

fn config_root_from_args(args: &CommandArgs) -> ConfigRoot {
    args.get("config-root")
        .map(|s| ConfigRoot::new(s.to_string()))
        .unwrap_or_else(ConfigRoot::from_env)
}

fn config_yaml_path(root: &ConfigRoot) -> PathBuf {
    root.path().join("config.yaml")
}

// ── path ────────────────────────────────────────────────────────────────────

pub struct ConfigPathCommand;

#[async_trait]
impl Command for ConfigPathCommand {
    fn name(&self) -> &str {
        "config:path"
    }

    fn describe(&self) -> CommandDescription {
        CommandDescription {
            summary: "Print the path of the resolved config.yaml".into(),
            help: "Resolves the active ConfigRoot (respecting SERA_CONFIG_ROOT) \
                   and prints the path to its config.yaml."
                .into(),
            category: CommandCategory::System,
        }
    }

    fn argument_schema(&self) -> CommandArgSchema {
        CommandArgSchema(clap::Command::new("path").about("Print config.yaml path"))
    }

    async fn execute(
        &self,
        args: CommandArgs,
        _ctx: &CommandContext,
    ) -> Result<CommandResult, CommandError> {
        let root = config_root_from_args(&args);
        let path = config_yaml_path(&root);
        println!("{}", path.display());
        Ok(CommandResult::ok(json!({ "path": path.display().to_string() })))
    }
}

// ── get ─────────────────────────────────────────────────────────────────────

pub struct ConfigGetCommand;

#[async_trait]
impl Command for ConfigGetCommand {
    fn name(&self) -> &str {
        "config:get"
    }

    fn describe(&self) -> CommandDescription {
        CommandDescription {
            summary: "Read a value by dotted path from config.yaml".into(),
            help: "Walks the YAML body. Example: `sera config get spec.gateway.mode`."
                .into(),
            category: CommandCategory::System,
        }
    }

    fn argument_schema(&self) -> CommandArgSchema {
        CommandArgSchema(
            clap::Command::new("get")
                .about("Read a value")
                .arg(clap::Arg::new("key").required(true).help("Dotted path")),
        )
    }

    async fn execute(
        &self,
        args: CommandArgs,
        _ctx: &CommandContext,
    ) -> Result<CommandResult, CommandError> {
        let key = args
            .get("key")
            .ok_or_else(|| CommandError::InvalidArgs("key required".into()))?
            .to_owned();
        let root = config_root_from_args(&args);
        let path = config_yaml_path(&root);
        let raw = std::fs::read_to_string(&path).map_err(|e| {
            CommandError::Execution(format!("failed to read {}: {e}", path.display()))
        })?;
        let doc: serde_yaml::Value = serde_yaml::from_str(&raw).map_err(map_err)?;
        let value = lookup_dotted(&doc, &key).ok_or_else(|| {
            CommandError::Execution(format!("key not found: {key}"))
        })?;
        println!("{}", scalar_to_string(value));
        Ok(CommandResult::ok(serde_yaml_value_to_json(value)))
    }
}

// ── set ─────────────────────────────────────────────────────────────────────

pub struct ConfigSetCommand;

#[async_trait]
impl Command for ConfigSetCommand {
    fn name(&self) -> &str {
        "config:set"
    }

    fn describe(&self) -> CommandDescription {
        CommandDescription {
            summary: "Write a string value at a dotted path in config.yaml".into(),
            help: "Creates intermediate maps as needed. Numbers/booleans must be \
                   quoted by the user — values are stored as strings to keep this \
                   command predictable."
                .into(),
            category: CommandCategory::System,
        }
    }

    fn argument_schema(&self) -> CommandArgSchema {
        CommandArgSchema(
            clap::Command::new("set")
                .about("Write a value")
                .arg(clap::Arg::new("key").required(true).help("Dotted path"))
                .arg(clap::Arg::new("value").required(true).help("Value (string)")),
        )
    }

    async fn execute(
        &self,
        args: CommandArgs,
        _ctx: &CommandContext,
    ) -> Result<CommandResult, CommandError> {
        let key = args
            .get("key")
            .ok_or_else(|| CommandError::InvalidArgs("key required".into()))?
            .to_owned();
        let value = args
            .get("value")
            .ok_or_else(|| CommandError::InvalidArgs("value required".into()))?
            .to_owned();
        let root = config_root_from_args(&args);
        let path = config_yaml_path(&root);
        let raw = std::fs::read_to_string(&path).map_err(|e| {
            CommandError::Execution(format!("failed to read {}: {e}", path.display()))
        })?;
        let mut doc: serde_yaml::Value = serde_yaml::from_str(&raw).map_err(map_err)?;
        set_dotted(&mut doc, &key, serde_yaml::Value::String(value.clone()))?;
        let serialised = serde_yaml::to_string(&doc).map_err(map_err)?;
        std::fs::write(&path, &serialised).map_err(|e| {
            CommandError::Execution(format!("failed to write {}: {e}", path.display()))
        })?;
        println!("Set {key} = {value} in {}", path.display());
        Ok(CommandResult::ok(json!({ "key": key, "value": value })))
    }
}

// ── edit ────────────────────────────────────────────────────────────────────

pub struct ConfigEditCommand;

#[async_trait]
impl Command for ConfigEditCommand {
    fn name(&self) -> &str {
        "config:edit"
    }

    fn describe(&self) -> CommandDescription {
        CommandDescription {
            summary: "Open config.yaml in $EDITOR (fallback: vi)".into(),
            help: "Spawns the user's editor against config.yaml. Returns once the \
                   editor exits."
                .into(),
            category: CommandCategory::System,
        }
    }

    fn argument_schema(&self) -> CommandArgSchema {
        CommandArgSchema(clap::Command::new("edit").about("Edit config.yaml"))
    }

    async fn execute(
        &self,
        args: CommandArgs,
        _ctx: &CommandContext,
    ) -> Result<CommandResult, CommandError> {
        let root = config_root_from_args(&args);
        let path = config_yaml_path(&root);
        let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
        let status = std::process::Command::new(&editor)
            .arg(&path)
            .status()
            .map_err(|e| CommandError::Execution(format!("failed to launch {editor}: {e}")))?;
        if !status.success() {
            return Err(CommandError::Execution(format!(
                "editor exited with status {status}"
            )));
        }
        Ok(CommandResult::ok(json!({
            "editor": editor,
            "path": path.display().to_string(),
        })))
    }
}

// ── helpers ─────────────────────────────────────────────────────────────────

fn lookup_dotted<'a>(doc: &'a serde_yaml::Value, key: &str) -> Option<&'a serde_yaml::Value> {
    let mut node = doc;
    for part in key.split('.') {
        node = node.get(part)?;
    }
    Some(node)
}

fn set_dotted(
    doc: &mut serde_yaml::Value,
    key: &str,
    value: serde_yaml::Value,
) -> Result<(), CommandError> {
    let parts: Vec<&str> = key.split('.').collect();
    if parts.iter().any(|p| p.is_empty()) {
        return Err(CommandError::InvalidArgs("empty key segment".into()));
    }
    if !doc.is_mapping() {
        *doc = serde_yaml::Value::Mapping(serde_yaml::Mapping::new());
    }
    let mut cursor = doc;
    for (idx, part) in parts.iter().enumerate() {
        let map = cursor
            .as_mapping_mut()
            .ok_or_else(|| CommandError::Execution(format!("path '{key}' traverses a non-map")))?;
        let ykey = serde_yaml::Value::String((*part).to_string());
        let last = idx + 1 == parts.len();
        if last {
            map.insert(ykey, value);
            return Ok(());
        }
        let entry = map
            .entry(ykey)
            .or_insert_with(|| serde_yaml::Value::Mapping(serde_yaml::Mapping::new()));
        if !entry.is_mapping() {
            *entry = serde_yaml::Value::Mapping(serde_yaml::Mapping::new());
        }
        cursor = entry;
    }
    Ok(())
}

fn scalar_to_string(v: &serde_yaml::Value) -> String {
    match v {
        serde_yaml::Value::String(s) => s.clone(),
        serde_yaml::Value::Number(n) => n.to_string(),
        serde_yaml::Value::Bool(b) => b.to_string(),
        serde_yaml::Value::Null => String::new(),
        other => serde_yaml::to_string(other).unwrap_or_default().trim().to_string(),
    }
}

fn serde_yaml_value_to_json(v: &serde_yaml::Value) -> serde_json::Value {
    serde_json::to_value(v).unwrap_or(serde_json::Value::Null)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_dotted_walks_nested_maps() {
        let doc: serde_yaml::Value = serde_yaml::from_str("a:\n  b:\n    c: hi\n").unwrap();
        assert_eq!(
            lookup_dotted(&doc, "a.b.c").and_then(|v| v.as_str()),
            Some("hi"),
        );
        assert!(lookup_dotted(&doc, "a.b.missing").is_none());
    }

    #[test]
    fn set_dotted_creates_intermediate_maps() {
        let mut doc: serde_yaml::Value = serde_yaml::from_str("a: 1\n").unwrap();
        set_dotted(&mut doc, "x.y.z", serde_yaml::Value::String("v".into())).unwrap();
        assert_eq!(
            lookup_dotted(&doc, "x.y.z").and_then(|v| v.as_str()),
            Some("v"),
        );
    }

    #[test]
    fn set_dotted_overwrites_existing_value() {
        let mut doc: serde_yaml::Value = serde_yaml::from_str("a:\n  b: old\n").unwrap();
        set_dotted(&mut doc, "a.b", serde_yaml::Value::String("new".into())).unwrap();
        assert_eq!(
            lookup_dotted(&doc, "a.b").and_then(|v| v.as_str()),
            Some("new"),
        );
    }
}
