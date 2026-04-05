//! Secrets repository — AES-256-GCM encrypted key-value store.

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use sqlx::PgPool;

use crate::error::DbError;

/// Row type for secrets table (metadata only, no decrypted value).
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SecretMetadataRow {
    pub id: uuid::Uuid,
    pub name: String,
    pub description: Option<String>,
    pub tags: Vec<String>,
    pub allowed_agents: Vec<String>,
    pub exposure: String,
    pub created_by: Option<String>,
    pub created_at: Option<time::OffsetDateTime>,
    pub updated_at: Option<time::OffsetDateTime>,
}

/// Row type for full secret (includes encrypted data).
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SecretFullRow {
    pub id: uuid::Uuid,
    pub name: String,
    pub encrypted_value: Vec<u8>,
    pub iv: Vec<u8>,
    pub description: Option<String>,
    pub tags: Vec<String>,
    pub allowed_agents: Vec<String>,
    pub exposure: String,
    pub created_by: Option<String>,
    pub created_at: Option<time::OffsetDateTime>,
    pub updated_at: Option<time::OffsetDateTime>,
}

pub struct SecretsRepository;

impl SecretsRepository {
    /// Derive a 32-byte AES key from the encryption secret.
    fn derive_key(secret: &str) -> [u8; 32] {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(secret.as_bytes());
        hasher.finalize().into()
    }

    /// Encrypt a plaintext value using AES-256-GCM.
    pub fn encrypt(plaintext: &str, encryption_key: &str) -> Result<(Vec<u8>, Vec<u8>), DbError> {
        let key = Self::derive_key(encryption_key);
        let cipher = Aes256Gcm::new_from_slice(&key)
            .map_err(|e| DbError::Integrity(format!("encryption key error: {e}")))?;

        // Generate random 12-byte nonce
        let nonce_bytes: [u8; 12] = rand_nonce();
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = cipher
            .encrypt(nonce, plaintext.as_bytes())
            .map_err(|e| DbError::Integrity(format!("encryption error: {e}")))?;

        Ok((ciphertext, nonce_bytes.to_vec()))
    }

    /// Decrypt an encrypted value using AES-256-GCM.
    pub fn decrypt(
        ciphertext: &[u8],
        iv: &[u8],
        encryption_key: &str,
    ) -> Result<String, DbError> {
        let key = Self::derive_key(encryption_key);
        let cipher = Aes256Gcm::new_from_slice(&key)
            .map_err(|e| DbError::Integrity(format!("decryption key error: {e}")))?;
        let nonce = Nonce::from_slice(iv);

        let plaintext = cipher
            .decrypt(nonce, ciphertext)
            .map_err(|e| DbError::Integrity(format!("decryption error: {e}")))?;

        String::from_utf8(plaintext)
            .map_err(|e| DbError::Integrity(format!("UTF-8 decode error: {e}")))
    }

    /// List secret metadata (no values).
    pub async fn list(pool: &PgPool) -> Result<Vec<SecretMetadataRow>, DbError> {
        let rows = sqlx::query_as::<_, SecretMetadataRow>(
            "SELECT id, name, description, tags, allowed_agents, exposure, created_by, created_at, updated_at
             FROM secrets WHERE deleted_at IS NULL
             ORDER BY name",
        )
        .fetch_all(pool)
        .await?;
        Ok(rows)
    }

    /// Get a secret by name (includes encrypted data for decryption).
    pub async fn get_by_name(pool: &PgPool, name: &str) -> Result<Option<SecretFullRow>, DbError> {
        let row = sqlx::query_as::<_, SecretFullRow>(
            "SELECT id, name, encrypted_value, iv, description, tags, allowed_agents, exposure, created_by, created_at, updated_at
             FROM secrets WHERE name = $1 AND deleted_at IS NULL",
        )
        .bind(name)
        .fetch_optional(pool)
        .await?;
        Ok(row)
    }

    /// Create or update a secret.
    #[allow(clippy::too_many_arguments)]
    pub async fn upsert(
        pool: &PgPool,
        name: &str,
        encrypted_value: &[u8],
        iv: &[u8],
        description: Option<&str>,
        tags: &[String],
        allowed_agents: &[String],
        exposure: &str,
        created_by: Option<&str>,
    ) -> Result<(), DbError> {
        sqlx::query(
            "INSERT INTO secrets (name, encrypted_value, iv, description, tags, allowed_agents, exposure, created_by, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, NOW(), NOW())
             ON CONFLICT (name) DO UPDATE SET
               encrypted_value = $2,
               iv = $3,
               description = COALESCE($4, secrets.description),
               tags = $5,
               allowed_agents = $6,
               exposure = $7,
               updated_at = NOW()",
        )
        .bind(name)
        .bind(encrypted_value)
        .bind(iv)
        .bind(description)
        .bind(tags)
        .bind(allowed_agents)
        .bind(exposure)
        .bind(created_by)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Soft-delete a secret by name.
    pub async fn delete(pool: &PgPool, name: &str) -> Result<bool, DbError> {
        let result =
            sqlx::query("UPDATE secrets SET deleted_at = NOW() WHERE name = $1 AND deleted_at IS NULL")
                .bind(name)
                .execute(pool)
                .await?;
        Ok(result.rows_affected() > 0)
    }
}

/// Generate a random 12-byte nonce using cryptographically secure randomness.
fn rand_nonce() -> [u8; 12] {
    use rand::RngCore;
    let mut buf = [0u8; 12];
    rand::rngs::OsRng.fill_bytes(&mut buf);
    buf
}
