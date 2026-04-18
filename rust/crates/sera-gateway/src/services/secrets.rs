//! Secrets Manager Service — high-level interface for secret management.
//!
//! Provides encryption, decryption, and lifecycle management for secrets
//! with secure key handling and comprehensive error types.

use std::sync::Arc;
use sqlx::PgPool;
use sera_db::secrets::{SecretsRepository, SecretMetadataRow};
use sera_db::DbError;

/// High-level secrets management service.
///
/// Wraps the SecretsRepository with business logic and ensures
/// secrets never leak into logs or tracing output.
pub struct SecretsManager {
    pool: Arc<PgPool>,
    master_key: Arc<String>, // In production, should use zeroize to clear on drop
}

impl SecretsManager {
    /// Create a new SecretsManager.
    pub fn new(pool: Arc<PgPool>, master_key: String) -> Self {
        Self {
            pool,
            master_key: Arc::new(master_key),
        }
    }

    /// Store a secret (encrypts plaintext, upserts to database).
    ///
    /// # Arguments
    /// * `key` - The secret name/identifier
    /// * `plaintext` - The plaintext value to encrypt
    ///
    /// # Returns
    /// The UUID of the stored secret, or a SecretsError
    pub async fn store_secret(&self, key: &str, plaintext: &str) -> Result<uuid::Uuid, SecretsError> {
        let (ciphertext, iv) = SecretsRepository::encrypt(plaintext, &self.master_key)
            .map_err(|e| SecretsError::Encryption(format!("encryption failed: {}", e)))?;

        SecretsRepository::upsert(
            &self.pool,
            sera_db::secrets::UpsertSecretInput {
                name: key,
                encrypted_value: &ciphertext,
                iv: &iv,
                description: None,
                tags: &[],
                allowed_agents: &[],
                exposure: "internal",
                created_by: None,
            },
        )
        .await
        .map_err(SecretsError::Db)?;

        // Fetch the stored secret to return its UUID
        let secret = SecretsRepository::get_by_name(&self.pool, key)
            .await
            .map_err(SecretsError::Db)?
            .ok_or_else(|| SecretsError::NotFound(key.to_string()))?;

        Ok(secret.id)
    }

    /// Retrieve and decrypt a secret by name.
    ///
    /// # Arguments
    /// * `key` - The secret name/identifier
    ///
    /// # Returns
    /// The decrypted plaintext value, or a SecretsError
    pub async fn retrieve_secret(&self, key: &str) -> Result<String, SecretsError> {
        let secret = SecretsRepository::get_by_name(&self.pool, key)
            .await
            .map_err(SecretsError::Db)?
            .ok_or_else(|| SecretsError::NotFound(key.to_string()))?;

        SecretsRepository::decrypt(&secret.encrypted_value, &secret.iv, &self.master_key)
            .map_err(|e| SecretsError::Encryption(format!("decryption failed: {}", e)))
    }

    /// List secret metadata (names, tags, etc. — no decrypted values).
    ///
    /// # Returns
    /// A vector of secret metadata rows
    pub async fn list_secrets(&self) -> Result<Vec<SecretMetadataRow>, SecretsError> {
        SecretsRepository::list(&self.pool)
            .await
            .map_err(SecretsError::Db)
    }

    /// Delete a secret by name (soft delete).
    ///
    /// # Arguments
    /// * `key` - The secret name/identifier
    ///
    /// # Returns
    /// true if a secret was deleted, false if it didn't exist
    pub async fn delete_secret(&self, key: &str) -> Result<bool, SecretsError> {
        SecretsRepository::delete(&self.pool, key)
            .await
            .map_err(SecretsError::Db)
    }
}

/// Error type for secrets operations.
#[derive(Debug, thiserror::Error)]
pub enum SecretsError {
    /// Database error (passed through from sera-db).
    #[error("database error: {0}")]
    Db(#[from] DbError),

    /// Secret not found by key.
    #[error("secret not found: {0}")]
    NotFound(String),

    /// Encryption/decryption error.
    #[error("encryption error: {0}")]
    Encryption(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encryption_roundtrip() {
        // Test encrypt/decrypt at the repository level
        let plaintext = "my-secret-value";
        let key = "test-master-key";

        let (ciphertext, iv) = SecretsRepository::encrypt(plaintext, key)
            .expect("encryption failed");

        let decrypted = SecretsRepository::decrypt(&ciphertext, &iv, key)
            .expect("decryption failed");

        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_different_plaintexts_produce_different_ciphertexts() {
        let key = "test-master-key";

        let (ct1, _) = SecretsRepository::encrypt("secret1", key).expect("encryption 1 failed");
        let (ct2, _) = SecretsRepository::encrypt("secret2", key).expect("encryption 2 failed");

        assert_ne!(ct1, ct2, "Different plaintexts should produce different ciphertexts");
    }

    #[test]
    fn test_same_plaintext_different_nonces() {
        // Same plaintext encrypted twice should produce different ciphertexts
        // due to different random nonces
        let key = "test-master-key";
        let plaintext = "same-secret";

        let (ct1, iv1) = SecretsRepository::encrypt(plaintext, key).expect("encryption 1 failed");
        let (ct2, iv2) = SecretsRepository::encrypt(plaintext, key).expect("encryption 2 failed");

        // IVs (nonces) should differ due to OsRng randomness
        assert_ne!(iv1, iv2, "Nonces should be different");
        // Ciphertexts should also differ
        assert_ne!(ct1, ct2, "Ciphertexts should differ even for same plaintext");
    }

    #[test]
    fn test_wrong_key_decryption_fails() {
        let plaintext = "secret";
        let key1 = "key1";
        let key2 = "key2";

        let (ciphertext, iv) = SecretsRepository::encrypt(plaintext, key1)
            .expect("encryption failed");

        let result = SecretsRepository::decrypt(&ciphertext, &iv, key2);
        assert!(result.is_err(), "Decryption with wrong key should fail");
    }
}
