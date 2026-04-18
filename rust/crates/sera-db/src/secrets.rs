//! Secrets repository — AES-256-GCM encrypted key-value store.
//!
//! Dual-backend (sera-mwb4):
//! * [`SecretsRepository`] — original Postgres repository with
//!   soft-delete, tags, allowed_agents, exposure metadata.
//! * [`SqliteSecretsStore`] — rusqlite store mirroring the same row shape
//!   (minus the Postgres ARRAY / JSONB niceties — tags & allowed_agents are
//!   serialised as JSON text).
//! * [`SecretsStore`] — trait shared by both.
//!
//! The crypto helpers [`SecretsRepository::encrypt`] and
//! [`SecretsRepository::decrypt`] are backend-independent and remain on the
//! existing type so both backends can call into them.

use std::sync::Arc;

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use async_trait::async_trait;
use rusqlite::{params, Connection, OptionalExtension};
use sqlx::PgPool;
use tokio::sync::Mutex;

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

/// Input for creating or updating a secret.
pub struct UpsertSecretInput<'a> {
    pub name: &'a str,
    pub encrypted_value: &'a [u8],
    pub iv: &'a [u8],
    pub description: Option<&'a str>,
    pub tags: &'a [String],
    pub allowed_agents: &'a [String],
    pub exposure: &'a str,
    pub created_by: Option<&'a str>,
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
    pub async fn upsert(pool: &PgPool, input: UpsertSecretInput<'_>) -> Result<(), DbError> {
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
        .bind(input.name)
        .bind(input.encrypted_value)
        .bind(input.iv)
        .bind(input.description)
        .bind(input.tags)
        .bind(input.allowed_agents)
        .bind(input.exposure)
        .bind(input.created_by)
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

// ---------------------------------------------------------------------------
// Dual-backend trait (sera-mwb4)
// ---------------------------------------------------------------------------

/// Secrets store surface shared by Postgres and SQLite backends.
#[async_trait]
pub trait SecretsStore: Send + Sync + std::fmt::Debug {
    async fn list(&self) -> Result<Vec<SecretMetadataRow>, DbError>;
    async fn get_by_name(&self, name: &str) -> Result<Option<SecretFullRow>, DbError>;
    async fn upsert(&self, input: UpsertSecretInput<'_>) -> Result<(), DbError>;
    /// Soft-delete; returns `true` iff a live row matched.
    async fn delete(&self, name: &str) -> Result<bool, DbError>;
}

#[derive(Debug, Clone)]
pub struct PgSecretsStore {
    pool: PgPool,
}

impl PgSecretsStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl SecretsStore for PgSecretsStore {
    async fn list(&self) -> Result<Vec<SecretMetadataRow>, DbError> {
        SecretsRepository::list(&self.pool).await
    }
    async fn get_by_name(&self, name: &str) -> Result<Option<SecretFullRow>, DbError> {
        SecretsRepository::get_by_name(&self.pool, name).await
    }
    async fn upsert(&self, input: UpsertSecretInput<'_>) -> Result<(), DbError> {
        SecretsRepository::upsert(&self.pool, input).await
    }
    async fn delete(&self, name: &str) -> Result<bool, DbError> {
        SecretsRepository::delete(&self.pool, name).await
    }
}

// ---------------------------------------------------------------------------
// SQLite implementation (sera-mwb4)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct SqliteSecretsStore {
    conn: Arc<Mutex<Connection>>,
}

impl SqliteSecretsStore {
    pub fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self { conn }
    }

    /// Create the `secrets` table idempotently. Tags / allowed_agents are
    /// serialised as JSON text arrays (SQLite has no array type).
    pub fn init_schema(conn: &Connection) -> rusqlite::Result<()> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS secrets (
                id              TEXT PRIMARY KEY,
                name            TEXT NOT NULL UNIQUE,
                encrypted_value BLOB NOT NULL,
                iv              BLOB NOT NULL,
                description     TEXT,
                tags            TEXT NOT NULL DEFAULT '[]',
                allowed_agents  TEXT NOT NULL DEFAULT '[]',
                exposure        TEXT NOT NULL DEFAULT 'internal',
                created_by      TEXT,
                created_at      TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at      TEXT NOT NULL DEFAULT (datetime('now')),
                deleted_at      TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_secrets_live ON secrets(name) WHERE deleted_at IS NULL;",
        )
    }
}

