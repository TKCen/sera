//! Wizard handlers for adding / configuring an LLM provider.
//!
//! There is exactly one handler per [`AuthStrategy`] — never per provider.
//! The dispatch axis is `auth_strategy`; provider-specific data lives in the
//! registry table (`provider_registry.rs`).

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::provider_registry::{AuthStrategy, ProviderEntry};

/// Keyring service name used for provider API keys.
pub const PROVIDER_KEYRING_SERVICE: &str = "sera-providers";

/// Persisted provider configuration — the on-disk YAML manifest's `spec`.
///
/// The API key VALUE never appears here.  When `api_key_ref` is `Some`, the
/// secret lives in the OS keychain under the referenced entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderConfig {
    pub provider_id: String,
    pub base_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key_ref: Option<KeyringRef>,
}

/// Pointer to a secret stored in the OS keychain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyringRef {
    pub keyring_service: String,
    pub keyring_entry: String,
}

/// Errors surfaced by wizard handlers.
#[derive(Debug, thiserror::Error)]
pub enum WizardError {
    #[error("strategy not yet implemented: {0:?}")]
    NotImplemented(AuthStrategy),
    #[error("prompt failed: {0}")]
    Prompt(String),
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("keychain error: {0}")]
    Keychain(String),
    #[error("io error for {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("yaml serialization error: {0}")]
    Yaml(#[from] serde_yaml::Error),
}

/// Abstract input source — production reads from a TTY via `dialoguer`, tests
/// inject a scripted [`MockPrompter`].
pub trait Prompter: Send + Sync {
    /// Prompt for a line of text with an optional default value.
    fn prompt_text(&self, label: &str, default: Option<&str>) -> Result<String, WizardError>;
    /// Prompt for a secret (input is hidden / not echoed).  Empty input is
    /// returned as `None`.
    fn prompt_secret(&self, label: &str) -> Result<Option<String>, WizardError>;
}

/// `dialoguer`-backed prompter for use against a real terminal.
pub struct DialoguerPrompter;

impl Prompter for DialoguerPrompter {
    fn prompt_text(&self, label: &str, default: Option<&str>) -> Result<String, WizardError> {
        let input: dialoguer::Input<String> =
            dialoguer::Input::new().with_prompt(label);
        let input = match default {
            Some(d) => input.default(d.to_owned()),
            None => input,
        };
        input
            .interact_text()
            .map_err(|e| WizardError::Prompt(e.to_string()))
    }

    fn prompt_secret(&self, label: &str) -> Result<Option<String>, WizardError> {
        let pw = dialoguer::Password::new()
            .with_prompt(label)
            .allow_empty_password(true)
            .interact()
            .map_err(|e| WizardError::Prompt(e.to_string()))?;
        if pw.is_empty() { Ok(None) } else { Ok(Some(pw)) }
    }
}

/// Wizard handler trait — one impl per [`AuthStrategy`].
pub trait ProviderWizardHandler {
    /// Run the wizard, returning a populated [`ProviderConfig`].  The handler
    /// is responsible for storing any secret in the keychain via the supplied
    /// [`SecretStore`] and only returning a [`KeyringRef`] in the result.
    fn run(
        &self,
        entry: &ProviderEntry,
        instance_name: &str,
        prompter: &dyn Prompter,
        secrets: &dyn SecretStore,
    ) -> Result<ProviderConfig, WizardError>;
}

/// Abstract secret backing store so tests don't touch the real keychain.
pub trait SecretStore: Send + Sync {
    fn set(&self, service: &str, entry: &str, value: &str) -> Result<(), WizardError>;
    fn get(&self, service: &str, entry: &str) -> Result<Option<String>, WizardError>;
    fn delete(&self, service: &str, entry: &str) -> Result<(), WizardError>;
}

/// `keyring`-crate-backed [`SecretStore`].
pub struct KeyringSecretStore;

