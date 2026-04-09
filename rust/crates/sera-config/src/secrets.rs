//! File-based secrets provider with env var fallback.
//! Secrets are stored as plain text files in a secrets directory,
//! with the file path mirroring the secret reference path.
//! E.g., secret ref "connectors/discord-main/token" → file "secrets/connectors/discord-main/token"

use std::fs;
use std::path::{Path, PathBuf};

/// A secrets resolver that checks file-based storage first, then env vars.
pub struct SecretResolver {
    secrets_dir: PathBuf,
}

impl SecretResolver {
    /// Create a resolver with a specific secrets directory.
    pub fn new(secrets_dir: impl Into<PathBuf>) -> Self {
        Self {
            secrets_dir: secrets_dir.into(),
        }
    }

    /// Create a resolver using the default secrets dir relative to a base path.
    /// E.g., base="/home/user/.sera" → secrets_dir="/home/user/.sera/secrets"
    pub fn from_base(base: &Path) -> Self {
        Self {
            secrets_dir: base.join("secrets"),
        }
    }

    /// Resolve a secret reference path to its value.
    /// Priority: 1) file in secrets_dir, 2) SERA_SECRET_* env var
    pub fn resolve(&self, secret_path: &str) -> Option<String> {
        // Try file first
        if let Some(value) = self.resolve_from_file(secret_path) {
            return Some(value);
        }
        // Fall back to env var
        resolve_from_env(secret_path)
    }

    /// Store a secret value to a file.
    pub fn store(&self, secret_path: &str, value: &str) -> std::io::Result<()> {
        let file_path = self.secrets_dir.join(secret_path);
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&file_path, value.trim())
    }

    /// Delete a stored secret.
    pub fn delete(&self, secret_path: &str) -> std::io::Result<()> {
        let file_path = self.secrets_dir.join(secret_path);
        if file_path.exists() {
            fs::remove_file(&file_path)?;
        }
        Ok(())
    }

    /// List all secret paths in the directory.
    pub fn list(&self) -> Vec<String> {
        let mut paths = Vec::new();
        if self.secrets_dir.is_dir() {
            self.collect_paths(&self.secrets_dir, &mut paths);
        }
        paths
    }

    /// Check if a secret exists (in file or env).
    pub fn exists(&self, secret_path: &str) -> bool {
        self.secrets_dir.join(secret_path).is_file()
            || resolve_from_env(secret_path).is_some()
    }

    fn resolve_from_file(&self, secret_path: &str) -> Option<String> {
        let file_path = self.secrets_dir.join(secret_path);
        fs::read_to_string(&file_path)
            .ok()
            .map(|s| s.trim().to_string())
    }

    fn collect_paths(&self, dir: &Path, out: &mut Vec<String>) {
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    self.collect_paths(&path, out);
                } else if let Ok(rel) = path.strip_prefix(&self.secrets_dir) {
                    out.push(rel.to_string_lossy().replace('\\', "/"));
                }
            }
        }
    }
}

/// Resolve from SERA_SECRET_* env var (the existing logic).
pub fn resolve_from_env(secret_path: &str) -> Option<String> {
    let env_key = format!(
        "SERA_SECRET_{}",
        secret_path.to_uppercase().replace(['/', '-'], "_")
    );
    std::env::var(&env_key).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use tempfile::tempdir;

    #[test]
    fn resolve_from_file_reads_stored_value() {
        let dir = tempdir().unwrap();
        let resolver = SecretResolver::new(dir.path());
        resolver
            .store("connectors/discord/token", "secret-value")
            .unwrap();
        let result = resolver.resolve("connectors/discord/token");
        assert_eq!(result, Some("secret-value".to_string()));
    }

    #[test]
    fn resolve_falls_back_to_env() {
        let dir = tempdir().unwrap();
        let resolver = SecretResolver::new(dir.path());
        // SAFETY: test-only env mutation
        unsafe { env::set_var("SERA_SECRET_PROVIDERS_OPENAI_APIKEY", "env-secret") };
        let result = resolver.resolve("providers/openai/apikey");
        unsafe { env::remove_var("SERA_SECRET_PROVIDERS_OPENAI_APIKEY") };
        assert_eq!(result, Some("env-secret".to_string()));
    }

    #[test]
    fn store_then_resolve() {
        let dir = tempdir().unwrap();
        let resolver = SecretResolver::new(dir.path());
        resolver.store("my/secret/path", "hello").unwrap();
        assert_eq!(resolver.resolve("my/secret/path"), Some("hello".to_string()));
    }

    #[test]
    fn store_then_delete_then_resolve_returns_none() {
        let dir = tempdir().unwrap();
        let resolver = SecretResolver::new(dir.path());
        resolver.store("some/secret", "value").unwrap();
        resolver.delete("some/secret").unwrap();
        // No env var set, so should return None
        assert_eq!(resolver.resolve("some/secret"), None);
    }

    #[test]
    fn list_returns_stored_paths() {
        let dir = tempdir().unwrap();
        let resolver = SecretResolver::new(dir.path());
        resolver.store("a/b/c", "v1").unwrap();
        resolver.store("a/d", "v2").unwrap();
        let mut paths = resolver.list();
        paths.sort();
        assert_eq!(paths, vec!["a/b/c", "a/d"]);
    }

    #[test]
    fn exists_checks_file() {
        let dir = tempdir().unwrap();
        let resolver = SecretResolver::new(dir.path());
        assert!(!resolver.exists("foo/bar"));
        resolver.store("foo/bar", "baz").unwrap();
        assert!(resolver.exists("foo/bar"));
    }

    #[test]
    fn exists_checks_env() {
        let dir = tempdir().unwrap();
        let resolver = SecretResolver::new(dir.path());
        unsafe { env::set_var("SERA_SECRET_FOO_BAR", "exists") };
        let result = resolver.exists("foo/bar");
        unsafe { env::remove_var("SERA_SECRET_FOO_BAR") };
        assert!(result);
    }

    #[test]
    fn store_trims_trailing_newline() {
        let dir = tempdir().unwrap();
        let resolver = SecretResolver::new(dir.path());
        resolver.store("trim/test", "token\n").unwrap();
        assert_eq!(resolver.resolve("trim/test"), Some("token".to_string()));
    }
}
