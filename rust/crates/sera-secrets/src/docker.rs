//! Docker secrets provider — reads from /run/secrets/ (or a custom path).

use std::path::PathBuf;

use async_trait::async_trait;

use crate::{SecretsError, SecretsProvider};

/// Reads Docker-mounted secrets from a directory (default: `/run/secrets/`).
///
/// Docker Swarm / Compose mounts each secret as a file named after the secret key.
/// Store and delete are not supported (read-only).
pub struct DockerSecretsProvider {
    secrets_dir: PathBuf,
}

impl DockerSecretsProvider {
    /// Create a provider reading from the standard Docker secrets path (`/run/secrets/`).
    pub fn new() -> Self {
        Self {
            secrets_dir: PathBuf::from("/run/secrets"),
        }
    }

    /// Create a provider reading from a custom path (useful for testing).
    pub fn with_path(path: impl Into<PathBuf>) -> Self {
        Self {
            secrets_dir: path.into(),
        }
    }
}

impl Default for DockerSecretsProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SecretsProvider for DockerSecretsProvider {
    fn provider_name(&self) -> &str {
        "docker"
    }

    async fn get_secret(&self, key: &str) -> Result<String, SecretsError> {
        let path = self.secrets_dir.join(key);
        let contents = std::fs::read_to_string(&path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                SecretsError::NotFound {
                    key: key.to_owned(),
                }
            } else {
                SecretsError::Io {
                    reason: e.to_string(),
                }
            }
        })?;
        Ok(contents.trim_end().to_owned())
    }

    async fn list_keys(&self) -> Result<Vec<String>, SecretsError> {
        let read_dir = std::fs::read_dir(&self.secrets_dir).map_err(|e| SecretsError::Io {
            reason: e.to_string(),
        })?;
        let mut keys = Vec::new();
        for entry in read_dir {
            let entry = entry.map_err(|e| SecretsError::Io {
                reason: e.to_string(),
            })?;
            let file_type = entry.file_type().map_err(|e| SecretsError::Io {
                reason: e.to_string(),
            })?;
            if file_type.is_file()
                && let Some(name) = entry.file_name().to_str()
            {
                keys.push(name.to_owned());
            }
        }
        Ok(keys)
    }

    async fn store(&self, _key: &str, _value: &str) -> Result<(), SecretsError> {
        Err(SecretsError::ReadOnly)
    }

    async fn delete(&self, _key: &str) -> Result<(), SecretsError> {
        Err(SecretsError::ReadOnly)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_get_secret_reads_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("MY_KEY"), "secret_value\n").unwrap();
        let p = DockerSecretsProvider::with_path(dir.path());
        let val = p.get_secret("MY_KEY").await.unwrap();
        assert_eq!(val, "secret_value");
    }

    #[tokio::test]
    async fn test_get_secret_trims_trailing_whitespace() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("WS_KEY"), "value  \n  ").unwrap();
        let p = DockerSecretsProvider::with_path(dir.path());
        let val = p.get_secret("WS_KEY").await.unwrap();
        assert_eq!(val, "value");
    }

    #[tokio::test]
    async fn test_get_secret_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let p = DockerSecretsProvider::with_path(dir.path());
        let err = p.get_secret("MISSING").await.unwrap_err();
        assert!(matches!(err, SecretsError::NotFound { .. }));
    }

    #[tokio::test]
    async fn test_list_keys() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("KEY_A"), "a").unwrap();
        std::fs::write(dir.path().join("KEY_B"), "b").unwrap();
        let p = DockerSecretsProvider::with_path(dir.path());
        let mut keys = p.list_keys().await.unwrap();
        keys.sort();
        assert_eq!(keys, vec!["KEY_A", "KEY_B"]);
    }

    #[tokio::test]
    async fn test_store_returns_read_only() {
        let dir = tempfile::tempdir().unwrap();
        let p = DockerSecretsProvider::with_path(dir.path());
        let err = p.store("K", "v").await.unwrap_err();
        assert!(matches!(err, SecretsError::ReadOnly));
    }

    #[tokio::test]
    async fn test_delete_returns_read_only() {
        let dir = tempfile::tempdir().unwrap();
        let p = DockerSecretsProvider::with_path(dir.path());
        let err = p.delete("K").await.unwrap_err();
        assert!(matches!(err, SecretsError::ReadOnly));
    }

    #[tokio::test]
    async fn test_provider_name() {
        let p = DockerSecretsProvider::new();
        assert_eq!(p.provider_name(), "docker");
    }

    #[tokio::test]
    async fn test_default_path_is_run_secrets() {
        let p = DockerSecretsProvider::default();
        assert_eq!(p.secrets_dir, std::path::PathBuf::from("/run/secrets"));
    }

    #[tokio::test]
    async fn test_get_secret_empty_file() {
        // An empty secret file should return Ok("") after trim_end
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("EMPTY_SECRET"), "").unwrap();
        let p = DockerSecretsProvider::with_path(dir.path());
        let val = p.get_secret("EMPTY_SECRET").await.unwrap();
        assert_eq!(val, "");
    }

    #[tokio::test]
    async fn test_list_keys_missing_dir_returns_io_error() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("nonexistent_subdir");
        let p = DockerSecretsProvider::with_path(&missing);
        let err = p.list_keys().await.unwrap_err();
        assert!(matches!(err, SecretsError::Io { .. }));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_get_secret_permission_denied_returns_io_error() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let secret_path = dir.path().join("LOCKED_SECRET");
        std::fs::write(&secret_path, "classified").unwrap();
        // Remove all read permissions
        std::fs::set_permissions(&secret_path, std::fs::Permissions::from_mode(0o000)).unwrap();
        let p = DockerSecretsProvider::with_path(dir.path());
        let err = p.get_secret("LOCKED_SECRET").await.unwrap_err();
        // Restore permissions so tempdir cleanup can delete the file
        std::fs::set_permissions(&secret_path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(
            matches!(err, SecretsError::Io { .. }),
            "permission denied should map to SecretsError::Io, got: {:?}",
            err
        );
    }

    #[tokio::test]
    async fn test_list_keys_excludes_subdirectories() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("SECRET_FILE"), "val").unwrap();
        std::fs::create_dir(dir.path().join("subdir")).unwrap();
        let p = DockerSecretsProvider::with_path(dir.path());
        let keys = p.list_keys().await.unwrap();
        assert!(keys.contains(&"SECRET_FILE".to_string()));
        assert!(!keys.contains(&"subdir".to_string()), "subdirs must not appear in list_keys");
    }
}
