//! Chained secrets provider — tries multiple providers in order for fallback.

use async_trait::async_trait;

use crate::{SecretsError, SecretsProvider};

/// Tries multiple providers in order, returning the first successful result.
///
/// - `get_secret`: Returns the first `Ok` value found across providers.
/// - `list_keys`: Merges and deduplicates keys from all providers.
/// - `store`: Delegates to the first provider that does not return `ReadOnly`.
/// - `delete`: Tries each provider in order, returns the first `Ok`.
pub struct ChainedSecretsProvider {
    providers: Vec<Box<dyn SecretsProvider>>,
}

impl ChainedSecretsProvider {
    pub fn new(providers: Vec<Box<dyn SecretsProvider>>) -> Self {
        Self { providers }
    }
}

#[async_trait]
impl SecretsProvider for ChainedSecretsProvider {
    fn provider_name(&self) -> &str {
        "chained"
    }

    async fn get_secret(&self, key: &str) -> Result<String, SecretsError> {
        let mut last_err = SecretsError::NotFound {
            key: key.to_owned(),
        };
        for provider in &self.providers {
            match provider.get_secret(key).await {
                Ok(val) => return Ok(val),
                Err(e) => last_err = e,
            }
        }
        Err(last_err)
    }

    async fn list_keys(&self) -> Result<Vec<String>, SecretsError> {
        let mut all_keys: Vec<String> = Vec::new();
        for provider in &self.providers {
            if let Ok(keys) = provider.list_keys().await {
                for key in keys {
                    if !all_keys.contains(&key) {
                        all_keys.push(key);
                    }
                }
            }
        }
        Ok(all_keys)
    }

    async fn store(&self, key: &str, value: &str) -> Result<(), SecretsError> {
        for provider in &self.providers {
            match provider.store(key, value).await {
                Ok(()) => return Ok(()),
                Err(SecretsError::ReadOnly) => continue,
                Err(e) => return Err(e),
            }
        }
        Err(SecretsError::ReadOnly)
    }

