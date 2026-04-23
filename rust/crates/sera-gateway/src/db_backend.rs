//! Pluggable database backend abstraction for sera-gateway.
//!
//! The MVS binary in `src/bin/sera.rs` runs against an
//! `Arc<Mutex<SqliteDb>>`. A future Postgres-backed deployment would carry
//! the same [`DbBackend`] shape, chosen at boot via manifest config. This
//! trait keeps the door open for that swap without committing to a specific
//! wiring today.
//!
//! Two concrete impls live here:
//!
//! * [`PgPoolBackend`] — wraps [`sera_db::DbPool`] and exposes the inner
//!   `sqlx::PgPool` for Postgres deployments.
//! * [`SqliteDbBackend`] — wraps [`sera_db::sqlite::SqliteDb`] behind an
//!   `Arc<Mutex<…>>` so SQLite-backed deployments can carry the same
//!   backend-trait shape.

use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::Mutex;

use sera_db::DbPool;
use sera_db::sqlite::SqliteDb;

/// Discriminant identifying which concrete backend a trait object wraps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DbBackendKind {
    /// Postgres via `sqlx::PgPool`.
    Postgres,
    /// SQLite via `rusqlite::Connection`.
    Sqlite,
}

/// Abstraction over the database backend carried in an application's
/// shared state.
///
/// The trait surface is intentionally narrow: it exposes typed accessors for
/// each concrete backend rather than a synthetic query API. Call sites that
/// need backend-specific behaviour pattern-match on [`DbBackend::kind`] or
/// pull the handle via [`DbBackend::pg_pool`] / [`DbBackend::sqlite`].
#[async_trait]
pub trait DbBackend: Send + Sync + 'static {
    /// Which concrete backend this trait object wraps.
    fn kind(&self) -> DbBackendKind;

    /// Returns the underlying `sqlx::PgPool` when this backend is
    /// Postgres-backed, otherwise `None`.
    ///
    /// Postgres-only `sera_db::*Repository` helpers accept `&PgPool`. Once
    /// handlers are ported to typed repository methods that dispatch on
    /// [`DbBackendKind`], call sites will stop reaching for this accessor.
    fn pg_pool(&self) -> Option<&sqlx::PgPool>;

    /// Returns the shared `SqliteDb` handle when this backend is
    /// SQLite-backed, otherwise `None`.
    ///
    /// `SqliteDb` exposes a synchronous `rusqlite` API. Callers that need to
    /// use it from async contexts should wrap the blocking work in
    /// `tokio::task::spawn_blocking`.
    fn sqlite(&self) -> Option<Arc<Mutex<SqliteDb>>>;

    /// Return the `&sqlx::PgPool`, panicking when this backend is not
    /// Postgres-backed.
    ///
    /// Temporary shim for routes/services that haven't yet been ported off
    /// the raw Postgres `sqlx` API. Handlers reached through this accessor
    /// are only safe to invoke after the deployment configured a
    /// [`PgPoolBackend`] — subsequent beads (sera-3l84.2) replace each call
    /// site with a typed repository method that dispatches on
    /// [`DbBackendKind`].
    fn require_pg_pool(&self) -> &sqlx::PgPool {
        self.pg_pool().expect(
            "DbBackend: this handler requires a Postgres-backed pool; \
                     configure a PgPoolBackend for this deployment",
        )
    }
}

// ---------------------------------------------------------------------------
// PgPoolBackend
// ---------------------------------------------------------------------------

/// Postgres-backed [`DbBackend`] implementation.
///
/// Thin wrapper around [`sera_db::DbPool`] so existing call sites can reach
/// the raw `sqlx::PgPool` while handlers are still Postgres-only.
#[derive(Clone)]
pub struct PgPoolBackend {
    pool: DbPool,
}

impl PgPoolBackend {
    /// Wrap an existing [`DbPool`].
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }

    /// Access the underlying [`DbPool`] — convenience accessor for places
    /// that still work with the typed newtype rather than a `&PgPool`.
    pub fn pool(&self) -> &DbPool {
        &self.pool
    }
}

