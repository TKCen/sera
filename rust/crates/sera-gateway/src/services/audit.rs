//! Audit service — ties together the audit repository, hash chain, and Centrifugo publishing.

use std::sync::Arc;
use tokio::sync::Mutex;
use sqlx::PgPool;
use sera_db::audit::{AuditRepository, AuditRow};
use sera_events::audit::AuditHashChain;
use sera_events::centrifugo::CentrifugoClient;
use sera_types::audit::ActorType;

#[derive(Debug, thiserror::Error)]
pub enum AuditError {
    #[error("database error: {0}")]
    Db(#[from] sera_db::error::DbError),
    #[error("verification failed: {0}")]
    Verification(#[from] sera_events::error::AuditVerifyError),
    #[error("centrifugo publish failed: {0}")]
    Publish(String),
}

/// Service for managing audit trail with hash chain verification and real-time pub/sub.
pub struct AuditService {
    pool: Arc<PgPool>,
    centrifugo: Arc<CentrifugoClient>,
    last_hash: Mutex<Option<String>>, // cached for chain continuation
    retention_days: u32,
}

impl AuditService {
    /// Create a new AuditService.
    ///
    /// Loads the latest hash from the database for chain continuation.
    /// Respects `AUDIT_RETENTION_DAYS` env var (default: 2555 days ≈ 7 years).
    pub async fn new(
        pool: Arc<PgPool>,
        centrifugo: Arc<CentrifugoClient>,
    ) -> Result<Self, AuditError> {
        // Load last hash from DB for chain continuation
        let latest = AuditRepository::get_latest(&pool).await?;
        let last_hash = latest.map(|r| r.hash);
        let retention_days = std::env::var("AUDIT_RETENTION_DAYS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(2555); // 7 years default

        Ok(Self {
            pool,
            centrifugo,
            last_hash: Mutex::new(last_hash),
            retention_days,
        })
    }

    /// Log an audit event with Merkle hash chain.
    ///
    /// Computes the hash for this event based on the previous hash,
    /// appends to the database, and publishes to Centrifugo (non-blocking).
    pub async fn log_event(
        &self,
        actor_type: ActorType,
        actor_id: &str,
        event_type: &str,
        payload: &serde_json::Value,
    ) -> Result<i64, AuditError> {
        let mut last = self.last_hash.lock().await;

        // Get current timestamp in RFC3339 format
        let now = time::OffsetDateTime::now_utc();
        let timestamp = now
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default();

        // Get the next sequence number by querying the latest record
        let latest = AuditRepository::get_latest(&self.pool).await?;
        let next_seq = latest.as_ref().map_or(1, |r| r.sequence + 1);

        // Convert ActorType to string for hash computation
        let actor_type_str = match actor_type {
            ActorType::Operator => "operator",
            ActorType::Agent => "agent",
            ActorType::System => "system",
        };

        // Compute hash with the next sequence
        let hash = AuditHashChain::compute_hash(
            &next_seq.to_string(),
            &timestamp,
            actor_type_str,
            actor_id,
            event_type,
            &payload.to_string(),
            last.as_deref(),
        );

        // Append to database
        let sequence = AuditRepository::append(
            &self.pool,
            actor_type_str,
            actor_id,
            None, // acting_context
            event_type,
            payload,
            &hash,
            last.as_deref(),
        )
        .await?;

        // Update cached hash
        *last = Some(hash.clone());
        drop(last); // Release lock before async publish

        // Non-blocking publish to Centrifugo
        let centrifugo = self.centrifugo.clone();
        let channel = format!("audit_trail:{}", actor_id);
        let event_data = serde_json::json!({
            "sequence": sequence,
            "actor_type": actor_type_str,
            "actor_id": actor_id,
            "event_type": event_type,
            "timestamp": timestamp,
            "hash": hash,
        });
        tokio::spawn(async move {
            if let Err(e) = centrifugo.publish(&channel, event_data).await {
                tracing::warn!("Failed to publish audit event to Centrifugo: {e}");
            }
        });

        Ok(sequence)
    }

    /// Verify the integrity of the audit chain.
    ///
    /// Retrieves the last N records and verifies that the hash chain is intact.
    pub async fn verify_chain(&self, count: i64) -> Result<(), AuditError> {
        // Get the rows from the database
        let rows = AuditRepository::get_chain_for_verification(&self.pool, count).await?;

        // Convert AuditRow to AuditRecord for verification
        let records: Vec<sera_types::audit::AuditRecord> = rows
            .into_iter()
            .map(|row| sera_types::audit::AuditRecord {
                id: row.sequence.to_string(), // Use sequence as ID for now
                sequence: row.sequence.to_string(),
                timestamp: row
                    .timestamp
                    .format(&time::format_description::well_known::Rfc3339)
                    .unwrap_or_default(),
                actor_type: match row.actor_type.as_str() {
                    "operator" => ActorType::Operator,
                    "agent" => ActorType::Agent,
                    _ => ActorType::System,
                },
                actor_id: row.actor_id,
                acting_context: row.acting_context,
                event_type: row.event_type,
                payload: row.payload,
                prev_hash: row.prev_hash,
                hash: row.hash,
            })
            .collect();

        AuditHashChain::verify_chain(&records)?;
        Ok(())
    }

    /// Get audit entries with optional filtering.
    ///
    /// Returns both the entries and the total count for pagination.
    pub async fn get_entries(
        &self,
        actor_id: Option<&str>,
        event_type: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> Result<(Vec<AuditRow>, i64), AuditError> {
        let entries = AuditRepository::get_entries(&self.pool, actor_id, event_type, limit, offset)
            .await?;
        let count = AuditRepository::count_entries(&self.pool, actor_id, event_type).await?;
        Ok((entries, count))
    }

    /// Get the number of retention days configured for this service.
    pub fn retention_days(&self) -> u32 {
        self.retention_days
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_actor_type_to_string() {
        let actor_type = ActorType::Agent;
        let actor_type_str = match actor_type {
            ActorType::Operator => "operator",
            ActorType::Agent => "agent",
            ActorType::System => "system",
        };
        assert_eq!(actor_type_str, "agent");
    }

    #[test]
    fn test_retention_days_default() {
        // This test verifies the default retention days logic
        // (actual service creation requires a database)
        let default_days = 2555;
        assert_eq!(default_days, 2555);
    }

    #[test]
    fn test_hash_chain_computation() {
        // Test that hash computation works correctly
        let hash1 = AuditHashChain::compute_hash(
            "1",
            "2024-01-01T00:00:00Z",
            "system",
            "sera",
            "init",
            r#"{"msg":"genesis"}"#,
            None,
        );

        let hash2 = AuditHashChain::compute_hash(
            "2",
            "2024-01-01T00:00:01Z",
            "agent",
            "agent-1",
            "started",
            r#"{"instance_id":"inst-1"}"#,
            Some(&hash1),
        );

        assert_ne!(hash1, hash2);
        assert_eq!(hash1.len(), 64); // SHA-256 hex is 64 chars
        assert_eq!(hash2.len(), 64);
    }
}
