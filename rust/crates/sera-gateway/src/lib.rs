//! SERA Gateway — reusable library for gateway types, transport, and harness dispatch.

pub mod admin;
pub mod agent_transport;
pub mod byoh_catalog;
pub mod byoh_dispatch_interceptor;
pub mod byoh_registration;
pub mod capability_enforcement;
pub mod embedded_transport;
pub mod connector;
pub mod constitutional_config;
pub mod db_backend;
pub mod envelope;
pub mod external_agent_registry;
pub mod generation;
#[cfg(feature = "gh-api")]
pub mod github_poller;
pub mod harness_dispatch;
pub mod hitl_gateway;
pub mod kill_switch;
pub mod party;
pub mod plugin;
pub mod policy_resolution;
pub mod process_manager;
pub mod response_sanitizer;
pub mod routes;
pub mod scheduler;
pub mod session_persist;
pub mod session_store;
pub mod signals;
pub mod transcript_persist;
pub mod transport;
pub mod workflow_store;

pub use sera_auth::{EvolveTokenError, EvolveTokenSigner};

pub use process_manager::{
    InMemoryProcessRegistryStore, ManagedProcess, ProcessError, ProcessId, ProcessKind,
    ProcessManager, ProcessRegistryStore, ProcessStatus, RestartPolicy, SpawnRequest,
};
