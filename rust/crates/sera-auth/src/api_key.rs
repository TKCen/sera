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

    // ── Security-edge API key tests ───────────────────────────────────────────

    /// A key that is a prefix of the correct key must not match.
    /// Argon2 hashes the full byte sequence; truncated input must fail.
    #[cfg(feature = "basic-auth")]
    #[test]
    fn test_prefix_of_correct_key_rejected() {
        let raw = "my-secret-key-abc";
        let hash = hash_key(raw);
        let stored_keys = vec![StoredApiKey {
            key_hash_argon2: hash,
            operator_id: "op-1".to_string(),
            key_id: "key-1".to_string(),
        }];

        // Provide only the first half of the correct key.
        let prefix = &raw[..raw.len() / 2];
        let result = ApiKeyValidator::validate(prefix, &stored_keys);
        assert!(
            matches!(result, Err(AuthError::Unauthorized)),
            "prefix of correct key must not authenticate"
        );
    }

    /// A key that is a superstring of the correct key (correct + extra bytes)
    /// must not match.
    #[cfg(feature = "basic-auth")]
    #[test]
    fn test_extended_key_rejected() {
        let raw = "my-secret-key-abc";
        let hash = hash_key(raw);
        let stored_keys = vec![StoredApiKey {
            key_hash_argon2: hash,
            operator_id: "op-1".to_string(),
            key_id: "key-1".to_string(),
        }];

        let extended = format!("{raw}-extra");
        let result = ApiKeyValidator::validate(&extended, &stored_keys);
        assert!(
            matches!(result, Err(AuthError::Unauthorized)),
            "extended key (correct + suffix) must not authenticate"
        );
    }

    /// An empty string as a token must not match any stored key.
    #[cfg(feature = "basic-auth")]
    #[test]
    fn test_empty_token_rejected() {
        let hash = hash_key("real-key");
        let stored_keys = vec![StoredApiKey {
            key_hash_argon2: hash,
            operator_id: "op-1".to_string(),
            key_id: "key-1".to_string(),
        }];

        let result = ApiKeyValidator::validate("", &stored_keys);
        assert!(
            matches!(result, Err(AuthError::Unauthorized)),
            "empty token must not authenticate"
        );
    }

    /// A stored key with a malformed (non-PHC) hash string must be skipped
    /// gracefully — the validator must not panic, and must return Unauthorized
    /// rather than propagating an internal error.
    #[cfg(feature = "basic-auth")]
    #[test]
    fn test_malformed_stored_hash_skipped_gracefully() {
        let stored_keys = vec![StoredApiKey {
            key_hash_argon2: "not-a-valid-phc-string".to_string(),
            operator_id: "op-1".to_string(),
            key_id: "key-bad".to_string(),
        }];

        // Should not panic; must return Unauthorized (malformed hash is skipped).
        let result = ApiKeyValidator::validate("some-key", &stored_keys);
        assert!(
            matches!(result, Err(AuthError::Unauthorized)),
            "malformed stored hash must be skipped and return Unauthorized"
        );
    }

    /// Validate returns the context for the *first* matching key when multiple
    /// keys are stored. The key_id in the returned context must correspond to
    /// the matching entry, not a different one.
    #[cfg(feature = "basic-auth")]
    #[test]
    fn test_correct_key_id_returned_when_multiple_keys_stored() {
        let raw_a = "key-alpha";
        let raw_b = "key-beta";
        let stored_keys = vec![
            StoredApiKey {
                key_hash_argon2: hash_key(raw_a),
                operator_id: "op-1".to_string(),
                key_id: "id-alpha".to_string(),
            },
            StoredApiKey {
                key_hash_argon2: hash_key(raw_b),
                operator_id: "op-2".to_string(),
                key_id: "id-beta".to_string(),
            },
        ];

        let ctx = ApiKeyValidator::validate(raw_b, &stored_keys)
            .expect("second key must validate");
        assert_eq!(
            ctx.api_key_id.as_deref(), Some("id-beta"),
            "returned context must identify the matching key, not the first stored key"
        );
        assert_eq!(ctx.operator_id, Some("op-2".to_string()));
    }
}