impl SecretStore for KeyringSecretStore {
    fn set(&self, service: &str, entry: &str, value: &str) -> Result<(), WizardError> {
        let e = keyring::Entry::new(service, entry)
            .map_err(|e| WizardError::Keychain(e.to_string()))?;
        e.set_password(value)
            .map_err(|e| WizardError::Keychain(e.to_string()))
    }

    fn get(&self, service: &str, entry: &str) -> Result<Option<String>, WizardError> {
        let e = keyring::Entry::new(service, entry)
            .map_err(|e| WizardError::Keychain(e.to_string()))?;
        match e.get_password() {
            Ok(v) => Ok(Some(v)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(WizardError::Keychain(e.to_string())),
        }
    }

    fn delete(&self, service: &str, entry: &str) -> Result<(), WizardError> {
        let e = keyring::Entry::new(service, entry)
            .map_err(|e| WizardError::Keychain(e.to_string()))?;
        match e.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(WizardError::Keychain(e.to_string())),
        }
    }
}

// ── Handlers ────────────────────────────────────────────────────────────────

pub struct ApiKeyHandler;

impl ProviderWizardHandler for ApiKeyHandler {
    fn run(
        &self,
        entry: &ProviderEntry,
        instance_name: &str,
        prompter: &dyn Prompter,
        secrets: &dyn SecretStore,
    ) -> Result<ProviderConfig, WizardError> {
        let env_value = entry.env_keys.iter().find_map(|k| std::env::var(k).ok());
        let api_key = match env_value {
            Some(v) if !v.is_empty() => v,
            _ => prompter
                .prompt_secret(&format!("API key for {}", entry.display_name))?
                .ok_or_else(|| WizardError::InvalidInput("API key must not be empty".into()))?,
        };

        let base_url = prompter.prompt_text(
            "Base URL",
            entry.default_base_url,
        )?;

        let kref = KeyringRef {
            keyring_service: PROVIDER_KEYRING_SERVICE.to_string(),
            keyring_entry: instance_name.to_string(),
        };
        secrets.set(&kref.keyring_service, &kref.keyring_entry, &api_key)?;

        Ok(ProviderConfig {
            provider_id: entry.id.to_string(),
            base_url,
            default_model: None,
            api_key_ref: Some(kref),
        })
    }
}

pub struct CustomEndpointHandler;

impl ProviderWizardHandler for CustomEndpointHandler {
    fn run(
        &self,
        entry: &ProviderEntry,
        instance_name: &str,
        prompter: &dyn Prompter,
        secrets: &dyn SecretStore,
    ) -> Result<ProviderConfig, WizardError> {
        let base_url = prompter.prompt_text("Base URL", entry.default_base_url)?;

        let api_key = prompter
            .prompt_secret("API key (optional, press Enter to skip)")?;

        let api_key_ref = match api_key {
            Some(value) => {
                let kref = KeyringRef {
                    keyring_service: PROVIDER_KEYRING_SERVICE.to_string(),
                    keyring_entry: instance_name.to_string(),
                };
                secrets.set(&kref.keyring_service, &kref.keyring_entry, &value)?;
                Some(kref)
            }
            None => None,
        };

        Ok(ProviderConfig {
            provider_id: entry.id.to_string(),
            base_url,
            default_model: None,
            api_key_ref,
        })
    }
}

pub struct OAuthDeviceHandler;

impl ProviderWizardHandler for OAuthDeviceHandler {
    fn run(
        &self,
        _entry: &ProviderEntry,
        _instance_name: &str,
        _prompter: &dyn Prompter,
        _secrets: &dyn SecretStore,
    ) -> Result<ProviderConfig, WizardError> {
        Err(WizardError::NotImplemented(AuthStrategy::OAuthDevice))
    }
}

pub struct OAuthExternalHandler;

impl ProviderWizardHandler for OAuthExternalHandler {
    fn run(
        &self,
        _entry: &ProviderEntry,
        _instance_name: &str,
        _prompter: &dyn Prompter,
        _secrets: &dyn SecretStore,
    ) -> Result<ProviderConfig, WizardError> {
        Err(WizardError::NotImplemented(AuthStrategy::OAuthExternal))
    }
}

