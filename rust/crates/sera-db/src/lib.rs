//! SERA Database Layer — PostgreSQL access via sqlx with compile-time checked queries.
//!
//! Design rules (from RUST-MIGRATION-PLAN.md):
//! - Domain objects live in `sera-types`
//! - SQL rows and query code live here in `sera-db`
//! - No leaking `sqlx::Row` or SQL types into handler/business layers

pub mod sqlite;
pub mod pool;
pub mod agents;
pub mod audit;
pub mod circles;
pub mod metering;
pub mod schedules;
pub mod sessions;
pub mod skills;
pub mod api_keys;
pub mod delegations;
pub mod memory;
pub mod notifications;
pub mod operator_requests;
pub mod secrets;
pub mod tasks;
pub mod webhooks;
pub mod job_queue;
pub mod lane_queue;
pub mod training_exports;
pub mod proposal_usage;
pub mod error;

pub use pool::DbPool;
pub use error::DbError;
