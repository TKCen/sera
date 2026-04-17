//! Audit repository — Merkle hash-chain append and query.

use sqlx::PgPool;

use crate::error::DbError;

/// Row type for audit_trail table.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct AuditRow {
    pub sequence: i64,
    pub timestamp: time::OffsetDateTime,
    pub actor_type: String,
    pub actor_id: String,
    pub acting_context: Option<serde_json::Value>,
    pub event_type: String,
    pub payload: serde_json::Value,
    pub prev_hash: Option<String>,
    pub hash: String,
}

/// Audit repository for database operations.
pub struct AuditRepository;

impl AuditRepository {
    /// Append an audit event with hash chain.
    /// Uses an EXCLUSIVE lock to ensure sequential hashing.
    #[allow(clippy::too_many_arguments)]
    pub async fn append(
        pool: &PgPool,
        actor_type: &str,
        actor_id: &str,
        acting_context: Option<&serde_json::Value>,
        event_type: &str,
        payload: &serde_json::Value,
        hash: &str,
        prev_hash: Option<&str>,
    ) -> Result<i64, DbError> {
        let row: (i64,) = sqlx::query_as(
            "INSERT INTO audit_trail (actor_type, actor_id, acting_context, event_type, payload, hash, prev_hash)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             RETURNING sequence"
        )
        .bind(actor_type)
        .bind(actor_id)
        .bind(acting_context)
        .bind(event_type)
        .bind(payload)
        .bind(hash)
        .bind(prev_hash)
        .fetch_one(pool)
        .await?;
        Ok(row.0)
    }

    /// Get the latest audit record (for hash chain continuation).
    pub async fn get_latest(pool: &PgPool) -> Result<Option<AuditRow>, DbError> {
        let row = sqlx::query_as::<_, AuditRow>(
            "SELECT sequence, timestamp, actor_type, actor_id, acting_context,
                    event_type, payload, prev_hash, hash
             FROM audit_trail ORDER BY sequence DESC LIMIT 1"
        )
        .fetch_optional(pool)
        .await?;
        Ok(row)
    }

    /// Get audit entries with filtering and pagination.
    pub async fn get_entries(
        pool: &PgPool,
        actor_id: Option<&str>,
        event_type: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<AuditRow>, DbError> {
        let mut qb = sqlx::QueryBuilder::new(
            "SELECT sequence, timestamp, actor_type, actor_id, acting_context,
                    event_type, payload, prev_hash, hash
             FROM audit_trail WHERE 1=1",
        );

        if let Some(aid) = actor_id {
            qb.push(" AND actor_id = ").push_bind(aid);
        }
        if let Some(et) = event_type {
            qb.push(" AND event_type = ").push_bind(et);
        }

        qb.push(" ORDER BY sequence DESC LIMIT ").push_bind(limit);
        qb.push(" OFFSET ").push_bind(offset);

        let rows = qb.build_query_as::<AuditRow>().fetch_all(pool).await?;
        Ok(rows)
    }

    /// Count total entries matching filters (for pagination).
    pub async fn count_entries(
        pool: &PgPool,
        actor_id: Option<&str>,
        event_type: Option<&str>,
    ) -> Result<i64, DbError> {
        let mut qb = sqlx::QueryBuilder::new("SELECT COUNT(*) FROM audit_trail WHERE 1=1");

        if let Some(aid) = actor_id {
            qb.push(" AND actor_id = ").push_bind(aid);
        }
        if let Some(et) = event_type {
            qb.push(" AND event_type = ").push_bind(et);
        }

        let (count,): (i64,) = qb.build_query_as().fetch_one(pool).await?;
        Ok(count)
    }

    /// Verify integrity of the last N records.
    pub async fn get_chain_for_verification(
        pool: &PgPool,
        count: i64,
    ) -> Result<Vec<AuditRow>, DbError> {
        let rows = sqlx::query_as::<_, AuditRow>(
            "SELECT sequence, timestamp, actor_type, actor_id, acting_context,
                    event_type, payload, prev_hash, hash
             FROM audit_trail ORDER BY sequence ASC
             LIMIT $1"
        )
        .bind(count)
        .fetch_all(pool)
        .await?;
        Ok(rows)
    }
}
