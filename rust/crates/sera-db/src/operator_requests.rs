//! Operator requests repository — permission/approval requests from agents.

use sqlx::PgPool;

use crate::error::DbError;

/// Row type for operator_requests table.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct OperatorRequestRow {
    pub id: uuid::Uuid,
    pub agent_id: String,
    pub agent_name: Option<String>,
    pub r#type: String,
    pub title: String,
    pub payload: serde_json::Value,
    pub status: String,
    pub response: Option<serde_json::Value>,
    pub created_at: time::OffsetDateTime,
    pub resolved_at: Option<time::OffsetDateTime>,
}

pub struct OperatorRequestRepository;

impl OperatorRequestRepository {
    /// Count pending operator requests.
    pub async fn count_pending(pool: &PgPool) -> Result<i64, DbError> {
        let row: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM operator_requests WHERE status = 'pending'",
        )
        .fetch_one(pool)
        .await?;
        Ok(row.0)
    }

    /// List operator requests with optional status/agent filter.
    pub async fn list(
        pool: &PgPool,
        status: Option<&str>,
        agent_id: Option<&str>,
        limit: i64,
    ) -> Result<Vec<OperatorRequestRow>, DbError> {
        let mut qb = sqlx::QueryBuilder::new(
            "SELECT id, agent_id, agent_name, type, title, payload, status, response, created_at, resolved_at
             FROM operator_requests WHERE 1=1",
        );

        if let Some(s) = status {
            qb.push(" AND status = ").push_bind(s);
        }
        if let Some(a) = agent_id {
            qb.push(" AND agent_id = ").push_bind(a);
        }

        qb.push(" ORDER BY created_at DESC LIMIT ").push_bind(limit);

        let rows = qb.build_query_as::<OperatorRequestRow>().fetch_all(pool).await?;
        Ok(rows)
    }

    /// Respond to an operator request (approve/reject/resolve).
    pub async fn respond(
        pool: &PgPool,
        id: &str,
        status: &str,
        response: Option<&serde_json::Value>,
    ) -> Result<OperatorRequestRow, DbError> {
        let affected = sqlx::query(
            "UPDATE operator_requests SET status = $1, response = $2, resolved_at = NOW()
             WHERE id = $3::uuid AND status = 'pending'",
        )
        .bind(status)
        .bind(response)
        .bind(id)
        .execute(pool)
        .await?;

        if affected.rows_affected() == 0 {
            return Err(DbError::NotFound {
                entity: "operator_request",
                key: "id",
                value: id.to_string(),
            });
        }

        sqlx::query_as::<_, OperatorRequestRow>(
            "SELECT id, agent_id, agent_name, type, title, payload, status, response, created_at, resolved_at
             FROM operator_requests WHERE id = $1::uuid",
        )
        .bind(id)
        .fetch_one(pool)
        .await
        .map_err(DbError::from)
    }
}
