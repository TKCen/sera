//! API key validation.

use crate::error::AuthError;
use crate::types::ActingContext;

/// A stored API key with associated metadata.
#[derive(Debug, Clone)]
pub struct StoredApiKey {
    /// Hash of the key (for plaintext comparison during alpha)
    pub key_hash: String,
    /// ID of the operator who owns this key
    pub operator_id: String,
    /// Unique identifier for this key
    pub key_id: String,
}

/// Validator for API keys.
pub struct ApiKeyValidator;

impl ApiKeyValidator {
    /// Validate an API key token against a list of stored keys.
    ///
    /// # Arguments
    ///
    /// * `token` - The API key token to validate
    /// * `stored_keys` - List of valid stored API keys
    ///
    /// # Returns
    ///
    /// An `ActingContext` if the key matches, or an error if invalid.
    pub fn validate(token: &str, stored_keys: &[StoredApiKey]) -> Result<ActingContext, AuthError> {
        // Find a matching key
        let key = stored_keys
            .iter()
            .find(|k| k.key_hash == token)
            .ok_or(AuthError::Unauthorized)?;

        // Return acting context with the operator ID and key ID
        Ok(ActingContext {
            operator_id: Some(key.operator_id.clone()),
            agent_id: None,
            instance_id: None,
            api_key_id: Some(key.key_id.clone()),
            auth_method: crate::types::AuthMethod::ApiKey,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_key_matches() {
        let stored_keys = vec![
            StoredApiKey {
                key_hash: "key123".to_string(),
                operator_id: "op-456".to_string(),
                key_id: "api-key-id-1".to_string(),
            },
            StoredApiKey {
                key_hash: "key456".to_string(),
                operator_id: "op-789".to_string(),
                key_id: "api-key-id-2".to_string(),
            },
        ];

        let result = ApiKeyValidator::validate("key123", &stored_keys);
        assert!(result.is_ok());

        let ctx = result.unwrap();
        assert_eq!(ctx.operator_id, Some("op-456".to_string()));
        assert_eq!(ctx.api_key_id, Some("api-key-id-1".to_string()));
        assert_eq!(ctx.auth_method, crate::types::AuthMethod::ApiKey);
    }

    #[test]
    fn test_invalid_key_returns_error() {
        let stored_keys = vec![
            StoredApiKey {
                key_hash: "key123".to_string(),
                operator_id: "op-456".to_string(),
                key_id: "api-key-id-1".to_string(),
            },
        ];

        let result = ApiKeyValidator::validate("wrong-key", &stored_keys);
        assert!(matches!(result, Err(AuthError::Unauthorized)));
    }

    #[test]
    fn test_empty_keys_list_returns_error() {
        let stored_keys = vec![];

        let result = ApiKeyValidator::validate("key123", &stored_keys);
        assert!(matches!(result, Err(AuthError::Unauthorized)));
    }
}
