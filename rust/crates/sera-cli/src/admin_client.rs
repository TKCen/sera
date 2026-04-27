//! Admin HTTP client used by the L.4 CLI verbs.
//!
//! Wraps a `reqwest::Client` that injects `Authorization: Bearer <admin-token>`
//! on every request. Endpoint + token come from the L.2 Instance manifest at
//! `<config-root>/config.yaml`:
//! - `gateway.mode == "local"`  → endpoint `http://127.0.0.1:3002`, token from
//!   `SERA_ADMIN_TOKEN` (matches the gateway's local-mode regenerate-each-boot
//!   token print) or, if absent, the `KeyringRef` at `gateway.admin_token_ref`
//!   when one was stored by `sera init`.
//! - `gateway.mode == "remote"` → endpoint from `gateway.endpoint`, token from
//!   `gateway.admin_token_ref` via the `SecretStore`.
//!
//! Override at call time with the `--endpoint` / `--admin-token` flags wired
//! through `CommandArgs`.

use std::path::Path;

use anyhow::{Context, Result, anyhow};
use reqwest::Client;
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};
use sera_config::ConfigRoot;
use sera_config::provider_wizard::{KeyringRef, KeyringSecretStore, SecretStore};

/// Default admin port — matches `sera_gateway::admin::DEFAULT_ADMIN_PORT`.
pub const DEFAULT_ADMIN_PORT: u16 = 3002;

/// Resolved gateway connection.
#[derive(Debug, Clone)]
pub struct AdminTarget {
    pub endpoint: String,
    pub token: String,
}

/// Thin wrapper around a `reqwest::Client` with the admin auth header pre-set.
#[derive(Debug, Clone)]
pub struct AdminClient {
    endpoint: String,
    client: Client,
}

impl AdminClient {
    /// Build a client from a resolved [`AdminTarget`].
    pub fn new(target: AdminTarget) -> Result<Self> {
        let mut headers = HeaderMap::new();
        let mut value = HeaderValue::from_str(&format!("Bearer {}", target.token))
            .context("invalid admin token value")?;
        value.set_sensitive(true);
        headers.insert(AUTHORIZATION, value);
        let client = Client::builder()
            .default_headers(headers)
            .build()
            .context("failed to build admin HTTP client")?;
        Ok(Self {
            endpoint: target.endpoint,
            client,
        })
    }

    /// Endpoint base URL (no trailing slash).
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    /// Underlying reqwest client (auth header is pre-applied).
    pub fn http(&self) -> &Client {
        &self.client
    }

    /// Build a full URL for an `/admin/...` path fragment.
    pub fn url(&self, path: &str) -> String {
        let path = path.trim_start_matches('/');
        format!("{}/{path}", self.endpoint.trim_end_matches('/'))
    }
}

/// Resolve the admin target from the L.2 Instance manifest under `root`.
///
/// `endpoint_override` and `token_override` come from per-command CLI flags
/// (`--endpoint`, `--admin-token`) and short-circuit the manifest lookup.
pub fn resolve_target(
    root: &ConfigRoot,
    endpoint_override: Option<&str>,
    token_override: Option<&str>,
    secrets: &dyn SecretStore,
) -> Result<AdminTarget> {
    // Skip the manifest read entirely when both pieces are overridden — this
    // keeps tests and ad-hoc invocations from needing a config.yaml on disk.
    if let (Some(e), Some(t)) = (endpoint_override, token_override) {
        return Ok(AdminTarget {
            endpoint: e.trim_end_matches('/').to_string(),
            token: t.to_string(),
        });
    }

    let manifest_path = root.path().join("config.yaml");
    let parsed = read_instance_manifest(&manifest_path)?;

    let endpoint = match endpoint_override {
        Some(e) => e.trim_end_matches('/').to_string(),
        None => resolve_endpoint(&parsed)?,
    };

    let token = match token_override {
        Some(t) => t.to_string(),
        None => resolve_token(&parsed, secrets)?,
    };

    Ok(AdminTarget { endpoint, token })
}

/// Production helper that uses the system keyring as the secret store.
pub fn resolve_target_default(
    root: &ConfigRoot,
    endpoint_override: Option<&str>,
    token_override: Option<&str>,
) -> Result<AdminTarget> {
    resolve_target(root, endpoint_override, token_override, &KeyringSecretStore)
}

#[derive(Debug, Clone)]
struct InstanceManifest {
    mode: String,
    endpoint: Option<String>,
    admin_token_ref: Option<KeyringRef>,
}

fn read_instance_manifest(path: &Path) -> Result<InstanceManifest> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read instance manifest: {}", path.display()))?;
    let doc: serde_yaml::Value = serde_yaml::from_str(&raw)
        .with_context(|| format!("failed to parse instance manifest: {}", path.display()))?;
    let gateway = doc
        .get("spec")
        .and_then(|s| s.get("gateway"))
        .ok_or_else(|| {
            anyhow!(
                "instance manifest at {} is missing spec.gateway",
                path.display()
            )
        })?;

    let mode = gateway
        .get("mode")
        .and_then(|v| v.as_str())
        .unwrap_or("local")
        .to_string();
    let endpoint = gateway
        .get("endpoint")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let admin_token_ref = gateway
        .get("admin_token_ref")
        .and_then(|v| {
            let svc = v.get("keyring_service")?.as_str()?.to_string();
            let entry = v.get("keyring_entry")?.as_str()?.to_string();
            Some(KeyringRef {
                keyring_service: svc,
                keyring_entry: entry,
            })
        });

    Ok(InstanceManifest {
        mode,
        endpoint,
        admin_token_ref,
    })
}

