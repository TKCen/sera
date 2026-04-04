//! Database connection pool and initialization.

use sqlx::PgPool;

/// Wrapper around sqlx PgPool for dependency injection.
#[derive(Clone)]
pub struct DbPool {
    inner: PgPool,
}

impl DbPool {
    /// Create a new pool from a database URL.
    pub async fn connect(database_url: &str) -> Result<Self, sqlx::Error> {
        let inner = PgPool::connect(database_url).await?;
        Ok(Self { inner })
    }

    /// Get a reference to the inner pool.
    pub fn inner(&self) -> &PgPool {
        &self.inner
    }
}