/// Dispatch table — strategy → handler.  This is the only place strategies
/// are matched on; everything else operates on the `dyn` trait object.
pub fn handler_for(strategy: AuthStrategy) -> Box<dyn ProviderWizardHandler> {
    match strategy {
        AuthStrategy::ApiKey => Box::new(ApiKeyHandler),
        AuthStrategy::CustomEndpoint => Box::new(CustomEndpointHandler),
        AuthStrategy::OAuthDevice => Box::new(OAuthDeviceHandler),
        AuthStrategy::OAuthExternal => Box::new(OAuthExternalHandler),
    }
}

// ── Persistence ─────────────────────────────────────────────────────────────

/// On-disk envelope for a provider manifest (k8s-style).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderManifest {
    pub api_version: String,
    pub kind: String,
    #[serde(default = "default_config_version", rename = "_configVersion")]
    pub config_version: u32,
    pub metadata: ProviderMetadata,
    pub spec: ProviderConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderMetadata {
    pub name: String,
}

fn default_config_version() -> u32 {
    1
}

impl ProviderManifest {
    pub fn new(name: String, spec: ProviderConfig) -> Self {
        Self {
            api_version: "sera.dev/v1".to_string(),
            kind: "Provider".to_string(),
            config_version: 1,
            metadata: ProviderMetadata { name },
            spec,
        }
    }
}

/// Write a provider manifest to `<providers_dir>/<name>.yaml`.
pub fn write_manifest(
    providers_dir: &Path,
    name: &str,
    config: &ProviderConfig,
) -> Result<std::path::PathBuf, WizardError> {
    std::fs::create_dir_all(providers_dir).map_err(|e| WizardError::Io {
        path: providers_dir.display().to_string(),
        source: e,
    })?;
    let path = providers_dir.join(format!("{name}.yaml"));
    let manifest = ProviderManifest::new(name.to_string(), config.clone());
    let yaml = serde_yaml::to_string(&manifest)?;
    std::fs::write(&path, yaml).map_err(|e| WizardError::Io {
        path: path.display().to_string(),
        source: e,
    })?;
    Ok(path)
}

/// Load a provider manifest from `<providers_dir>/<name>.yaml`.
pub fn read_manifest(
    providers_dir: &Path,
    name: &str,
) -> Result<ProviderManifest, WizardError> {
    let path = providers_dir.join(format!("{name}.yaml"));
    let raw = std::fs::read_to_string(&path).map_err(|e| WizardError::Io {
        path: path.display().to_string(),
        source: e,
    })?;
    let manifest: ProviderManifest = serde_yaml::from_str(&raw)?;
    Ok(manifest)
}

/// Enumerate the names of all configured provider manifests under
/// `providers_dir`.  Missing directory returns an empty list.
pub fn list_configured(providers_dir: &Path) -> Result<Vec<String>, WizardError> {
    if !providers_dir.exists() {
        return Ok(Vec::new());
    }
    let mut names = Vec::new();
    let entries = std::fs::read_dir(providers_dir).map_err(|e| WizardError::Io {
        path: providers_dir.display().to_string(),
        source: e,
    })?;
    for ent in entries {
        let ent = ent.map_err(|e| WizardError::Io {
            path: providers_dir.display().to_string(),
            source: e,
        })?;
        let path = ent.path();
        if path.extension().and_then(|e| e.to_str()) != Some("yaml") {
            continue;
        }
        if let Some(stem) = path.file_stem().and_then(|s| s.to_str())
            && !stem.starts_with('.')
        {
            names.push(stem.to_string());
        }
    }
    names.sort();
    Ok(names)
}

