//! API key validation with argon2 password hashing.
//!
//! Keys are stored in PHC string format (argon2id). Plaintext comparison is
//! never used — all validation goes through argon2 verification.

use crate::error::AuthError;
use crate::types::ActingContext;

/// A stored API key with associated metadata.
#[derive(Debug, Clone)]
pub struct StoredApiKey {
    /// Argon2id PHC-format hash of the raw key.
    pub key_hash_argon2: String,
    /// ID of the operator who owns this key.
    pub operator_id: String,
    /// Unique identifier for this key.
    pub key_id: String,
}

/// Validator for API keys.
pub struct ApiKeyValidator;

impl ApiKeyValidator {
    /// Validate an API key token against a list of stored keys.
    ///
    /// Performs constant-time argon2 verification against each stored key hash.
    /// Returns the first matching [`ActingContext`], or [`AuthError::Unauthorized`]
    /// if no key matches.
    pub fn validate(token: &str, stored_keys: &[StoredApiKey]) -> Result<ActingContext, AuthError> {
        #[cfg(feature = "basic-auth")]
        {
            use argon2::{Argon2, PasswordHash, PasswordVerifier};

            for key in stored_keys {
                let parsed_hash = match PasswordHash::new(&key.key_hash_argon2) {
                    Ok(h) => h,
                    Err(_) => continue, // skip malformed stored hashes
                };
                if Argon2::default()
                    .verify_password(token.as_bytes(), &parsed_hash)
                    .is_ok()
                {
                    return Ok(ActingContext {
                        operator_id: Some(key.operator_id.clone()),
                        agent_id: None,
                        instance_id: None,
                        api_key_id: Some(key.key_id.clone()),
                        auth_method: crate::types::AuthMethod::ApiKey,
                    });
                }
            }
            Err(AuthError::Unauthorized)
        }

        #[cfg(not(feature = "basic-auth"))]
        {
            // basic-auth feature disabled — reject all API key auth.
            let _ = (token, stored_keys);
            Err(AuthError::Unauthorized)
        }
    }
}

/// Hash a raw key using argon2id, returning a PHC-format string.
///
/// Used when storing a newly created API key. Only available with the
/// `basic-auth` feature.
#[cfg(feature = "basic-auth")]
pub fn hash_key(raw: &str) -> String {
    use argon2::{
        password_hash::{rand_core::OsRng, PasswordHasher, SaltString},
        Argon2,
    };
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(raw.as_bytes(), &salt)
        .expect("argon2 hash must not fail")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "basic-auth")]
    #[test]
    fn test_valid_key_matches() {
        let raw = "my-secret-key-abc";
        let hash = hash_key(raw);
        let stored_keys = vec![StoredApiKey {
            key_hash_argon2: hash,
            operator_id: "op-456".to_string(),
            key_id: "api-key-id-1".to_string(),
        }];

        let result = ApiKeyValidator::validate(raw, &stored_keys);
        assert!(result.is_ok());
        let ctx = result.unwrap();
        assert_eq!(ctx.operator_id, Some("op-456".to_string()));
        assert_eq!(ctx.api_key_id, Some("api-key-id-1".to_string()));
        assert_eq!(ctx.auth_method, crate::types::AuthMethod::ApiKey);
    }

    #[cfg(feature = "basic-auth")]
    #[test]
    fn test_invalid_key_returns_error() {
        let hash = hash_key("correct-key");
        let stored_keys = vec![StoredApiKey {
            key_hash_argon2: hash,
            operator_id: "op-456".to_string(),
            key_id: "api-key-id-1".to_string(),
        }];

        let result = ApiKeyValidator::validate("wrong-key", &stored_keys);
        assert!(matches!(result, Err(AuthError::Unauthorized)));
    }

    #[cfg(feature = "basic-auth")]
    #[test]
    fn test_empty_keys_list_returns_error() {
        let stored_keys = vec![];
        let result = ApiKeyValidator::validate("key123", &stored_keys);
        assert!(matches!(result, Err(AuthError::Unauthorized)));
    }

}
