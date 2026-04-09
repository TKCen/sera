//! SERA Runtime — reusable library for agent reasoning, tools, and LLM interaction.
//!
//! This crate provides the core agent runtime components used by both the
//! standalone sera-runtime binary (container agent) and the MVS sera binary
//! (integrated gateway).

pub mod config;
pub mod context;
pub mod context_pipeline;
pub mod context_assembler;
pub mod error;
pub mod health;
pub mod llm_client;
pub mod manifest;
pub mod reasoning_loop;
pub mod session_manager;
pub mod tool_loop_detector;
pub mod tools;
pub mod types;
