//! Service layer — business logic orchestration.
//!
//! Services combine repositories, external clients, and domain logic.
//! Services are wired into AppState and routes in later phases.
// TODO(sera-2q1d): these modules are scaffolded but not yet wired into routes;
// suppress dead_code until they are consumed in AppState / route handlers.
#![allow(dead_code)]

pub mod audit;
pub mod debounce;
pub mod dedupe;
pub mod cleanup;
pub mod circle_registry;
pub mod circle_state;
pub mod coordination;
pub mod embedding;
pub mod heartbeat;
pub mod intercom;
pub mod job_queue;
pub mod metering;
pub mod orchestrator;
pub mod secrets;
pub mod session;
pub mod knowledge_git;
pub mod llm_router;
pub mod memory_manager;
pub mod skill_registry;
pub mod tool_executor;
pub mod schedule_service;
pub mod circuit_breaker;
pub mod process_manager;
pub mod notification_service;
pub mod mcp_server_manager;
pub mod dynamic_provider_manager;