fn resolve_endpoint(m: &InstanceManifest) -> Result<String> {
    if m.mode == "local" {
        return Ok(format!("http://127.0.0.1:{DEFAULT_ADMIN_PORT}"));
    }
    let raw = m
        .endpoint
        .as_deref()
        .ok_or_else(|| anyhow!("remote gateway has no endpoint configured"))?;
    Ok(raw.trim_end_matches('/').to_string())
}

fn resolve_token(m: &InstanceManifest, secrets: &dyn SecretStore) -> Result<String> {
    if let Ok(t) = std::env::var("SERA_ADMIN_TOKEN")
        && !t.trim().is_empty()
    {
        return Ok(t);
    }
    let kref = m
        .admin_token_ref
        .as_ref()
        .ok_or_else(|| anyhow!(
            "no admin token configured: set SERA_ADMIN_TOKEN or run `sera init` to store one"
        ))?;
    secrets
        .get(&kref.keyring_service, &kref.keyring_entry)
        .map_err(|e| anyhow!("failed to read admin token from secret store: {e}"))?
        .ok_or_else(|| {
            anyhow!(
                "admin token reference {}::{} resolved to nothing",
                kref.keyring_service,
                kref.keyring_entry
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use sera_config::provider_wizard::test_support::MockSecretStore;
    use std::env;
    use std::sync::{Mutex, OnceLock};

    /// Serialise tests that mutate `SERA_ADMIN_TOKEN` — Rust runs unit tests
    /// concurrently within one binary, and process-global env vars race
    /// without a guard.
    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn write_manifest(dir: &Path, body: &str) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("config.yaml"), body).unwrap();
    }

    struct EnvGuard {
        saved: Option<String>,
    }

    impl EnvGuard {
        fn new(set_to: Option<&str>) -> Self {
            let saved = env::var("SERA_ADMIN_TOKEN").ok();
            // SAFETY: tests serialise via env_lock().
            unsafe {
                match set_to {
                    Some(v) => env::set_var("SERA_ADMIN_TOKEN", v),
                    None => env::remove_var("SERA_ADMIN_TOKEN"),
                }
            }
            Self { saved }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            unsafe {
                match &self.saved {
                    Some(v) => env::set_var("SERA_ADMIN_TOKEN", v),
                    None => env::remove_var("SERA_ADMIN_TOKEN"),
                }
            }
        }
    }

    #[test]
    fn local_mode_uses_loopback_admin_port() {
        let _guard = env_lock().lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        write_manifest(
            dir.path(),
            "apiVersion: sera.dev/v1\nkind: Instance\nmetadata:\n  name: default\nspec:\n  gateway:\n    mode: local\n  data_dir: /tmp\n",
        );
        let _env = EnvGuard::new(Some("tok-xyz"));
        let root = ConfigRoot::new(dir.path());
        let target =
            resolve_target(&root, None, None, &MockSecretStore::default()).unwrap();
        assert_eq!(target.endpoint, format!("http://127.0.0.1:{DEFAULT_ADMIN_PORT}"));
        assert_eq!(target.token, "tok-xyz");
    }

    #[test]
    fn remote_mode_uses_endpoint_and_secret_store() {
        let _guard = env_lock().lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        write_manifest(
            dir.path(),
            "apiVersion: sera.dev/v1\nkind: Instance\nmetadata:\n  name: default\nspec:\n  gateway:\n    mode: remote\n    endpoint: https://gw.example.com\n    admin_token_ref:\n      keyring_service: sera-gateway\n      keyring_entry: default\n  data_dir: /tmp\n",
        );
        let _env = EnvGuard::new(None);
        let secrets = MockSecretStore::default();
        secrets
            .set("sera-gateway", "default", "remote-token")
            .unwrap();
        let root = ConfigRoot::new(dir.path());
        let target = resolve_target(&root, None, None, &secrets).unwrap();
        assert_eq!(target.endpoint, "https://gw.example.com");
        assert_eq!(target.token, "remote-token");
    }

    #[test]
    fn endpoint_override_short_circuits_manifest() {
        let _guard = env_lock().lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        write_manifest(
            dir.path(),
            "apiVersion: sera.dev/v1\nkind: Instance\nmetadata:\n  name: default\nspec:\n  gateway:\n    mode: local\n  data_dir: /tmp\n",
        );
        let _env = EnvGuard::new(None);
        let root = ConfigRoot::new(dir.path());
        let target = resolve_target(
            &root,
            Some("http://override.local:9999/"),
            Some("explicit-token"),
            &MockSecretStore::default(),
        )
        .unwrap();
        assert_eq!(target.endpoint, "http://override.local:9999");
        assert_eq!(target.token, "explicit-token");
    }

    #[test]
    fn missing_token_returns_helpful_error() {
        let _guard = env_lock().lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        write_manifest(
            dir.path(),
            "apiVersion: sera.dev/v1\nkind: Instance\nmetadata:\n  name: default\nspec:\n  gateway:\n    mode: local\n  data_dir: /tmp\n",
        );
        let _env = EnvGuard::new(None);
        let root = ConfigRoot::new(dir.path());
        let err = resolve_target(&root, None, None, &MockSecretStore::default())
            .unwrap_err()
            .to_string();
        assert!(err.contains("SERA_ADMIN_TOKEN"), "got: {err}");
    }

    #[test]
    fn url_joins_admin_path() {
        let target = AdminTarget {
            endpoint: "http://127.0.0.1:3002".into(),
            token: "t".into(),
        };
        let client = AdminClient::new(target).unwrap();
        assert_eq!(client.url("/admin/workflows"), "http://127.0.0.1:3002/admin/workflows");
        assert_eq!(client.url("admin/workflows"), "http://127.0.0.1:3002/admin/workflows");
    }
}
