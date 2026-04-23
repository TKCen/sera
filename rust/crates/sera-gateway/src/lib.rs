//! SERA Gateway — reusable library for gateway types, transport, and harness dispatch.

pub mod connector;
pub mod db_backend;
pub mod envelope;
pub mod generation;
pub mod harness_dispatch;
pub mod kill_switch;
pub mod party;
pub mod plugin;
pub mod process_manager;
pub mod session_persist;
pub mod session_store;
pub mod signals;
pub mod transcript_persist;
pub mod transport;

pub use sera_auth::{EvolveTokenError, EvolveTokenSigner};

pub use process_manager::{
    InMemoryProcessRegistryStore, ManagedProcess, ProcessError, ProcessId, ProcessKind,
    ProcessManager, ProcessRegistryStore, ProcessStatus, RestartPolicy, SpawnRequest,
};
