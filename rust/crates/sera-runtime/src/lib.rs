//! # sera-runtime — Agent Worker Crate
//!
//! Standalone LLM agent execution engine with pluggable context assembly, tool dispatch,
//! and four-method turn lifecycle (observe, think, act, react).
//!
//! ## Overview
//!
//! The runtime is fully self-contained: it owns the LLM client, tool registry, and context engine.
//! Two modes of operation:
//! - **Interactive**: Human-friendly REPL (default when stdin is a TTY)
//! - **NDJSON**: Machine-readable submission/event protocol for gateway integration
//!
//! ## Quick Start
//!
//! ```no_run
//! use sera_runtime::config::RuntimeConfig;
//! use sera_runtime::default_runtime::DefaultRuntime;
//! use sera_runtime::context_engine::pipeline::ContextPipeline;
//! use sera_runtime::llm_client::LlmClient;
//! use sera_types::runtime::TurnContext;
//! # #[tokio::main]
//! # async fn main() -> anyhow::Result<()> {
//! let config = RuntimeConfig::from_env();
//! let runtime = DefaultRuntime::new(Box::new(ContextPipeline::new()))
//!     .with_llm(Box::new(LlmClient::new(&config)));
//! // runtime.execute_turn(ctx).await?;
//! # Ok(())
//! # }
//! ```
//!
//! See `CLAUDE.md` for module map, environment variables, and protocol details.

// New modules (Lane D, P0-6)
pub mod agent_tool_registry;
pub mod circle_activity;
pub mod compaction;
pub mod context_engine;
pub mod delegation;
pub mod delegation_bus;
pub mod handoff;
pub mod harness;
pub mod memory_assembler;
pub mod subagent;
pub mod turn;

pub mod sera_errors;
pub mod semantic;
pub mod shadow;

// Retained modules
pub mod config;
pub mod memory_budget;
pub mod context;
pub mod default_runtime;
pub mod error;
pub mod health;
pub mod llm_client;
pub mod manifest;
pub mod session_manager;
pub mod tools;
pub mod types;
