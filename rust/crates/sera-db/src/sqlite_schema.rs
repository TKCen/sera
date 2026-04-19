//! SQLite schema orchestration (sera-mwb4).
//!
//! Each module that owns a SQLite-backed store defines an `init_schema`
//! function that is idempotent (CREATE TABLE IF NOT EXISTS). The
//! [`init_all`] entrypoint stitches them together so the gateway boot path
//! can call a single function before constructing any store.
//!
//! Order matters only where foreign-key relationships exist; for the current
//! set of tables (audit_trail, token_usage, schedules, agent_instances,
//! secrets) the tables are independent, so the order below mirrors the
//! module-listing in [`crate::lib`].

use rusqlite::Connection;

use crate::agents::SqliteAgentStore;
use crate::audit::SqliteAuditStore;
use crate::metering::SqliteMeteringStore;
use crate::schedules::SqliteScheduleStore;
use crate::secrets::SqliteSecretsStore;
use crate::signals::SqliteSignalStore;

/// Create every SQLite-backed table used by the gateway's local-first boot
/// path. Safe to call on a pre-populated DB — each per-module `init_schema`
/// uses `CREATE TABLE IF NOT EXISTS`.
pub fn init_all(conn: &Connection) -> rusqlite::Result<()> {
    SqliteAgentStore::init_schema(conn)?;
    SqliteAuditStore::init_schema(conn)?;
    SqliteMeteringStore::init_schema(conn)?;
    SqliteScheduleStore::init_schema(conn)?;
    SqliteSecretsStore::init_schema(conn)?;
    SqliteSignalStore::init_schema(conn)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_all_creates_all_tables() {
        let conn = Connection::open_in_memory().unwrap();
        init_all(&conn).expect("init_all");

        // Verify each table is present via sqlite_master.
        for table in [
            "agent_instances",
            "agent_templates",
            "audit_trail",
            "token_usage",
            "usage_events",
            "token_quotas",
            "schedules",
            "secrets",
            "agent_signals",
        ] {
            let count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name = ?1",
                    [table],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(count, 1, "table {table} missing after init_all");
        }
    }

    #[test]
    fn init_all_is_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        init_all(&conn).expect("first init");
        init_all(&conn).expect("second init");
        init_all(&conn).expect("third init");
    }
}
