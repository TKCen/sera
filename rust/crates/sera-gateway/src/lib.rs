//! SERA Gateway — reusable library for gateway types, transport, and harness dispatch.

pub mod connector;
pub mod envelope;
pub mod generation;
pub mod harness_dispatch;
pub mod kill_switch;
pub mod plugin;
pub mod process_manager;
pub mod session_persist;
pub mod transcript_persist;
pub mod transport;

pub use process_manager::{
    InMemoryProcessRegistryStore, ManagedProcess, ProcessError, ProcessId, ProcessKind,
    ProcessManager, ProcessRegistryStore, ProcessStatus, RestartPolicy, SpawnRequest,
};