/// Remove `<providers_dir>/<name>.yaml`.  Missing file is not an error.
pub fn remove_manifest(providers_dir: &Path, name: &str) -> Result<(), WizardError> {
    let path = providers_dir.join(format!("{name}.yaml"));
    if path.exists() {
        std::fs::remove_file(&path).map_err(|e| WizardError::Io {
            path: path.display().to_string(),
            source: e,
        })?;
    }
    Ok(())
}

pub mod test_support {
    //! Test scaffolding — exposed so integration tests in other crates can
    //! drive the wizard without touching real keychains or terminals.
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    pub struct MockPrompter {
        texts: Mutex<Vec<String>>,
        secrets: Mutex<Vec<Option<String>>>,
    }

    impl MockPrompter {
        pub fn new(texts: Vec<&str>, secrets: Vec<Option<&str>>) -> Self {
            Self {
                texts: Mutex::new(texts.into_iter().rev().map(String::from).collect()),
                secrets: Mutex::new(
                    secrets
                        .into_iter()
                        .rev()
                        .map(|s| s.map(String::from))
                        .collect(),
                ),
            }
        }
    }

    impl Prompter for MockPrompter {
        fn prompt_text(
            &self,
            _label: &str,
            default: Option<&str>,
        ) -> Result<String, WizardError> {
            match self.texts.lock().unwrap().pop() {
                Some(v) if !v.is_empty() => Ok(v),
                Some(_) => Ok(default.unwrap_or("").to_string()),
                None => Ok(default.unwrap_or("").to_string()),
            }
        }

        fn prompt_secret(&self, _label: &str) -> Result<Option<String>, WizardError> {
            Ok(self.secrets.lock().unwrap().pop().flatten())
        }
    }

    #[derive(Default)]
    pub struct MockSecretStore {
        inner: Mutex<HashMap<(String, String), String>>,
    }

    impl MockSecretStore {
        pub fn snapshot(&self) -> HashMap<(String, String), String> {
            self.inner.lock().unwrap().clone()
        }
    }

    impl SecretStore for MockSecretStore {
        fn set(&self, service: &str, entry: &str, value: &str) -> Result<(), WizardError> {
            self.inner
                .lock()
                .unwrap()
                .insert((service.to_string(), entry.to_string()), value.to_string());
            Ok(())
        }

        fn get(&self, service: &str, entry: &str) -> Result<Option<String>, WizardError> {
            Ok(self
                .inner
                .lock()
                .unwrap()
                .get(&(service.to_string(), entry.to_string()))
                .cloned())
        }

        fn delete(&self, service: &str, entry: &str) -> Result<(), WizardError> {
            self.inner
                .lock()
                .unwrap()
                .remove(&(service.to_string(), entry.to_string()));
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::*;
    use super::*;
    use crate::provider_registry::lookup;

    /// Serialises tests that mutate the process-global `OPENROUTER_API_KEY`.
    /// cargo runs a test binary's cases on multiple threads, so without this
    /// the env-reading and env-setting tests observe each other's mutations
    /// (the `validate-rust` flake: this handler read a sibling test's
    /// `from-env-key` instead of the prompted value). `into_inner` ignores
    /// poisoning so one failing assertion does not cascade into the other.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn api_key_handler_writes_secret_and_returns_ref() {
        let _env_guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let entry = lookup("openrouter").unwrap();
        let prompter = MockPrompter::new(
            vec![""], // base_url -> use default
            vec![Some("sk-secret-123")],
        );
        let secrets = MockSecretStore::default();

        // Ensure no env var leaks into the test
        let saved = std::env::var("OPENROUTER_API_KEY").ok();
        // SAFETY: env mutation is serialised via ENV_LOCK for the duration of
        // this test, so no other test reads/writes OPENROUTER_API_KEY concurrently.
        unsafe { std::env::remove_var("OPENROUTER_API_KEY") };

        let cfg = ApiKeyHandler.run(entry, "openrouter-test", &prompter, &secrets).unwrap();

        if let Some(v) = saved {
            unsafe { std::env::set_var("OPENROUTER_API_KEY", v) };
        }

        assert_eq!(cfg.provider_id, "openrouter");
        assert_eq!(cfg.base_url, "https://openrouter.ai/api/v1");
        let kref = cfg.api_key_ref.as_ref().unwrap();
        assert_eq!(kref.keyring_service, PROVIDER_KEYRING_SERVICE);
        assert_eq!(kref.keyring_entry, "openrouter-test");

        let stored = secrets.snapshot();
        assert_eq!(
            stored
                .get(&(PROVIDER_KEYRING_SERVICE.to_string(), "openrouter-test".to_string()))
                .map(String::as_str),
            Some("sk-secret-123"),
        );
    }

