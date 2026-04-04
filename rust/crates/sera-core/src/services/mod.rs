//! Service layer — business logic orchestration.
//!
//! Services combine repositories, external clients, and domain logic.
//! Services are wired into AppState and routes in later phases.

#[allow(dead_code)]
pub mod audit;
#[allow(dead_code)]
pub mod cleanup;
#[allow(dead_code)]
pub mod circle_registry;
#[allow(dead_code)]
pub mod embedding;
#[allow(dead_code)]
pub mod heartbeat;
#[allow(dead_code)]
pub mod intercom;
#[allow(dead_code)]
pub mod job_queue;
#[allow(dead_code)]
pub mod metering;
#[allow(dead_code)]
pub mod orchestrator;
#[allow(dead_code)]
pub mod secrets;
#[allow(dead_code)]
pub mod session;
#[allow(dead_code)]
pub mod knowledge_git;
#[allow(dead_code)]
pub mod llm_router;
#[allow(dead_code)]
pub mod memory_manager;
#[allow(dead_code)]
pub mod skill_registry;
#[allow(dead_code)]
pub mod tool_executor;