    async fn delete(&self, key: &str) -> Result<(), SecretsError> {
        let mut last_err = SecretsError::NotFound {
            key: key.to_owned(),
        };
        for provider in &self.providers {
            match provider.delete(key).await {
                Ok(()) => return Ok(()),
                Err(e) => last_err = e,
            }
        }
        Err(last_err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EnvSecretsProvider, FileSecretsProvider};

    #[tokio::test]
    async fn test_get_falls_back_to_second_provider() {
        let dir = tempfile::tempdir().unwrap();
        let file_p = FileSecretsProvider::new(dir.path());
        file_p.store("FALLBACK_KEY", "from_file").await.unwrap();

        let chain = ChainedSecretsProvider::new(vec![
            Box::new(EnvSecretsProvider),
            Box::new(FileSecretsProvider::new(dir.path())),
        ]);

        // Key not in env — should fall back to file provider
        let val = chain.get_secret("FALLBACK_KEY").await.unwrap();
        assert_eq!(val, "from_file");
    }

    #[tokio::test]
    async fn test_get_returns_first_provider_value() {
        // SAFETY: single-threaded test, no concurrent env access
        unsafe { std::env::set_var("SERA_SECRET_CHAIN_FIRST_77", "from_env") };
        let dir = tempfile::tempdir().unwrap();
        let file_p = FileSecretsProvider::new(dir.path());
        file_p.store("CHAIN_FIRST_77", "from_file").await.unwrap();

        let chain = ChainedSecretsProvider::new(vec![
            Box::new(EnvSecretsProvider),
            Box::new(FileSecretsProvider::new(dir.path())),
        ]);

        let val = chain.get_secret("CHAIN_FIRST_77").await.unwrap();
        assert_eq!(val, "from_env");
        unsafe { std::env::remove_var("SERA_SECRET_CHAIN_FIRST_77") };
    }

    #[tokio::test]
    async fn test_list_keys_merges_and_deduplicates() {
        // SAFETY: single-threaded test, no concurrent env access
        unsafe { std::env::set_var("SERA_SECRET_CHAIN_LIST_SHARED", "x") };
        let dir = tempfile::tempdir().unwrap();
        let file_p = FileSecretsProvider::new(dir.path());
        file_p.store("CHAIN_LIST_SHARED", "x").await.unwrap();
        file_p.store("CHAIN_LIST_ONLY_FILE", "y").await.unwrap();

        let chain = ChainedSecretsProvider::new(vec![
            Box::new(EnvSecretsProvider),
            Box::new(FileSecretsProvider::new(dir.path())),
        ]);

        let keys = chain.list_keys().await.unwrap();
        let shared_count = keys.iter().filter(|k| k.as_str() == "CHAIN_LIST_SHARED").count();
        assert_eq!(shared_count, 1, "duplicate key should appear only once");
        assert!(keys.contains(&"CHAIN_LIST_ONLY_FILE".to_string()));
        unsafe { std::env::remove_var("SERA_SECRET_CHAIN_LIST_SHARED") };
    }

    #[tokio::test]
    async fn test_store_skips_read_only_and_uses_writable() {
        let dir = tempfile::tempdir().unwrap();
        // Env is read-only, file is writable — store should use file
        let chain = ChainedSecretsProvider::new(vec![
            Box::new(EnvSecretsProvider),
            Box::new(FileSecretsProvider::new(dir.path())),
        ]);

        chain.store("CHAIN_STORE_KEY", "value").await.unwrap();
        assert!(dir.path().join("CHAIN_STORE_KEY").exists());
    }

    #[tokio::test]
    async fn test_get_not_found_when_all_fail() {
        let chain = ChainedSecretsProvider::new(vec![Box::new(EnvSecretsProvider)]);
        let err = chain.get_secret("DEFINITELY_NOT_SET_XYZ_999").await.unwrap_err();
        assert!(matches!(err, SecretsError::NotFound { .. }));
    }

    #[tokio::test]
    async fn test_provider_name() {
        let chain = ChainedSecretsProvider::new(vec![]);
        assert_eq!(chain.provider_name(), "chained");
    }

    #[tokio::test]
    async fn test_get_empty_chain_returns_not_found() {
        let chain = ChainedSecretsProvider::new(vec![]);
        let err = chain.get_secret("ANY_KEY").await.unwrap_err();
        assert!(matches!(err, SecretsError::NotFound { .. }));
    }

    #[tokio::test]
    async fn test_store_all_read_only_returns_read_only() {
        // Both env providers are read-only — chained store should propagate ReadOnly
        let chain = ChainedSecretsProvider::new(vec![
            Box::new(EnvSecretsProvider),
            Box::new(EnvSecretsProvider),
        ]);
        let err = chain.store("KEY", "val").await.unwrap_err();
        assert!(matches!(err, SecretsError::ReadOnly));
    }

    #[tokio::test]
    async fn test_delete_falls_back_to_second_provider() {
        let dir = tempfile::tempdir().unwrap();
        let p = FileSecretsProvider::new(dir.path());
        p.store("DELETE_FALLBACK_KEY", "v").await.unwrap();

        // First provider (env) is read-only — delete falls through to file provider
        let chain = ChainedSecretsProvider::new(vec![
            Box::new(EnvSecretsProvider),
            Box::new(FileSecretsProvider::new(dir.path())),
        ]);
        chain.delete("DELETE_FALLBACK_KEY").await.unwrap();
        assert!(!dir.path().join("DELETE_FALLBACK_KEY").exists());
    }

    #[tokio::test]
    async fn test_delete_empty_chain_returns_not_found() {
        let chain = ChainedSecretsProvider::new(vec![]);
        let err = chain.delete("ANY_KEY").await.unwrap_err();
        assert!(matches!(err, SecretsError::NotFound { .. }));
    }

    #[tokio::test]
    async fn test_list_keys_empty_chain_returns_empty() {
        let chain = ChainedSecretsProvider::new(vec![]);
        let keys = chain.list_keys().await.unwrap();
        assert!(keys.is_empty());
    }

    #[tokio::test]
    async fn test_list_keys_ignores_provider_errors() {
        // A provider pointing at a missing dir will error on list_keys;
        // chained should skip it and return keys from the working provider.
        let dir = tempfile::tempdir().unwrap();
        let p = FileSecretsProvider::new(dir.path());
        p.store("GOOD_KEY", "v").await.unwrap();

        let missing_dir = dir.path().join("nonexistent");
        let chain = ChainedSecretsProvider::new(vec![
            Box::new(FileSecretsProvider::new(&missing_dir)), // will error
            Box::new(FileSecretsProvider::new(dir.path())),   // has GOOD_KEY
        ]);
        let keys = chain.list_keys().await.unwrap();
        assert!(keys.contains(&"GOOD_KEY".to_string()));
    }
}