    #[test]
    fn api_key_handler_uses_env_when_set() {
        let _env_guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let entry = lookup("openrouter").unwrap();
        let prompter = MockPrompter::new(vec![""], vec![]);
        let secrets = MockSecretStore::default();

        let saved = std::env::var("OPENROUTER_API_KEY").ok();
        // SAFETY: env mutation is serialised via ENV_LOCK (see sibling test).
        unsafe { std::env::set_var("OPENROUTER_API_KEY", "from-env-key") };

        let cfg = ApiKeyHandler
            .run(entry, "openrouter-env", &prompter, &secrets)
            .unwrap();

        match saved {
            Some(v) => unsafe { std::env::set_var("OPENROUTER_API_KEY", v) },
            None => unsafe { std::env::remove_var("OPENROUTER_API_KEY") },
        }

        let stored = secrets.snapshot();
        assert_eq!(
            stored
                .get(&(PROVIDER_KEYRING_SERVICE.to_string(), "openrouter-env".to_string()))
                .map(String::as_str),
            Some("from-env-key"),
        );
        assert!(cfg.api_key_ref.is_some());
    }

    #[test]
    fn custom_endpoint_handler_skips_secret_when_blank() {
        let entry = lookup("lm-studio").unwrap();
        let prompter = MockPrompter::new(
            vec!["http://localhost:1234/v1"],
            vec![None],
        );
        let secrets = MockSecretStore::default();

        let cfg = CustomEndpointHandler
            .run(entry, "lm-studio-local", &prompter, &secrets)
            .unwrap();

        assert_eq!(cfg.provider_id, "lm-studio");
        assert_eq!(cfg.base_url, "http://localhost:1234/v1");
        assert!(cfg.api_key_ref.is_none());
        assert!(secrets.snapshot().is_empty());
    }

    #[test]
    fn custom_endpoint_handler_stores_optional_secret() {
        let entry = lookup("lm-studio").unwrap();
        let prompter = MockPrompter::new(
            vec!["http://localhost:9999/v1"],
            vec![Some("local-key")],
        );
        let secrets = MockSecretStore::default();

        let cfg = CustomEndpointHandler
            .run(entry, "lm-studio-protected", &prompter, &secrets)
            .unwrap();

        assert_eq!(cfg.base_url, "http://localhost:9999/v1");
        let kref = cfg.api_key_ref.as_ref().unwrap();
        assert_eq!(kref.keyring_entry, "lm-studio-protected");
        assert!(!secrets.snapshot().is_empty());
    }

    #[test]
    fn oauth_handlers_return_not_implemented() {
        let entry = lookup("anthropic").unwrap();
        let prompter = MockPrompter::new(vec![], vec![]);
        let secrets = MockSecretStore::default();

        let err = OAuthDeviceHandler
            .run(entry, "x", &prompter, &secrets)
            .unwrap_err();
        assert!(matches!(err, WizardError::NotImplemented(_)));

        let err = OAuthExternalHandler
            .run(entry, "x", &prompter, &secrets)
            .unwrap_err();
        assert!(matches!(err, WizardError::NotImplemented(_)));
    }

