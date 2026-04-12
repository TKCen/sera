//! SERA Runtime — reusable library for agent reasoning, tools, and LLM interaction.
//!
//! This crate provides the core agent runtime components used by both the
//! standalone sera-runtime binary (container agent) and the MVS sera binary
//! (integrated gateway).

// New modules (Lane D, P0-6)
pub mod compaction;
pub mod context_engine;
pub mod handoff;
pub mod harness;
pub mod subagent;
pub mod turn;

// Existing modules
pub mod config;
pub mod context;
pub mod context_assembler;
pub mod context_pipeline;
#[allow(deprecated)]
pub mod default_runtime;
pub mod error;
pub mod health;
pub mod llm_client;
pub mod manifest;
pub mod reasoning_loop;
pub mod session_manager;
pub mod tool_loop_detector;
pub mod tools;
pub mod types;
