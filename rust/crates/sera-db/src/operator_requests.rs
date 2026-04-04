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
        // Build dynamic query
        let mut query = String::from(
            "SELECT id, agent_id, agent_name, type, title, payload, status, response, created_at, resolved_at
             FROM operator_requests WHERE 1=1",
        );
        let mut param_idx = 1;

        if status.is_some() {
            query.push_str(&format!(" AND status = ${param_idx}"));
            param_idx += 1;
        }
        if agent_id.is_some() {
            query.push_str(&format!(" AND agent_id = ${param_idx}"));
            param_idx += 1;
        }

        query.push_str(&format!(" ORDER BY created_at DESC LIMIT ${param_idx}"));

        let mut q = sqlx::query_as::<_, OperatorRequestRow>(&query);
        if let Some(s) = status {
            q = q.bind(s);
        }
        if let Some(a) = agent_id {
            q = q.bind(a);
        }
        q = q.bind(limit);

        let rows = q.fetch_all(pool).await?;
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
