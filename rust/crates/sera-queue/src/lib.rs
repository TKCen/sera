pub mod backend;
pub mod lane;
pub mod migration_kind;
pub mod throttle;

#[cfg(feature = "local")]
pub mod local;

// Re-exports
pub use backend::{QueueBackend, QueueError};
pub use lane::{EnqueueResult, LaneQueue, QueueMode, QueuedEvent};
pub use migration_kind::MigrationKind;
pub use throttle::GlobalThrottle;

#[cfg(feature = "local")]
pub use local::LocalQueueBackend;

#[cfg(feature = "apalis")]
pub mod sqlx_backend;

#[cfg(feature = "apalis")]
pub use sqlx_backend::SqlxQueueBackend;

#[cfg(feature = "apalis")]
pub mod apalis_backend;

#[cfg(feature = "apalis")]
pub use apalis_backend::{ApalisSeraStorage, SeraJobContext};

/// Re-export of [`apalis_cron`] — users enable the `apalis-cron` feature and
/// call `CronStream::new(...).pipe_to_storage(apalis_sera_storage)` to drive
/// cron-triggered jobs through `QueueBackend`.
#[cfg(feature = "apalis-cron")]
pub use apalis_cron;
