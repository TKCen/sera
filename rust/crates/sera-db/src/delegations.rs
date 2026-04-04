//! Delegation tokens repository.

use sqlx::PgPool;

use crate::error::DbError;

/// Row type for delegation_tokens table.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct DelegationRow {
    pub id: uuid::Uuid,
    pub principal_type: String,
    pub principal_id: String,
    pub principal_name: String,
    pub actor_agent_id: String,
    pub actor_instance_id: Option<uuid::Uuid>,
    pub scope: serde_json::Value,
    pub grant_type: String,
    pub credential_secret_name: String,
    pub signed_token: Option<String>,
    pub issued_at: Option<time::OffsetDateTime>,
    pub expires_at: Option<time::OffsetDateTime>,
    pub revoked_at: Option<time::OffsetDateTime>,
    pub use_count: Option<i32>,
    pub parent_delegation_id: Option<uuid::Uuid>,
}

pub struct DelegationRepository;

impl DelegationRepository {
    /// List active delegations.
    pub async fn list(pool: &PgPool, agent_id: Option<&str>) -> Result<Vec<DelegationRow>, DbError> {
        let rows = if let Some(aid) = agent_id {
            sqlx::query_as::<_, DelegationRow>(
                "SELECT id, principal_type, principal_id, principal_name, actor_agent_id,
                        actor_instance_id, scope, grant_type, credential_secret_name,
                        signed_token, issued_at, expires_at, revoked_at, use_count, parent_delegation_id
                 FROM delegation_tokens WHERE actor_agent_id = $1 AND revoked_at IS NULL
                 ORDER BY issued_at DESC",
            )
            .bind(aid)
            .fetch_all(pool)
            .await?
        } else {
            sqlx::query_as::<_, DelegationRow>(
                "SELECT id, principal_type, principal_id, principal_name, actor_agent_id,
                        actor_instance_id, scope, grant_type, credential_secret_name,
                        signed_token, issued_at, expires_at, revoked_at, use_count, parent_delegation_id
                 FROM delegation_tokens WHERE revoked_at IS NULL
                 ORDER BY issued_at DESC",
            )
            .fetch_all(pool)
            .await?
        };
        Ok(rows)
    }

    /// Issue a delegation token.
    #[allow(clippy::too_many_arguments)]
    pub async fn issue(
        pool: &PgPool,
        id: &str,
        principal_type: &str,
        principal_id: &str,
        principal_name: &str,
        actor_agent_id: &str,
        scope: &serde_json::Value,
        grant_type: &str,
        credential_secret_name: &str,
    ) -> Result<DelegationRow, DbError> {
        sqlx::query(
            "INSERT INTO delegation_tokens (id, principal_type, principal_id, principal_name,
                    actor_agent_id, scope, grant_type, credential_secret_name)
             VALUES ($1::uuid, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(id)
        .bind(principal_type)
        .bind(principal_id)
        .bind(principal_name)
        .bind(actor_agent_id)
        .bind(scope)
        .bind(grant_type)
        .bind(credential_secret_name)
        .execute(pool)
        .await?;

        sqlx::query_as::<_, DelegationRow>(
            "SELECT id, principal_type, principal_id, principal_name, actor_agent_id,
                    actor_instance_id, scope, grant_type, credential_secret_name,
                    signed_token, issued_at, expires_at, revoked_at, use_count, parent_delegation_id
             FROM delegation_tokens WHERE id = $1::uuid",
        )
        .bind(id)
        .fetch_one(pool)
        .await
        .map_err(DbError::from)
    }

    /// Revoke a delegation token.
    pub async fn revoke(pool: &PgPool, id: &str) -> Result<bool, DbError> {
        let result = sqlx::query(
            "UPDATE delegation_tokens SET revoked_at = NOW() WHERE id = $1::uuid AND revoked_at IS NULL",
        )
        .bind(id)
        .execute(pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Get children of a delegation.
    pub async fn get_children(pool: &PgPool, parent_id: &str) -> Result<Vec<DelegationRow>, DbError> {
        let rows = sqlx::query_as::<_, DelegationRow>(
            "SELECT id, principal_type, principal_id, principal_name, actor_agent_id,
                    actor_instance_id, scope, grant_type, credential_secret_name,
                    signed_token, issued_at, expires_at, revoked_at, use_count, parent_delegation_id
             FROM delegation_tokens WHERE parent_delegation_id = $1::uuid
             ORDER BY issued_at DESC",
        )
        .bind(parent_id)
        .fetch_all(pool)
        .await?;
        Ok(rows)
    }
}
