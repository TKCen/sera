//! Environment variable secrets provider.

use async_trait::async_trait;

use crate::{SecretsError, SecretsProvider};

/// Reads secrets from `SERA_SECRET_*` environment variables.
///
/// E.g. `SERA_SECRET_DB_PASSWORD` → key `DB_PASSWORD`, value from env.
/// Store and delete are not supported (read-only).
pub struct EnvSecretsProvider;

#[async_trait]
impl SecretsProvider for EnvSecretsProvider {
    fn provider_name(&self) -> &str {
        "env"
    }

    async fn get_secret(&self, key: &str) -> Result<String, SecretsError> {
        let env_key = format!("SERA_SECRET_{key}");
        std::env::var(&env_key).map_err(|_| SecretsError::NotFound {
            key: key.to_owned(),
        })
    }

    async fn list_keys(&self) -> Result<Vec<String>, SecretsError> {
        Ok(std::env::vars()
            .filter_map(|(k, _)| k.strip_prefix("SERA_SECRET_").map(|s| s.to_owned()))
            .collect())
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
    async fn test_get_secret_found() {
        // Use a unique prefix to avoid collisions with real env vars
        // SAFETY: single-threaded test, no concurrent env access
        unsafe { std::env::set_var("SERA_SECRET_TEST_ENV_GET_42", "hunter2") };
        let p = EnvSecretsProvider;
        let val = p.get_secret("TEST_ENV_GET_42").await.unwrap();
        assert_eq!(val, "hunter2");
        unsafe { std::env::remove_var("SERA_SECRET_TEST_ENV_GET_42") };
    }

    #[tokio::test]
    async fn test_get_secret_not_found() {
        let p = EnvSecretsProvider;
        let err = p.get_secret("TEST_ENV_DEFINITELY_MISSING_XYZ").await.unwrap_err();
        assert!(matches!(err, SecretsError::NotFound { .. }));
    }

    #[tokio::test]
    async fn test_list_keys_includes_set() {
        // SAFETY: single-threaded test, no concurrent env access
        unsafe { std::env::set_var("SERA_SECRET_TEST_ENV_LIST_99", "value") };
        let p = EnvSecretsProvider;
        let keys = p.list_keys().await.unwrap();
        assert!(keys.contains(&"TEST_ENV_LIST_99".to_string()));
        unsafe { std::env::remove_var("SERA_SECRET_TEST_ENV_LIST_99") };
    }

    #[tokio::test]
    async fn test_store_returns_read_only() {
        let p = EnvSecretsProvider;
        let err = p.store("FOO", "bar").await.unwrap_err();
        assert!(matches!(err, SecretsError::ReadOnly));
    }

    #[tokio::test]
    async fn test_delete_returns_read_only() {
        let p = EnvSecretsProvider;
        let err = p.delete("FOO").await.unwrap_err();
        assert!(matches!(err, SecretsError::ReadOnly));
    }

    #[tokio::test]
    async fn test_provider_name() {
        let p = EnvSecretsProvider;
        assert_eq!(p.provider_name(), "env");
    }

    #[tokio::test]
    async fn test_get_secret_empty_value() {
        // An env var set to "" is present but empty — should return Ok("") not NotFound
        // SAFETY: single-threaded test, no concurrent env access
        unsafe { std::env::set_var("SERA_SECRET_TEST_ENV_EMPTY_42", "") };
        let p = EnvSecretsProvider;
        let val = p.get_secret("TEST_ENV_EMPTY_42").await.unwrap();
        assert_eq!(val, "");
        unsafe { std::env::remove_var("SERA_SECRET_TEST_ENV_EMPTY_42") };
    }

    #[tokio::test]
    async fn test_get_secret_whitespace_value_preserved() {
        // Env provider does NOT trim — that is file/docker's responsibility
        // SAFETY: single-threaded test, no concurrent env access
        unsafe { std::env::set_var("SERA_SECRET_TEST_ENV_WS_77", "  value  ") };
        let p = EnvSecretsProvider;
        let val = p.get_secret("TEST_ENV_WS_77").await.unwrap();
        assert_eq!(val, "  value  ");
        unsafe { std::env::remove_var("SERA_SECRET_TEST_ENV_WS_77") };
    }

    #[tokio::test]
    async fn test_list_keys_excludes_non_prefixed_vars() {
        // Set a var WITHOUT the SERA_SECRET_ prefix — should never appear in list_keys
        // SAFETY: single-threaded test, no concurrent env access
        unsafe { std::env::set_var("NOT_A_SERA_SECRET_TEST_88", "x") };
        let p = EnvSecretsProvider;
        let keys = p.list_keys().await.unwrap();
        assert!(
            !keys.contains(&"NOT_A_SERA_SECRET_TEST_88".to_string()),
            "non-prefixed vars must not appear in list_keys"
        );
        unsafe { std::env::remove_var("NOT_A_SERA_SECRET_TEST_88") };
    }

    #[tokio::test]
    async fn test_list_keys_strips_prefix_correctly() {
        // SAFETY: single-threaded test, no concurrent env access
        unsafe { std::env::set_var("SERA_SECRET_MYAPP_TOKEN_55", "tok") };
        let p = EnvSecretsProvider;
        let keys = p.list_keys().await.unwrap();
        // Should appear as "MYAPP_TOKEN_55", not "SERA_SECRET_MYAPP_TOKEN_55"
        assert!(
            keys.contains(&"MYAPP_TOKEN_55".to_string()),
            "list_keys should strip the SERA_SECRET_ prefix"
        );
        assert!(
            !keys.contains(&"SERA_SECRET_MYAPP_TOKEN_55".to_string()),
            "list_keys should not include the full env var name"
        );
        unsafe { std::env::remove_var("SERA_SECRET_MYAPP_TOKEN_55") };
    }
}