fn parse_datetime_opt(s: Option<String>) -> Option<time::OffsetDateTime> {
    s.and_then(|v| {
        if let Ok(dt) = time::OffsetDateTime::parse(&v, &time::format_description::well_known::Rfc3339) {
            return Some(dt);
        }
        let fmt = time::macros::format_description!(
            "[year]-[month]-[day] [hour]:[minute]:[second]"
        );
        time::PrimitiveDateTime::parse(&v, &fmt)
            .ok()
            .map(|p| p.assume_utc())
    })
}

fn parse_string_list(s: &str) -> Vec<String> {
    serde_json::from_str(s).unwrap_or_default()
}

#[async_trait]
impl SecretsStore for SqliteSecretsStore {
    async fn list(&self) -> Result<Vec<SecretMetadataRow>, DbError> {
        let conn = self.conn.lock().await;
        let mut stmt = conn
            .prepare(
                "SELECT id, name, description, tags, allowed_agents, exposure, created_by,
                        created_at, updated_at
                 FROM secrets WHERE deleted_at IS NULL
                 ORDER BY name",
            )
            .map_err(|e| DbError::Integrity(format!("sqlite prepare: {e}")))?;
        let rows = stmt
            .query_map([], |row| {
                let id_str: String = row.get(0)?;
                let tags_str: String = row.get(3)?;
                let agents_str: String = row.get(4)?;
                Ok(SecretMetadataRow {
                    id: uuid::Uuid::parse_str(&id_str).unwrap_or(uuid::Uuid::nil()),
                    name: row.get(1)?,
                    description: row.get(2)?,
                    tags: parse_string_list(&tags_str),
                    allowed_agents: parse_string_list(&agents_str),
                    exposure: row.get(5)?,
                    created_by: row.get(6)?,
                    created_at: parse_datetime_opt(row.get(7)?),
                    updated_at: parse_datetime_opt(row.get(8)?),
                })
            })
            .map_err(|e| DbError::Integrity(format!("sqlite list: {e}")))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|e| DbError::Integrity(format!("sqlite row: {e}")))?);
        }
        Ok(out)
    }

    async fn get_by_name(&self, name: &str) -> Result<Option<SecretFullRow>, DbError> {
        let conn = self.conn.lock().await;
        let mut stmt = conn
            .prepare(
                "SELECT id, name, encrypted_value, iv, description, tags, allowed_agents,
                        exposure, created_by, created_at, updated_at
                 FROM secrets WHERE name = ?1 AND deleted_at IS NULL",
            )
            .map_err(|e| DbError::Integrity(format!("sqlite prepare: {e}")))?;
        let row = stmt
            .query_row(params![name], |row| {
                let id_str: String = row.get(0)?;
                let tags_str: String = row.get(5)?;
                let agents_str: String = row.get(6)?;
                Ok(SecretFullRow {
                    id: uuid::Uuid::parse_str(&id_str).unwrap_or(uuid::Uuid::nil()),
                    name: row.get(1)?,
                    encrypted_value: row.get(2)?,
                    iv: row.get(3)?,
                    description: row.get(4)?,
                    tags: parse_string_list(&tags_str),
                    allowed_agents: parse_string_list(&agents_str),
                    exposure: row.get(7)?,
                    created_by: row.get(8)?,
                    created_at: parse_datetime_opt(row.get(9)?),
                    updated_at: parse_datetime_opt(row.get(10)?),
                })
            })
            .optional()
            .map_err(|e| DbError::Integrity(format!("sqlite get: {e}")))?;
        Ok(row)
    }

    async fn upsert(&self, input: UpsertSecretInput<'_>) -> Result<(), DbError> {
        // Serialise tags / allowed_agents as JSON text pre-lock so the future
        // contains only `Send` values when it crosses the .await boundary.
        let tags_json = serde_json::to_string(input.tags).unwrap_or_else(|_| "[]".to_string());
        let agents_json =
            serde_json::to_string(input.allowed_agents).unwrap_or_else(|_| "[]".to_string());
        let new_id = uuid::Uuid::new_v4().to_string();

        let conn = self.conn.lock().await;
        // Postgres' UPSERT preserves the row id (the uuid); SQLite's ON
        // CONFLICT similarly leaves `id` alone on update. For first-insert we
        // generate a fresh v4 uuid.
        conn.execute(
            "INSERT INTO secrets (id, name, encrypted_value, iv, description, tags, allowed_agents, exposure, created_by, created_at, updated_at, deleted_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, datetime('now'), datetime('now'), NULL)
             ON CONFLICT(name) DO UPDATE SET
                encrypted_value = ?3,
                iv              = ?4,
                description     = COALESCE(?5, secrets.description),
                tags            = ?6,
                allowed_agents  = ?7,
                exposure        = ?8,
                updated_at      = datetime('now'),
                deleted_at      = NULL",
            params![
                new_id,
                input.name,
                input.encrypted_value,
                input.iv,
                input.description,
                tags_json,
                agents_json,
                input.exposure,
                input.created_by
            ],
        )
        .map_err(|e| DbError::Integrity(format!("sqlite upsert secret: {e}")))?;
        Ok(())
    }

    async fn delete(&self, name: &str) -> Result<bool, DbError> {
        let conn = self.conn.lock().await;
        let n = conn
            .execute(
                "UPDATE secrets SET deleted_at = datetime('now')
                 WHERE name = ?1 AND deleted_at IS NULL",
                params![name],
            )
            .map_err(|e| DbError::Integrity(format!("sqlite delete secret: {e}")))?;
        Ok(n > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let plaintext = "super-secret-value";
        let key = "test-encryption-key";
        let (ciphertext, iv) = SecretsRepository::encrypt(plaintext, key).expect("encrypt");
        let recovered = SecretsRepository::decrypt(&ciphertext, &iv, key).expect("decrypt");
        assert_eq!(recovered, plaintext);
    }

    #[test]
    fn encrypt_produces_ciphertext_different_from_plaintext() {
        let plaintext = "my secret";
        let key = "key123";
        let (ciphertext, _iv) = SecretsRepository::encrypt(plaintext, key).expect("encrypt");
        assert_ne!(ciphertext, plaintext.as_bytes());
    }

    #[test]
    fn encrypt_same_plaintext_different_nonces() {
        // Two encryptions of the same value should produce different ciphertexts (random nonce).
        let plaintext = "same-value";
        let key = "key-abc";
        let (ct1, _) = SecretsRepository::encrypt(plaintext, key).expect("first");
        let (ct2, _) = SecretsRepository::encrypt(plaintext, key).expect("second");
        // Probability of collision is negligible; this guards against static nonce bugs.
        assert_ne!(ct1, ct2, "two encryptions should use different nonces");
    }

    #[test]
    fn decrypt_wrong_key_returns_error() {
        let plaintext = "secret";
        let (ciphertext, iv) = SecretsRepository::encrypt(plaintext, "correct-key").expect("encrypt");
        let result = SecretsRepository::decrypt(&ciphertext, &iv, "wrong-key");
        assert!(result.is_err(), "decryption with wrong key must fail");
    }

    #[test]
    fn decrypt_tampered_ciphertext_returns_error() {
        let (mut ciphertext, iv) =
            SecretsRepository::encrypt("value", "key").expect("encrypt");
        // Flip a byte to simulate tampering.
        if let Some(b) = ciphertext.last_mut() {
            *b ^= 0xFF;
        }
        let result = SecretsRepository::decrypt(&ciphertext, &iv, "key");
        assert!(result.is_err(), "tampered ciphertext must fail authentication");
    }

    #[test]
    fn encrypt_empty_string_roundtrip() {
        let (ciphertext, iv) = SecretsRepository::encrypt("", "key").expect("encrypt");
        let recovered = SecretsRepository::decrypt(&ciphertext, &iv, "key").expect("decrypt");
        assert_eq!(recovered, "");
    }

    #[test]
    fn encrypt_unicode_roundtrip() {
        let plaintext = "こんにちは🔑";
        let key = "unicode-test-key";
        let (ciphertext, iv) = SecretsRepository::encrypt(plaintext, key).expect("encrypt");
        let recovered = SecretsRepository::decrypt(&ciphertext, &iv, key).expect("decrypt");
        assert_eq!(recovered, plaintext);
    }

    #[test]
    fn iv_is_12_bytes() {
        let (_ct, iv) = SecretsRepository::encrypt("data", "key").expect("encrypt");
        assert_eq!(iv.len(), 12, "AES-GCM nonce must be 12 bytes");
    }

    // --- SQLite backend tests (sera-mwb4) ---------------------------------

    fn new_store() -> SqliteSecretsStore {
        let conn = Connection::open_in_memory().unwrap();
        SqliteSecretsStore::init_schema(&conn).unwrap();
        SqliteSecretsStore::new(Arc::new(Mutex::new(conn)))
    }

    #[tokio::test]
    async fn sqlite_upsert_and_get_roundtrip() {
        let store = new_store();
        let (ct, iv) = SecretsRepository::encrypt("token-value", "master").unwrap();
        store
            .upsert(UpsertSecretInput {
                name: "discord-token",
                encrypted_value: &ct,
                iv: &iv,
                description: Some("discord bot"),
                tags: &["prod".to_string()],
                allowed_agents: &["sera".to_string(), "helper".to_string()],
                exposure: "internal",
                created_by: Some("admin"),
            })
            .await
            .unwrap();

        let full = store.get_by_name("discord-token").await.unwrap().unwrap();
        assert_eq!(full.name, "discord-token");
        assert_eq!(full.tags, vec!["prod".to_string()]);
        assert_eq!(full.allowed_agents.len(), 2);
        assert_eq!(full.exposure, "internal");

        let recovered = SecretsRepository::decrypt(&full.encrypted_value, &full.iv, "master").unwrap();
        assert_eq!(recovered, "token-value");
    }

    #[tokio::test]
    async fn sqlite_upsert_updates_existing() {
        let store = new_store();
        let (ct1, iv1) = SecretsRepository::encrypt("v1", "k").unwrap();
        store
            .upsert(UpsertSecretInput {
                name: "n",
                encrypted_value: &ct1,
                iv: &iv1,
                description: None,
                tags: &[],
                allowed_agents: &[],
                exposure: "internal",
                created_by: None,
            })
            .await
            .unwrap();
        let (ct2, iv2) = SecretsRepository::encrypt("v2", "k").unwrap();
        store
            .upsert(UpsertSecretInput {
                name: "n",
                encrypted_value: &ct2,
                iv: &iv2,
                description: None,
                tags: &[],
                allowed_agents: &[],
                exposure: "public",
                created_by: None,
            })
            .await
            .unwrap();
        let full = store.get_by_name("n").await.unwrap().unwrap();
        let recovered = SecretsRepository::decrypt(&full.encrypted_value, &full.iv, "k").unwrap();
        assert_eq!(recovered, "v2");
        assert_eq!(full.exposure, "public");
    }

    #[tokio::test]
    async fn sqlite_soft_delete_hides_from_list_and_get() {
        let store = new_store();
        let (ct, iv) = SecretsRepository::encrypt("x", "k").unwrap();
        store
            .upsert(UpsertSecretInput {
                name: "n",
                encrypted_value: &ct,
                iv: &iv,
                description: None,
                tags: &[],
                allowed_agents: &[],
                exposure: "internal",
                created_by: None,
            })
            .await
            .unwrap();
        assert!(store.delete("n").await.unwrap());
        // Second delete should return false.
        assert!(!store.delete("n").await.unwrap());
        assert!(store.get_by_name("n").await.unwrap().is_none());
        assert!(store.list().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn sqlite_list_multi_tenant_isolation_by_allowed_agents() {
        let store = new_store();
        for (name, allowed) in [
            ("secret-a", vec!["tenant-a".to_string()]),
            ("secret-b", vec!["tenant-b".to_string()]),
            ("secret-shared", vec!["tenant-a".to_string(), "tenant-b".to_string()]),
        ] {
            let (ct, iv) = SecretsRepository::encrypt("v", "k").unwrap();
            store
                .upsert(UpsertSecretInput {
                    name,
                    encrypted_value: &ct,
                    iv: &iv,
                    description: None,
                    tags: &[],
                    allowed_agents: &allowed,
                    exposure: "internal",
                    created_by: None,
                })
                .await
                .unwrap();
        }
        let rows = store.list().await.unwrap();
        assert_eq!(rows.len(), 3);
        let a_secrets: Vec<_> = rows
            .iter()
            .filter(|r| r.allowed_agents.contains(&"tenant-a".to_string()))
            .collect();
        assert_eq!(a_secrets.len(), 2);
    }

    #[tokio::test]
    async fn sqlite_init_schema_is_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        SqliteSecretsStore::init_schema(&conn).unwrap();
        SqliteSecretsStore::init_schema(&conn).unwrap();
        SqliteSecretsStore::init_schema(&conn).unwrap();
    }
}
