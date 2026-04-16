//! File-backed secrets provider — reads and writes secrets as files in a directory.

use std::path::PathBuf;

use async_trait::async_trait;

use crate::{SecretsError, SecretsProvider};

/// Reads and writes secrets as files in a configurable base directory.
///
/// Each secret is stored as a file named after the key. The directory is created
/// automatically on first write if it does not exist.
pub struct FileSecretsProvider {
    base_dir: PathBuf,
}

impl FileSecretsProvider {
    pub fn new(base_dir: impl Into<PathBuf>) -> Self {
        Self {
            base_dir: base_dir.into(),
        }
    }
}

#[async_trait]
impl SecretsProvider for FileSecretsProvider {
    fn provider_name(&self) -> &str {
        "file"
    }

    async fn get_secret(&self, key: &str) -> Result<String, SecretsError> {
        let path = self.base_dir.join(key);
        std::fs::read_to_string(&path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                SecretsError::NotFound {
                    key: key.to_owned(),
                }
            } else {
                SecretsError::Io {
                    reason: e.to_string(),
                }
            }
        })
    }

    async fn list_keys(&self) -> Result<Vec<String>, SecretsError> {
        let read_dir = std::fs::read_dir(&self.base_dir).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                SecretsError::Io {
                    reason: format!("base_dir does not exist: {}", self.base_dir.display()),
                }
            } else {
                SecretsError::Io {
                    reason: e.to_string(),
                }
            }
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

    async fn store(&self, key: &str, value: &str) -> Result<(), SecretsError> {
        std::fs::create_dir_all(&self.base_dir).map_err(|e| SecretsError::Io {
            reason: e.to_string(),
        })?;
        let path = self.base_dir.join(key);
        std::fs::write(&path, value).map_err(|e| SecretsError::Io {
            reason: e.to_string(),
        })
    }

    async fn delete(&self, key: &str) -> Result<(), SecretsError> {
        let path = self.base_dir.join(key);
        std::fs::remove_file(&path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                SecretsError::NotFound {
                    key: key.to_owned(),
                }
            } else {
                SecretsError::Io {
                    reason: e.to_string(),
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_crud_cycle() {
        let dir = tempfile::tempdir().unwrap();
        let p = FileSecretsProvider::new(dir.path());

        // Store
        p.store("MY_SECRET", "top_secret").await.unwrap();

        // Get
        let val = p.get_secret("MY_SECRET").await.unwrap();
        assert_eq!(val, "top_secret");

        // List
        let keys = p.list_keys().await.unwrap();
        assert!(keys.contains(&"MY_SECRET".to_string()));

        // Delete
        p.delete("MY_SECRET").await.unwrap();

        // Gone
        let err = p.get_secret("MY_SECRET").await.unwrap_err();
        assert!(matches!(err, SecretsError::NotFound { .. }));
    }

    #[tokio::test]
    async fn test_store_creates_directory() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("a").join("b").join("c");
        let p = FileSecretsProvider::new(&nested);
        p.store("KEY", "val").await.unwrap();
        assert!(nested.join("KEY").exists());
    }

    #[tokio::test]
    async fn test_delete_missing_returns_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let p = FileSecretsProvider::new(dir.path());
        let err = p.delete("NONEXISTENT").await.unwrap_err();
        assert!(matches!(err, SecretsError::NotFound { .. }));
    }

    #[tokio::test]
    async fn test_list_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        let p = FileSecretsProvider::new(dir.path());
        let keys = p.list_keys().await.unwrap();
        assert!(keys.is_empty());
    }
}