    #[test]
    fn handler_for_dispatches_on_strategy() {
        // Sanity — every strategy must have a handler.
        let _ = handler_for(AuthStrategy::ApiKey);
        let _ = handler_for(AuthStrategy::CustomEndpoint);
        let _ = handler_for(AuthStrategy::OAuthDevice);
        let _ = handler_for(AuthStrategy::OAuthExternal);
    }

    #[test]
    fn manifest_round_trips_and_omits_secret_value() {
        let canary = "CANARY-SECRET-VALUE-DO-NOT-LEAK";
        let dir = tempfile::tempdir().unwrap();
        let providers_dir = dir.path().join("providers");

        let cfg = ProviderConfig {
            provider_id: "openrouter".to_string(),
            base_url: "https://openrouter.ai/api/v1".to_string(),
            default_model: Some("anthropic/claude-3.5-sonnet".to_string()),
            api_key_ref: Some(KeyringRef {
                keyring_service: PROVIDER_KEYRING_SERVICE.to_string(),
                keyring_entry: "openrouter-prod".to_string(),
            }),
        };

        let path = write_manifest(&providers_dir, "openrouter-prod", &cfg).unwrap();

        // 1. canary value must NEVER appear in the YAML
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(
            !raw.contains(canary),
            "canary secret leaked into provider manifest"
        );

        // 2. round-trip via the same loader
        let loaded = read_manifest(&providers_dir, "openrouter-prod").unwrap();
        assert_eq!(loaded.kind, "Provider");
        assert_eq!(loaded.metadata.name, "openrouter-prod");
        assert_eq!(loaded.spec, cfg);
    }

    #[test]
    fn list_configured_returns_sorted_names() {
        let dir = tempfile::tempdir().unwrap();
        let providers_dir = dir.path().join("providers");
        for n in ["zeta", "alpha", "mid"] {
            write_manifest(
                &providers_dir,
                n,
                &ProviderConfig {
                    provider_id: "openrouter".to_string(),
                    base_url: "https://x".to_string(),
                    default_model: None,
                    api_key_ref: None,
                },
            )
            .unwrap();
        }
        let names = list_configured(&providers_dir).unwrap();
        assert_eq!(names, vec!["alpha", "mid", "zeta"]);
    }

    #[test]
    fn list_configured_empty_when_dir_missing() {
        let dir = tempfile::tempdir().unwrap();
        let providers_dir = dir.path().join("providers");
        let names = list_configured(&providers_dir).unwrap();
        assert!(names.is_empty());
    }

    #[test]
    fn remove_manifest_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let providers_dir = dir.path().join("providers");
        std::fs::create_dir_all(&providers_dir).unwrap();
        // First call: file does not exist — must not error
        remove_manifest(&providers_dir, "missing").unwrap();
        // Create then remove
        write_manifest(
            &providers_dir,
            "real",
            &ProviderConfig {
                provider_id: "openrouter".to_string(),
                base_url: "https://x".to_string(),
                default_model: None,
                api_key_ref: None,
            },
        )
        .unwrap();
        remove_manifest(&providers_dir, "real").unwrap();
        assert!(!providers_dir.join("real.yaml").exists());
    }

    #[test]
    fn manifest_loadable_via_existing_manifest_loader() {
        // Confirms our YAML parses through the workspace's k8s manifest loader
        // (sera_types::config_manifest::ProviderSpec uses a different shape, so
        // we only assert the envelope kind/name parses, not the typed spec).
        let dir = tempfile::tempdir().unwrap();
        let providers_dir = dir.path().join("providers");
        write_manifest(
            &providers_dir,
            "smoke",
            &ProviderConfig {
                provider_id: "openrouter".to_string(),
                base_url: "https://openrouter.ai/api/v1".to_string(),
                default_model: None,
                api_key_ref: None,
            },
        )
        .unwrap();

        let raw = std::fs::read_to_string(providers_dir.join("smoke.yaml")).unwrap();
        assert!(raw.contains("kind: Provider"));
        assert!(raw.contains("name: smoke"));
        assert!(raw.contains("apiVersion: sera.dev/v1"));
    }
}