#[async_trait]
impl DbBackend for PgPoolBackend {
    fn kind(&self) -> DbBackendKind {
        DbBackendKind::Postgres
    }

    fn pg_pool(&self) -> Option<&sqlx::PgPool> {
        Some(self.pool.inner())
    }

    fn sqlite(&self) -> Option<Arc<Mutex<SqliteDb>>> {
        None
    }
}

// ---------------------------------------------------------------------------
// SqliteDbBackend
// ---------------------------------------------------------------------------

/// SQLite-backed [`DbBackend`] implementation.
///
/// Wraps `SqliteDb` in `Arc<Mutex<…>>` so the `rusqlite` connection can be
/// shared across handlers. The `Mutex` is a tokio mutex — handlers already
/// live in async contexts and need to hold the guard across `.await` points
/// to keep the `SqliteDb` API usable without splitting transactions.
///
/// `SqliteDb` itself is synchronous; call sites that touch the handle from
/// inside a route should move blocking work onto
/// [`tokio::task::spawn_blocking`] when doing heavy I/O. Most MVS handlers
/// do trivial single-statement queries and take the guard directly.
#[derive(Clone)]
pub struct SqliteDbBackend {
    inner: Arc<Mutex<SqliteDb>>,
}

impl SqliteDbBackend {
    /// Wrap an already-constructed, shared [`SqliteDb`].
    pub fn new(inner: Arc<Mutex<SqliteDb>>) -> Self {
        Self { inner }
    }

    /// Wrap a freshly-constructed [`SqliteDb`], taking ownership.
    pub fn from_db(db: SqliteDb) -> Self {
        Self {
            inner: Arc::new(Mutex::new(db)),
        }
    }

    /// Access the shared handle directly.
    pub fn handle(&self) -> Arc<Mutex<SqliteDb>> {
        Arc::clone(&self.inner)
    }
}

#[async_trait]
impl DbBackend for SqliteDbBackend {
    fn kind(&self) -> DbBackendKind {
        DbBackendKind::Sqlite
    }

    fn pg_pool(&self) -> Option<&sqlx::PgPool> {
        None
    }

    fn sqlite(&self) -> Option<Arc<Mutex<SqliteDb>>> {
        Some(Arc::clone(&self.inner))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn sqlite_backend_exposes_sqlite_handle_and_reports_kind() {
        let db = SqliteDb::open_in_memory().expect("in-memory sqlite");
        let backend = SqliteDbBackend::from_db(db);
        let erased: Arc<dyn DbBackend> = Arc::new(backend);

        assert_eq!(erased.kind(), DbBackendKind::Sqlite);
        assert!(erased.pg_pool().is_none());

        let handle = erased.sqlite().expect("sqlite handle");
        // Drive a round-trip through the handle to prove the Mutex-wrapped
        // SqliteDb is actually usable from an async context.
        {
            let guard = handle.lock().await;
            guard
                .append_audit("test.event", "actor-1", "agent", Some("details"))
                .expect("append audit");
            let rows = guard.query_audit(10).expect("query audit");
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].event_type, "test.event");
        }
    }

    #[tokio::test]
    async fn sqlite_backend_handle_clones_share_state() {
        let db = SqliteDb::open_in_memory().expect("in-memory sqlite");
        let backend = SqliteDbBackend::from_db(db);

        // Writing through one clone must be visible through another — proves
        // the Arc<Mutex<…>> plumbing isn't accidentally duplicating state.
        let a = backend.handle();
        let b = backend.handle();
        {
            let guard = a.lock().await;
            guard
                .append_audit("evt", "actor", "agent", None)
                .expect("append");
        }
        {
            let guard = b.lock().await;
            let rows = guard.query_audit(5).expect("query");
            assert_eq!(rows.len(), 1);
        }
    }
}
